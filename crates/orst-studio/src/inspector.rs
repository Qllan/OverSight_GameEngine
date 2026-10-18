//! The Inspector for an entity: its components with a widget per field,
//! built from the field schema, so game components written in C# or C++
//! show up the same way as the engine's own. It only reads the world and
//! returns what the user changed; the editor applies it, with undo.

use crate::egui;
use crate::scene_view::{label, AssetPath};
use orst::components::{NAME, PREFAB};
use orst::ecs::{Entity, World};
use orst::schema::{text_bytes, text_str, Field, FieldType, Value, FLAG_ANGLE, FLAG_LAYERS};
use orst::{Color, Engine, Rect, Vec2};
use std::collections::HashSet;

#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    Set {
        component: u32,
        field: String,
        value: Value,
    },
    /// An asset dropped on a texture or map field, loaded by the editor
    SetAsset {
        component: u32,
        field: String,
        ty: FieldType,
        path: String,
    },
    Add(u32),
    Remove(u32),
    /// Save the entity as a new prefab and make it an instance
    MakePrefab,
    /// Put the instance's changes into its prefab and every instance
    ApplyPrefab,
    /// Drop the instance's changes but its position
    RevertPrefab,
    /// Make the instance an entity of its own
    UnlinkPrefab,
}

/// What a prefab instance changes, as (component, field) names
pub type Overrides = HashSet<(String, String)>;

enum Change {
    Value(Value),
    Asset(String),
}

/// The entity's name and components; `more` adds to the end of the list.
/// Fields in `overrides` are bold: the instance changed them.
pub fn ui(
    ui: &mut egui::Ui,
    engine: &Engine,
    e: Entity,
    overrides: &Overrides,
    more: &mut dyn FnMut(&mut egui::Ui),
) -> Vec<Action> {
    let world = &engine.world;
    let mut actions = Vec::new();
    let entities: Vec<(Entity, String)> = world
        .query(&[])
        .into_iter()
        .map(|x| (x, label(engine, x)))
        .collect();

    // The name goes on top, like a title
    let name = world.get(e, NAME).map(text_str).unwrap_or("").to_string();
    ui.horizontal(|ui| {
        ui.label("Name");
        let mut text = name;
        if ui.text_edit_singleline(&mut text).changed() {
            actions.push(Action::Set {
                component: NAME,
                field: "text".into(),
                value: Value::Text(text_bytes(&text)),
            });
        }
    });
    match engine.prefab_instance(e) {
        Some(p) => {
            let file = engine.prefab_file(p).and_then(|f| f.file_name());
            let file = file.map_or("?".into(), |f| f.to_string_lossy());
            ui.horizontal(|ui| {
                ui.label("Prefab");
                ui.strong(file);
            });
            ui.horizontal(|ui| {
                if ui
                    .button("Apply")
                    .on_hover_text("Put the changes in bold into the prefab and all its instances")
                    .clicked()
                {
                    actions.push(Action::ApplyPrefab);
                }
                if ui
                    .button("Revert")
                    .on_hover_text("Take the prefab's values back; the position stays")
                    .clicked()
                {
                    actions.push(Action::RevertPrefab);
                }
                if ui
                    .button("Unlink")
                    .on_hover_text("Keep this entity as it is, no longer an instance")
                    .clicked()
                {
                    actions.push(Action::UnlinkPrefab);
                }
            });
        }
        None => {
            if ui
                .button("Make prefab")
                .on_hover_text("Save the entity to Assets/Prefabs to place it again and again")
                .clicked()
            {
                actions.push(Action::MakePrefab);
            }
        }
    }
    ui.separator();

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            for id in 1..=world.component_types() {
                if id == NAME || id == PREFAB || !world.has(e, id) {
                    continue;
                }
                let title = world.storage(id).map_or("?", |s| s.name.as_str());
                let changed =
                    |field: &str| overrides.contains(&(title.to_string(), field.to_string()));
                egui::CollapsingHeader::new(egui::RichText::new(title).strong())
                    .id_salt(("component", id))
                    .default_open(true)
                    .show(ui, |ui| {
                        let fields = world.fields(id);
                        if fields.is_empty() {
                            ui.weak("No fields described");
                        }
                        egui::Grid::new(("fields", id))
                            .num_columns(2)
                            .striped(true)
                            .show(ui, |ui| {
                                for f in fields {
                                    let Some(value) = world.field(e, id, &f.name) else {
                                        continue;
                                    };
                                    if changed(&f.name) {
                                        ui.label(egui::RichText::new(&f.name).strong())
                                            .on_hover_text("Changed in this instance");
                                    } else {
                                        ui.label(&f.name);
                                    }
                                    match field_ui(ui, engine, f, value, &entities) {
                                        Some(Change::Value(value)) => actions.push(Action::Set {
                                            component: id,
                                            field: f.name.clone(),
                                            value,
                                        }),
                                        Some(Change::Asset(path)) => {
                                            actions.push(Action::SetAsset {
                                                component: id,
                                                field: f.name.clone(),
                                                ty: f.ty,
                                                path,
                                            })
                                        }
                                        None => {}
                                    }
                                    ui.end_row();
                                }
                            });
                        if ui.small_button("Remove component").clicked() {
                            actions.push(Action::Remove(id));
                        }
                    });
            }
            ui.add_space(8.0);
            let missing: Vec<u32> = (1..=world.component_types())
                .filter(|&id| id != NAME && id != PREFAB && !world.has(e, id))
                .collect();
            if !missing.is_empty() {
                egui::ComboBox::from_id_salt("add component")
                    .selected_text("Add component")
                    .show_ui(ui, |ui| {
                        for id in missing {
                            let name = world.storage(id).map_or("?", |s| s.name.as_str());
                            if ui.selectable_label(false, name).clicked() {
                                actions.push(Action::Add(id));
                            }
                        }
                    });
            }
            more(ui);
        });
    actions
}

