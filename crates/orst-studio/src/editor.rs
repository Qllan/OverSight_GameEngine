//! The editor with a project open: menu and toolbar on top, Hierarchy on
//! the left, Inspector on the right, Project and Console below, Scene and
//! Game in the middle

use crate::console::Console;
use crate::egui;
use crate::project::Project;
use crate::scene_view::{self, SceneView};
use orst::components::{write, Name, Transform, NAME, TRANSFORM};
use orst::ecs::Entity;
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
    /// The edited scene; Err when the engine could not start
    pub scene: Result<SceneView, String>,
    pub selected: Option<Entity>,
    /// The Name field while it is being typed
    name_edit: String,
}

impl Editor {
    pub fn new(
        project: Project,
        mut console: Console,
        wgpu: Option<&eframe::egui_wgpu::RenderState>,
    ) -> Editor {
        let scene = SceneView::new(wgpu);
        match &scene {
            Ok(view) => console.info(format!("Scene view on {}", view.engine.renderer_name())),
            Err(e) => console.error(format!("The scene view could not start: {e}")),
        }
        Editor {
            project,
            console,
            center: Center::Scene,
            bottom: Bottom::Project,
            selected_asset: None,
            scene,
            selected: None,
            name_edit: String::new(),
        }
    }

    /// A new entity with a Name and a Transform at the middle of the view
    pub fn create_entity(&mut self) {
        let Ok(view) = &mut self.scene else { return };
        let engine = &mut view.engine;
        let e = engine.world.spawn();
        let t = Transform {
            pos: engine.camera().center,
            ..Default::default()
        };
        write(&mut engine.world, e, TRANSFORM, &t);
        write(&mut engine.world, e, NAME, &Name::new("Entity"));
        self.select(Some(e));
    }

    pub fn delete_selected(&mut self) {
        if let (Ok(view), Some(e)) = (&mut self.scene, self.selected) {
            view.engine.world.despawn(e);
        }
        self.select(None);
    }

    fn select(&mut self, e: Option<Entity>) {
        self.selected = e;
        self.name_edit = match (&self.scene, e) {
            (Ok(view), Some(e)) => orst::components::read::<Name>(&view.engine.world, e, NAME)
                .map(|n| n.as_str().to_string())
                .unwrap_or_default(),
            _ => String::new(),
        };
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
                ui.menu_button("Entity", |ui| {
                    if ui.button("Create empty").clicked() {
                        self.create_entity();
                    }
                    if ui
                        .add_enabled(self.selected.is_some(), egui::Button::new("Delete"))
                        .clicked()
                    {
                        self.delete_selected();
                    }
                });
                ui.separator();
                ui.add_enabled(false, egui::Button::new("Play"));
                ui.add_enabled(false, egui::Button::new("Pause"));
                ui.add_enabled(false, egui::Button::new("Step"));
                ui.separator();
                ui.label(&self.project.file.name);
            });
        });
        egui::Panel::left("hierarchy")
            .resizable(true)
            .default_size(220.0)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.strong("Hierarchy");
                    if ui
                        .small_button("+")
                        .on_hover_text("Create an empty entity")
                        .clicked()
                    {
                        self.create_entity();
                    }
                });
                ui.separator();
                self.hierarchy_ui(ui);
            });
        egui::Panel::right("inspector")
            .resizable(true)
            .default_size(300.0)
            .show(ui, |ui| {
                ui.strong("Inspector");
                ui.separator();
                if self.selected.is_some() {
                    self.entity_ui(ui);
                    return;
                }
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
            match (self.center, &mut self.scene) {
                (Center::Scene, Ok(view)) => {
                    if let Some(picked) = view.ui(ui, self.selected) {
                        self.select(picked);
                    }
                }
                (Center::Scene, Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                }
                (Center::Game, _) => {
                    ui.centered_and_justified(|ui| {
                        ui.weak("Press Play to run the game here");
                    });
                }
            }
        });
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing && self.selected.is_some() && ui.input(|i| i.key_pressed(egui::Key::Delete)) {
            self.delete_selected();
        }
    }

    fn hierarchy_ui(&mut self, ui: &mut egui::Ui) {
        let Ok(view) = &self.scene else {
            ui.weak("No scene");
            return;
        };
        let mut picked = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                for e in view.engine.world.query(&[]) {
                    let label = scene_view::label(&view.engine, e);
                    if ui
                        .selectable_label(self.selected == Some(e), label)
                        .clicked()
                    {
                        picked = Some(e);
                    }
                }
            });
        if let Some(e) = picked {
            self.select(Some(e));
        }
    }

    /// The selected entity: its name and components
    fn entity_ui(&mut self, ui: &mut egui::Ui) {
        let (Ok(view), Some(e)) = (&mut self.scene, self.selected) else {
            return;
        };
        let world = &mut view.engine.world;
        if !world.alive(e) {
            self.selected = None;
            return;
        }
        ui.horizontal(|ui| {
            ui.label("Name");
            if ui.text_edit_singleline(&mut self.name_edit).changed() {
                write(world, e, NAME, &Name::new(&self.name_edit));
            }
        });
        ui.separator();
        for id in 1..=world.component_types() {
            if id != NAME && world.has(e, id) {
                ui.label(world.storage(id).map_or("?", |s| s.name.as_str()));
            }
        }
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
            egui::CollapsingHeader::new(format!("{name}/"))
                .id_salt(&path)
                .default_open(false)
                .show(ui, |ui| dir.ui(ui, &path, selected));
        }
        for name in &self.files {
            let path = format!("{prefix}{name}");
            let picked = selected.as_deref() == Some(path.as_str());
            if ui.selectable_label(picked, name.as_str()).clicked() {
                *selected = Some(path);
            }
        }
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
