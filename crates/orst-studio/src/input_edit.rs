//! The Input tab: the game's actions and what they are bound to by
//! default, kept in Assets/Input.actions in the engine's bindings text
//! (`jump = key:Space pad:South`). Play and built games load it before the
//! scripts start, then the bindings the player changed over it.

use crate::egui;
use crate::game_input::key_of;
use orst::actions::Binding;
use orst::asset_db::AssetDb;

/// The file under Assets
pub const FILE: &str = "Input.actions";

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Row {
    pub name: String,
    /// Bindings as text, split by spaces
    pub bindings: String,
}

impl Row {
    /// The words that are not bindings
    pub fn bad(&self) -> Vec<&str> {
        self.bindings
            .split_whitespace()
            .filter(|w| Binding::parse(w).is_none())
            .collect()
    }
}

#[derive(Default)]
pub struct InputEdit {
    pub rows: Vec<Row>,
    loaded: bool,
    /// The row whose next key press is added to it
    listening: Option<usize>,
}

/// The rows of a bindings text (comments and blank lines dropped)
pub fn parse(text: &str) -> Vec<Row> {
    text.lines()
        .filter_map(|line| {
            let line = line.split('#').next()?.trim();
            let (name, rest) = line.split_once('=')?;
            Some(Row {
                name: name.trim().to_string(),
                bindings: rest.split_whitespace().collect::<Vec<_>>().join(" "),
            })
        })
        .collect()
}

/// The text of the rows that have a name
pub fn text(rows: &[Row]) -> String {
    rows.iter()
        .filter(|r| !r.name.trim().is_empty())
        .map(|r| format!("{} = {}\n", r.name.trim(), r.bindings.trim()).replace(" \n", "\n"))
        .collect()
}

/// The defaults of the project, "" when it has none
pub fn read(assets: &AssetDb) -> String {
    std::fs::read_to_string(assets.file(FILE)).unwrap_or_default()
}

impl InputEdit {
    /// The tab; writes the file when something changed and every
    /// binding is valid. Errors go back for the Console.
    pub fn ui(&mut self, ui: &mut egui::Ui, assets: &AssetDb) -> Result<(), String> {
        if !self.loaded {
            self.rows = parse(&read(assets));
            self.loaded = true;
        }
        let mut changed = false;
        if let Some(i) = self.listening {
            // The keys are taken, so Space does not also press a button
            let pressed = ui.input_mut(|input| {
                let key = input.events.iter().find_map(|e| match e {
                    egui::Event::Key {
                        key, pressed: true, ..
                    } => Some(*key),
                    _ => None,
                });
                input
                    .events
                    .retain(|e| !matches!(e, egui::Event::Key { .. }));
                key
            });
            match pressed {
                Some(egui::Key::Escape) => self.listening = None,
                Some(key) => {
                    if let (Some(k), Some(row)) = (key_of(key), self.rows.get_mut(i)) {
                        let word = Binding::Key(k).to_text();
                        if !row.bindings.split_whitespace().any(|w| w == word) {
                            row.bindings = format!("{} {word}", row.bindings).trim().to_string();
                            changed = true;
                        }
                    }
                    self.listening = None;
                }
                None => ui.ctx().request_repaint(),
            }
        }
        ui.horizontal(|ui| {
            if ui.button("Add action").clicked() {
                self.rows.push(Row {
                    name: format!("action{}", self.rows.len() + 1),
                    bindings: String::new(),
                });
                changed = true;
            }
            ui.weak("key:Space  mouse:Left  pad:South  pad:LeftX-  (pad0..pad3: one gamepad)");
        });
        let mut remove = None;
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Grid::new("input actions")
                .num_columns(4)
                .striped(true)
                .show(ui, |ui| {
                    for (i, row) in self.rows.iter_mut().enumerate() {
                        let name = egui::TextEdit::singleline(&mut row.name);
                        changed |= ui.add_sized([120.0, 20.0], name).changed();
                        let bad: Vec<String> = row.bad().into_iter().map(String::from).collect();
                        let mut edit = egui::TextEdit::singleline(&mut row.bindings);
                        if !bad.is_empty() {
                            edit = edit.text_color(ui.visuals().error_fg_color);
                        }
                        let r = ui.add_sized([320.0, 20.0], edit);
                        let r = if bad.is_empty() {
                            r
                        } else {
                            r.on_hover_text(format!("Not inputs: {}", bad.join(", ")))
                        };
                        changed |= r.changed();
                        let listening = self.listening == Some(i);
                        let label = if listening { "Press a key…" } else { "+ Key" };
                        if ui
                            .selectable_label(listening, label)
                            .on_hover_text("Add the next key pressed; Esc stops")
                            .clicked()
                        {
                            self.listening = (!listening).then_some(i);
                        }
                        if ui
                            .small_button("x")
                            .on_hover_text("Remove the action")
                            .clicked()
                        {
                            remove = Some(i);
                        }
                        ui.end_row();
                    }
                });
        });
        if let Some(i) = remove {
            self.rows.remove(i);
            self.listening = None;
            changed = true;
        }
        if !changed || self.rows.iter().any(|r| !r.bad().is_empty()) {
            return Ok(());
        }
        let file = assets.file(FILE);
        std::fs::write(&file, text(&self.rows)).map_err(|e| format!("{FILE}: {e}"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_round_trip_through_the_bindings_text() {
        let rows = parse("# controls\njump = key:Space  pad:South\n\nfire=mouse:Left\n");
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].bindings, "key:Space pad:South");
        assert_eq!(
            text(&rows),
            "jump = key:Space pad:South\nfire = mouse:Left\n"
        );
        let mut engine = orst::Engine::headless(8, 8).unwrap();
        engine.actions.load(&text(&rows)).unwrap();
        let unnamed = Row {
            name: " ".into(),
            bindings: "key:A".into(),
        };
        assert_eq!(text(&[unnamed]), "", "rows without a name are left out");
        let typo = Row {
            name: "x".into(),
            bindings: "key:Space key:Spcae".into(),
        };
        assert_eq!(typo.bad(), ["key:Spcae"]);
    }

    #[test]
    fn a_listening_row_takes_the_next_key() {
        let dir = std::env::temp_dir().join(format!("orst_input_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "jump = pad:South\n").unwrap();
        let (assets, _) = AssetDb::scan(dir.clone()).unwrap();
        let mut edit = InputEdit::default();
        let ctx = egui::Context::default();
        let run = |events: Vec<egui::Event>, edit: &mut InputEdit| {
            let raw = egui::RawInput {
                events,
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| edit.ui(ui, &assets).unwrap());
            out.textures_delta.clear();
        };
        run(vec![], &mut edit);
        edit.listening = Some(0);
        let space = egui::Event::Key {
            key: egui::Key::Space,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: egui::Modifiers::NONE,
        };
        run(vec![space], &mut edit);
        assert_eq!(edit.listening, None);
        assert_eq!(
            std::fs::read_to_string(dir.join(FILE)).unwrap(),
            "jump = pad:South key:Space\n"
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
