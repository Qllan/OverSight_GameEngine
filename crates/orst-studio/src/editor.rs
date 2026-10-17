//! The editor with a project open: menu and toolbar on top, Hierarchy on
//! the left, Inspector on the right, Project and Console below, Scene and
//! Game in the middle

use crate::console::Console;
use crate::egui;
use crate::gizmo::Tool;
use crate::history::History;
use crate::inspector::{self, Action};
use crate::play::{self, Play};
use crate::project::{Language, Project};
use crate::scene_view::{self, AssetPath, SceneEvent, SceneView};
use crate::scripts::{self, BuildResult, ScriptProject, Scripts, Sources};
use orst::components::{
    read, write, Name, SpriteRenderer, Transform, NAME, SPRITE_RENDERER, TRANSFORM,
};
use orst::ecs::Entity;
use orst::scene_file::Assets;
use orst::schema::FieldType;
use std::collections::BTreeMap;
use std::sync::mpsc;
use std::time::{Duration, Instant};

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
    pub history: History,
    /// The open scene's file under Assets; None until it is first saved
    pub scene_path: Option<String>,
    /// Changed since it was opened or saved
    pub dirty: bool,
    /// Unsaved changes stand in the way of this; the user is asked
    ask_save: Option<After>,
    /// The Save scene as window: the name typed, and what comes after
    save_as: Option<(String, Option<After>)>,
    /// The New script window: the name typed
    new_script: Option<String>,
    want_close: bool,
    title: String,
    /// The game is running in the editor; Stop throws its changes away
    pub play: Option<Play>,
    pub scripts: ScriptState,
}

/// C# scripts: built when a .cs file under Assets changes, loaded in
/// place when the build is good (after Stop, while playing)
#[derive(Default)]
pub struct ScriptState {
    pub host: Option<Scripts>,
    project: Option<ScriptProject>,
    /// The sources of the build loaded or running now
    sources: Sources,
    building: Option<mpsc::Receiver<BuildResult>>,
    /// A good build waits for Stop
    pending: bool,
    next_check: Option<Instant>,
}

