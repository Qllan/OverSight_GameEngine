//! The Console window: what the engine, the editor and scripts report

use crate::egui;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub level: Level,
    pub text: String,
}

#[derive(Default)]
pub struct Console {
    pub lines: Vec<Line>,
    /// Show only warnings and errors
    pub problems_only: bool,
}

impl Console {
    /// Lines kept; older ones go first
    const MAX: usize = 2000;

    pub fn log(&mut self, level: Level, text: impl Into<String>) {
        self.lines.push(Line {
            level,
            text: text.into(),
        });
        if self.lines.len() > Self::MAX {
            self.lines.drain(..self.lines.len() - Self::MAX);
        }
    }

    pub fn info(&mut self, text: impl Into<String>) {
        self.log(Level::Info, text);
    }

    pub fn warn(&mut self, text: impl Into<String>) {
        self.log(Level::Warning, text);
    }

    pub fn error(&mut self, text: impl Into<String>) {
        self.log(Level::Error, text);
    }

    pub fn ui(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            if ui.button("Clear").clicked() {
                self.lines.clear();
            }
            ui.checkbox(&mut self.problems_only, "Problems only");
        });
        ui.separator();
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .stick_to_bottom(true)
            .show(ui, |ui| {
                for line in &self.lines {
                    let color = match line.level {
                        Level::Info if self.problems_only => continue,
                        Level::Info => ui.visuals().text_color(),
                        Level::Warning => ui.visuals().warn_fg_color,
                        Level::Error => ui.visuals().error_fg_color,
                    };
                    ui.label(egui::RichText::new(&line.text).color(color).monospace());
                }
            });
    }
}
