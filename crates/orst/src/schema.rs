//! What is inside a component: its fields by name, type and offset.
//!
//! The world stores a component as plain bytes. A schema says how to read
//! them, so tools can show and change a component without knowing its
//! type at compile time: the editor's inspector draws a widget per field,
//! scene files store fields by name (so a component may grow a field and
//! old scenes still load), and undo records one field's old and new value.
//! The built-in components have schemas; games give their own components
//! one through the C API (the C# binding does it from the struct's fields).
//! A component without a schema still works, tools just see it as bytes.

use crate::types::{Color, Rect, Vec2};
use serde_json::{json, Value as Json};

/// Field types, numbered as in orst.h
#[repr(u32)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FieldType {
    F32 = 1,
    I32 = 2,
    U32 = 3,
    /// One byte, 0 or 1
    Bool = 4,
    Vec2 = 5,
    /// Four bytes: red, green, blue, alpha
    Color = 6,
    Rect = 7,
    /// An entity id (u64)
    Entity = 8,
    /// Ids of engine resources (u32); scene files store them as asset paths
    Texture = 9,
    Tilemap = 10,
    Polygon = 11,
    U8 = 12,
    /// 64 bytes of UTF-8, the rest zeros (a Name)
    Text = 13,
}

impl FieldType {
    pub fn from_u32(v: u32) -> Option<FieldType> {
        use FieldType::*;
        Some(match v {
            1 => F32,
            2 => I32,
            3 => U32,
            4 => Bool,
            5 => Vec2,
            6 => Color,
            7 => Rect,
            8 => Entity,
            9 => Texture,
            10 => Tilemap,
            11 => Polygon,
            12 => U8,
            13 => Text,
            _ => return None,
        })
    }

    pub fn size(self) -> u32 {
        use FieldType::*;
        match self {
            Bool | U8 => 1,
            F32 | I32 | U32 | Color | Texture | Tilemap | Polygon => 4,
            Vec2 | Entity => 8,
            Rect => 16,
            Text => TEXT_BYTES as u32,
        }
    }

    pub fn name(self) -> &'static str {
        use FieldType::*;
        match self {
            F32 => "f32",
            I32 => "i32",
            U32 => "u32",
            Bool => "bool",
            Vec2 => "vec2",
            Color => "color",
            Rect => "rect",
            Entity => "entity",
            Texture => "texture",
            Tilemap => "tilemap",
            Polygon => "polygon",
            U8 => "u8",
            Text => "text",
        }
    }
}

pub const TEXT_BYTES: usize = 64;

/// A string as a Text field's bytes: cut to 64 bytes at a character
/// boundary, the rest zeros
pub fn text_bytes(s: &str) -> [u8; TEXT_BYTES] {
    let mut end = s.len().min(TEXT_BYTES);
    while !s.is_char_boundary(end) {
        end -= 1;
    }
    let mut out = [0; TEXT_BYTES];
    out[..end].copy_from_slice(&s.as_bytes()[..end]);
    out
}

/// A Text field's bytes as a string, up to the first zero (bytes that are
/// not UTF-8 end it early)
pub fn text_str(bytes: &[u8]) -> &str {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    match std::str::from_utf8(&bytes[..end]) {
        Ok(s) => s,
        Err(e) => std::str::from_utf8(&bytes[..e.valid_up_to()]).unwrap(),
    }
}

/// An f32 in radians, shown in degrees
pub const FLAG_ANGLE: u32 = 1;
/// A u32 of layer bits, shown as 32 checkboxes
pub const FLAG_LAYERS: u32 = 2;
const FLAGS: u32 = FLAG_ANGLE | FLAG_LAYERS;

#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    pub name: String,
    pub ty: FieldType,
    /// Bytes from the start of the component
    pub offset: u32,
    pub flags: u32,
    /// A slider from min to max when min < max (numbers)
    pub min: f32,
    pub max: f32,
    /// Names of the values 0, 1, 2... of a number that is a choice
    pub options: Vec<String>,
}