/// What waits for unsaved changes to be saved or dropped
#[derive(Debug, Clone, PartialEq)]
enum After {
    New,
    Open(String),
    Close,
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
            history: History::default(),
            scene_path: None,
            dirty: false,
            ask_save: None,
            save_as: None,
            new_script: None,
            want_close: false,
            title: String::new(),
            play: None,
            scripts: ScriptState::default(),
        }
        .with_scripts()
        .with_main_scene()
    }

    /// Start .NET and load the scripts before the scene, which may hold
    /// their components; a build out of date is made again first
    fn with_scripts(mut self) -> Editor {
        // .NET is one per process: editor tests must not take it from the
        // scripts test running beside them
        if self.project.file.language != Language::CSharp || cfg!(test) {
            return self;
        }
        let Ok(view) = &mut self.scene else {
            return self;
        };
        let Some(host) = Scripts::host_dir() else {
            self.console
                .warn("C# scripts are off: ORST.Scripting was not found next to ORST.exe");
            return self;
        };
        match Scripts::start(&host, &mut view.engine) {
            Ok(s) => self.scripts.host = Some(s),
            Err(e) => {
                self.console.warn(format!("C# scripts are off: {e}"));
                return self;
            }
        }
        let project = ScriptProject {
            root: self.project.root.clone(),
            host,
        };
        if let Err(e) = project.write_csproj() {
            self.console.error(e);
        }
        let sources = project.sources();
        self.scripts.sources = sources;
        self.scripts.project = Some(project.clone());
        if sources.count > 0 {
            if project.built() != Some(sources) || !project.dll().is_file() {
                self.console.info("Building scripts");
                let result = project.build();
                self.build_done(result);
            } else {
                self.load_scripts();
            }
        }
        self
    }

    /// Report a build; load it when it is good
    fn build_done(&mut self, result: BuildResult) {
        // Files changed during the build make the watcher build again
        self.scripts.sources = result.sources;
        let errors = result
            .messages
            .iter()
            .filter(|(l, _)| *l == crate::console::Level::Error)
            .count();
        for (level, text) in result.messages {
            self.console.log(level, text);
        }
        if !result.ok {
            let s = if errors == 1 { "" } else { "s" };
            self.console.error(format!(
                "Scripts did not build ({errors} error{s}); the last good build stays"
            ));
        } else if self.play.is_some() {
            self.scripts.pending = true;
            self.console.info("Scripts built; they load after Stop");
        } else {
            self.load_scripts();
        }
    }

    /// Load the last good build in place of the scripts before. When a
    /// component's fields changed, undo steps from before cannot be put
    /// back and are dropped.
    fn load_scripts(&mut self) {
        self.scripts.pending = false;
        let (Some(host), Some(project), Ok(view)) = (
            &mut self.scripts.host,
            &self.scripts.project,
            &mut self.scene,
        ) else {
            return;
        };
        let before = layouts(&view.engine.world);
        match host.load(&project.dll()) {
            Ok(n) => self.console.info(format!(
                "Scripts loaded: {n} behaviour{}",
                if n == 1 { "" } else { "s" }
            )),
            Err(e) => self.console.error(format!("Scripts did not load: {e}")),
        }
        if layouts(&view.engine.world) != before && self.history.can_undo() {
            self.history = History::default();
            self.console
                .info("Script fields changed: the undo history was cleared");
        }
    }

    /// Build again when a .cs file changed, at most once a second
    fn watch_scripts(&mut self, ctx: &egui::Context) {
        for (level, text) in scripts::take_log() {
            self.console.log(level, text);
        }
        if let Some(rx) = &self.scripts.building {
            match rx.try_recv() {
                Ok(result) => {
                    self.scripts.building = None;
                    self.build_done(result);
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ctx.request_repaint_after(Duration::from_millis(200))
                }
                Err(mpsc::TryRecvError::Disconnected) => self.scripts.building = None,
            }
        }
        if self.scripts.pending && self.play.is_none() {
            self.load_scripts();
        }
        let now = Instant::now();
        if let Some(t) = self.scripts.next_check.filter(|&t| now < t) {
            ctx.request_repaint_after(t - now);
            return;
        }
        self.scripts.next_check = Some(now + Duration::from_secs(1));
        ctx.request_repaint_after(Duration::from_secs(1));
        let Some(project) = &self.scripts.project else {
            return;
        };
        if self.scripts.building.is_some() {
            return;
        }
        let sources = project.sources();
        if sources == self.scripts.sources {
            return;
        }
        self.scripts.sources = sources;
        if sources.count == 0 {
            if let Some(host) = &mut self.scripts.host {
                host.unload();
            }
            return;
        }
        let (tx, rx) = mpsc::channel();
        let project = project.clone();
        let repaint = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(project.build());
            repaint.request_repaint();
        });
        self.scripts.building = Some(rx);
    }

    fn with_main_scene(mut self) -> Editor {
        if let Some(path) = self.project.file.main_scene.clone() {
            if let Err(e) = self.open_scene(&path) {
                self.console.error(e);
            }
        }
        self
    }

    /// Replace the edited scene with the file `path` under Assets
    pub fn open_scene(&mut self, path: &str) -> Result<(), String> {
        let file = self.project.assets.file(path);
        let text = std::fs::read_to_string(&file).map_err(|e| format!("{path}: {e}"))?;
        let Ok(view) = &mut self.scene else {
            return Err("no scene view".into());
        };
        let engine = &mut view.engine;
        let mut before = Vec::new();
        engine.world.save(&mut before);
        clear_world(&mut engine.world);
        match engine.load_scene(&text, &mut self.project.assets) {
            Ok(loaded) => {
                for w in loaded.warnings {
                    self.console.warn(format!("{path}: {w}"));
                }
                self.console
                    .info(format!("Opened {path}: {} entities", loaded.entities.len()));
            }
            Err(e) => {
                let _ = engine.world.load(&before);
                return Err(format!("{path}: {e}"));
            }
        }
        self.reset_scene(Some(path.to_string()));
        Ok(())
    }

    /// An empty scene, not yet saved anywhere
    pub fn new_scene(&mut self) {
        if let Ok(view) = &mut self.scene {
            clear_world(&mut view.engine.world);
        }
        self.reset_scene(None);
    }

    fn reset_scene(&mut self, path: Option<String>) {
        self.scene_path = path;
        self.history = History::default();
        self.selected = None;
        self.dirty = false;
    }

    /// Write the scene to `path` under Assets. The first scene saved
    /// becomes the one the game starts with.
    pub fn save_scene(&mut self, path: &str) -> Result<(), String> {
        let Ok(view) = &self.scene else {
            return Err("no scene view".into());
        };
        let engine = &view.engine;
        let entities = engine.world.query(&[]);
        let (text, warnings) = engine.save_scene(&entities, &self.project.assets);
        let file = self.project.assets.file(path);
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(|e| format!("{path}: {e}"))?;
        }
        std::fs::write(&file, text).map_err(|e| format!("{path}: {e}"))?;
        for w in warnings {
            self.console.warn(format!("{path}: {w}"));
        }
        self.console.info(format!("Saved {path}"));
        self.scene_path = Some(path.to_string());
        self.dirty = false;
        if self.project.file.main_scene.is_none() {
            self.project.file.main_scene = Some(path.to_string());
            self.project.save()?;
        }
        // The new file gets its .meta
        self.rescan();
        Ok(())
    }

    /// Run the scene as the game, from how it is now
    pub fn start_play(&mut self) {
        if self.play.is_some() {
            return;
        }
        let Ok(view) = &mut self.scene else { return };
        let history = std::mem::take(&mut self.history);
        self.play = Some(Play::start(
            &mut view.engine,
            history,
            self.dirty,
            self.selected,
        ));
        if let Some(host) = &mut self.scripts.host {
            host.begin_play();
        }
        self.center = Center::Game;
        self.console.info("Play");
    }

    /// Stop the game and put the scene back as it was at Play
    pub fn stop_play(&mut self) {
        let (Some(play), Ok(view)) = (self.play.take(), &mut self.scene) else {
            return;
        };
        if let Some(host) = &mut self.scripts.host {
            host.end_play();
        }
        let (restored, history, dirty, selected) = play.stop(&mut view.engine);
        if let Err(e) = restored {
            self.console
                .error(format!("The scene could not be put back: {e}"));
        }
        self.history = history;
        self.dirty = dirty;
        self.selected = selected;
        self.center = Center::Scene;
    }

    /// Save to the scene's own file, or ask for a name the first time
    fn save(&mut self, then: Option<After>) {
        if self.play.is_some() {
            self.console
                .warn("Stop the game to save: changes made while playing are not kept");
            return;
        }
        match self.scene_path.clone() {
            Some(path) => match self.save_scene(&path) {
                Ok(()) => {
                    if let Some(after) = then {
                        self.proceed(after);
                    }
                }
                Err(e) => self.console.error(e),
            },
            None => self.save_as = Some(("New Scene".into(), then)),
        }
    }

    /// Do `after`, asking first when there are unsaved changes
    fn request(&mut self, after: After) {
        if self.dirty {
            self.ask_save = Some(after);
        } else {
            self.proceed(after);
        }
    }

    fn proceed(&mut self, after: After) {
        self.stop_play();
        match after {
            After::New => self.new_scene(),
            After::Open(path) => {
                if let Err(e) = self.open_scene(&path) {
                    self.console.error(e);
                }
            }
            After::Close => self.want_close = true,
        }
    }

    /// A new entity with a Name and a Transform at the middle of the view
    pub fn create_entity(&mut self) {
        let Ok(view) = &mut self.scene else { return };
        let engine = &mut view.engine;
        self.history.record(&engine.world);
        self.history.close();
        let e = engine.world.spawn();
        let t = Transform {
            pos: view.camera.center,
            ..Default::default()
        };
        write(&mut engine.world, e, TRANSFORM, &t);
        write(&mut engine.world, e, NAME, &Name::new("Entity"));
        self.dirty = true;
        self.select(Some(e));
    }

    pub fn delete_selected(&mut self) {
        if let (Ok(view), Some(e)) = (&mut self.scene, self.selected) {
            self.history.record(&view.engine.world);
            self.history.close();
            view.engine.world.despawn(e);
            self.dirty = true;
        }
        self.select(None);
    }

    fn select(&mut self, e: Option<Entity>) {
        self.selected = e;
    }

    pub fn scene_event(&mut self, event: SceneEvent) {
        match event {
            SceneEvent::Pick(e) => self.select(e),
            SceneEvent::Transform(e, t) => {
                let Ok(view) = &mut self.scene else { return };
                let world = &mut view.engine.world;
                if read::<Transform>(world, e, TRANSFORM) != Some(t) {
                    self.history.record(world);
                    write(world, e, TRANSFORM, &t);
                    self.dirty = true;
                }
            }
            SceneEvent::Drop(path, at) => {
                if let Err(e) = self.drop_asset(&path, at) {
                    self.console.error(e);
                }
            }
        }
    }

    /// An image dropped into the scene becomes a sprite there
    pub fn drop_asset(&mut self, path: &str, at: orst::Vec2) -> Result<(), String> {
        if !is_image(path) {
            return Err(format!("{path}: only images can be put into the scene"));
        }
        let Ok(view) = &mut self.scene else {
            return Err("no scene view".into());
        };
        let engine = &mut view.engine;
        let texture = self
            .project
            .assets
            .load(engine, FieldType::Texture, &path.into())?;
        let (w, h) = engine.texture_size(texture).unwrap_or((0, 0));
        self.history.record(&engine.world);
        self.history.close();
        let e = engine.world.spawn();
        let name = path.rsplit('/').next().unwrap_or(path);
        let name = name.rsplit_once('.').map_or(name, |(stem, _)| stem);
        write(&mut engine.world, e, NAME, &Name::new(name));
        let t = Transform {
            pos: at,
            ..Default::default()
        };
        write(&mut engine.world, e, TRANSFORM, &t);
        let s = SpriteRenderer {
            texture,
            origin: orst::Vec2::new(w as f32 / 2.0, h as f32 / 2.0),
            ..Default::default()
        };
        write(&mut engine.world, e, SPRITE_RENDERER, &s);
        self.dirty = true;
        self.select(Some(e));
        Ok(())
    }

    /// Undo (or redo) the last change to the scene
    pub fn undo(&mut self, redo: bool) {
        let Ok(view) = &mut self.scene else { return };
        let world = &mut view.engine.world;
        let done = if redo {
            self.history.redo(world)
        } else {
            self.history.undo(world)
        };
        match done {
            Ok(changed) => self.dirty |= changed,
            Err(e) => self.console.error(format!("Undo failed: {e}")),
        }
        if self.selected.is_some_and(|e| !world.alive(e)) {
            self.selected = None;
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
                    if ui.button("New scene").clicked() {
                        self.request(After::New);
                    }
                    if ui.button("Save scene").on_hover_text("Ctrl+S").clicked() {
                        self.save(None);
                    }
                    if ui.button("Save scene as...").clicked() {
                        let name = self
                            .scene_path
                            .as_deref()
                            .map_or("New Scene", |p| p.trim_end_matches(".scene"))
                            .to_string();
                        self.save_as = Some((name, None));
                    }
                    ui.separator();
                    if ui.button("Refresh assets").clicked() {
                        self.rescan();
                    }
                    if ui.button("Close project").clicked() {
                        self.request(After::Close);
                    }
                });
                ui.menu_button("Edit", |ui| {
                    if ui
                        .add_enabled(
                            self.history.can_undo(),
                            egui::Button::new("Undo").shortcut_text("Ctrl+Z"),
                        )
                        .clicked()
                    {
                        self.undo(false);
                    }
                    if ui
                        .add_enabled(
                            self.history.can_redo(),
                            egui::Button::new("Redo").shortcut_text("Ctrl+Y"),
                        )
                        .clicked()
                    {
                        self.undo(true);
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
                self.play_buttons(ui);
                ui.separator();
                ui.label(&self.project.file.name);
                ui.weak(self.scene_title());
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
                if let (Center::Scene, Ok(view)) = (self.center, &mut self.scene) {
                    ui.separator();
                    for (tool, name, key) in [
                        (Tool::Move, "Move", "W"),
                        (Tool::Rotate, "Rotate", "E"),
                        (Tool::Scale, "Scale", "R"),
                    ] {
                        ui.selectable_value(&mut view.tool, tool, name)
                            .on_hover_text(format!("{key}; hold Ctrl to snap"));
                    }
                }
            });
            ui.separator();
            match (self.center, &mut self.scene) {
                (Center::Scene, Ok(view)) => {
                    let events = view.ui(ui, self.selected);
                    for event in events {
                        self.scene_event(event);
                    }
                }
                (Center::Scene, Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                }
                (Center::Game, Ok(view)) => {
                    let camera = match &self.play {
                        Some(p) => p.camera,
                        None => play::game_camera(&mut view.engine),
                    };
                    view.game_ui(ui, camera, self.play.is_some());
                }
                (Center::Game, Err(e)) => {
                    ui.colored_label(ui.visuals().error_fg_color, e.as_str());
                }
            }
        });
        self.watch_scripts(ui.ctx());
        self.run_game(ui.ctx());
        let save = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::S);
        if ui.input_mut(|i| i.consume_shortcut(&save)) {
            self.save(None);
        }
        let typing = ui.ctx().egui_wants_keyboard_input();
        if !typing {
            let (undo, redo, delete) = ui.input_mut(|i| {
                let z = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Z);
                let y = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::Y);
                let shift_z = egui::KeyboardShortcut::new(
                    egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                    egui::Key::Z,
                );
                let redo = i.consume_shortcut(&y) || i.consume_shortcut(&shift_z);
                (
                    i.consume_shortcut(&z),
                    redo,
                    i.key_pressed(egui::Key::Delete),
                )
            });
            if undo || redo {
                self.undo(redo);
            }
            if delete && self.selected.is_some() {
                self.delete_selected();
            }
        }
        // A drag or a field being typed in is one step of undo
        let busy = ui.input(|i| i.pointer.any_down()) || ui.memory(|m| m.focused().is_some());
        if !busy {
            self.history.close();
        }
        self.dialogs(ui.ctx());
        let title = format!(
            "{} - {} - ORST Studio",
            self.scene_title(),
            self.project.file.name
        );
        if title != self.title {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.title = title;
        }
        *close |= std::mem::take(&mut self.want_close);
    }

    fn play_buttons(&mut self, ui: &mut egui::Ui) {
        let playing = self.play.is_some();
        let label = if playing { "Stop" } else { "Play" };
        let play = egui::Button::new(label).selected(playing);
        if ui.add(play).on_hover_text("Ctrl+P").clicked() {
            if playing {
                self.stop_play();
            } else {
                self.start_play();
            }
        }
        let paused = self.play.as_ref().is_some_and(|p| p.paused);
        if ui
            .add_enabled(playing, egui::Button::new("Pause").selected(paused))
            .clicked()
        {
            if let Some(p) = &mut self.play {
                p.paused = !p.paused;
            }
        }
        if ui
            .add_enabled(playing, egui::Button::new("Step"))
            .on_hover_text("One tick, then pause")
            .clicked()
        {
            if let Some(p) = &mut self.play {
                p.step_once();
            }
        }
        if self.scripts.building.is_some() {
            ui.spinner();
            ui.weak("Building scripts");
        }
        if let Some(p) = &self.play {
            ui.colored_label(
                ui.visuals().warn_fg_color,
                format!("Playing, tick {}", p.ticks),
            );
        }
    }

    /// Run the ticks of this frame while playing; Ctrl+P starts and stops
    fn run_game(&mut self, ctx: &egui::Context) {
        let toggle = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, egui::Key::P);
        if ctx.input_mut(|i| i.consume_shortcut(&toggle)) {
            if self.play.is_some() {
                self.stop_play();
            } else {
                self.start_play();
            }
        }
        if let (Some(p), Ok(view)) = (&mut self.play, &mut self.scene) {
            let dt = ctx.input(|i| i.stable_dt);
            p.advance(&mut view.engine, self.scripts.host.as_mut(), dt);
            if !p.paused {
                ctx.request_repaint();
            }
        }
    }

    /// "Level 1.scene", with a star while there are unsaved changes
    fn scene_title(&self) -> String {
        let name = self
            .scene_path
            .as_deref()
            .map_or("Untitled", |p| p.rsplit('/').next().unwrap_or(p));
        format!("{name}{}", if self.dirty { "*" } else { "" })
    }

    fn dialogs(&mut self, ctx: &egui::Context) {
        if let Some(after) = self.ask_save.clone() {
            let mut answer = None;
            egui::Window::new("Unsaved changes")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label(format!("Save the changes to {}?", self.scene_title()));
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            answer = Some(0);
                        }
                        if ui.button("Don't save").clicked() {
                            answer = Some(1);
                        }
                        if ui.button("Cancel").clicked() {
                            answer = Some(2);
                        }
                    });
                });
            match answer {
                Some(0) => {
                    self.ask_save = None;
                    self.save(Some(after));
                }
                Some(1) => {
                    self.ask_save = None;
                    self.proceed(after);
                }
                Some(_) => self.ask_save = None,
                None => {}
            }
        }
        if let Some(mut name) = self.new_script.take() {
            let mut keep = true;
            let mut chosen = None;
            egui::Window::new("New C# script")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Class name");
                    let r = ui.text_edit_singleline(&mut name);
                    r.request_focus();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() || enter {
                            chosen = Some(name.clone());
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            match chosen.map(|n| self.create_script(&n)) {
                Some(Ok(())) => {}
                Some(Err(e)) => {
                    self.console.error(e);
                    self.new_script = Some(name);
                }
                None if keep => self.new_script = Some(name),
                None => {}
            }
        }
        if let Some((mut name, then)) = self.save_as.take() {
            let mut keep = true;
            let mut chosen = None;
            egui::Window::new("Save scene as")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Name, under Assets (folders with /)");
                    let r = ui.text_edit_singleline(&mut name);
                    r.request_focus();
                    let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() || enter {
                            chosen = Some(scene_file_name(&name));
                        }
                        if ui.button("Cancel").clicked() {
                            keep = false;
                        }
                    });
                });
            match chosen {
                Some(Ok(path)) => match self.save_scene(&path) {
                    Ok(()) => {
                        if let Some(after) = then {
                            self.proceed(after);
                        }
                    }
                    Err(e) => self.console.error(e),
                },
                Some(Err(e)) => {
                    self.console.error(e);
                    self.save_as = Some((name, then));
                }
                None if keep => self.save_as = Some((name, then)),
                None => {}
            }
        }
    }

    /// Assets/Scripts/<class>.cs from the template, opened in the
    /// system's editor for .cs files; the watcher builds it
    pub fn create_script(&mut self, name: &str) -> Result<(), String> {
        let class = crate::project::class_name(name)?;
        let path = format!("{}/{class}.cs", crate::project::SCRIPTS);
        let file = self.project.assets.file(&path);
        if file.exists() {
            return Err(format!("{path} already exists"));
        }
        let io = |e: std::io::Error| format!("{path}: {e}");
        std::fs::create_dir_all(file.parent().unwrap()).map_err(io)?;
        std::fs::write(&file, crate::project::script_source(&class)).map_err(io)?;
        self.rescan();
        self.selected_asset = Some(path.clone());
        self.console.info(format!("Created {path}"));
        if !cfg!(test) {
            if let Err(e) = open_external(&file) {
                self.console.warn(format!("{path}: {e}"));
            }
        }
        Ok(())
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
        if !view.engine.world.alive(e) {
            self.selected = None;
            return;
        }
        let actions = inspector::ui(ui, &view.engine, e);
        if actions.is_empty() {
            return;
        }
        let engine = &mut view.engine;
        self.history.record(&engine.world);
        self.dirty = true;
        for action in actions {
            let world = &mut engine.world;
            match action {
                Action::Set {
                    component,
                    field,
                    value,
                } => {
                    world.set_field(e, component, &field, value);
                }
                Action::SetAsset {
                    component,
                    field,
                    ty,
                    path,
                } => match self.project.assets.load(engine, ty, &path.as_str().into()) {
                    Ok(id) => {
                        let value = orst::schema::Value::U32(id);
                        engine.world.set_field(e, component, &field, value);
                        self.history.close();
                    }
                    Err(err) => self.console.error(err),
                },
                Action::Add(id) => {
                    inspector::add_component(world, e, id);
                    self.history.close();
                }
                Action::Remove(id) => {
                    world.remove(e, id);
                    self.history.close();
                }
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
        if self.project.file.language == Language::CSharp
            && ui
                .small_button("New C# script")
                .on_hover_text("A Behaviour in Assets/Scripts")
                .clicked()
        {
            self.new_script = Some("NewBehaviour".into());
        }
        let tree = Tree::of(self.project.assets.paths());
        let mut opened = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if tree.files.is_empty() && tree.dirs.is_empty() {
                    ui.weak("Assets is empty: put images, sounds and maps into it");
                }
                tree.ui(ui, "", &mut self.selected_asset, &mut opened);
            });
        if let Some(path) = opened {
            if path.ends_with(".scene") {
                self.request(After::Open(path));
            } else if path.ends_with(".cs") {
                let file = self.project.assets.file(&path);
                if let Err(e) = open_external(&file) {
                    self.console.error(format!("{path}: {e}"));
                }
            }
        }
    }
}

