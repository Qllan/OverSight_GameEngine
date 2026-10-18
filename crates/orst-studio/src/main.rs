//! ORST Studio: the editor of the ORST Game Engine

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod console;
mod editor;
mod export;
mod game_input;
mod gizmo;
mod history;
mod inspector;
mod native;
mod play;
mod player;
mod prefabs;
mod project;
mod scene_view;
mod scripts;
mod sprite_anim;
mod tiles;

pub(crate) use eframe::egui;

use app::{App, Studio};
use std::path::PathBuf;

fn main() -> eframe::Result {
    // A built game: the exe is a copy of ORST.exe beside the game's Data
    if let Some(data) = player::data_dir() {
        if player::run(&data).is_err() {
            std::process::exit(1);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        renderer: eframe::Renderer::Wgpu,
        viewport: egui::ViewportBuilder::default()
            .with_title("ORST Studio")
            .with_inner_size([1440.0, 900.0])
            .with_min_inner_size([800.0, 500.0]),
        ..Default::default()
    };
    // `ORST <project folder>` opens that project right away
    let start = std::env::args_os().nth(1).map(PathBuf::from);
    eframe::run_native(
        "ORST Studio",
        options,
        Box::new(move |cc| {
            let mut studio = Studio::new(None);
            if let Some(root) = start {
                studio.open(&root, cc.wgpu_render_state.as_ref());
            }
            Ok(Box::new(App { studio }))
        }),
    )
}
