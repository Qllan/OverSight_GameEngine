//! GPU renderer: draws the frame's sorted draw calls with wgpu (DirectX 12
//! or Vulkan on Windows, Vulkan on Linux, Metal on macOS) into an
//! offscreen texture the size of the canvas, then scales it into the
//! window with black bars, the same way the software renderer does.
//!
//! Everything becomes textured triangles: sprites use their texture,
//! shapes a 1x1 white texture, text a glyph atlas. Consecutive triangles
//! with the same texture go out in one draw call, so the order of the
//! draw calls (layer, depth, call order) is kept exactly.
//!
//! Render targets are textures drawn into the same way, each in its own
//! pass before the screen. Post effects are full-screen passes that read
//! the picture and write it into a scratch texture of the same size.

use std::collections::HashMap;
use std::sync::Arc;

use tiny_skia::{Pixmap, Point, Transform};
use wgpu::util::DeviceExt;

use crate::light::LightPx;
use crate::particles::Record;
use crate::post::{self, Effect};
use crate::render::{
    normal_map_of, normal_matrix, quad_visible, sprite_quad, sprite_src, sprite_transform, DrawCmd,
    DrawKind, Renderer, Shape,
};
use crate::text::Font;
use crate::texture::{Store, Texture};
use crate::types::{Color, Rect, Vec2};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;
const ATLAS_SIZE: u32 = 2048;
/// Samples per pixel when shapes are antialiased
const MSAA: u32 = 4;
/// Bytes between per-pass uniform blocks (the dynamic offset alignment)
const SLOT: u64 = 256;
/// Bytes of one uniform block the shaders see
const BLOCK: u64 = SLOT;

#[repr(C)]
#[derive(Clone, Copy)]
struct Vertex {
    pos: [f32; 2],
    uv: [f32; 2],
    /// Premultiplied colour the texel is multiplied by
    color: [f32; 4],
}

impl Vertex {
    fn bytes(v: &[Vertex]) -> &[u8] {
        // Vertex is repr(C) and made only of f32s, so it has no padding
        unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, std::mem::size_of_val(v)) }
    }
}

/// A textured quad, drawn as an instance: corner `a`, edges `ex` and
/// `ey` in pixels, the texture region and the colour. Sprites, tiles and
/// glyphs are quads, so each costs one of these, not six vertices.
#[repr(C)]
#[derive(Clone, Copy)]
struct Quad {
    a: [f32; 2],
    ex: [f32; 2],
    ey: [f32; 2],
    uv: [f32; 4],
    /// Premultiplied colour the texel is multiplied by
    color: [f32; 4],
}

impl Quad {
    fn bytes(q: &[Quad]) -> &[u8] {
        // Quad is repr(C) and made only of f32s, so it has no padding
        unsafe { std::slice::from_raw_parts(q.as_ptr() as *const u8, std::mem::size_of_val(q)) }
    }
}

/// Which texture a run of triangles samples
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    White,
    Atlas,
    Texture(u32),
    /// One GPU emitter's draw: an index into `particle_draws`
    Particles(u32),
    /// A page of the sprite atlas
    Page(u32),
}

struct Batch {
    source: Source,
    /// Quads (instances) rather than vertices
    quads: bool,
    start: u32,
    count: u32,
}

/// A GPU emitter's ring of particles, kept on the card between frames
struct ParticleBuffer {
    buffer: wgpu::Buffer,
    len: usize,
    epoch: u32,
    written: u64,
    /// Drawn this frame (rings not drawn are dropped)
    used: bool,
}

/// One GPU emitter drawn in a pass
struct ParticleDraw {
    ring: u64,
    len: u32,
    /// The texture, or white for dots
    texture: Option<u32>,
    time: f32,
    size: (u32, u32),
    params: [f32; 28],
    block: u64,
}

struct GpuTexture {
    generation: u64,
    smooth: bool,
    bind_group: wgpu::BindGroup,
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    size: (u32, u32),
    /// Render targets only: the antialiased picture, made on first use
    msaa: Option<wgpu::TextureView>,
}

/// One target's draw calls (a range of batches) and its effects
struct GpuPass {
    target: u32,
    size: (u32, u32),
    clear: Option<Color>,
    batches: std::ops::Range<usize>,
    effects: Vec<Effect>,
    /// Sprites' normals for the lighting effect (empty: all flat)
    normal_batches: std::ops::Range<usize>,
    /// The lights it sees, each with its shadow triangles' vertices
    lights: Vec<(LightPx, std::ops::Range<u32>)>,
    /// Byte offset of its uniform block (picture size)
    block: u64,
}

/// Pictures the lighting effect works in, per picture size
struct LightTextures {
    /// Shadow mask and the blur's other half
    masks: [(wgpu::Texture, wgpu::TextureView); 2],
    /// Light gathered per pixel (beyond 1 where lights add up)
    lightmap: (wgpu::Texture, wgpu::TextureView),
    normals: (wgpu::Texture, wgpu::TextureView),
}

/// Built-in effect passes, in the order of `FX_ENTRIES`
#[derive(Clone, Copy)]
enum Fx {
    GlowA,
    GlowB,
    BlurA,
    BlurB,
    Grade,
    Crt,
    MaskBlurA,
    MaskBlurB,
    Lit,
}

const FX_ENTRIES: [&str; 9] = [
    "fx_glow_a",
    "fx_glow_b",
    "fx_blur_a",
    "fx_blur_b",
    "fx_grade",
    "fx_crt",
    "fx_mask_blur_a",
    "fx_mask_blur_b",
    "fx_lit",
];
/// Lightmap format: lights add up past 1
const LIGHT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;

/// Which texture an effect pass reads or writes: the target itself or
/// one of its two scratch textures
#[derive(Clone, Copy, PartialEq)]
enum Slot {
    Main,
    A,
    B,
}

impl Slot {
    /// A slot that is neither of these
    fn other(a: Slot, b: Slot) -> Slot {
        [Slot::Main, Slot::A, Slot::B]
            .into_iter()
            .find(|s| *s != a && *s != b)
            .unwrap_or(Slot::A)
    }
}

/// Glyph images packed into one texture, row by row
struct Atlas {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    places: HashMap<(u32, char, u32), Rect>,
    x: u32,
    y: u32,
    row_h: u32,
}

impl Atlas {
    /// Place for a glyph image; when the atlas is full it starts over
    /// (glyphs already placed this frame may show garbage for a frame)
    fn allocate(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        if w + 2 > ATLAS_SIZE || h + 2 > ATLAS_SIZE {
            return None;
        }
        if self.x + w + 1 > ATLAS_SIZE {
            self.x = 1;
            self.y += self.row_h + 1;
            self.row_h = 0;
        }
        if self.y + h + 1 > ATLAS_SIZE {
            self.places.clear();
            self.x = 1;
            self.y = 1;
            self.row_h = 0;
        }
        let at = (self.x, self.y);
        self.x += w + 1;
        self.row_h = self.row_h.max(h);
        Some(at)
    }
}

/// Maps keyed by texture ids, which the renderer looks up for every
/// sprite: a multiply instead of SipHash
type IdMap<V> = HashMap<u32, V, std::hash::BuildHasherDefault<IdHash>>;

#[derive(Default)]
struct IdHash(u64);

impl std::hash::Hasher for IdHash {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = (self.0 << 8 | *b as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
        }
    }

    fn write_u32(&mut self, n: u32) {
        self.0 = (n as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    }
}

/// Textures up to this size share atlas pages, so sprites from many
/// small textures draw in one batch
const PACK_MAX: u32 = 512;

/// One page of the sprite atlas; its images keep a 1-pixel border of
/// their own edge pixels, so filtering never reaches a neighbour
struct Page {
    texture: wgpu::Texture,
    bind_group: wgpu::BindGroup,
    smooth: bool,
    x: u32,
    y: u32,
    row_h: u32,
}

impl Page {
    /// Shelf packing; the image's top left inside its border
    fn place(&mut self, w: u32, h: u32) -> Option<(u32, u32)> {
        let (w, h) = (w + 2, h + 2);
        if self.x + w > ATLAS_SIZE {
            self.x = 0;
            self.y += self.row_h;
            self.row_h = 0;
        }
        if self.y + h > ATLAS_SIZE {
            return None;
        }
        let at = (self.x + 1, self.y + 1);
        self.x += w;
        self.row_h = self.row_h.max(h);
        Some(at)
    }
}

/// An atlas page and the image's top left in it
type AtlasSpot = (u32, f32, f32);

/// Where a texture sits in the sprite atlas
struct Packed {
    page: u32,
    x: u32,
    y: u32,
    size: (u32, u32),
    generation: u64,
}

struct Target {
    size: (u32, u32),
    texture: wgpu::Texture,
    view: wgpu::TextureView,
    msaa: Option<wgpu::TextureView>,
    /// The canvas texture, for scaling it into the window
    blit_group: wgpu::BindGroup,
}

struct Presenter {
    surface: wgpu::Surface<'static>,
    config: wgpu::SurfaceConfiguration,
    pipeline: wgpu::RenderPipeline,
}

pub struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
    pub adapter_name: String,
    texture_layout: wgpu::BindGroupLayout,
    uniform_layout: wgpu::BindGroupLayout,
    uniforms: wgpu::Buffer,
    uniform_group: wgpu::BindGroup,
    /// Two textures and a sampler, for effect passes
    post_layout: wgpu::BindGroupLayout,
    fx_layout: wgpu::PipelineLayout,
    fx: Vec<wgpu::RenderPipeline>,
    /// Compiled custom shaders by id
    shaders: HashMap<u32, wgpu::RenderPipeline>,
    /// Two scratch textures per picture size, for effect passes
    scratch: HashMap<(u32, u32), [(wgpu::Texture, wgpu::TextureView); 2]>,
    light_textures: HashMap<(u32, u32), LightTextures>,
    /// Sprites' normals turned to the screen
    normal_pipeline: wgpu::RenderPipeline,
    /// One light added into the lightmap
    light_pipeline: wgpu::RenderPipeline,
    /// 1 sample (crisp) and MSAA (antialiased) versions
    pipelines: [wgpu::RenderPipeline; 2],
    /// The same for quads
    quad_pipelines: [wgpu::RenderPipeline; 2],
    quads: Vec<Quad>,
    quad_buffer: wgpu::Buffer,
    /// The same for GPU emitters
    particle_pipelines: [wgpu::RenderPipeline; 2],
    particle_buffers: HashMap<u64, ParticleBuffer>,
    particle_draws: Vec<ParticleDraw>,
    nearest: wgpu::Sampler,
    linear: wgpu::Sampler,
    white: wgpu::BindGroup,
    atlas: Atlas,
    pages: Vec<Page>,
    packed: IdMap<Packed>,
    /// Atlas pixels of images that were freed or moved
    wasted: u64,
    /// Textures up to this size go into the atlas (tests turn it off)
    pack_max: u32,
    textures: IdMap<GpuTexture>,
    target: Target,
    presenter: Option<Presenter>,
    vertex_buffer: wgpu::Buffer,
    vertices: Vec<Vertex>,
    batches: Vec<Batch>,
    passes: Vec<GpuPass>,
    /// The next triangles start a new batch (a new pass begins)
    split: bool,
}

const SHADER: &str = r#"
struct Uniforms { size: vec2<f32> };
@group(0) @binding(0) var<uniform> u: Uniforms;
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;

// Centroid: with MSAA an edge pixel's centre can lie outside the quad,
// and a uv taken there reads the next tile or frame of the sheet
struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) @interpolate(perspective, centroid) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
};

@vertex
fn vs(@location(0) pos: vec2<f32>, @location(1) uv: vec2<f32>, @location(2) color: vec4<f32>) -> VOut {
    var o: VOut;
    o.pos = vec4<f32>(pos.x / u.size.x * 2.0 - 1.0, 1.0 - pos.y / u.size.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    o.color = color;
    return o;
}

@vertex
fn vs_quad(@builtin(vertex_index) vi: u32, @location(0) a: vec2<f32>, @location(1) ex: vec2<f32>,
           @location(2) ey: vec2<f32>, @location(3) uv: vec4<f32>, @location(4) color: vec4<f32>) -> VOut {
    var corners = array<vec2<f32>, 6>(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
                                      vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let q = corners[vi];
    let pos = a + ex * q.x + ey * q.y;
    var o: VOut;
    o.pos = vec4<f32>(pos.x / u.size.x * 2.0 - 1.0, 1.0 - pos.y / u.size.y * 2.0, 0.0, 1.0);
    o.uv = uv.xy + uv.zw * q;
    o.color = color;
    return o;
}

@fragment
fn fs(i: VOut) -> @location(0) vec4<f32> {
    return textureSample(tex, samp, i.uv) * i.color;
}

// Normals for lighting: the colour is the 2x2 matrix turning the sprite's
// directions to the screen (all zero: flat, only the texel's alpha
// counts). Same maths as post::normal_texel.
@fragment
fn fs_normal(i: VOut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv);
    if t.a <= 0.0 {
        return vec4<f32>(0.0);
    }
    let m = i.color;
    var n = vec3<f32>(0.0, 0.0, 1.0);
    if any(m != vec4<f32>(0.0)) {
        let d = t.rgb / t.a * 2.0 - 1.0;
        let l = vec2<f32>(d.x, -d.y);
        let v = vec3<f32>(m.x * l.x + m.y * l.y, m.z * l.x + m.w * l.y, d.z);
        let len = length(v);
        if len > 1e-6 {
            n = v / len;
        }
    }
    return vec4<f32>((n * 0.5 + 0.5) * t.a, t.a);
}

// Scaling the canvas into the window, and effect passes: one triangle
// covering the viewport
struct BOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn blit_vs(@builtin(vertex_index) i: u32) -> BOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: BOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

@fragment
fn blit_fs(i: BOut) -> @location(0) vec4<f32> {
    let c = textureSample(tex, samp, i.uv);
    // Premultiplied colour over black is its rgb
    return vec4<f32>(c.rgb, 1.0);
}
"#;

/// GPU emitters: one quad per particle slot (instanced), placed from how
/// the particle was born and the time, the same maths as
/// `particles::ParticleDraw::each`. Dead slots collapse to nothing.
const PARTICLE_SHADER: &str = r#"
@group(1) @binding(0) var tex: texture_2d<f32>;
@group(1) @binding(1) var samp: sampler;
struct Part {
    size: vec2<f32>, time: f32, pass_: f32,
    // the world-to-pixels transform: x axis, y axis, then the offset
    m0: vec4<f32>, m1: vec4<f32>,
    // drag, size at birth, size at death, 1 for a texture
    k: vec4<f32>,
    c0: vec4<f32>, c1: vec4<f32>,
    uv: vec4<f32>,
    // region width and height, 1 for antialiased dots
    src: vec4<f32>,
};
@group(0) @binding(0) var<uniform> part: Part;

struct POut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
    @location(1) color: vec4<f32>,
    @location(2) disc: vec2<f32>,
    @location(3) radius: f32,
};