impl Field {
    pub fn new(name: &str, ty: FieldType, offset: u32) -> Field {
        Field {
            name: name.to_string(),
            ty,
            offset,
            flags: 0,
            min: 0.0,
            max: 0.0,
            options: Vec::new(),
        }
    }

    pub fn range(mut self, min: f32, max: f32) -> Field {
        (self.min, self.max) = (min, max);
        self
    }

    pub fn flags(mut self, flags: u32) -> Field {
        self.flags = flags;
        self
    }

    pub fn options(mut self, options: &[&str]) -> Field {
        self.options = options.iter().map(|s| s.to_string()).collect();
        self
    }

    fn bytes<'a>(&self, component: &'a [u8]) -> &'a [u8] {
        &component[self.offset as usize..(self.offset + self.ty.size()) as usize]
    }

    /// The field's value in a component's bytes (checked by `check`)
    pub fn read(&self, component: &[u8]) -> Value {
        let b = self.bytes(component);
        let f = |i: usize| f32::from_le_bytes(b[i..i + 4].try_into().unwrap());
        let u = || u32::from_le_bytes(b[..4].try_into().unwrap());
        match self.ty {
            FieldType::F32 => Value::F32(f(0)),
            FieldType::I32 => Value::I32(u() as i32),
            FieldType::U32 | FieldType::Texture | FieldType::Tilemap | FieldType::Polygon => {
                Value::U32(u())
            }
            FieldType::Bool => Value::Bool(b[0] != 0),
            FieldType::U8 => Value::U32(b[0] as u32),
            FieldType::Vec2 => Value::Vec2(Vec2::new(f(0), f(4))),
            FieldType::Color => Value::Color(Color::rgba(b[0], b[1], b[2], b[3])),
            FieldType::Rect => Value::Rect(Rect::new(f(0), f(4), f(8), f(12))),
            FieldType::Entity => Value::Entity(u64::from_le_bytes(b[..8].try_into().unwrap())),
            FieldType::Text => Value::Text(text_bytes(text_str(b))),
        }
    }

    /// Put a value into a component's bytes; false when it is of another
    /// type (numbers convert between f32, i32 and u32)
    pub fn write(&self, component: &mut [u8], value: Value) -> bool {
        let at = self.offset as usize;
        let mut put = |bytes: &[u8]| component[at..at + bytes.len()].copy_from_slice(bytes);
        let number = value.number();
        match (self.ty, value) {
            (FieldType::F32, _) if number.is_some() => put(&(number.unwrap() as f32).to_le_bytes()),
            (FieldType::I32, _) if number.is_some() => put(&(number.unwrap() as i32).to_le_bytes()),
            (FieldType::U32 | FieldType::Texture | FieldType::Tilemap | FieldType::Polygon, _)
                if number.is_some() =>
            {
                put(&(number.unwrap() as u32).to_le_bytes())
            }
            (FieldType::U8, _) if number.is_some() => put(&[number.unwrap() as u8]),
            (FieldType::Bool, Value::Bool(v)) => put(&[v as u8]),
            (FieldType::Vec2, Value::Vec2(v)) => {
                put(&[v.x.to_le_bytes(), v.y.to_le_bytes()].concat())
            }
            (FieldType::Color, Value::Color(c)) => put(&[c.r, c.g, c.b, c.a]),
            (FieldType::Rect, Value::Rect(r)) => put(&[
                r.x.to_le_bytes(),
                r.y.to_le_bytes(),
                r.w.to_le_bytes(),
                r.h.to_le_bytes(),
            ]
            .concat()),
            (FieldType::Entity, Value::Entity(e)) => put(&e.to_le_bytes()),
            (FieldType::Text, Value::Text(t)) => put(&t),
            _ => return false,
        }
        true
    }

    /// The value as it is written in a scene file
    pub fn to_json(&self, value: Value) -> Json {
        match value {
            Value::F32(v) => json!(v),
            Value::I32(v) => json!(v),
            Value::U32(v) => match self.options.get(v as usize) {
                Some(name) => json!(name),
                None => json!(v),
            },
            Value::Bool(v) => json!(v),
            Value::Vec2(v) => json!([v.x, v.y]),
            Value::Color(c) => json!(format!("#{:02x}{:02x}{:02x}{:02x}", c.r, c.g, c.b, c.a)),
            Value::Rect(r) => json!([r.x, r.y, r.w, r.h]),
            Value::Entity(e) => json!(e),
            Value::Text(t) => json!(text_str(&t)),
        }
    }

    /// The value written in a scene file; None when it does not fit the field
    pub fn from_json(&self, v: &Json) -> Option<Value> {
        let floats = |n: usize| -> Option<Vec<f32>> {
            let a = v.as_array().filter(|a| a.len() == n)?;
            a.iter().map(|x| x.as_f64().map(|x| x as f32)).collect()
        };
        Some(match self.ty {
            FieldType::F32 => Value::F32(v.as_f64()? as f32),
            FieldType::I32 => Value::I32(i32::try_from(v.as_i64()?).ok()?),
            FieldType::U32
            | FieldType::U8
            | FieldType::Texture
            | FieldType::Tilemap
            | FieldType::Polygon => match v.as_str() {
                Some(name) => Value::U32(self.options.iter().position(|o| o == name)? as u32),
                None => {
                    let n = u32::try_from(v.as_u64()?).ok()?;
                    if self.ty == FieldType::U8 && n > 255 {
                        return None;
                    }
                    Value::U32(n)
                }
            },
            FieldType::Bool => Value::Bool(v.as_bool()?),
            FieldType::Vec2 => {
                let f = floats(2)?;
                Value::Vec2(Vec2::new(f[0], f[1]))
            }
            FieldType::Color => Value::Color(parse_color(v.as_str()?)?),
            FieldType::Rect => {
                let f = floats(4)?;
                Value::Rect(Rect::new(f[0], f[1], f[2], f[3]))
            }
            FieldType::Entity => Value::Entity(v.as_u64()?),
            FieldType::Text => Value::Text(text_bytes(v.as_str()?)),
        })
    }
}