/// Open a file in the app the system has for it
fn open_external(file: &std::path::Path) -> Result<(), String> {
    let program = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    std::process::Command::new(program)
        .arg(file)
        .spawn()
        .map(drop)
        .map_err(|e| format!("{program} did not start: {e}"))
}

fn is_image(path: &str) -> bool {
    let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    matches!(ext.as_str(), "png" | "jpg" | "jpeg")
}

fn clear_world(world: &mut orst::ecs::World) {
    for e in world.query(&[]) {
        world.despawn(e);
    }
}

/// What the user typed as a scene name, as a path under Assets
pub fn scene_file_name(name: &str) -> Result<String, String> {
    let name = name.trim().trim_matches('/').replace('\\', "/");
    let bad = name.is_empty()
        || name
            .split('/')
            .any(|p| p.trim().is_empty() || p == "." || p == "..")
        || name.contains(|c: char| ":*?\"<>|".contains(c));
    if bad {
        return Err(format!("\"{name}\" can't be a scene name"));
    }
    Ok(if name.ends_with(".scene") {
        name
    } else {
        format!("{name}.scene")
    })
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

    /// `opened`: the file double-clicked
    fn ui(
        &self,
        ui: &mut egui::Ui,
        prefix: &str,
        selected: &mut Option<String>,
        opened: &mut Option<String>,
    ) {
        for (name, dir) in &self.dirs {
            let path = format!("{prefix}{name}/");
            egui::CollapsingHeader::new(format!("{name}/"))
                .id_salt(&path)
                .default_open(false)
                .show(ui, |ui| dir.ui(ui, &path, selected, opened));
        }
        for name in &self.files {
            let path = format!("{prefix}{name}");
            let picked = selected.as_deref() == Some(path.as_str());
            let r = ui
                .selectable_label(picked, name.as_str())
                .interact(egui::Sense::drag());
            r.dnd_set_drag_payload(AssetPath(path.clone()));
            if r.double_clicked() {
                *opened = Some(path.clone());
            }
            if r.clicked() {
                *selected = Some(path);
            }
        }
    }
}