/// One field's widget; what the user changed, if anything
fn field_ui(
    ui: &mut egui::Ui,
    engine: &Engine,
    f: &Field,
    value: Value,
    entities: &[(Entity, String)],
) -> Option<Change> {
    if let FieldType::Texture | FieldType::Tilemap = f.ty {
        // Drop an image or a map from the Project window here
        let (_, dropped) = ui.dnd_drop_zone::<AssetPath, _>(egui::Frame::NONE, |ui| {
            field_value_ui(ui, engine, f, value, entities)
        });
        return dropped.map(|path| Change::Asset(path.0.clone()));
    }
    field_value_ui(ui, engine, f, value, entities).map(Change::Value)
}

fn field_value_ui(
    ui: &mut egui::Ui,
    engine: &Engine,
    f: &Field,
    value: Value,
    entities: &[(Entity, String)],
) -> Option<Value> {
    let ranged = f.min < f.max;
    let changed = |r: egui::Response, v: Value| r.changed().then_some(v);
    match (f.ty, value) {
        (FieldType::Texture | FieldType::Tilemap | FieldType::Polygon, Value::U32(id)) => {
            let file = match f.ty {
                FieldType::Texture => engine.texture_file(id),
                FieldType::Tilemap => engine.tilemap_file(id),
                _ => None,
            };
            let text = match (id, file) {
                (0, _) => "None".to_string(),
                (_, Some(path)) => path
                    .file_name()
                    .map_or(path.display().to_string(), |n| n.to_string_lossy().into()),
                (id, None) => format!("#{id}"),
            };
            ui.weak(text)
                .on_hover_text("Drop a file from the Project window here");
            None
        }
        (_, Value::F32(mut v)) => {
            let r = if f.flags & FLAG_ANGLE != 0 {
                ui.drag_angle(&mut v)
            } else if ranged {
                ui.add(egui::Slider::new(&mut v, f.min..=f.max))
            } else {
                ui.add(egui::DragValue::new(&mut v).speed(0.1))
            };
            changed(r, Value::F32(v))
        }
        (_, Value::I32(mut v)) => {
            if !f.options.is_empty() {
                return choice(ui, f, v as i64).map(|i| Value::I32(i as i32));
            }
            let mut drag = egui::DragValue::new(&mut v);
            if ranged {
                drag = drag.range(f.min as i32..=f.max as i32);
            }
            let r = ui.add(drag);
            changed(r, Value::I32(v))
        }
        (_, Value::U32(mut v)) => {
            if !f.options.is_empty() {
                return choice(ui, f, v as i64).map(|i| Value::U32(i as u32));
            }
            if f.flags & FLAG_LAYERS != 0 {
                return layers(ui, v).map(Value::U32);
            }
            let (lo, hi) = match (ranged, f.ty) {
                (true, _) => (f.min as u32, f.max as u32),
                (false, FieldType::U8) => (0, 255),
                _ => (0, u32::MAX),
            };
            let r = ui.add(egui::DragValue::new(&mut v).range(lo..=hi));
            changed(r, Value::U32(v))
        }
        (_, Value::Bool(mut v)) => {
            let r = ui.checkbox(&mut v, "");
            changed(r, Value::Bool(v))
        }
        (_, Value::Vec2(mut v)) => {
            let r = ui
                .horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut v.x).prefix("x ").speed(0.1))
                        | ui.add(egui::DragValue::new(&mut v.y).prefix("y ").speed(0.1))
                })
                .inner;
            changed(r, Value::Vec2(Vec2::new(v.x, v.y)))
        }
        (_, Value::Color(c)) => {
            let mut rgba = [c.r, c.g, c.b, c.a];
            let r = ui.color_edit_button_srgba_unmultiplied(&mut rgba);
            changed(
                r,
                Value::Color(Color::rgba(rgba[0], rgba[1], rgba[2], rgba[3])),
            )
        }
        (_, Value::Rect(mut v)) => {
            let r = ui
                .horizontal(|ui| {
                    ui.add(egui::DragValue::new(&mut v.x).prefix("x "))
                        | ui.add(egui::DragValue::new(&mut v.y).prefix("y "))
                        | ui.add(egui::DragValue::new(&mut v.w).prefix("w "))
                        | ui.add(egui::DragValue::new(&mut v.h).prefix("h "))
                })
                .inner;
            changed(r, Value::Rect(Rect::new(v.x, v.y, v.w, v.h)))
        }
        (_, Value::Entity(v)) => {
            let current = entities
                .iter()
                .find(|(x, _)| *x == v)
                .map_or("None", |(_, l)| l.as_str());
            let mut picked = None;
            egui::ComboBox::from_id_salt(("entity", f.name.as_str()))
                .selected_text(current)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(v == 0, "None").clicked() {
                        picked = Some(0);
                    }
                    for (x, l) in entities {
                        if ui.selectable_label(*x == v, l).clicked() {
                            picked = Some(*x);
                        }
                    }
                });
            picked.filter(|&p| p != v).map(Value::Entity)
        }
        (_, Value::Text(bytes)) => {
            let mut text = text_str(&bytes).to_string();
            let r = ui.text_edit_singleline(&mut text);
            changed(r, Value::Text(text_bytes(&text)))
        }
    }
}

