//! How long a frame of many sprites takes the CPU: queueing the draw
//! calls, sorting them and building the GPU's vertices. Run with
//! `cargo run --release --example sprite_bench [count]`.

use orst::engine::Engine;
use orst::types::{Color, Rect, Sprite, Vec2};
use std::time::Instant;

fn main() -> Result<(), String> {
    let count: u32 = std::env::args()
        .nth(1)
        .and_then(|a| a.parse().ok())
        .unwrap_or(100_000);
    let mut e = Engine::headless_gpu(1280, 720)?;
    // 16 small textures, as a game's separate sprite files would be
    let textures: Vec<u32> = (0..16u8)
        .map(|i| {
            let px: Vec<u8> = [i * 16, 255 - i * 16, 128, 255]
                .iter()
                .copied()
                .cycle()
                .take(16 * 16 * 4)
                .collect();
            e.texture_from_rgba(16, 16, &px)
        })
        .collect::<Result<_, _>>()?;
    let mut frames = Vec::new();
    let frames_n: u32 = std::env::args()
        .nth(2)
        .and_then(|a| a.parse().ok())
        .unwrap_or(60);
    for frame in 0..frames_n {
        let start = Instant::now();
        e.begin_frame(Color::BLACK);
        for i in 0..count {
            let t = frame as f32 * 0.01 + i as f32 * 0.37;
            let sprite = Sprite {
                src: Rect::default(),
                pos: Vec2::new(
                    (i % 1280) as f32 + t.sin() * 4.0,
                    ((i / 1280) * 9 % 720) as f32,
                ),
                rotation: t,
                tint: Color::WHITE,
                ..Sprite::default()
            };
            e.draw_sprite(textures[(i % 16) as usize], &sprite);
        }
        let queued = start.elapsed();
        e.end_frame();
        frames.push((queued, start.elapsed()));
    }
    e.pixel(0, 0); // waits for the GPU
    frames.sort_by_key(|f| f.1);
    let (q, total) = frames[frames.len() / 2];
    println!(
        "{count} sprites: median frame {:.2} ms (draw calls {:.2} ms), {}",
        total.as_secs_f64() * 1000.0,
        q.as_secs_f64() * 1000.0,
        e.renderer_name()
    );
    Ok(())
}
