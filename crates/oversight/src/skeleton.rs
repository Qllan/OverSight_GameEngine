//! Skeletal animation: a tree of bones moved by keyframed animations, and
//! texture regions (slots) riding on the bones. One picture of parts
//! makes many moves, and code can turn bones itself (aiming a gun arm)
//! or ask where a bone is (where the muzzle is).
//!
//! The file format is JSON. Angles are degrees, clockwise (y points down);
//! a bone's position is relative to its parent.
//!
//! ```json
//! {
//!   "bones": [
//!     {"name": "root"},
//!     {"name": "arm", "parent": "root", "x": 4, "y": -10, "rotation": 0}
//!   ],
//!   "slots": [
//!     {"name": "arm", "bone": "arm", "attachment": "arm",
//!      "attachments": {"arm": {"region": [0, 0, 16, 4], "origin": [0, 2]}}}
//!   ],
//!   "animations": {
//!     "wave": {
//!       "bones": {"arm": {"rotate": [{"time": 0, "value": 0},
//!                                    {"time": 0.5, "value": -60, "curve": "smooth"}]}},
//!       "slots": {"arm": {"color": [{"time": 0, "color": "ffffffff"}]}},
//!       "events": [{"time": 0.25, "name": "swoosh"}]
//!     }
//!   }
//! }
//! ```
//!
//! Bone timelines: `rotate` (value), `translate` and `scale` (x, y). Slot
//! timelines: `attachment` (name, or null to hide) and `color` (RRGGBBAA).
//! Curves: `linear` (the default), `stepped` or `smooth`. Slots are drawn
//! in the order they are listed. Bones may also have `scale_x`,
//! `scale_y`; attachments `x`, `y`, `rotation`, `scale_x`, `scale_y` and
//! `origin` (default: the region's centre).

use std::collections::HashMap;

use serde::Deserialize;

use crate::types::{Color, Rect, Sprite, Vec2};