/// Each component's size and fields, to see that a script changed them
fn layouts(world: &orst::ecs::World) -> Vec<(u32, Vec<orst::schema::Field>)> {
    (1..=world.component_types())
        .filter_map(|id| world.storage(id).map(|s| (s.size, s.fields.clone())))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn editor(name: &str) -> Editor {
        let root = crate::project::temp_dir(name);
        let project = Project::create(&root, "Test", crate::project::Language::CSharp).unwrap();
        Editor::new(project, Console::default(), None)
    }

    fn frame(ctx: &egui::Context, editor: &mut Editor) {
        let input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(1280.0, 720.0),
            )),
            ..Default::default()
        };
        let mut close = false;
        let mut out = ctx.run_ui(input, |ui| editor.ui(ui, None, &mut close));
        out.textures_delta.clear();
    }

    #[test]
    fn every_component_shows_in_the_inspector() {
        let mut ed = editor("inspector");
        ed.create_entity();
        let e = ed.selected.unwrap();
        let world = &mut ed.scene.as_mut().unwrap().engine.world;
        for id in 1..=world.component_types() {
            inspector::add_component(world, e, id);
        }
        let ctx = egui::Context::default();
        for _ in 0..3 {
            frame(&ctx, &mut ed);
        }
        assert!(
            !ed.console
                .lines
                .iter()
                .any(|l| l.level == crate::console::Level::Error),
            "the scene view started"
        );
    }

    #[test]
    fn creating_and_deleting_entities_undo() {
        let mut ed = editor("undo");
        ed.create_entity();
        let e = ed.selected.unwrap();
        ed.delete_selected();
        let alive = |ed: &Editor| ed.scene.as_ref().unwrap().engine.world.alive(e);
        assert!(!alive(&ed));
        ed.undo(false);
        assert!(alive(&ed));
        ed.undo(false);
        assert!(!alive(&ed) && ed.scene.as_ref().unwrap().engine.world.count() == 0);
        ed.undo(true);
        assert!(alive(&ed));
    }

    #[test]
    fn scenes_save_and_open_again() {
        let mut ed = editor("scenes");
        let root = ed.project.root.clone();
        ed.create_entity();
        let e = ed.selected.unwrap();
        let world = &mut ed.scene.as_mut().unwrap().engine.world;
        world.set_field(
            e,
            NAME,
            "text",
            orst::schema::Value::Text(orst::schema::text_bytes("Hero")),
        );
        assert!(ed.dirty);
        ed.request(After::New);
        assert_eq!(
            ed.ask_save,
            Some(After::New),
            "unsaved changes are asked about"
        );
        ed.ask_save = None;

        ed.save_scene("levels/One.scene").unwrap();
        assert!(!ed.dirty);
        assert!(root.join("Assets/levels/One.scene").is_file());
        assert!(
            ed.project.assets.uid("levels/One.scene").is_some(),
            "it got a .meta"
        );
        assert_eq!(
            ed.project.file.main_scene.as_deref(),
            Some("levels/One.scene")
        );

        ed.request(After::New);
        assert_eq!(ed.scene.as_ref().unwrap().engine.world.count(), 0);
        assert_eq!(ed.scene_path, None);

        // The project opens on its main scene
        let (project, _) = Project::open(&root).unwrap();
        let ed = Editor::new(project, Console::default(), None);
        let engine = &ed.scene.as_ref().unwrap().engine;
        assert!(engine.world.find_named("Hero").is_some());
        assert_eq!(ed.scene_path.as_deref(), Some("levels/One.scene"));
        assert!(!ed.dirty && !ed.history.can_undo());
    }

    #[test]
    fn dropped_images_become_sprites_and_gizmo_drags_undo() {
        let mut ed = editor("drop");
        let png = ed.project.assets.file("hero.png");
        let mut pixels = Vec::new();
        for _ in 0..(8 * 4) {
            pixels.extend_from_slice(&[255, 0, 0, 255]);
        }
        image_png(&png, 8, 4, &pixels);
        ed.rescan();
        assert!(ed
            .drop_asset("notes.txt", orst::Vec2::new(0.0, 0.0))
            .is_err());
        ed.drop_asset("hero.png", orst::Vec2::new(30.0, 40.0))
            .unwrap();
        let e = ed.selected.unwrap();
        let engine = &ed.scene.as_ref().unwrap().engine;
        assert_eq!(scene_view::label(engine, e), "hero");
        let s: SpriteRenderer = read(&engine.world, e, SPRITE_RENDERER).unwrap();
        assert_eq!(s.origin, orst::Vec2::new(4.0, 2.0));
        assert_eq!(engine.texture_size(s.texture), Some((8, 4)));

        // A drag is many transforms and one undo step
        for x in [31.0, 35.0, 50.0] {
            let t = Transform {
                pos: orst::Vec2::new(x, 40.0),
                ..Default::default()
            };
            ed.scene_event(SceneEvent::Transform(e, t));
        }
        ed.history.close();
        let x = |ed: &Editor| {
            let world = &ed.scene.as_ref().unwrap().engine.world;
            read::<Transform>(world, e, TRANSFORM).unwrap().pos.x
        };
        assert_eq!(x(&ed), 50.0);
        ed.undo(false);
        assert_eq!(x(&ed), 30.0);

        ed.save_scene("s.scene").unwrap();
        let text = std::fs::read_to_string(ed.project.assets.file("s.scene")).unwrap();
        assert!(text.contains("hero.png") && text.contains("uid"), "{text}");
    }

    fn image_png(path: &std::path::Path, w: u32, h: u32, rgba: &[u8]) {
        image::save_buffer(path, rgba, w, h, image::ColorType::Rgba8).unwrap();
    }

    #[test]
    fn stop_throws_away_what_play_changed() {
        let mut ed = editor("play");
        ed.create_entity();
        let e = ed.selected.unwrap();
        let world = &mut ed.scene.as_mut().unwrap().engine.world;
        let v = orst::components::Velocity {
            linear: orst::Vec2::new(60.0, 0.0),
            ..Default::default()
        };
        write(world, e, orst::components::VELOCITY, &v);
        let ctx = egui::Context::default();
        ed.start_play();
        assert_eq!(ed.center, Center::Game);
        ed.save(None);
        ed.play.as_mut().unwrap().step_once();
        frame(&ctx, &mut ed);
        frame(&ctx, &mut ed);
        assert_eq!(ed.play.as_ref().unwrap().ticks, 1);
        ed.delete_selected();
        ed.stop_play();
        let world = &ed.scene.as_ref().unwrap().engine.world;
        let t: Transform = read(world, e, TRANSFORM).unwrap();
        assert_eq!(t.pos, orst::Vec2::new(0.0, 0.0));
        assert_eq!(ed.selected, Some(e), "the entity deleted in play is back");
        assert!(
            ed.history.can_undo() && ed.dirty,
            "the editor's history is back"
        );
        assert!(ed.scene_path.is_none(), "nothing was saved while playing");
    }

    #[test]
    fn scene_names_become_paths_under_assets() {
        assert_eq!(scene_file_name(" Level 1 ").unwrap(), "Level 1.scene");
        assert_eq!(scene_file_name("maps\\a.scene").unwrap(), "maps/a.scene");
        for bad in ["", "../x", "a//b", "a:b", "/"] {
            assert!(scene_file_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn asset_paths_become_a_tree() {
        let t = Tree::of(["a.png", "art/b.png", "art/ui/c.png", "art/d.wav"].into_iter());
        assert_eq!(t.files, ["a.png"]);
        let art = &t.dirs["art"];
        assert_eq!(art.files, ["b.png", "d.wav"]);
        assert_eq!(art.dirs["ui"].files, ["c.png"]);
    }

    #[test]
    fn new_scripts_come_from_the_template() {
        let mut ed = editor("new_script");
        ed.create_script("coin pickup").unwrap();
        let path = "Scripts/CoinPickup.cs";
        assert_eq!(ed.selected_asset.as_deref(), Some(path));
        assert!(ed.project.assets.uid(path).is_some(), "it has a .meta");
        let text = std::fs::read_to_string(ed.project.assets.file(path)).unwrap();
        assert!(text.contains("class CoinPickup : Behaviour"));
        assert!(
            ed.create_script("CoinPickup").is_err(),
            "never over a script"
        );
    }
}