fn part_path(p0: vec2<f32>, v0: vec2<f32>, age: f32) -> vec2<f32> {
    let g = part.m1.zw;
    let k = part.k.x;
    if k < 1e-4 {
        return p0 + v0 * age + g * (0.5 * age * age);
    }
    let gk = g / k;
    return p0 + gk * age + (v0 - gk) * ((1.0 - exp(-k * age)) / k);
}

@vertex
fn part_vs(@builtin(vertex_index) vi: u32, @location(0) p0: vec2<f32>, @location(1) v0: vec2<f32>,
           @location(2) r: vec4<f32>) -> POut {
    var o: POut;
    o.pos = vec4<f32>(-2.0, -2.0, 0.0, 1.0);
    o.uv = vec2<f32>(0.5);
    o.color = vec4<f32>(0.0);
    o.disc = vec2<f32>(0.0);
    o.radius = 0.0;
    let age = part.time - r.x;
    if age < 0.0 || age >= r.y {
        return o;
    }
    let t = age / r.y;
    let size = mix(part.k.y, part.k.z, t);
    let c = floor(mix(part.c0, part.c1, t) * 255.0 + 0.5) / 255.0;
    if size <= 0.0 || c.a <= 0.0 {
        return o;
    }
    var corners = array<vec2<f32>, 6>(vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 0.0), vec2<f32>(1.0, 1.0),
                                      vec2<f32>(0.0, 0.0), vec2<f32>(1.0, 1.0), vec2<f32>(0.0, 1.0));
    let q = corners[vi];
    var local: vec2<f32>;
    if part.k.w > 0.5 {
        let l = (q * part.src.xy - part.src.xy * 0.5) * (size / part.src.x);
        let a = r.z + r.w * age;
        let cs = cos(a);
        let sn = sin(a);
        local = vec2<f32>(l.x * cs - l.y * sn, l.x * sn + l.y * cs);
        o.uv = part.uv.xy + q * part.uv.zw;
    } else {
        let px = length(part.m0.xy);
        let radius = size * 0.5;
        local = (q * 2.0 - 1.0) * (radius + 1.0 / max(px, 1e-6));
        o.disc = local * px;
        o.radius = radius * px;
    }
    let w = part_path(p0, v0, age) + local;
    let s = vec2<f32>(part.m0.x * w.x + part.m0.z * w.y + part.m1.x, part.m0.y * w.x + part.m0.w * w.y + part.m1.y);
    o.pos = vec4<f32>(s.x / part.size.x * 2.0 - 1.0, 1.0 - s.y / part.size.y * 2.0, 0.0, 1.0);
    o.color = vec4<f32>(c.rgb * c.a, c.a);
    return o;
}

@fragment
fn part_fs(i: POut) -> @location(0) vec4<f32> {
    let t = textureSample(tex, samp, i.uv);
    if part.k.w > 0.5 {
        return t * i.color;
    }
    let d = length(i.disc);
    var cov = select(0.0, 1.0, d <= i.radius);
    if part.src.z > 0.5 {
        cov = clamp(i.radius - d + 0.5, 0.0, 1.0);
    }
    return i.color * cov;
}
"#;

/// What every effect shader (built-in or custom) starts with
const FX_PRELUDE: &str = r#"
struct Post { size: vec2<f32>, time: f32, pass_: f32, p: array<vec4<f32>, 3> };
@group(0) @binding(0) var<uniform> post: Post;
@group(1) @binding(0) var source: texture_2d<f32>;
@group(1) @binding(2) var samp: sampler;

struct FxOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn fx_vs(@builtin(vertex_index) i: u32) -> FxOut {
    let uv = vec2<f32>(f32((i << 1u) & 2u), f32(i & 2u));
    var o: FxOut;
    o.pos = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.0, 1.0);
    o.uv = uv;
    return o;
}

// One pixel of the picture, clamped to its edges
fn load(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(source, clamp(p, vec2<i32>(0), vec2<i32>(post.size) - 1), 0);
}
"#;

/// The built-in effects; the same maths as `post.rs`
const FX_SHADER: &str = r#"
@group(1) @binding(1) var second: texture_2d<f32>;

fn load2(p: vec2<i32>) -> vec4<f32> {
    return textureLoad(second, clamp(p, vec2<i32>(0), vec2<i32>(post.size) - 1), 0);
}

fn luma(c: vec3<f32>) -> f32 {
    return dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
}

fn radius(v: f32) -> i32 {
    return i32(ceil(clamp(v, 1.0, 32.0)));
}

// Gaussian blur of `source` along `dir`; threshold >= 0 keeps only the
// bright part of each tap
fn blur(p: vec2<i32>, dir: vec2<i32>, r: i32, threshold: f32) -> vec4<f32> {
    let sigma = f32(r) / 2.0;
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var k = -r; k <= r; k++) {
        let w = exp(-f32(k * k) / (2.0 * sigma * sigma));
        var c = load(p + dir * k);
        if threshold >= 0.0 {
            c *= clamp((luma(c.rgb) - threshold) / max(1.0 - threshold, 1e-3), 0.0, 1.0);
        }
        sum += c * w;
        total += w;
    }
    return sum / total;
}

fn pixel(pos: vec4<f32>) -> vec2<i32> {
    return vec2<i32>(floor(pos.xy));
}

@fragment
fn fx_glow_a(i: FxOut) -> @location(0) vec4<f32> {
    return blur(pixel(i.pos), vec2<i32>(1, 0), radius(post.p[0].z), max(post.p[0].x, 0.0));
}

@fragment
fn fx_glow_b(i: FxOut) -> @location(0) vec4<f32> {
    let p = pixel(i.pos);
    let s = load2(p);
    let g = blur(p, vec2<i32>(0, 1), radius(post.p[0].z), -1.0);
    let o = s.rgb + g.rgb * post.p[0].y;
    let a = max(s.a, min(max(o.r, max(o.g, o.b)), 1.0));
    return vec4<f32>(min(o, vec3<f32>(a)), a);
}

@fragment
fn fx_blur_a(i: FxOut) -> @location(0) vec4<f32> {
    return blur(pixel(i.pos), vec2<i32>(1, 0), radius(post.p[0].x), -1.0);
}

@fragment
fn fx_blur_b(i: FxOut) -> @location(0) vec4<f32> {
    return blur(pixel(i.pos), vec2<i32>(0, 1), radius(post.p[0].x), -1.0);
}

@fragment
fn fx_grade(i: FxOut) -> @location(0) vec4<f32> {
    let c = load(pixel(i.pos));
    if c.a <= 0.0 {
        return c;
    }
    let p0 = post.p[0];
    let p1 = post.p[1];
    var rgb = (c.rgb / c.a + p0.x - 0.5) * p0.y + 0.5;
    let l = luma(rgb);
    let s = clamp(vec3<f32>(l) + (rgb - vec3<f32>(l)) * p0.z, vec3<f32>(0.0), vec3<f32>(1.0));
    let g = pow(max(s, vec3<f32>(1e-6)), vec3<f32>(1.0 / max(p0.w, 0.01)));
    rgb = clamp(g * p1.xyz, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb * c.a, c.a);
}

// Bilinear between texel centres; `s` is in texels
fn sample_at(s: vec2<f32>) -> vec4<f32> {
    let f0 = floor(s);
    let f = s - f0;
    let p = vec2<i32>(f0);
    let top = mix(load(p), load(p + vec2<i32>(1, 0)), f.x);
    let bottom = mix(load(p + vec2<i32>(0, 1)), load(p + vec2<i32>(1, 1)), f.x);
    return mix(top, bottom, f.y);
}

// Lighting. Mask blur: p[0].x radius, p[2] the light's bounds (reads stay
// inside them, like the software mask of just that area)
fn load_in(p: vec2<i32>) -> vec4<f32> {
    let b = vec4<i32>(post.p[2]);
    return textureLoad(source, clamp(p, b.xy, b.zw - 1), 0);
}

fn mask_blur(p: vec2<i32>, dir: vec2<i32>) -> vec4<f32> {
    let r = radius(post.p[0].x);
    let sigma = f32(r) / 2.0;
    var sum = vec4<f32>(0.0);
    var total = 0.0;
    for (var k = -r; k <= r; k++) {
        let w = exp(-f32(k * k) / (2.0 * sigma * sigma));
        sum += load_in(p + dir * k) * w;
        total += w;
    }
    return sum / total;
}

@fragment
fn fx_mask_blur_a(i: FxOut) -> @location(0) vec4<f32> {
    return mask_blur(pixel(i.pos), vec2<i32>(1, 0));
}

@fragment
fn fx_mask_blur_b(i: FxOut) -> @location(0) vec4<f32> {
    return mask_blur(pixel(i.pos), vec2<i32>(0, 1));
}

// One light: p[0] position, radius, height; p[1] colour; source the
// shadow mask, second the normals. Added into the lightmap.
@fragment
fn fx_light(i: FxOut) -> @location(0) vec4<f32> {
    let p = pixel(i.pos);
    let k = post.p[0];
    let d2 = (k.xy - (vec2<f32>(p) + 0.5)) / k.z;
    let d = length(d2);
    if d >= 1.0 {
        return vec4<f32>(0.0);
    }
    let nb = load2(p);
    var n = vec3<f32>(0.0, 0.0, 1.0);
    if nb.a > 0.0 {
        let v = nb.rgb / nb.a * 2.0 - 1.0;
        let len = length(v);
        if len > 1e-6 {
            n = v / len;
        }
    }
    let l = normalize(vec3<f32>(d2, max(k.w, 0.01)));
    let facing = clamp(dot(n, l) / l.z, 0.0, 2.0);
    let v = (1.0 - d) * (1.0 - d) * facing * load(p).r;
    return vec4<f32>(post.p[1].rgb * v, 0.0);
}

// The picture (source) times the light gathered (second), at most twice
// as bright
@fragment
fn fx_lit(i: FxOut) -> @location(0) vec4<f32> {
    let p = pixel(i.pos);
    let c = load(p);
    let l = min(load2(p).rgb, vec3<f32>(2.0));
    return vec4<f32>(min(c.rgb * l, vec3<f32>(c.a)), c.a);
}

@fragment
fn fx_crt(i: FxOut) -> @location(0) vec4<f32> {
    let p = pixel(i.pos);
    let k = post.p[0];
    let q = (vec2<f32>(p) + 0.5) / post.size * 2.0 - 1.0;
    let r2 = dot(q, q);
    let c = q * (1.0 + k.y * r2);
    if abs(c.x) > 1.0 || abs(c.y) > 1.0 {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }
    let s = (c * 0.5 + 0.5) * post.size - 0.5;
    let g = sample_at(s);
    var rgb = vec3<f32>(
        sample_at(s + vec2<f32>(k.w, 0.0)).r,
        g.g,
        sample_at(s - vec2<f32>(k.w, 0.0)).b,
    );
    var dim = clamp(1.0 - k.z * r2 * 0.5, 0.0, 1.0);
    if p.y % 2 == 1 {
        dim *= 1.0 - clamp(k.x, 0.0, 1.0);
    }
    return vec4<f32>(rgb * dim, g.a);
}
"#;

/// A custom effect shader with the prelude and its entry point around it
fn shader_source(wgsl: &str) -> String {
    format!(
        "{}\n{}\n@fragment\nfn fx_custom(i: FxOut) -> @location(0) vec4<f32> {{\n    return effect(i.uv, floor(i.pos.xy));\n}}\n",
        FX_PRELUDE, wgsl
    )
}

/// Check a custom effect shader without a graphics card
pub fn validate_shader(wgsl: &str) -> Result<(), String> {
    use wgpu::naga;
    let full = shader_source(wgsl);
    let module = naga::front::wgsl::parse_str(&full).map_err(|e| e.emit_to_string(&full))?;
    naga::valid::Validator::new(
        naga::valid::ValidationFlags::all(),
        naga::valid::Capabilities::empty(),
    )
    .validate(&module)
    .map_err(|e| e.emit_to_string(&full))?;
    Ok(())
}