/// A number that names one of `f.options`
fn choice(ui: &mut egui::Ui, f: &Field, v: i64) -> Option<i64> {
    let current = usize::try_from(v)
        .ok()
        .and_then(|i| f.options.get(i))
        .map_or_else(|| v.to_string(), |s| s.clone());
    let mut picked = None;
    egui::ComboBox::from_id_salt(("choice", f.name.as_str()))
        .selected_text(current)
        .show_ui(ui, |ui| {
            for (i, name) in f.options.iter().enumerate() {
                if ui.selectable_label(i as i64 == v, name).clicked() {
                    picked = Some(i as i64);
                }
            }
        });
    picked.filter(|&p| p != v)
}

/// 32 layer bits as two rows of checkboxes
fn layers(ui: &mut egui::Ui, v: u32) -> Option<u32> {
    let mut out = v;
    ui.vertical(|ui| {
        ui.spacing_mut().item_spacing.x = 1.0;
        for row in 0..2 {
            ui.horizontal(|ui| {
                for bit in row * 16..row * 16 + 16 {
                    let mut on = out & (1 << bit) != 0;
                    if ui
                        .checkbox(&mut on, "")
                        .on_hover_text(format!("Layer {bit}"))
                        .changed()
                    {
                        out ^= 1 << bit;
                    }
                }
            });
        }
    });
    (out != v).then_some(out)
}

/// Attach a component with its default values: the engine's own
/// defaults for built-ins (a Transform at scale 1, a white sprite), the
/// field initialisers for scripts, zeros for other game components
pub fn add_component(world: &mut World, e: Entity, id: u32) -> bool {
    let defaults = world.default_value(id);
    world.set(e, id, &defaults)
}

#[cfg(test)]
mod tests {
    use super::*;
    use orst::components::{read, SpriteRenderer, Transform, SPRITE_RENDERER, TRANSFORM};

    #[test]
    fn added_components_start_from_their_defaults() {
        let mut engine = Engine::headless(8, 8).unwrap();
        let world = &mut engine.world;
        let e = world.spawn();
        for id in 1..=world.component_types() {
            assert!(add_component(world, e, id), "component {id}");
        }
        let t: Transform = read(world, e, TRANSFORM).unwrap();
        assert_eq!(t.scale, Vec2::new(1.0, 1.0));
        let s: SpriteRenderer = read(world, e, SPRITE_RENDERER).unwrap();
        assert_eq!(s.tint, Color::WHITE);
    }
}
