//! The editor with a project open: menu and toolbar on top, Hierarchy on
//! the left, Inspector on the right, Project and Console below, Scene and
//! Game in the middle

use crate::console::Console;
use crate::egui;
use crate::project::Project;
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Center {
    Scene,
    Game,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bottom {
    Project,
    Console,
}

pub struct Editor {
    pub project: Project,
    pub console: Console,
    pub center: Center,
    pub bottom: Bottom,
    /// The asset picked in the Project window
    pub selected_asset: Option<String>,
}

impl Editor {
    pub fn new(
        project: Project,
        console: Console,
        _wgpu: Option<&eframe::egui_wgpu::RenderState>,
    ) -> Editor {
        Editor {
            project,
            console,
            center: Center::Scene,
            bottom: Bottom::Project,
            selected_asset: None,
        }
    }

    /// One frame of the editor; `close` asks to go back to the hub
    pub fn ui(
        &mut self,
        ui: &mut egui::Ui,
        _wgpu: Option<&eframe::egui_wgpu::RenderState>,
        close: &mut bool,
    ) {
        egui::Panel::top("menu").show(ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                ui.menu_button("File", |ui| {
                    if ui.button("Refresh assets").clicked() {
                        self.rescan();
                    }
                    if ui.button("Close project").clicked() {
                        *close = true;
                    }
                });
                ui.separator();
                ui.add_enabled(false, egui::Button::new("▶ Play"));
                ui.add_enabled(false, egui::Button::new("⏸ Pause"));
                ui.add_enabled(false, egui::Button::new("⏭ Step"));
                ui.separator();
                ui.label(&self.project.file.name);
            });
        });
        egui::Panel::left("hierarchy")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                ui.strong("Hierarchy");
                ui.separator();
                ui.weak("No scene open");
            });
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.strong("Inspector");
                ui.separator();
                match &self.selected_asset {
                    Some(path) => {
                        ui.label(path);
                        if let Some(uid) = self.project.assets.uid(path) {
                            ui.weak(format!("uid {uid}"));
                        }
                    }
                    None => {
                        ui.weak("Nothing selected");
                    }
                }
            });
        egui::Panel::bottom("bottom")
            .resizable(true)
            .default_size(200.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.selectable_value(&mut self.bottom, Bottom::Project, "Project");
                    ui.selectable_value(&mut self.bottom, Bottom::Console, "Console");
                });
                ui.separator();
                match self.bottom {
                    Bottom::Project => self.project_ui(ui),
                    Bottom::Console => self.console.ui(ui),
                }
            });
        egui::CentralPanel::default_margins().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.selectable_value(&mut self.center, Center::Scene, "Scene");
                ui.selectable_value(&mut self.center, Center::Game, "Game");
            });
            ui.separator();
            ui.centered_and_justified(|ui| {
                ui.weak("The scene view comes in the next step");
            });
        });
    }

    fn rescan(&mut self) {
        match self.project.rescan() {
            Ok(scan) => {
                for path in &scan.created {
                    self.console.info(format!("New asset: {path}"));
                }
                for path in &scan.orphans {
                    self.console.warn(format!("{path}.meta has no asset"));
                }
            }
            Err(e) => self.console.error(e),
        }
    }

    /// The Assets folder as a tree
    fn project_ui(&mut self, ui: &mut egui::Ui) {
        let tree = Tree::of(self.project.assets.paths());
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if tree.files.is_empty() && tree.dirs.is_empty() {
                    ui.weak("Assets is empty: put images, sounds and maps into it");
                }
                tree.ui(ui, "", &mut self.selected_asset);
            });
    }
}

/// Folders and files of asset paths
#[derive(Default, Debug, PartialEq)]
pub struct Tree {
    pub dirs: BTreeMap<String, Tree>,
    pub files: Vec<String>,
}

impl Tree {
    pub fn of<'a>(paths: impl Iterator<Item = &'a str>) -> Tree {
        let mut root = Tree::default();
        for path in paths {
            let mut node = &mut root;
            let mut parts: Vec<&str> = path.split('/').collect();
            let file = parts.pop().unwrap_or_default();
            for dir in parts {
                node = node.dirs.entry(dir.to_string()).or_default();
            }
            node.files.push(file.to_string());
        }
        root
    }

    fn ui(&self, ui: &mut egui::Ui, prefix: &str, selected: &mut Option<String>) {
        for (name, dir) in &self.dirs {
            let path = format!("{prefix}{name}/");
            egui::CollapsingHeader::new(format!("📁 {name}"))
                .id_salt(&path)
                .default_open(false)
                .show(ui, |ui| dir.ui(ui, &path, selected));
        }
        for name in &self.files {
            let path = format!("{prefix}{name}");
            let picked = selected.as_deref() == Some(path.as_str());
            if ui
                .selectable_label(picked, format!("{} {name}", icon(name)))
                .clicked()
            {
                *selected = Some(path);
            }
        }
    }
}

fn icon(file: &str) -> &'static str {
    match file
        .rsplit('.')
        .next()
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "png" | "jpg" | "jpeg" => "🖼",
        "wav" | "ogg" | "mp3" => "🔊",
        "scene" => "🎬",
        "tmj" | "tsj" => "🗺",
        "cs" | "cpp" | "h" | "hpp" => "📜",
        _ => "📄",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn asset_paths_become_a_tree() {
        let t = Tree::of(["a.png", "art/b.png", "art/ui/c.png", "art/d.wav"].into_iter());
        assert_eq!(t.files, ["a.png"]);
        let art = &t.dirs["art"];
        assert_eq!(art.files, ["b.png", "d.wav"]);
        assert_eq!(art.dirs["ui"].files, ["c.png"]);
    }
}