impl Gpu {
    /// A GPU renderer for a canvas of `size`; with a window it also
    /// presents into it. Tries a real graphics card first, then the
    /// system's software adapter (WARP, llvmpipe).
    #[cfg(feature = "window")]
    pub fn for_window(window: Arc<winit::window::Window>, size: (u32, u32)) -> Result<Gpu, String> {
        let desc = wgpu::InstanceDescriptor::new_with_display_handle(Box::new(window.clone()));
        let instance = wgpu::Instance::new(desc);
        let surface = instance
            .create_surface(window.clone())
            .map_err(|e| e.to_string())?;
        let (mut gpu, adapter) = Gpu::create(&instance, Some(&surface), size)?;
        let caps = surface.get_capabilities(&adapter);
        // A non-sRGB format: blending happens on the stored values, as in
        // the software renderer, so both look the same
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| !f.is_srgb())
            .or_else(|| caps.formats.first().copied())
            .ok_or("the window cannot show GPU frames")?;
        let inner = window.inner_size();
        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: inner.width.max(1),
            height: inner.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            desired_maximum_frame_latency: 2,
            alpha_mode: caps
                .alpha_modes
                .first()
                .copied()
                .unwrap_or(wgpu::CompositeAlphaMode::Auto),
            view_formats: vec![],
            color_space: Default::default(),
        };
        surface.configure(&gpu.device, &config);
        let pipeline = gpu.blit_pipeline(format);
        gpu.presenter = Some(Presenter {
            surface,
            config,
            pipeline,
        });
        Ok(gpu)
    }

    /// A GPU renderer without a window (tests, comparing with software)
    pub fn headless(size: (u32, u32)) -> Result<Gpu, String> {
        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
        Ok(Gpu::create(&instance, None, size)?.0)
    }

    fn create(
        instance: &wgpu::Instance,
        surface: Option<&wgpu::Surface<'_>>,
        size: (u32, u32),
    ) -> Result<(Gpu, wgpu::Adapter), String> {
        let adapter = request_adapter(instance, surface)?;
        let adapter_name = adapter.get_info().name;
        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("orst"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::downlevel_defaults().using_resolution(adapter.limits()),
            ..Default::default()
        }))
        .map_err(|e| format!("no GPU device: {}", e))?;
        Ok((Gpu::shared(device, queue, adapter_name, size), adapter))
    }

    /// A renderer on a device someone else made (the editor's, so its
    /// panels can show the canvas without copying it); see `canvas`
    pub fn shared(
        device: wgpu::Device,
        queue: wgpu::Queue,
        adapter_name: String,
        size: (u32, u32),
    ) -> Gpu {
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("orst"),
            source: wgpu::ShaderSource::Wgsl(SHADER.into()),
        });
        let uniform_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: None,
                },
                count: None,
            }],
        });
        let texture_entry = |binding: u32| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::FRAGMENT,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let post_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("post"),
            entries: &[
                texture_entry(0),
                texture_entry(1),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: None,
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let (uniforms, uniform_group) = uniform_buffer(&device, &uniform_layout, 16);
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: None,
            bind_group_layouts: &[Some(&uniform_layout), Some(&texture_layout)],
            immediate_size: 0,
        });
        let pipeline = |samples: u32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("orst"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Vertex>() as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x2, 1 => Float32x2, 2 => Float32x4
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pipelines = [pipeline(1), pipeline(MSAA)];
        let quad_layout = wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Quad>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &wgpu::vertex_attr_array![
                0 => Float32x2, 1 => Float32x2, 2 => Float32x2, 3 => Float32x4, 4 => Float32x4
            ],
        };
        let quad_pipeline = |samples: u32, fs: &str| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("quads"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("vs_quad"),
                    compilation_options: Default::default(),
                    buffers: &[Some(quad_layout.clone())],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some(fs),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let quad_pipelines = [quad_pipeline(1, "fs"), quad_pipeline(MSAA, "fs")];
        let part_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("particles"),
            source: wgpu::ShaderSource::Wgsl(PARTICLE_SHADER.into()),
        });
        let particle_pipeline = |samples: u32| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("particles"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &part_module,
                    entry_point: Some("part_vs"),
                    compilation_options: Default::default(),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: std::mem::size_of::<Record>() as u64,
                        step_mode: wgpu::VertexStepMode::Instance,
                        attributes: &wgpu::vertex_attr_array![
                            0 => Float32x2, 1 => Float32x2, 2 => Float32x4
                        ],
                    })],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: wgpu::MultisampleState {
                    count: samples,
                    ..Default::default()
                },
                fragment: Some(wgpu::FragmentState {
                    module: &part_module,
                    entry_point: Some("part_fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: FORMAT,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let particle_pipelines = [particle_pipeline(1), particle_pipeline(MSAA)];
        let fx_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("post"),
            bind_group_layouts: &[Some(&uniform_layout), Some(&post_layout)],
            immediate_size: 0,
        });
        let fx_module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("effects"),
            source: wgpu::ShaderSource::Wgsl(format!("{}{}", FX_PRELUDE, FX_SHADER).into()),
        });
        let fx = FX_ENTRIES
            .iter()
            .map(|entry| fx_pipeline(&device, &fx_layout, &fx_module, entry))
            .collect();
        // Normals come from sprites, so they are quads
        let normal_pipeline = quad_pipeline(1, "fs_normal");
        let add = wgpu::BlendComponent {
            src_factor: wgpu::BlendFactor::One,
            dst_factor: wgpu::BlendFactor::One,
            operation: wgpu::BlendOperation::Add,
        };
        let light_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("light"),
            layout: Some(&fx_layout),
            vertex: wgpu::VertexState {
                module: &fx_module,
                entry_point: Some("fx_vs"),
                compilation_options: Default::default(),
                buffers: &[],
            },
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: None,
            multisample: Default::default(),
            fragment: Some(wgpu::FragmentState {
                module: &fx_module,
                entry_point: Some("fx_light"),
                compilation_options: Default::default(),
                targets: &[Some(wgpu::ColorTargetState {
                    format: LIGHT_FORMAT,
                    blend: Some(wgpu::BlendState {
                        color: add,
                        alpha: add,
                    }),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
            }),
            multiview_mask: None,
            cache: None,
        });
        let sampler = |filter: wgpu::FilterMode| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                address_mode_u: wgpu::AddressMode::ClampToEdge,
                address_mode_v: wgpu::AddressMode::ClampToEdge,
                mag_filter: filter,
                min_filter: filter,
                ..Default::default()
            })
        };
        let (nearest, linear) = (
            sampler(wgpu::FilterMode::Nearest),
            sampler(wgpu::FilterMode::Linear),
        );
        let bind = |texture: &wgpu::Texture, sampler: &wgpu::Sampler| {
            let view = texture.create_view(&Default::default());
            device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: None,
                layout: &texture_layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: wgpu::BindingResource::TextureView(&view),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    },
                ],
            })
        };
        let white_texture = device.create_texture_with_data(
            &queue,
            &texture_desc(1, 1, wgpu::TextureUsages::TEXTURE_BINDING),
            Default::default(),
            &[255; 4],
        );
        let white = bind(&white_texture, &nearest);
        let atlas_texture = device.create_texture(&texture_desc(
            ATLAS_SIZE,
            ATLAS_SIZE,
            wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        ));
        let atlas = Atlas {
            bind_group: bind(&atlas_texture, &linear),
            texture: atlas_texture,
            places: HashMap::new(),
            x: 1,
            y: 1,
            row_h: 0,
        };
        let target = make_target(&device, &texture_layout, &nearest, size);
        let (vertex_buffer, quad_buffer) =
            (vertex_buffer(&device, 4096), vertex_buffer(&device, 4096));
        Gpu {
            device,
            queue,
            adapter_name,
            texture_layout,
            uniform_layout,
            uniforms,
            uniform_group,
            post_layout,
            fx_layout,
            fx,
            shaders: HashMap::new(),
            scratch: HashMap::new(),
            light_textures: HashMap::new(),
            normal_pipeline,
            light_pipeline,
            pipelines,
            quad_pipelines,
            quads: Vec::new(),
            quad_buffer,
            particle_pipelines,
            particle_buffers: HashMap::new(),
            particle_draws: Vec::new(),
            nearest,
            linear,
            white,
            atlas,
            pages: Vec::new(),
            packed: IdMap::default(),
            wasted: 0,
            pack_max: PACK_MAX,
            textures: IdMap::default(),
            target,
            presenter: None,
            vertex_buffer,
            vertices: Vec::new(),
            batches: Vec::new(),
            passes: Vec::new(),
            split: true,
        }
    }

    /// The texture the last frame was drawn into (Rgba8Unorm, the canvas
    /// size); a new one after the canvas changes size
    pub fn canvas(&self) -> (&wgpu::Texture, &wgpu::TextureView) {
        (&self.target.texture, &self.target.view)
    }

    #[cfg(feature = "window")]
    fn blit_pipeline(&self, format: wgpu::TextureFormat) -> wgpu::RenderPipeline {
        let shader = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("blit"),
                source: wgpu::ShaderSource::Wgsl(SHADER.into()),
            });
        let layout = self
            .device
            .create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: None,
                bind_group_layouts: &[None, Some(&self.texture_layout)],
                immediate_size: 0,
            });
        self.device
            .create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("blit"),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &shader,
                    entry_point: Some("blit_vs"),
                    compilation_options: Default::default(),
                    buffers: &[],
                },
                primitive: wgpu::PrimitiveState::default(),
                depth_stencil: None,
                multisample: Default::default(),
                fragment: Some(wgpu::FragmentState {
                    module: &shader,
                    entry_point: Some("blit_fs"),
                    compilation_options: Default::default(),
                    targets: &[Some(format.into())],
                }),
                multiview_mask: None,
                cache: None,
            })
    }

    /// Draw a frame's sorted draw calls into the render targets and the
    /// canvas texture, with their effects; returns how many sprites were
    /// drawn (after culling). `time` is seconds, for custom shaders.
    pub fn render(
        &mut self,
        renderer: &Renderer,
        commands: &[DrawCmd],
        textures: &mut Store<Texture>,
        fonts: &mut Store<Font>,
        time: f32,
    ) -> u32 {
        let size = renderer.size();
        if self.target.size != size {
            self.target = make_target(&self.device, &self.texture_layout, &self.nearest, size);
        }
        self.sync_textures(textures);
        self.sync_atlas(textures);
        self.vertices.clear();
        self.quads.clear();
        self.batches.clear();
        self.passes.clear();
        self.particle_draws.clear();
        for b in self.particle_buffers.values_mut() {
            b.used = false;
        }
        let mut sprites = 0;
        for pass in renderer.passes(commands) {
            let fits = pass.target == 0
                || self
                    .textures
                    .get(&pass.target)
                    .is_some_and(|t| t.size == pass.size);
            if !fits {
                continue;
            }
            self.split = true;
            let first = self.batches.len();
            let world = renderer.space_transform(crate::types::Space::World, pass.target);
            // The last texture looked up: (id, its size if it is on the
            // card, its place in the atlas)
            let mut last: (u32, Option<(u32, u32)>, Option<AtlasSpot>) = (0, None, None);
            for cmd in &commands[pass.commands.clone()] {
                let base = match cmd.space {
                    crate::types::Space::World => world,
                    crate::types::Space::Screen => Transform::identity(),
                };
                match &cmd.kind {
                    DrawKind::Sprite { texture, sprite } => {
                        // A target cannot show itself while it is drawn
                        if *texture == pass.target {
                            continue;
                        }
                        if last.0 != *texture {
                            let on_card = self.textures.contains_key(texture);
                            let size = textures.get(*texture).filter(|_| on_card);
                            let packed = self
                                .packed
                                .get(texture)
                                .map(|p| (p.page, p.x as f32, p.y as f32));
                            last = (*texture, size.map(|t| (t.width(), t.height())), packed);
                        }
                        let Some((tw, th)) = last.1 else {
                            continue;
                        };
                        let src = sprite_src(sprite, tw, th);
                        let q = sprite_quad(base, sprite, src);
                        if !quad_visible(pass.size, &q) {
                            continue;
                        }
                        let color = premultiplied(sprite.tint);
                        let (tw, th) = (tw as f32, th as f32);
                        let sample = crate::render::inset(src);
                        // A region past the texture's edge needs its own
                        // texture's clamping, not the atlas neighbours
                        let inside =
                            src.x >= 0.0 && src.y >= 0.0 && src.right() <= tw && src.bottom() <= th;
                        match last.2.filter(|_| inside) {
                            Some((page, x, y)) => {
                                let a = ATLAS_SIZE as f32;
                                let uv = Rect::new(
                                    (x + sample.x) / a,
                                    (y + sample.y) / a,
                                    sample.w / a,
                                    sample.h / a,
                                );
                                self.quad_at(Source::Page(page), q, uv, color);
                            }
                            None => {
                                let s = sample;
                                let uv = Rect::new(s.x / tw, s.y / th, s.w / tw, s.h / th);
                                self.quad_at(Source::Texture(*texture), q, uv, color);
                            }
                        }
                        sprites += 1;
                    }
                    DrawKind::Shape { shape, color } => self.shape(shape, *color, base),
                    DrawKind::Particles(d) => {
                        if !d.textured() || d.texture != pass.target {
                            self.particles(d, base, textures, renderer.antialias, pass.size);
                        }
                    }
                    DrawKind::Text {
                        font,
                        text,
                        pos,
                        size,
                        color,
                    } => {
                        if let Some(f) = fonts.get_mut(*font) {
                            self.text(*font, f, text, *pos, *size, *color, base);
                        }
                    }
                }
            }
            let batches = first..self.batches.len();
            let effects = renderer.pass_effects(&pass).to_vec();
            let (mut normal_batches, mut lights) = (0..0, Vec::new());
            if effects.iter().any(|e| e.kind == post::EFFECT_LIGHTING) {
                let cmds = &commands[pass.commands.clone()];
                let world = renderer.space_transform(crate::types::Space::World, pass.target);
                normal_batches = self.normal_batches(cmds, pass.target, pass.size, world, textures);
                if let Some(scene) = renderer.light_scenes.get(&pass.target) {
                    for light in &scene.lights {
                        let start = self.vertices.len() as u32;
                        for q in light.shadow_quads(&scene.edges) {
                            let v = |p: Vec2| Vertex {
                                pos: [p.x, p.y],
                                uv: [0.5, 0.5],
                                color: [0.0, 0.0, 0.0, 1.0],
                            };
                            let tri = [q[0], q[1], q[2], q[0], q[2], q[3]].map(v);
                            self.vertices.extend_from_slice(&tri);
                        }
                        lights.push((*light, start..self.vertices.len() as u32));
                    }
                    // Raw vertices: the next batch must not grow over them
                    self.split = true;
                }
            }
            self.passes.push(GpuPass {
                target: pass.target,
                size: pass.size,
                clear: pass.clear,
                batches,
                effects,
                normal_batches,
                lights,
                block: 0,
            });
        }
        self.particle_buffers.retain(|_, b| b.used);
        self.draw(renderer.antialias, time);
        sprites
    }

    /// Batches drawing a pass's sprites' normals (see `fs_normal`); none
    /// when no sprite has a normal map
    fn normal_batches(
        &mut self,
        commands: &[DrawCmd],
        target: u32,
        size: (u32, u32),
        world: Transform,
        textures: &Store<Texture>,
    ) -> std::ops::Range<usize> {
        let has_maps = commands.iter().any(|c| match &c.kind {
            DrawKind::Sprite { texture, .. } => textures
                .get(*texture)
                .is_some_and(|t| normal_map_of(textures, t).is_some()),
            _ => false,
        });
        let first = self.batches.len();
        if !has_maps {
            return first..first;
        }
        self.split = true;
        for cmd in commands {
            let DrawKind::Sprite { texture, sprite } = &cmd.kind else {
                continue;
            };
            let Some(tex) = textures.get(*texture).filter(|_| *texture != target) else {
                continue;
            };
            let base = match cmd.space {
                crate::types::Space::World => world,
                crate::types::Space::Screen => Transform::identity(),
            };
            let (tw, th) = (tex.width(), tex.height());
            let src = sprite_src(sprite, tw, th);
            let q = sprite_quad(base, sprite, src);
            if !quad_visible(size, &q) {
                continue;
            }
            let (source, m) = match normal_map_of(textures, tex) {
                Some(n) => (
                    n,
                    normal_matrix(base.pre_concat(sprite_transform(sprite, src))),
                ),
                None => (*texture, [0.0; 4]),
            };
            if !self.textures.contains_key(&source) {
                continue;
            }
            let s = crate::render::inset(src);
            let (tw, th) = (tw as f32, th as f32);
            let uv = Rect::new(s.x / tw, s.y / th, s.w / tw, s.h / th);
            self.quad_at(Source::Texture(source), q, uv, m);
        }
        first..self.batches.len()
    }

    /// Draw calls of the last frame's passes (sprites from atlas pages
    /// share them)
    #[cfg(test)]
    fn batch_count(&self) -> usize {
        self.batches.len()
    }

    /// Compile a custom effect shader (see `Engine::create_shader`)
    pub fn compile_shader(&mut self, id: u32, wgsl: &str) -> Result<(), String> {
        let scope = self.device.push_error_scope(wgpu::ErrorFilter::Validation);
        let module = self
            .device
            .create_shader_module(wgpu::ShaderModuleDescriptor {
                label: Some("custom effect"),
                source: wgpu::ShaderSource::Wgsl(shader_source(wgsl).into()),
            });
        let pipeline = fx_pipeline(&self.device, &self.fx_layout, &module, "fx_custom");
        if let Some(e) = pollster::block_on(scope.pop()) {
            return Err(e.to_string());
        }
        self.shaders.insert(id, pipeline);
        Ok(())
    }

    pub fn free_shader(&mut self, id: u32) {
        self.shaders.remove(&id);
    }

    /// Upload new or changed textures, drop freed ones
    fn sync_textures(&mut self, textures: &Store<Texture>) {
        self.textures.retain(|id, g| {
            textures
                .get(*id)
                .is_some_and(|t| t.generation == g.generation)
        });
        for id in textures.ids() {
            let Some(tex) = textures.get(id) else {
                continue;
            };
            let rebind = match self.textures.get(&id) {
                Some(g) if g.smooth == tex.smooth => continue,
                // Only the filter changed: same image, other sampler
                Some(g) => Some(g.texture.create_view(&Default::default())),
                None => None,
            };
            match rebind {
                Some(view) => {
                    let bind_group = self.bind_group(&view, tex.smooth);
                    if let Some(g) = self.textures.get_mut(&id) {
                        g.smooth = tex.smooth;
                        g.bind_group = bind_group;
                    }
                }
                None => {
                    let mut usage = wgpu::TextureUsages::TEXTURE_BINDING;
                    if tex.target {
                        usage |= wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::COPY_SRC
                            | wgpu::TextureUsages::COPY_DST;
                    }
                    let texture = self.device.create_texture_with_data(
                        &self.queue,
                        &texture_desc(tex.width(), tex.height(), usage),
                        Default::default(),
                        tex.pixmap.data(),
                    );
                    let view = texture.create_view(&Default::default());
                    let bind_group = self.bind_group(&view, tex.smooth);
                    self.textures.insert(
                        id,
                        GpuTexture {
                            generation: tex.generation,
                            smooth: tex.smooth,
                            bind_group,
                            texture,
                            view,
                            size: (tex.width(), tex.height()),
                            msaa: None,
                        },
                    );
                }
            }
        }
    }

    /// Put small textures into the sprite atlas, updating changed ones in
    /// place; when the pages run out and much of them is freed space,
    /// pack everything again
    fn sync_atlas(&mut self, textures: &Store<Texture>) {
        let max = self.pack_max;
        let fits = |t: &Texture| !t.target && t.width() <= max && t.height() <= max;
        let mut wasted = 0;
        self.packed.retain(|id, p| {
            let keep = textures
                .get(*id)
                .is_some_and(|t| fits(t) && (t.width(), t.height()) == p.size);
            if !keep {
                wasted += (p.size.0 + 2) as u64 * (p.size.1 + 2) as u64;
            }
            keep
        });
        self.wasted += wasted;
        for id in textures.ids() {
            let Some(tex) = textures.get(id).filter(|t| fits(t)) else {
                continue;
            };
            if let Some(p) = self.packed.get(&id) {
                if self.pages[p.page as usize].smooth != tex.smooth {
                    self.wasted += (p.size.0 + 2) as u64 * (p.size.1 + 2) as u64;
                    self.packed.remove(&id);
                } else if p.generation != tex.generation {
                    let (page, x, y) = (p.page, p.x, p.y);
                    self.upload_packed(page, x, y, tex);
                    if let Some(p) = self.packed.get_mut(&id) {
                        p.generation = tex.generation;
                    }
                    continue;
                } else {
                    continue;
                }
            }
            let (w, h) = (tex.width(), tex.height());
            let mut spot = self
                .pages
                .iter_mut()
                .enumerate()
                .filter(|(_, p)| p.smooth == tex.smooth)
                .find_map(|(i, p)| p.place(w, h).map(|(x, y)| (i as u32, x, y)));
            let area = (ATLAS_SIZE * ATLAS_SIZE) as u64;
            if spot.is_none() && self.wasted * 2 > area {
                // Start the pages over and place everything again (with
                // nothing wasted this cannot come back here)
                for p in &mut self.pages {
                    (p.x, p.y, p.row_h) = (0, 0, 0);
                }
                self.packed.clear();
                self.wasted = 0;
                return self.sync_atlas(textures);
            }
            if spot.is_none() {
                let texture = self.device.create_texture(&texture_desc(
                    ATLAS_SIZE,
                    ATLAS_SIZE,
                    wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                ));
                let view = texture.create_view(&Default::default());
                let mut page = Page {
                    bind_group: self.bind_group(&view, tex.smooth),
                    texture,
                    smooth: tex.smooth,
                    x: 0,
                    y: 0,
                    row_h: 0,
                };
                spot = page
                    .place(w, h)
                    .map(|(x, y)| (self.pages.len() as u32, x, y));
                self.pages.push(page);
            }
            let Some((page, x, y)) = spot else {
                continue;
            };
            self.upload_packed(page, x, y, tex);
            let packed = Packed {
                page,
                x,
                y,
                size: (w, h),
                generation: tex.generation,
            };
            self.packed.insert(id, packed);
        }
    }

    /// Write a texture into an atlas page with a border of its own edges
    fn upload_packed(&self, page: u32, x: u32, y: u32, tex: &Texture) {
        let (w, h) = (tex.width() as usize, tex.height() as usize);
        let (pw, ph) = (w + 2, h + 2);
        let src = tex.pixmap.data();
        let mut px = vec![0u8; pw * ph * 4];
        for py in 0..ph {
            let sy = py.saturating_sub(1).min(h - 1);
            for qx in 0..pw {
                let sx = qx.saturating_sub(1).min(w - 1);
                let (d, s) = ((py * pw + qx) * 4, (sy * w + sx) * 4);
                px[d..d + 4].copy_from_slice(&src[s..s + 4]);
            }
        }
        self.queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &self.pages[page as usize].texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: x - 1,
                    y: y - 1,
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            &px,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(pw as u32 * 4),
                rows_per_image: Some(ph as u32),
            },
            wgpu::Extent3d {
                width: pw as u32,
                height: ph as u32,
                depth_or_array_layers: 1,
            },
        );
    }

    fn bind_group(&self, view: &wgpu::TextureView, smooth: bool) -> wgpu::BindGroup {
        let sampler = if smooth { &self.linear } else { &self.nearest };
        self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.texture_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(sampler),
                },
            ],
        })
    }

    /// Queue a GPU emitter's draw, bringing its ring on the card up to date
    fn particles(
        &mut self,
        d: &crate::particles::ParticleDraw,
        base: Transform,
        textures: &Store<Texture>,
        antialias: bool,
        size: (u32, u32),
    ) {
        let ring = &d.ring;
        let len = ring.records.len();
        if len == 0 {
            return;
        }
        let bytes = |r: &[Record]| {
            // Record is repr(C) and made only of f32s, so it has no padding
            unsafe { std::slice::from_raw_parts(r.as_ptr() as *const u8, std::mem::size_of_val(r)) }
        };
        let fresh = match self.particle_buffers.get(&ring.id) {
            Some(b) => {
                b.len != len
                    || b.epoch != ring.epoch
                    || ring.written < b.written
                    || ring.written - b.written >= len as u64
            }
            None => true,
        };
        if fresh {
            let same = self
                .particle_buffers
                .get(&ring.id)
                .is_some_and(|b| b.len == len);
            if !same {
                let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("particles"),
                    size: (len * std::mem::size_of::<Record>()) as u64,
                    usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                });
                let b = ParticleBuffer {
                    buffer,
                    len,
                    epoch: 0,
                    written: 0,
                    used: true,
                };
                self.particle_buffers.insert(ring.id, b);
            }
            let b = &self.particle_buffers[&ring.id];
            self.queue.write_buffer(&b.buffer, 0, bytes(&ring.records));
        } else {
            let b = &self.particle_buffers[&ring.id];
            let size = std::mem::size_of::<Record>() as u64;
            let (from, to) = (
                (b.written % len as u64) as usize,
                (ring.written % len as u64) as usize,
            );
            let write = |a: usize, z: usize| {
                if z > a {
                    self.queue
                        .write_buffer(&b.buffer, a as u64 * size, bytes(&ring.records[a..z]));
                }
            };
            if from <= to {
                write(from, to);
            } else {
                write(from, len);
                write(0, to);
            }
        }
        if let Some(b) = self.particle_buffers.get_mut(&ring.id) {
            b.epoch = ring.epoch;
            b.written = ring.written;
            b.used = true;
        }
        let textured = d.textured() && self.textures.contains_key(&d.texture);
        let (tw, th) = textures
            .get(d.texture)
            .map_or((1.0, 1.0), |t| (t.width() as f32, t.height() as f32));
        let o = map(base, 0.0, 0.0);
        let (x, y) = (map(base, 1.0, 0.0), map(base, 0.0, 1.0));
        let straight = |c: Color| [c.r, c.g, c.b, c.a].map(|v| v as f32 / 255.0);
        let mut p = [0.0; 28];
        p[..8].copy_from_slice(&[
            x[0] - o[0],
            x[1] - o[1],
            y[0] - o[0],
            y[1] - o[1],
            o[0],
            o[1],
            d.gravity.x,
            d.gravity.y,
        ]);
        p[8..12].copy_from_slice(&[d.drag, d.size[0], d.size[1], textured as u8 as f32]);
        p[12..16].copy_from_slice(&straight(d.colors[0]));
        p[16..20].copy_from_slice(&straight(d.colors[1]));
        p[20..24].copy_from_slice(&[d.src.x / tw, d.src.y / th, d.src.w / tw, d.src.h / th]);
        p[24..27].copy_from_slice(&[d.src.w, d.src.h, antialias as u8 as f32]);
        let index = self.particle_draws.len() as u32;
        self.particle_draws.push(ParticleDraw {
            ring: ring.id,
            len: len as u32,
            texture: textured.then_some(d.texture),
            time: d.time,
            size,
            params: p,
            block: 0,
        });
        self.batches.push(Batch {
            source: Source::Particles(index),
            quads: false,
            start: 0,
            count: 0,
        });
    }

    fn push(&mut self, source: Source, vertices: &[Vertex]) {
        let start = self.vertices.len() as u32;
        self.vertices.extend_from_slice(vertices);
        let split = std::mem::take(&mut self.split);
        match self.batches.last_mut() {
            Some(b) if b.source == source && !b.quads && !split => b.count += vertices.len() as u32,
            _ => self.batches.push(Batch {
                source,
                quads: false,
                start,
                count: vertices.len() as u32,
            }),
        }
    }

    /// Two triangles: `local` rectangle mapped by `transform`, sampling `uv`
    fn quad(&mut self, source: Source, local: Rect, uv: Rect, transform: Transform, tint: Color) {
        self.quad_raw(source, local, uv, transform, premultiplied(tint));
    }

    /// A quad with its vertex colour as given (the normal pass puts a
    /// matrix there)
    fn quad_raw(
        &mut self,
        source: Source,
        local: Rect,
        uv: Rect,
        transform: Transform,
        color: [f32; 4],
    ) {
        let p = |x: f32, y: f32| {
            let [x, y] = map(transform, x, y);
            Vec2::new(x, y)
        };
        let (a, b, d) = (
            p(local.x, local.y),
            p(local.right(), local.y),
            p(local.x, local.bottom()),
        );
        let edges = [
            a,
            Vec2::new(b.x - a.x, b.y - a.y),
            Vec2::new(d.x - a.x, d.y - a.y),
        ];
        self.quad_at(source, edges, uv, color);
    }

    /// A quad from its corner and edges in pixels, sampling `uv`
    fn quad_at(&mut self, source: Source, q: [Vec2; 3], uv: Rect, color: [f32; 4]) {
        let start = self.quads.len() as u32;
        self.quads.push(Quad {
            a: [q[0].x, q[0].y],
            ex: [q[1].x, q[1].y],
            ey: [q[2].x, q[2].y],
            uv: [uv.x, uv.y, uv.w, uv.h],
            color,
        });
        let split = std::mem::take(&mut self.split);
        match self.batches.last_mut() {
            Some(b) if b.source == source && b.quads && !split => b.count += 1,
            _ => self.batches.push(Batch {
                source,
                quads: true,
                start,
                count: 1,
            }),
        }
    }

    /// Filled triangles from a list of points in local space
    fn triangles(&mut self, points: &[Vec2], color: Color, transform: Transform) {
        let color = premultiplied(color);
        let vertices: Vec<Vertex> = points
            .iter()
            .map(|p| Vertex {
                pos: map(transform, p.x, p.y),
                uv: [0.5, 0.5],
                color,
            })
            .collect();
        self.push(Source::White, &vertices);
    }

    fn shape(&mut self, shape: &Shape, color: Color, transform: Transform) {
        let mut tris = Vec::new();
        let rect = |tris: &mut Vec<Vec2>, x: f32, y: f32, w: f32, h: f32| {
            let (a, b) = (Vec2::new(x, y), Vec2::new(x + w, y));
            let (c, d) = (Vec2::new(x + w, y + h), Vec2::new(x, y + h));
            tris.extend_from_slice(&[a, b, c, a, c, d]);
        };
        // Enough segments that a circle looks round at its size on screen
        let scale = (transform.sx * transform.sx + transform.ky * transform.ky).sqrt();
        let segments = |r: f32| ((r * scale).sqrt() * 6.0).clamp(12.0, 256.0) as usize;
        match *shape {
            Shape::Rect {
                rect: r,
                filled: true,
                ..
            } => rect(&mut tris, r.x, r.y, r.w, r.h),
            Shape::Rect {
                rect: r,
                filled: false,
                thickness,
            } => {
                // A stroke centred on the edge, like the software renderer
                let t = thickness.max(0.1);
                let h = t / 2.0;
                rect(&mut tris, r.x - h, r.y - h, r.w + t, t);
                rect(&mut tris, r.x - h, r.bottom() - h, r.w + t, t);
                rect(&mut tris, r.x - h, r.y + h, t, r.h - t);
                rect(&mut tris, r.right() - h, r.y + h, t, r.h - t);
            }
            Shape::Line {
                from,
                to,
                thickness,
            } => {
                let (dx, dy) = (to.x - from.x, to.y - from.y);
                let len = (dx * dx + dy * dy).sqrt();
                if len > 0.0 {
                    let h = thickness.max(0.1) / 2.0;
                    let (nx, ny) = (-dy / len * h, dx / len * h);
                    let a = Vec2::new(from.x + nx, from.y + ny);
                    let b = Vec2::new(to.x + nx, to.y + ny);
                    let c = Vec2::new(to.x - nx, to.y - ny);
                    let d = Vec2::new(from.x - nx, from.y - ny);
                    tris.extend_from_slice(&[a, b, c, a, c, d]);
                }
            }
            Shape::Circle {
                center,
                radius,
                filled,
                thickness,
            } => {
                let half = thickness.max(0.1) / 2.0;
                let (inner, outer) = if filled {
                    (0.0, radius)
                } else {
                    ((radius - half).max(0.0), radius + half)
                };
                let n = segments(outer);
                let at = |r: f32, i: usize| {
                    let a = i as f32 / n as f32 * std::f32::consts::TAU;
                    Vec2::new(center.x + a.cos() * r, center.y + a.sin() * r)
                };
                for i in 0..n {
                    let (o0, o1) = (at(outer, i), at(outer, i + 1));
                    if inner <= 0.0 {
                        tris.extend_from_slice(&[center, o0, o1]);
                    } else {
                        let (i0, i1) = (at(inner, i), at(inner, i + 1));
                        tris.extend_from_slice(&[i0, o0, o1, i0, o1, i1]);
                    }
                }
            }
        }
        self.triangles(&tris, color, transform);
    }

    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        font_id: u32,
        font: &mut Font,
        text: &str,
        pos: Vec2,
        size: f32,
        color: Color,
        base: Transform,
    ) {
        // White glyphs: coverage in every channel, coloured per vertex
        let (glyphs, _) = font.layout(text, size, Color::WHITE);
        let transform = base.pre_translate(pos.x, pos.y);
        for placed in glyphs {
            let Some(pixmap) = font.cached(&placed.key).and_then(|g| g.pixmap.as_ref()) else {
                continue;
            };
            let key = (font_id, placed.key.0, placed.key.1);
            let place = match self.atlas.places.get(&key) {
                Some(&r) => r,
                None => {
                    let (w, h) = (pixmap.width(), pixmap.height());
                    let Some((x, y)) = self.atlas.allocate(w, h) else {
                        continue;
                    };
                    self.queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &self.atlas.texture,
                            mip_level: 0,
                            origin: wgpu::Origin3d { x, y, z: 0 },
                            aspect: wgpu::TextureAspect::All,
                        },
                        pixmap.data(),
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(w * 4),
                            rows_per_image: Some(h),
                        },
                        wgpu::Extent3d {
                            width: w,
                            height: h,
                            depth_or_array_layers: 1,
                        },
                    );
                    let r = Rect::new(x as f32, y as f32, w as f32, h as f32);
                    self.atlas.places.insert(key, r);
                    r
                }
            };
            let s = ATLAS_SIZE as f32;
            let uv = Rect::new(place.x / s, place.y / s, place.w / s, place.h / s);
            let (x, y) = (placed.pos.x.round(), placed.pos.y.round());
            let local = Rect::new(x, y, place.w, place.h);
            self.quad(Source::Atlas, local, uv, transform, color);
        }
    }

    fn draw(&mut self, antialias: bool, time: f32) {
        // Everything the passes need is made before encoding them
        let runs: usize = self
            .passes
            .iter()
            .flat_map(|p| &p.effects)
            .map(|e| match e.kind {
                post::EFFECT_GLOW | post::EFFECT_BLUR => 2,
                _ => 1,
            })
            .sum::<usize>()
            + self
                .passes
                .iter()
                .filter(|p| p.effects.iter().any(|e| e.kind == post::EFFECT_LIGHTING))
                .map(|p| p.lights.len() * 3 * p.effects.len())
                .sum::<usize>();
        let slots = (self.passes.len() + self.particle_draws.len() + runs) as u64;
        if slots * SLOT > self.uniforms.size() {
            (self.uniforms, self.uniform_group) =
                uniform_buffer(&self.device, &self.uniform_layout, slots * 2);
        }
        for i in 0..self.passes.len() {
            let pass = &self.passes[i];
            if !pass.effects.is_empty() && !self.scratch.contains_key(&pass.size) {
                let make = || {
                    let t = self.device.create_texture(&texture_desc(
                        pass.size.0,
                        pass.size.1,
                        wgpu::TextureUsages::RENDER_ATTACHMENT
                            | wgpu::TextureUsages::TEXTURE_BINDING
                            | wgpu::TextureUsages::COPY_SRC,
                    ));
                    let v = t.create_view(&Default::default());
                    (t, v)
                };
                let pair = [make(), make()];
                self.scratch.insert(pass.size, pair);
            }
            let lit = pass.effects.iter().any(|e| e.kind == post::EFFECT_LIGHTING);
            if lit && !self.light_textures.contains_key(&pass.size) {
                let make = |format: wgpu::TextureFormat| {
                    let t = self.device.create_texture(&wgpu::TextureDescriptor {
                        format,
                        ..texture_desc(
                            pass.size.0,
                            pass.size.1,
                            wgpu::TextureUsages::RENDER_ATTACHMENT
                                | wgpu::TextureUsages::TEXTURE_BINDING,
                        )
                    });
                    let v = t.create_view(&Default::default());
                    (t, v)
                };
                let textures = LightTextures {
                    masks: [make(FORMAT), make(FORMAT)],
                    lightmap: make(LIGHT_FORMAT),
                    normals: make(FORMAT),
                };
                self.light_textures.insert(pass.size, textures);
            }
            let (target, wants_msaa) = (pass.target, antialias && pass.clear.is_some());
            if let Some(t) = self.textures.get_mut(&target) {
                if wants_msaa && t.msaa.is_none() {
                    t.msaa = Some(msaa_view(&self.device, t.size));
                }
            }
        }
        let bytes = Vertex::bytes(&self.vertices);
        if bytes.len() as u64 > self.vertex_buffer.size() {
            self.vertex_buffer = vertex_buffer(&self.device, bytes.len() as u64 * 2);
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.vertex_buffer, 0, bytes);
        }
        let bytes = Quad::bytes(&self.quads);
        if bytes.len() as u64 > self.quad_buffer.size() {
            self.quad_buffer = vertex_buffer(&self.device, bytes.len() as u64 * 2);
        }
        if !bytes.is_empty() {
            self.queue.write_buffer(&self.quad_buffer, 0, bytes);
        }
        let mut blocks = vec![0u8; (slots * SLOT) as usize];
        for (i, pass) in self.passes.iter_mut().enumerate() {
            pass.block = i as u64 * SLOT;
        }
        let mut slot = self.passes.len() as u64;
        for d in &mut self.particle_draws {
            d.block = slot * SLOT;
            write_block(&mut blocks, d.block, d.size, d.time, &d.params);
            slot += 1;
        }
        let mut encoder = self.device.create_command_encoder(&Default::default());
        for pass in &self.passes {
            let (view, msaa) = if pass.target == 0 {
                (&self.target.view, self.target.msaa.as_ref())
            } else {
                match self.textures.get(&pass.target) {
                    Some(t) => (&t.view, t.msaa.as_ref()),
                    None => continue,
                }
            };
            // A target that keeps its picture is drawn over without MSAA,
            // so what it loads is exactly what it showed
            let msaa = msaa.filter(|_| antialias && pass.clear.is_some());
            let (attachment, resolve) = match msaa {
                Some(m) => (m, Some(view)),
                None => (view, None),
            };
            let load = match pass.clear {
                Some(c) => {
                    let c = premultiplied(c);
                    wgpu::LoadOp::Clear(wgpu::Color {
                        r: c[0] as f64,
                        g: c[1] as f64,
                        b: c[2] as f64,
                        a: c[3] as f64,
                    })
                }
                None => wgpu::LoadOp::Load,
            };
            let offset = pass.block;
            write_block(&mut blocks, offset, pass.size, time, &[]);
            {
                let mut rp = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("frame"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: attachment,
                        depth_slice: None,
                        resolve_target: resolve,
                        ops: wgpu::Operations {
                            load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    ..Default::default()
                });
                let samples = resolve.is_some() as usize;
                rp.set_bind_group(0, &self.uniform_group, &[offset as u32]);
                // Quads or triangles, set when it changes
                let mut drawing = None;
                for batch in &self.batches[pass.batches.clone()] {
                    if let Source::Particles(i) = batch.source {
                        let d = &self.particle_draws[i as usize];
                        let Some(b) = self.particle_buffers.get(&d.ring) else {
                            continue;
                        };
                        let group = match d.texture.and_then(|t| self.textures.get(&t)) {
                            Some(t) => &t.bind_group,
                            None => &self.white,
                        };
                        rp.set_pipeline(&self.particle_pipelines[samples]);
                        rp.set_bind_group(0, &self.uniform_group, &[d.block as u32]);
                        rp.set_bind_group(1, group, &[]);
                        rp.set_vertex_buffer(0, b.buffer.slice(..));
                        rp.draw(0..6, 0..d.len);
                        rp.set_bind_group(0, &self.uniform_group, &[offset as u32]);
                        drawing = None;
                        continue;
                    }
                    if drawing != Some(batch.quads) {
                        drawing = Some(batch.quads);
                        if batch.quads {
                            rp.set_pipeline(&self.quad_pipelines[samples]);
                            rp.set_vertex_buffer(0, self.quad_buffer.slice(..));
                        } else {
                            rp.set_pipeline(&self.pipelines[samples]);
                            rp.set_vertex_buffer(0, self.vertex_buffer.slice(..));
                        }
                    }
                    let group = match batch.source {
                        Source::White => &self.white,
                        Source::Atlas => &self.atlas.bind_group,
                        Source::Texture(id) => match self.textures.get(&id) {
                            Some(t) => &t.bind_group,
                            None => continue,
                        },
                        Source::Page(n) => &self.pages[n as usize].bind_group,
                        Source::Particles(_) => continue,
                    };
                    rp.set_bind_group(1, group, &[]);
                    let range = batch.start..batch.start + batch.count;
                    if batch.quads {
                        rp.draw(0..6, range);
                    } else {
                        rp.draw(range, 0..1);
                    }
                }
            }
            let texture = if pass.target == 0 {
                &self.target.texture
            } else {
                &self.textures[&pass.target].texture
            };
            let mut fx = FxRun {
                encoder: &mut encoder,
                blocks: &mut blocks,
                slot: &mut slot,
                size: pass.size,
                time,
            };
            self.effects(&mut fx, pass, (texture, view));
        }
        self.queue.write_buffer(&self.uniforms, 0, &blocks);
        self.queue.submit([encoder.finish()]);
    }

    /// One full-screen effect pass from `from` (and `second`) into `to`
    #[allow(clippy::too_many_arguments)]
    fn fx_pass(
        &self,
        fx: &mut FxRun,
        pipeline: &wgpu::RenderPipeline,
        from: &wgpu::TextureView,
        second: &wgpu::TextureView,
        to: &wgpu::TextureView,
        load: wgpu::LoadOp<wgpu::Color>,
        scissor: Option<[i32; 4]>,
        p: &[f32],
    ) {
        let offset = *fx.slot * SLOT;
        write_block(fx.blocks, offset, fx.size, fx.time, p);
        *fx.slot += 1;
        let group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: None,
            layout: &self.post_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(from),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(second),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Sampler(&self.linear),
                },
            ],
        });
        let mut rp = fx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("effect"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: to,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        if let Some(b) = scissor {
            let (w, h) = ((b[2] - b[0]) as u32, (b[3] - b[1]) as u32);
            rp.set_scissor_rect(b[0] as u32, b[1] as u32, w, h);
        }
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &self.uniform_group, &[offset as u32]);
        rp.set_bind_group(1, &group, &[]);
        rp.draw(0..3, 0..1);
    }

    /// Run a pass's effect chain over its picture, ping-ponging between
    /// the picture and two scratch textures, and leave the result in it
    fn effects(&self, fx: &mut FxRun, pass: &GpuPass, main: (&wgpu::Texture, &wgpu::TextureView)) {
        let Some(scratch) = self.scratch.get(&pass.size) else {
            return;
        };
        let view = |s: Slot| match s {
            Slot::Main => main.1,
            Slot::A => &scratch[0].1,
            Slot::B => &scratch[1].1,
        };
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let builtin = |f: Fx| &self.fx[f as usize];
        let mut cur = Slot::Main;
        for e in &pass.effects {
            match e.kind {
                post::EFFECT_GLOW => {
                    let a = Slot::other(cur, cur);
                    let b = Slot::other(cur, a);
                    let (vc, va, vb) = (view(cur), view(a), view(b));
                    self.fx_pass(fx, builtin(Fx::GlowA), vc, vc, va, clear, None, &e.p);
                    self.fx_pass(fx, builtin(Fx::GlowB), va, vc, vb, clear, None, &e.p);
                    cur = b;
                }
                post::EFFECT_BLUR => {
                    let a = Slot::other(cur, cur);
                    let (vc, va) = (view(cur), view(a));
                    self.fx_pass(fx, builtin(Fx::BlurA), vc, vc, va, clear, None, &e.p);
                    self.fx_pass(fx, builtin(Fx::BlurB), va, va, vc, clear, None, &e.p);
                }
                post::EFFECT_LIGHTING => {
                    let to = Slot::other(cur, cur);
                    self.lighting(fx, pass, &e.p, view(cur), view(to));
                    cur = to;
                }
                post::EFFECT_GRADE | post::EFFECT_CRT | post::EFFECT_SHADER => {
                    let pipeline = match e.kind {
                        post::EFFECT_GRADE => builtin(Fx::Grade),
                        post::EFFECT_CRT => builtin(Fx::Crt),
                        _ => match self.shaders.get(&e.shader) {
                            Some(p) => p,
                            None => continue,
                        },
                    };
                    let to = Slot::other(cur, cur);
                    let vc = view(cur);
                    self.fx_pass(fx, pipeline, vc, vc, view(to), clear, None, &e.p);
                    cur = to;
                }
                _ => {}
            }
        }
        if cur != Slot::Main {
            let from = match cur {
                Slot::A => &scratch[0].0,
                _ => &scratch[1].0,
            };
            fx.encoder.copy_texture_to_texture(
                from.as_image_copy(),
                main.0.as_image_copy(),
                wgpu::Extent3d {
                    width: pass.size.0,
                    height: pass.size.1,
                    depth_or_array_layers: 1,
                },
            );
        }
    }

    /// A render pass into `view` that clears it and draws nothing
    fn clear_pass(fx: &mut FxRun, view: &wgpu::TextureView, color: wgpu::Color) {
        fx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
    }

    /// Triangles from the vertex buffer (or quads) into `view`, after
    /// clearing it
    #[allow(clippy::too_many_arguments)]
    fn triangles_pass(
        &self,
        fx: &mut FxRun,
        view: &wgpu::TextureView,
        clear: wgpu::Color,
        pipeline: &wgpu::RenderPipeline,
        block: u64,
        scissor: Option<[i32; 4]>,
        draws: &[(&wgpu::BindGroup, std::ops::Range<u32>)],
        quads: bool,
    ) {
        let mut rp = fx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("triangles"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(clear),
                    store: wgpu::StoreOp::Store,
                },
            })],
            ..Default::default()
        });
        if draws.is_empty() {
            return;
        }
        if let Some(b) = scissor {
            let (w, h) = ((b[2] - b[0]) as u32, (b[3] - b[1]) as u32);
            rp.set_scissor_rect(b[0] as u32, b[1] as u32, w, h);
        }
        rp.set_pipeline(pipeline);
        rp.set_bind_group(0, &self.uniform_group, &[block as u32]);
        let buffer = if quads {
            &self.quad_buffer
        } else {
            &self.vertex_buffer
        };
        rp.set_vertex_buffer(0, buffer.slice(..));
        for (group, range) in draws {
            rp.set_bind_group(1, *group, &[]);
            if quads {
                rp.draw(0..6, range.clone());
            } else {
                rp.draw(range.clone(), 0..1);
            }
        }
    }

    /// The lighting effect, the same steps as `post::lighting`: normals,
    /// then per light its shadow mask (blurred) and its light added into
    /// the lightmap, then the picture times the lightmap
    fn lighting(
        &self,
        fx: &mut FxRun,
        pass: &GpuPass,
        p: &[f32; 8],
        from: &wgpu::TextureView,
        to: &wgpu::TextureView,
    ) {
        let Some(lt) = self.light_textures.get(&pass.size) else {
            return;
        };
        let flat = wgpu::Color {
            r: 0.5,
            g: 0.5,
            b: 1.0,
            a: 1.0,
        };
        let normals: Vec<_> = self.batches[pass.normal_batches.clone()]
            .iter()
            .filter_map(|b| match b.source {
                Source::Texture(id) => self
                    .textures
                    .get(&id)
                    .map(|t| (&t.bind_group, b.start..b.start + b.count)),
                _ => None,
            })
            .collect();
        let np = &self.normal_pipeline;
        self.triangles_pass(
            fx,
            &lt.normals.1,
            flat,
            np,
            pass.block,
            None,
            &normals,
            true,
        );
        let ambient = wgpu::Color {
            r: p[0] as f64,
            g: p[1] as f64,
            b: p[2] as f64,
            a: 1.0,
        };
        Gpu::clear_pass(fx, &lt.lightmap.1, ambient);
        let (mask, other) = (&lt.masks[0].1, &lt.masks[1].1);
        let load = wgpu::LoadOp::Load;
        for (light, shadows) in &pass.lights {
            let Some(b) = light.bounds(pass.size) else {
                continue;
            };
            let bounds = b.map(|v| v as f32);
            let draws = if shadows.is_empty() {
                Vec::new()
            } else {
                vec![(&self.white, shadows.clone())]
            };
            let white = wgpu::Color::WHITE;
            let crisp = &self.pipelines[0];
            self.triangles_pass(fx, mask, white, crisp, pass.block, Some(b), &draws, false);
            let blur = light.blur();
            if blur > 0 {
                let mut mp = [0.0; 12];
                mp[0] = blur as f32;
                mp[8..].copy_from_slice(&bounds);
                let (ba, bb) = (
                    &self.fx[Fx::MaskBlurA as usize],
                    &self.fx[Fx::MaskBlurB as usize],
                );
                self.fx_pass(fx, ba, mask, mask, other, load, Some(b), &mp);
                self.fx_pass(fx, bb, other, other, mask, load, Some(b), &mp);
            }
            let c = light.color;
            let mut lp = [0.0; 12];
            lp[..7].copy_from_slice(&[
                light.pos.x,
                light.pos.y,
                light.radius,
                light.height,
                c[0],
                c[1],
                c[2],
            ]);
            lp[8..].copy_from_slice(&bounds);
            let lm = &lt.lightmap.1;
            self.fx_pass(
                fx,
                &self.light_pipeline,
                mask,
                &lt.normals.1,
                lm,
                load,
                Some(b),
                &lp,
            );
        }
        let clear = wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT);
        let lit = &self.fx[Fx::Lit as usize];
        self.fx_pass(fx, lit, from, &lt.lightmap.1, to, clear, None, &[]);
    }

    /// Show the last frame in the window, scaled like the software
    /// renderer: `view` is the letterboxed area (x, y, scale)
    #[cfg(feature = "window")]
    pub fn present(&mut self, window: (u32, u32), view: (f32, f32, f32)) -> Result<(), String> {
        let Some(p) = self.presenter.as_mut() else {
            return Ok(());
        };
        if window.0 == 0 || window.1 == 0 {
            return Ok(()); // minimized
        }
        if (p.config.width, p.config.height) != window {
            p.config.width = window.0;
            p.config.height = window.1;
            p.surface.configure(&self.device, &p.config);
        }
        let frame = match p.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(f)
            | wgpu::CurrentSurfaceTexture::Suboptimal(f) => f,
            wgpu::CurrentSurfaceTexture::Outdated | wgpu::CurrentSurfaceTexture::Lost => {
                p.surface.configure(&self.device, &p.config);
                return Ok(());
            }
            _ => return Ok(()), // occluded or timed out: skip this frame
        };
        let out = frame.texture.create_view(&Default::default());
        let mut encoder = self.device.create_command_encoder(&Default::default());
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("present"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &out,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                ..Default::default()
            });
            let (cw, ch) = self.target.size;
            let (x, y, scale) = view;
            // The viewport must stay inside the window
            let (vx, vy) = (x.max(0.0), y.max(0.0));
            let vw = (cw as f32 * scale).min(window.0 as f32 - vx);
            let vh = (ch as f32 * scale).min(window.1 as f32 - vy);
            if vw > 0.0 && vh > 0.0 {
                pass.set_viewport(vx, vy, vw, vh, 0.0, 1.0);
                pass.set_pipeline(&p.pipeline);
                pass.set_bind_group(1, &self.target.blit_group, &[]);
                pass.draw(0..3, 0..1);
            }
        }
        self.queue.submit([encoder.finish()]);
        self.queue.present(frame);
        Ok(())
    }

    /// Copy the last frame into `canvas` (screenshots, pixel reads)
    pub fn read_canvas(&self, canvas: &mut Pixmap) -> Result<(), String> {
        let (w, h) = self.target.size;
        if (canvas.width(), canvas.height()) != (w, h) {
            return Err("canvas size changed".into());
        }
        let row = w * 4;
        let padded =
            row.div_ceil(wgpu::COPY_BYTES_PER_ROW_ALIGNMENT) * wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("readback"),
            size: padded as u64 * h as u64,
            usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
            mapped_at_creation: false,
        });
        let mut encoder = self.device.create_command_encoder(&Default::default());
        encoder.copy_texture_to_buffer(
            self.target.texture.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &buffer,
                layout: wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(padded),
                    rows_per_image: Some(h),
                },
            },
            wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
        );
        self.queue.submit([encoder.finish()]);
        let slice = buffer.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        self.device
            .poll(wgpu::PollType::Wait {
                submission_index: None,
                timeout: None,
            })
            .map_err(|e| e.to_string())?;
        {
            let data = slice.get_mapped_range().map_err(|e| e.to_string())?;
            let out = canvas.data_mut();
            for y in 0..h as usize {
                let src = &data[y * padded as usize..][..row as usize];
                out[y * row as usize..][..row as usize].copy_from_slice(src);
            }
        }
        buffer.unmap();
        Ok(())
    }
}