/// A 2D affine map: x' = a x + c y + tx, y' = b x + d y + ty
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Affine {
    pub a: f32,
    pub b: f32,
    pub c: f32,
    pub d: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Affine {
    pub const IDENTITY: Affine = Affine {
        a: 1.0,
        b: 0.0,
        c: 0.0,
        d: 1.0,
        tx: 0.0,
        ty: 0.0,
    };

    /// Translate, then rotate (radians, clockwise), then scale
    pub fn new(pos: Vec2, rotation: f32, scale: Vec2) -> Affine {
        let (sin, cos) = (libm::sinf(rotation), libm::cosf(rotation));
        Affine {
            a: cos * scale.x,
            b: sin * scale.x,
            c: -sin * scale.y,
            d: cos * scale.y,
            tx: pos.x,
            ty: pos.y,
        }
    }

    pub fn then(&self, inner: &Affine) -> Affine {
        Affine {
            a: self.a * inner.a + self.c * inner.b,
            b: self.b * inner.a + self.d * inner.b,
            c: self.a * inner.c + self.c * inner.d,
            d: self.b * inner.c + self.d * inner.d,
            tx: self.a * inner.tx + self.c * inner.ty + self.tx,
            ty: self.b * inner.tx + self.d * inner.ty + self.ty,
        }
    }

    pub fn apply(&self, p: Vec2) -> Vec2 {
        Vec2::new(
            self.a * p.x + self.c * p.y + self.tx,
            self.b * p.x + self.d * p.y + self.ty,
        )
    }

    /// Position, rotation and scale (shear is dropped; a mirrored map
    /// gets a negative y scale)
    pub fn decompose(&self) -> (Vec2, f32, Vec2) {
        let sx = (self.a * self.a + self.b * self.b).sqrt();
        let rotation = libm::atan2f(self.b, self.a);
        let det = self.a * self.d - self.b * self.c;
        let sy = if sx > 0.0 { det / sx } else { 0.0 };
        (Vec2::new(self.tx, self.ty), rotation, Vec2::new(sx, sy))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
enum Curve {
    #[default]
    Linear,
    Stepped,
    Smooth,
}

#[derive(Debug, Clone, Copy)]
struct Key<T> {
    time: f32,
    value: T,
    curve: Curve,
}

/// The value at `time`: the keys around it, blended by the first one's curve
fn sample<T: Copy>(keys: &[Key<T>], time: f32, mix: impl Fn(T, T, f32) -> T) -> Option<T> {
    let first = keys.first()?;
    if time <= first.time {
        return Some(first.value);
    }
    let i = keys.partition_point(|k| k.time <= time);
    let a = &keys[i - 1];
    let Some(b) = keys.get(i) else {
        return Some(a.value);
    };
    let t = (time - a.time) / (b.time - a.time).max(1e-6);
    let t = match a.curve {
        Curve::Linear => t,
        Curve::Stepped => 0.0,
        Curve::Smooth => t * t * (3.0 - 2.0 * t),
    };
    Some(mix(a.value, b.value, t))
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// Angles blend the short way round
fn lerp_angle(a: f32, b: f32, t: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let d = (b - a).rem_euclid(tau);
    let d = if d > std::f32::consts::PI { d - tau } else { d };
    a + d * t
}

fn lerp_vec(a: Vec2, b: Vec2, t: f32) -> Vec2 {
    Vec2::new(lerp(a.x, b.x, t), lerp(a.y, b.y, t))
}

fn lerp_rgba(a: [f32; 4], b: [f32; 4], t: f32) -> [f32; 4] {
    [0, 1, 2, 3].map(|i| lerp(a[i], b[i], t))
}

/// A bone's pose relative to its parent
#[derive(Debug, Clone, Copy, PartialEq)]
struct Local {
    pos: Vec2,
    rotation: f32,
    scale: Vec2,
}

#[derive(Debug, Clone)]
struct BoneData {
    name: String,
    parent: Option<usize>,
    setup: Local,
}

#[derive(Debug, Clone)]
struct AttachmentData {
    region: Rect,
    local: Affine,
    origin: Vec2,
}

#[derive(Debug, Clone)]
struct SlotData {
    name: String,
    bone: usize,
    attachments: Vec<(String, AttachmentData)>,
    setup_attachment: Option<usize>,
    setup_color: [f32; 4],
}

#[derive(Debug, Clone, Default)]
struct BoneTimeline {
    rotate: Vec<Key<f32>>,
    translate: Vec<Key<Vec2>>,
    scale: Vec<Key<Vec2>>,
}

#[derive(Debug, Clone, Default)]
struct SlotTimeline {
    attachment: Vec<(f32, Option<usize>)>,
    color: Vec<Key<[f32; 4]>>,
}

#[derive(Debug, Clone)]
struct Animation {
    name: String,
    duration: f32,
    bones: Vec<(usize, BoneTimeline)>,
    slots: Vec<(usize, SlotTimeline)>,
    /// (time, event name), by time
    events: Vec<(f32, String)>,
}

/// A skeleton file: bones, slots and animations
#[derive(Debug, Clone)]
pub struct SkeletonData {
    bones: Vec<BoneData>,
    slots: Vec<SlotData>,
    animations: Vec<Animation>,
}

// --- the JSON format ------------------------------------------------------

fn one() -> f32 {
    1.0
}

#[derive(Deserialize)]
struct JBone {
    name: String,
    parent: Option<String>,
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    rotation: f32,
    #[serde(default = "one")]
    scale_x: f32,
    #[serde(default = "one")]
    scale_y: f32,
}

#[derive(Deserialize)]
struct JAttachment {
    region: [f32; 4],
    #[serde(default)]
    x: f32,
    #[serde(default)]
    y: f32,
    #[serde(default)]
    rotation: f32,
    #[serde(default = "one")]
    scale_x: f32,
    #[serde(default = "one")]
    scale_y: f32,
    origin: Option<[f32; 2]>,
}

#[derive(Deserialize)]
struct JSlot {
    name: String,
    bone: String,
    attachment: Option<String>,
    #[serde(default)]
    attachments: HashMap<String, JAttachment>,
    color: Option<String>,
}

#[derive(Deserialize)]
struct JKey {
    time: f32,
    #[serde(default)]
    value: f32,
    #[serde(default)]
    x: Option<f32>,
    #[serde(default)]
    y: Option<f32>,
    curve: Option<String>,
    name: Option<String>,
    color: Option<String>,
}

#[derive(Deserialize, Default)]
struct JBoneTimeline {
    #[serde(default)]
    rotate: Vec<JKey>,
    #[serde(default)]
    translate: Vec<JKey>,
    #[serde(default)]
    scale: Vec<JKey>,
}

#[derive(Deserialize, Default)]
struct JSlotTimeline {
    #[serde(default)]
    attachment: Vec<JKey>,
    #[serde(default)]
    color: Vec<JKey>,
}

#[derive(Deserialize)]
struct JEvent {
    time: f32,
    name: String,
}

#[derive(Deserialize)]
struct JAnimation {
    #[serde(default)]
    bones: HashMap<String, JBoneTimeline>,
    #[serde(default)]
    slots: HashMap<String, JSlotTimeline>,
    #[serde(default)]
    events: Vec<JEvent>,
}

#[derive(Deserialize)]
struct JSkeleton {
    bones: Vec<JBone>,
    #[serde(default)]
    slots: Vec<JSlot>,
    #[serde(default)]
    animations: HashMap<String, JAnimation>,
}

fn parse_color(s: &str) -> Result<[f32; 4], String> {
    let hex = s.trim_start_matches('#');
    let v = u32::from_str_radix(hex, 16).map_err(|_| format!("bad colour {s:?}"))?;
    let v = match hex.len() {
        6 => (v << 8) | 0xff,
        8 => v,
        _ => return Err(format!("bad colour {s:?}")),
    };
    Ok([24, 16, 8, 0].map(|shift| ((v >> shift) & 0xff) as f32 / 255.0))
}

fn curve(k: &JKey) -> Result<Curve, String> {
    match k.curve.as_deref() {
        None | Some("linear") => Ok(Curve::Linear),
        Some("stepped") => Ok(Curve::Stepped),
        Some("smooth") => Ok(Curve::Smooth),
        Some(c) => Err(format!("unknown curve {c:?}")),
    }
}

impl SkeletonData {
    pub fn from_json(text: &str) -> Result<SkeletonData, String> {
        let j: JSkeleton = serde_json::from_str(text).map_err(|e| e.to_string())?;
        if j.bones.is_empty() {
            return Err("a skeleton needs at least one bone".into());
        }
        let mut bones: Vec<BoneData> = Vec::new();
        for b in &j.bones {
            let parent =
                match &b.parent {
                    None => None,
                    Some(p) => Some(bones.iter().position(|x| &x.name == p).ok_or_else(|| {
                        format!("bone {:?}: parent {p:?} must come first", b.name)
                    })?),
                };
            if bones.iter().any(|x| x.name == b.name) {
                return Err(format!("two bones named {:?}", b.name));
            }
            bones.push(BoneData {
                name: b.name.clone(),
                parent,
                setup: Local {
                    pos: Vec2::new(b.x, b.y),
                    rotation: b.rotation.to_radians(),
                    scale: Vec2::new(b.scale_x, b.scale_y),
                },
            });
        }
        let bone = |name: &str| {
            bones
                .iter()
                .position(|b| b.name == name)
                .ok_or_else(|| format!("no bone {name:?}"))
        };
        let mut slots = Vec::new();
        for s in &j.slots {
            // Sorted so the same file gives the same order everywhere
            let mut names: Vec<&String> = s.attachments.keys().collect();
            names.sort();
            let attachments: Vec<(String, AttachmentData)> = names
                .into_iter()
                .map(|n| {
                    let a = &s.attachments[n];
                    let [x, y, w, h] = a.region;
                    let origin = a.origin.unwrap_or([w / 2.0, h / 2.0]);
                    let data = AttachmentData {
                        region: Rect::new(x, y, w, h),
                        local: Affine::new(
                            Vec2::new(a.x, a.y),
                            a.rotation.to_radians(),
                            Vec2::new(a.scale_x, a.scale_y),
                        ),
                        origin: Vec2::new(origin[0], origin[1]),
                    };
                    (n.clone(), data)
                })
                .collect();
            let find = |name: &str| {
                attachments
                    .iter()
                    .position(|(n, _)| n == name)
                    .ok_or_else(|| format!("slot {:?}: no attachment {name:?}", s.name))
            };
            let setup_attachment = s.attachment.as_deref().map(find).transpose()?;
            let setup_color = s.color.as_deref().map(parse_color).transpose()?;
            slots.push(SlotData {
                name: s.name.clone(),
                bone: bone(&s.bone)?,
                setup_attachment,
                setup_color: setup_color.unwrap_or([1.0; 4]),
                attachments,
            });
        }
        let slot = |name: &str| {
            slots
                .iter()
                .position(|s: &SlotData| s.name == name)
                .ok_or_else(|| format!("no slot {name:?}"))
        };
        let mut names: Vec<&String> = j.animations.keys().collect();
        names.sort();
        let mut animations = Vec::new();
        for name in names {
            let a = &j.animations[name];
            let mut duration: f32 = 0.0;
            let mut seen = |t: f32| duration = duration.max(t);
            let mut tracks = Vec::new();
            let mut bone_names: Vec<&String> = a.bones.keys().collect();
            bone_names.sort();
            for b in bone_names {
                let t = &a.bones[b];
                let mut tl = BoneTimeline::default();
                for k in &t.rotate {
                    seen(k.time);
                    tl.rotate.push(Key {
                        time: k.time,
                        value: k.value.to_radians(),
                        curve: curve(k)?,
                    });
                }
                for (src, dst, default) in [
                    (&t.translate, &mut tl.translate, 0.0),
                    (&t.scale, &mut tl.scale, 1.0),
                ] {
                    for k in src {
                        seen(k.time);
                        dst.push(Key {
                            time: k.time,
                            value: Vec2::new(k.x.unwrap_or(default), k.y.unwrap_or(default)),
                            curve: curve(k)?,
                        });
                    }
                }
                tracks.push((bone(b)?, tl));
            }
            let mut slot_tracks = Vec::new();
            let mut slot_names: Vec<&String> = a.slots.keys().collect();
            slot_names.sort();
            for s in slot_names {
                let t = &a.slots[s];
                let index = slot(s)?;
                let mut tl = SlotTimeline::default();
                for k in &t.attachment {
                    seen(k.time);
                    let which = match &k.name {
                        None => None,
                        Some(n) => Some(
                            slots[index]
                                .attachments
                                .iter()
                                .position(|(x, _)| x == n)
                                .ok_or_else(|| format!("slot {s:?}: no attachment {n:?}"))?,
                        ),
                    };
                    tl.attachment.push((k.time, which));
                }
                for k in &t.color {
                    seen(k.time);
                    let c = k.color.as_deref().ok_or("a colour key needs a colour")?;
                    tl.color.push(Key {
                        time: k.time,
                        value: parse_color(c)?,
                        curve: curve(k)?,
                    });
                }
                slot_tracks.push((index, tl));
            }
            let mut events: Vec<(f32, String)> =
                a.events.iter().map(|e| (e.time, e.name.clone())).collect();
            for e in &events {
                seen(e.0);
            }
            events.sort_by(|x, y| x.0.total_cmp(&y.0));
            let sort = |k: &mut Vec<Key<f32>>| k.sort_by(|x, y| x.time.total_cmp(&y.time));
            for (_, tl) in &mut tracks {
                sort(&mut tl.rotate);
                tl.translate.sort_by(|x, y| x.time.total_cmp(&y.time));
                tl.scale.sort_by(|x, y| x.time.total_cmp(&y.time));
            }
            for (_, tl) in &mut slot_tracks {
                tl.attachment.sort_by(|x, y| x.0.total_cmp(&y.0));
                tl.color.sort_by(|x, y| x.time.total_cmp(&y.time));
            }
            animations.push(Animation {
                name: name.clone(),
                duration,
                bones: tracks,
                slots: slot_tracks,
                events,
            });
        }
        Ok(SkeletonData {
            bones,
            slots,
            animations,
        })
    }

    pub fn bone(&self, name: &str) -> Option<usize> {
        self.bones.iter().position(|b| b.name == name)
    }

    pub fn animation(&self, name: &str) -> Option<usize> {
        self.animations.iter().position(|a| a.name == name)
    }

    pub fn duration(&self, animation: usize) -> f32 {
        self.animations.get(animation).map_or(0.0, |a| a.duration)
    }
}

#[derive(Debug, Clone, Copy)]
struct Track {
    animation: usize,
    time: f32,
    looped: bool,
}

/// One animated skeleton
#[derive(Debug, Clone)]
pub struct Skeleton {
    pub data: std::sync::Arc<SkeletonData>,
    /// Where the skeleton stands: its root bone's parent
    pub root: Affine,
    pub tint: Color,
    pub speed: f32,
    track: Option<Track>,
    /// The animation being faded out, and the fade's progress
    from: Option<(Track, f32, f32)>,
    pose: Vec<Local>,
    world: Vec<Affine>,
    attachments: Vec<Option<usize>>,
    colors: Vec<[f32; 4]>,
    /// Bone rotations set by code: (rotation relative to the parent,
    /// weight)
    overrides: Vec<Option<(f32, f32)>>,
    /// Events passed in the last update
    fired: Vec<String>,
}

impl Skeleton {
    pub fn new(data: std::sync::Arc<SkeletonData>) -> Skeleton {
        let n = data.bones.len();
        let mut s = Skeleton {
            root: Affine::IDENTITY,
            tint: Color::WHITE,
            speed: 1.0,
            track: None,
            from: None,
            pose: data.bones.iter().map(|b| b.setup).collect(),
            world: vec![Affine::IDENTITY; n],
            attachments: data.slots.iter().map(|s| s.setup_attachment).collect(),
            colors: data.slots.iter().map(|s| s.setup_color).collect(),
            overrides: vec![None; n],
            fired: Vec::new(),
            data,
        };
        s.pose_and_place();
        s
    }

    /// Start an animation, fading from the current one over `mix`
    /// seconds; playing the one already on keeps it going
    pub fn play(&mut self, name: &str, looped: bool, mix: f32) -> Result<(), String> {
        let animation = self
            .data
            .animation(name)
            .ok_or_else(|| format!("no animation {name:?}"))?;
        if let Some(t) = &mut self.track {
            if t.animation == animation {
                t.looped = looped;
                return Ok(());
            }
        }
        let new = Track {
            animation,
            time: 0.0,
            looped,
        };
        self.from = match self.track.replace(new) {
            Some(old) if mix > 0.0 => Some((old, 0.0, mix)),
            _ => None,
        };
        self.pose_and_place();
        Ok(())
    }

    pub fn stop(&mut self) {
        self.track = None;
        self.from = None;
    }

    /// The current animation's name, if any
    pub fn current(&self) -> Option<&str> {
        let t = self.track?;
        Some(&self.data.animations[t.animation].name)
    }

    /// A non-looping animation reached its end
    pub fn done(&self) -> bool {
        self.track
            .is_some_and(|t| !t.looped && t.time >= self.data.duration(t.animation))
    }

    pub fn fired(&self, event: &str) -> bool {
        self.fired.iter().any(|e| e == event)
    }

    pub fn fired_events(&self) -> &[String] {
        &self.fired
    }

    /// Turn a bone by code: its rotation relative to its parent, blended
    /// over the animation by `weight` (0 gives the bone back)
    pub fn set_override(&mut self, bone: usize, rotation: f32, weight: f32) {
        if let Some(o) = self.overrides.get_mut(bone) {
            *o = (weight > 0.0).then_some((rotation, weight.min(1.0)));
        }
        self.pose_and_place();
    }

    /// Show another attachment in a slot (None hides it)
    pub fn set_attachment(&mut self, slot: &str, attachment: Option<&str>) -> Result<(), String> {
        let s = self
            .data
            .slots
            .iter()
            .position(|x| x.name == slot)
            .ok_or_else(|| format!("no slot {slot:?}"))?;
        let which = match attachment {
            None => None,
            Some(n) => Some(
                self.data.slots[s]
                    .attachments
                    .iter()
                    .position(|(x, _)| x == n)
                    .ok_or_else(|| format!("slot {slot:?}: no attachment {n:?}"))?,
            ),
        };
        self.attachments[s] = which;
        Ok(())
    }

    /// Advance by `dt` seconds (times `speed`)
    pub fn update(&mut self, dt: f32) {
        self.fired.clear();
        let dt = dt * self.speed;
        if let Some(mut t) = self.track {
            let duration = self.data.duration(t.animation);
            let before = t.time;
            t.time += dt;
            let anim = &self.data.animations[t.animation];
            let fire = |from: f32, to: f32, fired: &mut Vec<String>| {
                for (time, name) in &anim.events {
                    if *time > from && *time <= to || (from == 0.0 && *time == 0.0 && to > 0.0) {
                        fired.push(name.clone());
                    }
                }
            };
            if t.looped && duration > 0.0 && t.time >= duration {
                fire(before, duration, &mut self.fired);
                t.time = t.time.rem_euclid(duration);
                fire(0.0, t.time, &mut self.fired);
            } else {
                let end = if t.looped {
                    t.time
                } else {
                    t.time.min(duration)
                };
                fire(before, end, &mut self.fired);
                if !t.looped {
                    t.time = t.time.min(duration);
                }
            }
            self.track = Some(t);
        }
        if let Some((mut old, done, mix)) = self.from {
            let duration = self.data.duration(old.animation);
            old.time += dt;
            if old.looped && duration > 0.0 {
                old.time = old.time.rem_euclid(duration);
            }
            let done = done + dt;
            self.from = (done < mix).then_some((old, done, mix));
        }
        self.pose_and_place();
    }

    /// Pose the bones from the animations and overrides, then work out
    /// where every bone is
    fn pose_and_place(&mut self) {
        let data = self.data.clone();
        for (p, b) in self.pose.iter_mut().zip(&data.bones) {
            *p = b.setup;
        }
        if let Some((old, done, mix)) = self.from {
            self.apply(&data, old, 1.0);
            if let Some(t) = self.track {
                self.apply(&data, t, (done / mix).min(1.0));
            }
        } else if let Some(t) = self.track {
            self.apply(&data, t, 1.0);
        }
        for (i, o) in self.overrides.iter().enumerate() {
            if let Some((rotation, weight)) = o {
                let p = &mut self.pose[i];
                p.rotation = lerp_angle(p.rotation, *rotation, *weight);
            }
        }
        for i in 0..data.bones.len() {
            let p = self.pose[i];
            let local = Affine::new(p.pos, p.rotation, p.scale);
            let parent = data.bones[i].parent.map_or(self.root, |j| self.world[j]);
            self.world[i] = parent.then(&local);
        }
    }

    /// Blend an animation's pose over the current one by `alpha`
    fn apply(&mut self, data: &SkeletonData, track: Track, alpha: f32) {
        let anim = &data.animations[track.animation];
        let time = track.time;
        for (bone, tl) in &anim.bones {
            let setup = data.bones[*bone].setup;
            let p = &mut self.pose[*bone];
            if let Some(r) = sample(&tl.rotate, time, lerp_angle) {
                p.rotation = lerp_angle(p.rotation, setup.rotation + r, alpha);
            }
            if let Some(v) = sample(&tl.translate, time, lerp_vec) {
                let to = Vec2::new(setup.pos.x + v.x, setup.pos.y + v.y);
                p.pos = lerp_vec(p.pos, to, alpha);
            }
            if let Some(v) = sample(&tl.scale, time, lerp_vec) {
                let to = Vec2::new(setup.scale.x * v.x, setup.scale.y * v.y);
                p.scale = lerp_vec(p.scale, to, alpha);
            }
        }
        for (slot, tl) in &anim.slots {
            if alpha >= 0.5 {
                let i = tl.attachment.partition_point(|k| k.0 <= time);
                if i > 0 {
                    self.attachments[*slot] = tl.attachment[i - 1].1;
                }
            }
            if let Some(c) = sample(&tl.color, time, lerp_rgba) {
                let cur = self.colors[*slot];
                self.colors[*slot] = lerp_rgba(cur, c, alpha);
            }
        }
    }

    /// Where a bone is in the world: position, rotation, scale
    pub fn bone_world(&self, bone: usize) -> Option<(Vec2, f32, Vec2)> {
        self.world.get(bone).map(Affine::decompose)
    }

    /// A point in a bone's own space, in the world (a muzzle's tip)
    pub fn bone_point(&self, bone: usize, local: Vec2) -> Option<Vec2> {
        self.world.get(bone).map(|w| w.apply(local))
    }

    /// Place the skeleton; poses are worked out again
    pub fn set_root(&mut self, pos: Vec2, rotation: f32, scale: Vec2) {
        self.root = Affine::new(pos, rotation, scale);
        self.pose_and_place();
    }

    /// The sprites to draw, back to front
    pub fn sprites(&self, layer: i32, depth: f32) -> Vec<Sprite> {
        let mut out = Vec::new();
        for (i, slot) in self.data.slots.iter().enumerate() {
            let Some(a) = self.attachments[i] else {
                continue;
            };
            let att = &slot.attachments[a].1;
            let m = self.world[slot.bone].then(&att.local);
            let (pos, rotation, scale) = m.decompose();
            let c = self.colors[i];
            let tint = Color::rgba(
                (c[0] * self.tint.r as f32).round() as u8,
                (c[1] * self.tint.g as f32).round() as u8,
                (c[2] * self.tint.b as f32).round() as u8,
                (c[3] * self.tint.a as f32).round() as u8,
            );
            out.push(Sprite {
                src: att.region,
                pos,
                origin: att.origin,
                scale,
                rotation,
                tint,
                layer,
                depth,
                ..Sprite::default()
            });
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    const ARM: &str = r#"{
        "bones": [
            {"name": "root"},
            {"name": "arm", "parent": "root", "x": 10, "y": 0},
            {"name": "hand", "parent": "arm", "x": 20}
        ],
        "slots": [
            {"name": "arm", "bone": "arm", "attachment": "arm",
             "attachments": {"arm": {"region": [0, 0, 20, 4], "origin": [0, 2]},
                             "claw": {"region": [0, 4, 8, 8]}}}
        ],
        "animations": {
            "wave": {
                "bones": {"arm": {"rotate": [{"time": 0, "value": 0}, {"time": 1, "value": 90}]}},
                "slots": {"arm": {"color": [{"time": 0, "color": "ffffffff"}, {"time": 1, "color": "ff000080"}],
                                  "attachment": [{"time": 0.75, "name": "claw"}]}},
                "events": [{"time": 0.5, "name": "half"}]
            },
            "rest": {
                "bones": {"arm": {"rotate": [{"time": 0, "value": 0}]}}
            }
        }
    }"#;

    fn close(a: Vec2, b: Vec2) -> bool {
        (a.x - b.x).abs() < 1e-3 && (a.y - b.y).abs() < 1e-3
    }

    fn arm() -> Skeleton {
        Skeleton::new(Arc::new(SkeletonData::from_json(ARM).unwrap()))
    }

    #[test]
    fn bones_follow_their_parents() {
        let mut s = arm();
        let hand = s.data.bone("hand").unwrap();
        assert!(close(s.bone_world(hand).unwrap().0, Vec2::new(30.0, 0.0)));
        s.play("wave", false, 0.0).unwrap();
        s.update(0.5);
        // The arm turned 45 degrees clockwise: the hand went down-right
        let p = s.bone_world(hand).unwrap().0;
        let r = 20.0 / std::f32::consts::SQRT_2;
        assert!(close(p, Vec2::new(10.0 + r, r)), "{p:?}");
        assert!(s.fired("half") && !s.done());
        s.update(0.6);
        assert!(s.done() && !s.fired("half"));
        let p = s.bone_world(hand).unwrap().0;
        assert!(close(p, Vec2::new(10.0, 20.0)), "held at the end: {p:?}");
        // Placed and mirrored
        s.set_root(Vec2::new(100.0, 50.0), 0.0, Vec2::new(-1.0, 1.0));
        assert!(close(s.bone_world(hand).unwrap().0, Vec2::new(90.0, 70.0)));
    }

    #[test]
    fn slots_switch_attachments_and_colours() {
        let mut s = arm();
        let first = s.sprites(3, 0.0);
        assert_eq!(first.len(), 1);
        assert_eq!(first[0].src, Rect::new(0.0, 0.0, 20.0, 4.0));
        assert_eq!(first[0].origin, Vec2::new(0.0, 2.0));
        assert_eq!(first[0].layer, 3);
        s.play("wave", true, 0.0).unwrap();
        s.update(0.8);
        let now = s.sprites(0, 0.0);
        assert_eq!(now[0].src, Rect::new(0.0, 4.0, 8.0, 8.0), "the claw");
        assert_eq!(now[0].origin, Vec2::new(4.0, 4.0), "the centre by default");
        assert!(now[0].tint.g < 80 && now[0].tint.a < 170);
        s.set_attachment("arm", None).unwrap();
        assert!(s.sprites(0, 0.0).is_empty());
        assert!(s.set_attachment("arm", Some("nope")).is_err());
    }

    #[test]
    fn loops_fire_events_every_round_and_mixes_blend() {
        let mut s = arm();
        s.play("wave", true, 0.0).unwrap();
        let mut halves = 0;
        for _ in 0..40 {
            s.update(0.1);
            halves += s.fired("half") as u32;
        }
        assert_eq!(halves, 4, "once per second for 4 seconds");
        let arm = s.data.bone("arm").unwrap();
        s.play("wave", false, 0.0).unwrap();
        s.update(2.0);
        s.play("rest", true, 1.0).unwrap();
        let start = s.bone_world(arm).unwrap().1;
        s.update(0.5);
        let mid = s.bone_world(arm).unwrap().1;
        s.update(0.6);
        let end = s.bone_world(arm).unwrap().1;
        assert!(start > mid && mid > end.abs(), "{start} {mid} {end}");
        assert!(end.abs() < 1e-4);
        assert!(s.play("dance", true, 0.0).is_err());
    }

    #[test]
    fn code_can_turn_a_bone() {
        let mut s = arm();
        let (arm, hand) = (s.data.bone("arm").unwrap(), s.data.bone("hand").unwrap());
        s.set_override(arm, std::f32::consts::FRAC_PI_2, 1.0);
        assert!(close(s.bone_world(hand).unwrap().0, Vec2::new(10.0, 20.0)));
        assert!(close(
            s.bone_point(hand, Vec2::new(5.0, 0.0)).unwrap(),
            Vec2::new(10.0, 25.0)
        ));
        s.set_override(arm, 0.0, 0.0);
        assert!(close(s.bone_world(hand).unwrap().0, Vec2::new(30.0, 0.0)));
    }

    #[test]
    fn bad_files_say_what_is_wrong() {
        let err = |j: &str| SkeletonData::from_json(j).unwrap_err();
        assert!(err(r#"{"bones": []}"#).contains("one bone"));
        assert!(err(r#"{"bones": [{"name": "a", "parent": "b"}]}"#).contains("\"b\""));
        assert!(
            err(r#"{"bones": [{"name": "a"}], "slots": [{"name": "s", "bone": "x"}]}"#)
                .contains("no bone")
        );
        assert!(err(r#"{"bones": [{"name": "a"}], "animations": {"x": {"bones": {"a": {"rotate": [{"time": 0, "curve": "wobbly"}]}}}}}"#)
            .contains("wobbly"));
    }

    #[test]
    fn decompose_round_trips() {
        let m = Affine::new(Vec2::new(3.0, 4.0), 0.7, Vec2::new(2.0, -0.5));
        let (pos, rot, scale) = m.decompose();
        let back = Affine::new(pos, rot, scale);
        for (a, b) in [(m.a, back.a), (m.b, back.b), (m.c, back.c), (m.d, back.d)] {
            assert!((a - b).abs() < 1e-5);
        }
    }
}