/// "#rrggbb" or "#rrggbbaa"
fn parse_color(s: &str) -> Option<Color> {
    let hex = s.strip_prefix('#')?;
    if !(hex.len() == 6 || hex.len() == 8) || !hex.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(&hex[i..i + 2], 16).ok();
    let a = if hex.len() == 8 { byte(6)? } else { 255 };
    Some(Color::rgba(byte(0)?, byte(2)?, byte(4)?, a))
}

/// A field's value
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Value {
    F32(f32),
    I32(i32),
    /// Also u8 and resource ids
    U32(u32),
    Bool(bool),
    Vec2(Vec2),
    Color(Color),
    Rect(Rect),
    Entity(u64),
    /// See text_str and text_bytes
    Text([u8; TEXT_BYTES]),
}

impl Value {
    fn number(self) -> Option<f64> {
        match self {
            Value::F32(v) => Some(v as f64),
            Value::I32(v) => Some(v as f64),
            Value::U32(v) => Some(v as f64),
            _ => None,
        }
    }
}

/// Whether fields fit a component of `size` bytes: known flags, unique
/// non-empty names, inside the component and not overlapping
pub fn check(component: &str, size: u32, fields: &[Field]) -> Result<(), String> {
    let mut spans: Vec<(u32, u32, &str)> = Vec::new();
    for (i, f) in fields.iter().enumerate() {
        let what = || format!("component {component}, field {:?}", f.name);
        if f.name.is_empty() {
            return Err(format!("component {component}: field {i} has no name"));
        }
        if fields[..i].iter().any(|g| g.name == f.name) {
            return Err(format!("{}: named twice", what()));
        }
        let end = f.offset as u64 + f.ty.size() as u64;
        if end > size as u64 {
            return Err(format!(
                "{}: {} bytes at {} go past the component's {size}",
                what(),
                f.ty.size(),
                f.offset
            ));
        }
        if f.flags & !FLAGS != 0 {
            return Err(format!("{}: unknown flags {:#x}", what(), f.flags & !FLAGS));
        }
        if let Some(&(_, _, other)) = spans
            .iter()
            .find(|&&(a, b, _)| f.offset < b && a < end as u32)
        {
            return Err(format!("{}: overlaps field {other:?}", what()));
        }
        spans.push((f.offset, end as u32, &f.name));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields() -> Vec<Field> {
        vec![
            Field::new("speed", FieldType::F32, 0).range(0.0, 10.0),
            Field::new("mode", FieldType::U32, 4).options(&["Walk", "Run"]),
            Field::new("target", FieldType::Entity, 8),
            Field::new("tint", FieldType::Color, 16),
            Field::new("alive", FieldType::Bool, 20),
        ]
    }

    #[test]
    fn fields_read_and_write_component_bytes() {
        let f = fields();
        check("Enemy", 24, &f).unwrap();
        let mut bytes = vec![0u8; 24];
        assert!(f[0].write(&mut bytes, Value::F32(2.5)));
        assert!(f[1].write(&mut bytes, Value::I32(1)));
        assert!(f[2].write(&mut bytes, Value::Entity(7 << 32 | 3)));
        assert!(f[3].write(&mut bytes, Value::Color(Color::rgba(1, 2, 3, 4))));
        assert!(f[4].write(&mut bytes, Value::Bool(true)));
        assert!(
            !f[4].write(&mut bytes, Value::F32(1.0)),
            "a number is not a bool"
        );
        assert_eq!(f[0].read(&bytes), Value::F32(2.5));
        assert_eq!(f[1].read(&bytes), Value::U32(1));
        assert_eq!(f[2].read(&bytes), Value::Entity(7 << 32 | 3));
        assert_eq!(f[3].read(&bytes), Value::Color(Color::rgba(1, 2, 3, 4)));
        assert_eq!(f[4].read(&bytes), Value::Bool(true));
    }

    #[test]
    fn json_round_trips_and_names_choices() {
        let f = fields();
        let mut bytes = vec![0u8; 24];
        f[1].write(&mut bytes, Value::U32(1));
        f[3].write(&mut bytes, Value::Color(Color::rgba(255, 128, 0, 255)));
        assert_eq!(f[1].to_json(f[1].read(&bytes)), json!("Run"));
        assert_eq!(f[3].to_json(f[3].read(&bytes)), json!("#ff8000ff"));
        for field in &f {
            let v = field.read(&bytes);
            assert_eq!(
                field.from_json(&field.to_json(v)),
                Some(v),
                "{}",
                field.name
            );
        }
        assert_eq!(f[1].from_json(&json!("Fly")), None);
        assert_eq!(
            f[3].from_json(&json!("#ff8000")),
            Some(Value::Color(Color::rgba(255, 128, 0, 255)))
        );
        assert_eq!(f[0].from_json(&json!("fast")), None);
    }

    #[test]
    fn bad_fields_are_refused() {
        let at = |name: &str, ty, offset| vec![Field::new(name, ty, offset)];
        assert!(
            check("C", 8, &at("x", FieldType::Vec2, 4)).is_err(),
            "past the end"
        );
        assert!(
            check("C", 8, &at("", FieldType::F32, 0)).is_err(),
            "no name"
        );
        let two = [
            Field::new("a", FieldType::Vec2, 0),
            Field::new("b", FieldType::F32, 4),
        ];
        assert!(check("C", 8, &two).unwrap_err().contains("overlaps"));
        let same = [
            Field::new("a", FieldType::F32, 0),
            Field::new("a", FieldType::F32, 4),
        ];
        assert!(check("C", 8, &same).is_err());
        assert!(check("C", 8, &[Field::new("a", FieldType::F32, 0).flags(64)]).is_err());
    }
}