fn request_adapter(
    instance: &wgpu::Instance,
    surface: Option<&wgpu::Surface<'_>>,
) -> Result<wgpu::Adapter, String> {
    let mut last = String::new();
    for fallback in [false, true] {
        let options = wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            force_fallback_adapter: fallback,
            compatible_surface: surface,
            apply_limit_buckets: false,
        };
        match pollster::block_on(instance.request_adapter(&options)) {
            Ok(a) => return Ok(a),
            Err(e) => last = e.to_string(),
        }
    }
    Err(format!("no GPU adapter: {}", last))
}

fn texture_desc(w: u32, h: u32, usage: wgpu::TextureUsages) -> wgpu::TextureDescriptor<'static> {
    wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: w.max(1),
            height: h.max(1),
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage,
        view_formats: &[],
    }
}

fn make_target(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    sampler: &wgpu::Sampler,
    size: (u32, u32),
) -> Target {
    let texture = device.create_texture(&texture_desc(
        size.0,
        size.1,
        wgpu::TextureUsages::RENDER_ATTACHMENT
            | wgpu::TextureUsages::TEXTURE_BINDING
            | wgpu::TextureUsages::COPY_SRC
            | wgpu::TextureUsages::COPY_DST,
    ));
    let view = texture.create_view(&Default::default());
    let msaa = msaa_view(device, size);
    let blit_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ],
    });
    Target {
        size,
        texture,
        view,
        msaa: Some(msaa),
        blit_group,
    }
}

