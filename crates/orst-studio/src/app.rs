//! The editor window: the project hub until a project is open, then the
//! editor itself

use crate::console::Console;
use crate::editor::Editor;
use crate::egui;
use crate::project::{settings_dir, Language, Project, Recent};
use std::path::{Path, PathBuf};

pub struct Studio {
    pub screen: Screen,
    pub recent: Recent,
    recent_file: PathBuf,
    /// Problems the hub shows (a project that would not open)
    pub hub_error: Option<String>,
    new_project: NewProject,
    open_path: String,
}

pub enum Screen {
    Hub,
    Editor(Box<Editor>),
}

struct NewProject {
    name: String,
    /// The folder the project folder goes into
    parent: String,
    language: Language,
}

impl Studio {
    pub fn new(recent_file: Option<PathBuf>) -> Studio {
        let recent_file = recent_file.unwrap_or_else(|| settings_dir().join("recent.json"));
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from)
            .unwrap_or_default();
        Studio {
            screen: Screen::Hub,
            recent: Recent::load(&recent_file),
            recent_file,
            hub_error: None,
            new_project: NewProject {
                name: "My Game".into(),
                parent: home.join("ORST Projects").display().to_string(),
                language: Language::CSharp,
            },
            open_path: String::new(),
        }
    }

    /// Open a project folder; on failure the hub says why
    pub fn open(&mut self, root: &Path, wgpu: Option<&eframe::egui_wgpu::RenderState>) {
        match Project::open(root) {
            Ok((project, scan)) => {
                let mut console = Console::default();
                console.info(format!("Opened {}", root.display()));
                for path in &scan.created {
                    console.info(format!("New asset: {path}"));
                }
                for path in &scan.renewed {
                    console.warn(format!("{path} was a copy and got its own uid"));
                }
                for path in &scan.broken {
                    console.warn(format!(
                        "{path}.meta could not be read and was written again"
                    ));
                }
                for path in &scan.orphans {
                    console.warn(format!("{path}.meta has no asset"));
                }
                self.remember(root);
                self.hub_error = None;
                self.screen = Screen::Editor(Box::new(Editor::new(project, console, wgpu)));
            }
            Err(e) => self.hub_error = Some(e),
        }
    }

    fn remember(&mut self, root: &Path) {
        self.recent.add(root);
        // Losing the list of recent projects is not worth stopping for
        let _ = self.recent.save(&self.recent_file);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui, wgpu: Option<&eframe::egui_wgpu::RenderState>) {
        let mut close = false;
        match &mut self.screen {
            Screen::Editor(editor) => editor.ui(ui, wgpu, &mut close),
            Screen::Hub => self.hub(ui, wgpu),
        }
        if close {
            self.screen = Screen::Hub;
        }
    }

    fn hub(&mut self, ui: &mut egui::Ui, wgpu: Option<&eframe::egui_wgpu::RenderState>) {
        let mut open: Option<PathBuf> = None;
        egui::CentralPanel::default_margins().show(ui, |ui| {
            ui.heading("ORST Studio");
            ui.label(format!("ORST Game Engine {}", env!("CARGO_PKG_VERSION")));
            ui.add_space(12.0);
            if let Some(e) = &self.hub_error {
                ui.colored_label(ui.visuals().error_fg_color, e);
                ui.add_space(8.0);
            }
            ui.columns(2, |cols| {
                let ui = &mut cols[0];
                ui.strong("Recent projects");
                if self.recent.projects.is_empty() {
                    ui.weak("None yet");
                }
                for root in &self.recent.projects {
                    let name = root
                        .file_name()
                        .map_or(root.display().to_string(), |n| n.to_string_lossy().into());
                    if ui
                        .button(name)
                        .on_hover_text(root.display().to_string())
                        .clicked()
                    {
                        open = Some(root.clone());
                    }
                }
                ui.add_space(12.0);
                ui.strong("Open a project folder");
                ui.horizontal(|ui| {
                    ui.text_edit_singleline(&mut self.open_path);
                    if ui.button("Open").clicked() && !self.open_path.trim().is_empty() {
                        open = Some(PathBuf::from(self.open_path.trim()));
                    }
                });

                let ui = &mut cols[1];
                ui.strong("New project");
                let n = &mut self.new_project;
                egui::Grid::new("new project")
                    .num_columns(2)
                    .show(ui, |ui| {
                        ui.label("Name");
                        ui.text_edit_singleline(&mut n.name);
                        ui.end_row();
                        ui.label("In folder");
                        ui.text_edit_singleline(&mut n.parent);
                        ui.end_row();
                        ui.label("Scripts");
                        ui.horizontal(|ui| {
                            ui.radio_value(&mut n.language, Language::CSharp, "C#");
                            ui.radio_value(&mut n.language, Language::Cpp, "C++");
                        });
                        ui.end_row();
                    });
                if ui.button("Create").clicked() {
                    let root = Path::new(n.parent.trim()).join(n.name.trim());
                    match Project::create(&root, &n.name, n.language) {
                        Ok(_) => open = Some(root),
                        Err(e) => self.hub_error = Some(e),
                    }
                }
            });
        });
        if let Some(root) = open {
            self.open(&root, wgpu);
        }
    }
}

pub struct App {
    pub studio: Studio,
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        self.studio.ui(ui, frame.wgpu_render_state());
    }
}