fn msaa_view(device: &wgpu::Device, size: (u32, u32)) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            sample_count: MSAA,
            ..texture_desc(size.0, size.1, wgpu::TextureUsages::RENDER_ATTACHMENT)
        })
        .create_view(&Default::default())
}

/// A buffer of `slots` uniform blocks, and its bind group
fn uniform_buffer(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    slots: u64,
) -> (wgpu::Buffer, wgpu::BindGroup) {
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("uniforms"),
        size: slots.max(1) * SLOT,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });
    let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: None,
        layout,
        entries: &[wgpu::BindGroupEntry {
            binding: 0,
            resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: &buffer,
                offset: 0,
                size: wgpu::BufferSize::new(BLOCK),
            }),
        }],
    });
    (buffer, group)
}

/// What effect passes record into
struct FxRun<'a> {
    encoder: &'a mut wgpu::CommandEncoder,
    blocks: &'a mut [u8],
    slot: &'a mut u64,
    size: (u32, u32),
    time: f32,
}

/// One uniform block: size, time, then up to 60 numbers
fn write_block(blocks: &mut [u8], offset: u64, size: (u32, u32), time: f32, p: &[f32]) {
    let mut values = [0f32; BLOCK as usize / 4];
    values[0] = size.0 as f32;
    values[1] = size.1 as f32;
    values[2] = time;
    let n = p.len().min(values.len() - 4);
    values[4..4 + n].copy_from_slice(&p[..n]);
    let at = &mut blocks[offset as usize..][..BLOCK as usize];
    for (dst, v) in at.as_chunks_mut::<4>().0.iter_mut().zip(values) {
        dst.copy_from_slice(&v.to_le_bytes());
    }
}

fn fx_pipeline(
    device: &wgpu::Device,
    layout: &wgpu::PipelineLayout,
    module: &wgpu::ShaderModule,
    entry: &str,
) -> wgpu::RenderPipeline {
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(entry),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module,
            entry_point: Some("fx_vs"),
            compilation_options: Default::default(),
            buffers: &[],
        },
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: Default::default(),
        fragment: Some(wgpu::FragmentState {
            module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets: &[Some(FORMAT.into())],
        }),
        multiview_mask: None,
        cache: None,
    })
}

fn vertex_buffer(device: &wgpu::Device, size: u64) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("vertices"),
        size,
        usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn map(t: Transform, x: f32, y: f32) -> [f32; 2] {
    let mut p = Point::from_xy(x, y);
    t.map_point(&mut p);
    [p.x, p.y]
}

fn premultiplied(c: Color) -> [f32; 4] {
    let a = c.a as f32 / 255.0;
    [
        c.r as f32 / 255.0 * a,
        c.g as f32 / 255.0 * a,
        c.b as f32 / 255.0 * a,
        a,
    ]
}

#[cfg(test)]
mod tests {
    use crate::engine::Engine;
    use crate::post::Effect;
    use crate::types::{Color, Rect, Space, Sprite, Vec2};

    /// A GPU engine, or None when the machine has no adapter at all (CI
    /// sets ORST_REQUIRE_GPU so a missing one fails there)
    fn gpu_engine(w: u32, h: u32) -> Option<Engine> {
        match Engine::headless_gpu(w, h) {
            Ok(e) => Some(e),
            Err(err) if std::env::var_os("ORST_REQUIRE_GPU").is_none() => {
                eprintln!("skipped, {}", err);
                None
            }
            Err(err) => panic!("{}", err),
        }
    }

    /// The same frame through either renderer
    fn scene(e: &mut Engine, antialias: bool) {
        e.set_antialias(antialias);
        let mut rgba = Vec::new();
        for i in 0..64u32 {
            let (x, y) = (i % 8, i / 8);
            let on = (x + y) % 2 == 0;
            rgba.extend_from_slice(&[
                if on { 255 } else { 40 },
                (x * 30) as u8,
                (y * 30) as u8,
                255,
            ]);
        }
        let tex = e.texture_from_rgba(8, 8, &rgba).unwrap();
        let mut camera = e.camera();
        camera.center = Vec2::new(64.0, 48.0);
        camera.zoom = 2.0;
        e.set_camera(camera);
        e.begin_frame(Color::rgba(20, 30, 60, 255));
        e.set_space(Space::World);
        for (i, flip) in [false, true].into_iter().enumerate() {
            e.draw_sprite(
                tex,
                &Sprite {
                    pos: Vec2::new(40.0 + i as f32 * 12.0, 30.0),
                    flip_x: flip,
                    tint: Color::rgba(255, 200, 200, 200),
                    ..Sprite::default()
                },
            );
        }
        e.draw_sprite(
            tex,
            &Sprite {
                src: Rect::new(2.0, 2.0, 4.0, 4.0),
                pos: Vec2::new(70.0, 50.0),
                scale: Vec2::new(3.0, 3.0),
                ..Sprite::default()
            },
        );
        e.fill_rect(
            Rect::new(30.0, 55.0, 20.0, 6.0),
            Color::rgba(0, 255, 0, 160),
        );
        e.set_space(Space::Screen);
        e.set_layer(5);
        e.draw_rect(Rect::new(4.0, 4.0, 40.0, 20.0), 2.0, Color::WHITE);
        e.draw_line(
            Vec2::new(4.0, 90.0),
            Vec2::new(120.0, 90.0),
            2.0,
            Color::rgba(255, 255, 0, 255),
        );
        e.fill_circle(Vec2::new(100.0, 20.0), 10.0, Color::rgba(255, 0, 255, 255));
        e.draw_text(1, "Ab Жж", Vec2::new(60.0, 64.0), 14.0, Color::WHITE);
        e.end_frame();
    }

    /// Share of pixels whose channels differ by more than `tolerance`
    fn differ(a: &mut Engine, b: &mut Engine, tolerance: u8) -> f32 {
        let (w, h) = a.size();
        let mut bad = 0;
        for y in 0..h {
            for x in 0..w {
                let (p, q) = (a.pixel(x, y).unwrap(), b.pixel(x, y).unwrap());
                let d = [p.r.abs_diff(q.r), p.g.abs_diff(q.g), p.b.abs_diff(q.b)];
                if d.into_iter().max().unwrap() > tolerance {
                    bad += 1;
                }
            }
        }
        bad as f32 / (w * h) as f32
    }

    #[test]
    fn gpu_frame_matches_software() {
        let Some(mut gpu) = gpu_engine(128, 96) else {
            return;
        };
        let mut soft = Engine::headless(128, 96).unwrap();
        scene(&mut soft, false);
        scene(&mut gpu, false);
        // Crisp: only a few edge pixels of the circle and glyph filtering differ
        let crisp = differ(&mut soft, &mut gpu, 24);
        // Antialiased: the GPU smooths edges with 4x MSAA instead of coverage
        scene(&mut soft, true);
        scene(&mut gpu, true);
        let smooth = differ(&mut soft, &mut gpu, 48);
        eprintln!(
            "crisp {:.3}%, smooth {:.3}%, {}",
            crisp * 100.0,
            smooth * 100.0,
            gpu.renderer_name()
        );
        if crisp > 0.01 || smooth > 0.02 || std::env::var_os("ORST_GPU_SHOTS").is_some() {
            soft.save_screenshot("gpu_test_software.png").ok();
            gpu.save_screenshot("gpu_test_gpu.png").ok();
        }
        assert!(crisp <= 0.01, "{:.2}% pixels differ (crisp)", crisp * 100.0);
        assert!(
            smooth <= 0.02,
            "{:.2}% pixels differ (antialiased)",
            smooth * 100.0
        );
        assert!(gpu.renderer_name().starts_with("gpu: "));
        assert_eq!(gpu.renderer.drawn_sprites, 3);
    }

    #[test]
    fn gpu_follows_reloaded_and_freed_textures() {
        let Some(mut e) = gpu_engine(4, 4) else {
            return;
        };
        e.set_space(Space::Screen);
        let red = e.texture_from_rgba(1, 1, &[255, 0, 0, 255]).unwrap();
        let draw = |e: &mut Engine, id: u32| {
            e.begin_frame(Color::BLACK);
            let s = Sprite {
                scale: Vec2::new(4.0, 4.0),
                ..Sprite::default()
            };
            e.draw_sprite(id, &s);
            e.end_frame();
        };
        draw(&mut e, red);
        assert_eq!(e.pixel(1, 1), Some(Color::rgba(255, 0, 0, 255)));
        // Same id, new image (what hot reload does)
        *e.textures.get_mut(red).unwrap() =
            crate::texture::Texture::from_rgba(1, 1, &[0, 0, 255, 255]).unwrap();
        draw(&mut e, red);
        assert_eq!(e.pixel(1, 1), Some(Color::rgba(0, 0, 255, 255)));
        e.free_texture(red);
        let green = e.texture_from_rgba(1, 1, &[0, 255, 0, 255]).unwrap();
        assert_eq!(green, red, "the id is reused");
        draw(&mut e, green);
        assert_eq!(e.pixel(1, 1), Some(Color::rgba(0, 255, 0, 255)));
    }

    /// Targets and every built-in effect, the same through either renderer
    fn effects_scene(e: &mut Engine, effects: &[Effect]) {
        e.set_antialias(false);
        let t = e.create_target(48, 32).unwrap();
        e.set_effects(t, &[Effect::grade(0.1, 1.2, 0.5, 1.0, Color::WHITE)])
            .unwrap();
        e.set_effects(0, effects).unwrap();
        e.begin_frame(Color::rgba(10, 20, 40, 255));
        e.set_target(t).unwrap();
        e.clear_target(t, Color::rgba(0, 0, 0, 0));
        e.set_space(Space::Screen);
        e.fill_circle(Vec2::new(24.0, 16.0), 10.0, Color::rgba(255, 120, 0, 255));
        e.set_target(0).unwrap();
        e.draw_sprite(
            t,
            &Sprite {
                pos: Vec2::new(10.0, 20.0),
                scale: Vec2::new(2.0, 2.0),
                ..Sprite::default()
            },
        );
        e.fill_rect(Rect::new(80.0, 10.0, 30.0, 6.0), Color::WHITE);
        e.fill_rect(
            Rect::new(70.0, 50.0, 40.0, 30.0),
            Color::rgba(40, 200, 90, 255),
        );
        e.end_frame();
    }

    #[test]
    fn gpu_targets_and_effects_match_software() {
        let chains: [(&str, Vec<Effect>); 5] = [
            ("none", vec![]),
            ("glow", vec![Effect::glow(0.6, 1.5, 6.0)]),
            ("blur", vec![Effect::blur(3.0)]),
            ("crt", vec![Effect::crt(0.4, 0.15, 0.6, 1.5)]),
            (
                "chain",
                vec![
                    Effect::glow(0.5, 1.0, 4.0),
                    Effect::grade(0.0, 1.1, 1.3, 1.2, Color::rgba(255, 230, 200, 255)),
                    Effect::crt(0.3, 0.1, 0.4, 1.0),
                ],
            ),
        ];
        for (name, chain) in chains {
            let Some(mut gpu) = gpu_engine(128, 96) else {
                return;
            };
            let mut soft = Engine::headless(128, 96).unwrap();
            effects_scene(&mut soft, &chain);
            effects_scene(&mut gpu, &chain);
            let bad = differ(&mut soft, &mut gpu, 6);
            if bad > 0.01 || std::env::var_os("ORST_GPU_SHOTS").is_some() {
                soft.save_screenshot(&format!("fx_{name}_software.png"))
                    .ok();
                gpu.save_screenshot(&format!("fx_{name}_gpu.png")).ok();
            }
            assert!(bad <= 0.01, "{name}: {:.2}% pixels differ", bad * 100.0);
        }
    }

    #[test]
    fn gpu_runs_custom_shaders() {
        let Some(mut e) = gpu_engine(8, 8) else {
            return;
        };
        let invert = "fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32> {
            let c = load(vec2<i32>(pixel));
            return vec4<f32>(vec3<f32>(c.a) - c.rgb, c.a) * post.p[0].x;
        }";
        let id = e.create_shader(invert).unwrap();
        e.set_effects(
            0,
            &[Effect::shader(id, [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])],
        )
        .unwrap();
        e.begin_frame(Color::rgba(255, 0, 0, 255));
        e.end_frame();
        assert_eq!(e.pixel(3, 3), Some(Color::rgba(0, 255, 255, 255)));
        let err = e
            .create_shader(
                "fn effect(uv: vec2<f32>, pixel: vec2<f32>) -> vec4<f32> { return nope; }",
            )
            .unwrap_err();
        assert!(err.contains("nope"), "{err}");
        e.free_shader(id);
        e.begin_frame(Color::rgba(255, 0, 0, 255));
        e.end_frame();
        assert_eq!(
            e.pixel(3, 3),
            Some(Color::rgba(255, 0, 0, 255)),
            "skipped once freed"
        );
    }

    /// Two lights, a wall, a lit target and a normal-mapped sprite
    fn lit_scene(e: &mut Engine) {
        use crate::components::{write, Collider, Light, Transform, COLLIDER, LIGHT, TRANSFORM};
        e.set_antialias(false);
        e.set_camera(crate::camera::Camera {
            center: Vec2::new(64.0, 48.0),
            ..Default::default()
        });
        e.set_effects(0, &[Effect::lighting(Color::rgba(60, 60, 90, 255), 1.0)])
            .unwrap();
        e.set_shadow_layers(1);
        let at = |x, y| Transform {
            pos: Vec2::new(x, y),
            ..Transform::default()
        };
        for (x, y, color, soft) in [
            (30.0, 40.0, Color::rgba(255, 220, 160, 255), 3.0),
            (100.0, 70.0, Color::rgba(120, 160, 255, 255), 0.0),
        ] {
            let lamp = e.world.spawn();
            write(&mut e.world, lamp, TRANSFORM, &at(x, y));
            let l = Light {
                color,
                radius: 70.0,
                softness: soft,
                ..Light::default()
            };
            write(&mut e.world, lamp, LIGHT, &l);
        }
        let wall = e.world.spawn();
        write(&mut e.world, wall, TRANSFORM, &at(60.0, 45.0));
        let c = Collider {
            size: Vec2::new(6.0, 30.0),
            ..Collider::default()
        };
        write(&mut e.world, wall, COLLIDER, &c);
        // A sphere-ish normal map: x and y lean out from the middle
        let mut normal = Vec::new();
        for y in 0..16 {
            for x in 0..16 {
                let (nx, ny) = ((x as f32 - 7.5) / 8.0, -(y as f32 - 7.5) / 8.0);
                let nz = (1.0 - (nx * nx + ny * ny).min(1.0)).sqrt();
                let enc = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
                normal.extend_from_slice(&[enc(nx), enc(ny), enc(nz), 255]);
            }
        }
        let rock = e.texture_from_rgba(16, 16, &[200; 16 * 16 * 4]).unwrap();
        let map = e.texture_from_rgba(16, 16, &normal).unwrap();
        e.set_normal_map(rock, map);
        e.begin_frame(Color::BLACK);
        e.fill_rect(
            Rect::new(0.0, 0.0, 128.0, 96.0),
            Color::rgba(220, 220, 220, 255),
        );
        e.draw_sprite(
            rock,
            &Sprite {
                pos: Vec2::new(40.0, 60.0),
                rotation: 0.5,
                scale: Vec2::new(2.0, 2.0),
                ..Sprite::default()
            },
        );
        e.end_frame();
    }

    #[test]
    fn the_interface_stays_out_of_the_screen_effects() {
        use crate::components::{write, UiImage, UiRect, UI_IMAGE, UI_RECT};
        let red = Color::rgba(255, 0, 0, 255);
        let frame = |e: &mut Engine| {
            let panel = e.world.spawn();
            write(&mut e.world, panel, UI_RECT, &UiRect::default());
            let image = UiImage {
                color: red,
                ..Default::default()
            };
            write(&mut e.world, panel, UI_IMAGE, &image);
            // No light anywhere: the world goes black
            e.set_effects(0, &[Effect::lighting(Color::WHITE, 0.0)])
                .unwrap();
            e.begin_frame(Color::WHITE);
            e.draw_entities();
            e.end_frame();
            (e.pixel(100, 50), e.pixel(5, 5))
        };
        let mut software = Engine::headless(200, 100).unwrap();
        let black = Some(Color::BLACK);
        assert_eq!(frame(&mut software), (Some(red), black));
        if let Some(mut gpu) = gpu_engine(200, 100) {
            assert_eq!(frame(&mut gpu), (Some(red), black));
        }
    }

    #[test]
    fn gpu_lighting_matches_software() {
        let Some(mut gpu) = gpu_engine(128, 96) else {
            return;
        };
        let mut soft = Engine::headless(128, 96).unwrap();
        lit_scene(&mut soft);
        lit_scene(&mut gpu);
        let bad = differ(&mut soft, &mut gpu, 8);
        if bad > 0.01 || std::env::var_os("ORST_GPU_SHOTS").is_some() {
            soft.save_screenshot("light_software.png").ok();
            gpu.save_screenshot("light_gpu.png").ok();
        }
        assert!(bad <= 0.01, "{:.2}% pixels differ", bad * 100.0);
    }

    /// GPU emitters: dots under gravity and drag, spinning textured
    /// particles fading out, in a zoomed world
    fn particle_scene(e: &mut Engine, antialias: bool, steps: u32) {
        use crate::particles::EmitterConfig;
        e.set_antialias(antialias);
        e.set_camera(crate::camera::Camera {
            center: Vec2::new(60.0, 40.0),
            zoom: 1.5,
            ..Default::default()
        });
        let mut rgba = Vec::new();
        for i in 0..16u32 {
            let on = (i % 4 + i / 4) % 2 == 0;
            rgba.extend_from_slice(&if on {
                [255, 255, 255, 255]
            } else {
                [255, 120, 0, 255]
            });
        }
        let tex = e.texture_from_rgba(4, 4, &rgba).unwrap();
        let dots = EmitterConfig {
            rate: 120.0,
            max: 2000,
            life_min: 0.5,
            life_max: 1.5,
            speed_min: 20.0,
            speed_max: 60.0,
            spread: std::f32::consts::TAU,
            gravity: Vec2::new(0.0, 40.0),
            drag: 0.8,
            size_start: 5.0,
            size_end: 1.0,
            color_start: Color::rgba(80, 200, 255, 255),
            color_end: Color::rgba(255, 40, 40, 60),
            ..EmitterConfig::default()
        };
        let a = e.create_gpu_emitter(dots, Vec2::new(40.0, 40.0));
        let sprites = EmitterConfig {
            texture: tex,
            spin_min: -3.0,
            spin_max: 3.0,
            size_start: 6.0,
            size_end: 10.0,
            drag: 0.0,
            seed: 9,
            ..dots
        };
        let b = e.create_gpu_emitter(sprites, Vec2::new(80.0, 40.0));
        for i in 0..steps {
            e.update_gpu_emitters(1.0 / 60.0);
            if i % 20 == 0 {
                e.begin_frame(Color::rgba(10, 10, 20, 255));
                e.draw_gpu_emitter(a);
                e.draw_gpu_emitter(b);
                e.end_frame();
            }
        }
        e.begin_frame(Color::rgba(10, 10, 20, 255));
        e.draw_gpu_emitter(a);
        e.draw_gpu_emitter(b);
        e.end_frame();
    }

    #[test]
    fn gpu_particles_match_software() {
        for antialias in [false, true] {
            let Some(mut gpu) = gpu_engine(128, 96) else {
                return;
            };
            let mut soft = Engine::headless(128, 96).unwrap();
            // Drawn every 20 steps on the way, so the GPU copy is updated in
            // pieces and wraps around its ring
            particle_scene(&mut soft, antialias, 150);
            particle_scene(&mut gpu, antialias, 150);
            // Antialiased: 4x MSAA against coverage, as in gpu_frame_matches_software
            let bad = differ(&mut soft, &mut gpu, if antialias { 48 } else { 24 });
            if bad > 0.02 || std::env::var_os("ORST_GPU_SHOTS").is_some() {
                soft.save_screenshot(&format!("particles_{antialias}_software.png"))
                    .ok();
                gpu.save_screenshot(&format!("particles_{antialias}_gpu.png"))
                    .ok();
            }
            assert!(
                bad <= 0.02,
                "antialias {antialias}: {:.2}% pixels differ",
                bad * 100.0
            );
            let lit = (0..96).flat_map(|y| (0..128).map(move |x| (x, y)));
            let shown = lit
                .filter(|&(x, y)| gpu.pixel(x, y).unwrap().b > 40)
                .count();
            assert!(shown > 200, "particles are on screen: {shown}");
        }
    }

    /// Many small textures, crisp and smooth, flipped, rotated and cut
    /// into regions, plus one too big for the atlas
    fn atlas_scene(e: &mut Engine, textures: &[u32], big: u32) {
        e.set_space(Space::Screen);
        e.begin_frame(Color::rgba(20, 20, 30, 255));
        for i in 0..240u32 {
            let tex = textures[i as usize % textures.len()];
            let s = Sprite {
                pos: Vec2::new((i % 16) as f32 * 8.0 + 4.0, (i / 16) as f32 * 6.0 + 4.0),
                src: if i % 3 == 0 {
                    Rect::new(1.0, 1.0, 4.0, 3.0)
                } else {
                    Rect::default()
                },
                origin: Vec2::new(3.0, 3.0),
                rotation: (i % 5) as f32 * 0.4,
                scale: Vec2::new(1.0 + (i % 2) as f32, 1.0),
                flip_x: i % 4 == 1,
                flip_y: i % 7 == 2,
                tint: Color::rgba(255, 255, 255, if i % 6 == 0 { 160 } else { 255 }),
                ..Sprite::default()
            };
            e.draw_sprite(tex, &s);
        }
        let s = Sprite {
            pos: Vec2::new(0.0, 92.0),
            src: Rect::new(0.0, 0.0, 128.0, 4.0),
            ..Sprite::default()
        };
        e.draw_sprite(big, &s);
        e.end_frame();
    }

    fn atlas_textures(e: &mut Engine) -> (Vec<u32>, u32) {
        let textures = (0..24u32)
            .map(|i| {
                let px: Vec<u8> = (0..36u32)
                    .flat_map(|p| {
                        let (x, y) = (p % 6, p / 6);
                        let v = ((x * 40 + i * 9) % 256) as u8;
                        [v, ((y * 50 + i * 20) % 256) as u8, (255 - v), 255]
                    })
                    .collect();
                let t = e.texture_from_rgba(6, 6, &px).unwrap();
                e.set_texture_smooth(t, i % 5 == 0);
                t
            })
            .collect();
        let px: Vec<u8> = (0..600 * 4)
            .flat_map(|p| [(p % 256) as u8, 128, 64, 255])
            .collect();
        let big = e.texture_from_rgba(600, 4, &px).unwrap();
        (textures, big)
    }

    #[test]
    fn gpu_atlas_looks_the_same_and_batches() {
        let (Some(mut gpu), Some(mut plain)) = (gpu_engine(128, 96), gpu_engine(128, 96)) else {
            return;
        };
        plain.gpu.as_mut().unwrap().pack_max = 0;
        let (textures, big) = atlas_textures(&mut gpu);
        atlas_textures(&mut plain);
        atlas_scene(&mut plain, &textures, big);
        atlas_scene(&mut gpu, &textures, big);
        // Sampling the same texels from a page, edges included
        let bad = differ(&mut plain, &mut gpu, 2);
        if bad > 0.001 || std::env::var_os("ORST_GPU_SHOTS").is_some() {
            plain.save_screenshot("atlas_off.png").ok();
            gpu.save_screenshot("atlas_on.png").ok();
        }
        assert!(bad <= 0.001, "{:.2}% pixels differ", bad * 100.0);
        assert!(
            gpu.gpu.as_ref().unwrap().pages.len() == 2,
            "a crisp and a smooth page"
        );
        // Only the crisp sprites now: one page, so one draw call
        gpu.begin_frame(Color::BLACK);
        for i in 0..200u32 {
            let tex = textures[i as usize % 24];
            if i % 24 % 5 != 0 {
                gpu.draw_sprite(
                    tex,
                    &Sprite {
                        pos: Vec2::new(i as f32 * 0.5, 10.0),
                        ..Sprite::default()
                    },
                );
            }
        }
        gpu.end_frame();
        assert_eq!(gpu.gpu.as_ref().unwrap().batch_count(), 1);
    }

    #[test]
    fn gpu_atlas_reuses_freed_space() {
        let Some(mut e) = gpu_engine(64, 64) else {
            return;
        };
        e.set_space(Space::Screen);
        let solid = |e: &mut Engine, c: u8| {
            e.texture_from_rgba(500, 500, &[c, 255 - c, 9, 255].repeat(500 * 500))
                .unwrap()
        };
        let frame = |e: &mut Engine, id: u32| {
            e.begin_frame(Color::BLACK);
            e.draw_sprite(id, &Sprite::default());
            e.end_frame();
            e.pixel(10, 10).unwrap()
        };
        // A page holds 16 of these
        let first: Vec<u32> = (0..16).map(|i| solid(&mut e, i * 10)).collect();
        assert_eq!(frame(&mut e, first[3]), Color::rgba(30, 225, 9, 255));
        for id in &first[..12] {
            e.free_texture(*id);
        }
        let late = solid(&mut e, 200);
        assert_eq!(frame(&mut e, late), Color::rgba(200, 55, 9, 255));
        assert_eq!(frame(&mut e, first[13]), Color::rgba(130, 125, 9, 255));
        assert_eq!(
            e.gpu.as_ref().unwrap().pages.len(),
            1,
            "packed again instead of a new page"
        );
    }

    #[test]
    fn engines_draw_on_a_device_they_are_given() {
        // The editor's device: made outside the engine
        let Some(other) = gpu_engine(8, 8) else {
            return;
        };
        let made = other.gpu.as_ref().unwrap();
        let (device, queue) = (made.device.clone(), made.queue.clone());
        let mut e = Engine::on_device(16, 16, device, queue).unwrap();
        e.resize_canvas(32, 24);
        e.set_space(Space::Screen);
        e.begin_frame(Color::BLACK);
        e.fill_rect(Rect::new(4.0, 4.0, 8.0, 8.0), Color::rgba(10, 200, 30, 255));
        e.end_frame();
        let (texture, _) = e.gpu_canvas().unwrap();
        assert_eq!(
            (texture.width(), texture.height()),
            (32, 24),
            "drawn at the new size"
        );
        assert_eq!(e.pixel(6, 6), Some(Color::rgba(10, 200, 30, 255)));
        assert_eq!(e.pixel(20, 20), Some(Color::BLACK));
    }
}
