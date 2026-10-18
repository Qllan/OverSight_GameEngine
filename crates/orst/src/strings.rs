//! Text of any length for String fields of components. Components are
//! plain bytes, so a String field holds an id here: the same text always
//! gets the same id, ids never change, and 0 is the empty text. Snapshots
//! of the world stay valid because nothing is ever removed.

use std::collections::HashMap;

#[derive(Default)]
pub struct Strings {
    texts: Vec<String>,
    ids: HashMap<String, u32>,
}

impl Strings {
    /// The id of `text`, added if it is new
    pub fn intern(&mut self, text: &str) -> u32 {
        if text.is_empty() {
            return 0;
        }
        if let Some(&id) = self.ids.get(text) {
            return id;
        }
        self.texts.push(text.to_string());
        let id = self.texts.len() as u32;
        self.ids.insert(text.to_string(), id);
        id
    }

    /// The text of an id; "" for 0 and ids it never gave
    pub fn get(&self, id: u32) -> &str {
        match id {
            0 => "",
            id => self.texts.get(id as usize - 1).map_or("", String::as_str),
        }
    }

    pub fn len(&self) -> usize {
        self.texts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.texts.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_text_keeps_its_id() {
        let mut s = Strings::default();
        assert_eq!(s.intern(""), 0);
        let a = s.intern("Новая игра");
        assert_eq!(s.intern("Новая игра"), a);
        assert_ne!(s.intern("Выход"), a);
        assert_eq!(s.get(a), "Новая игра");
        assert_eq!((s.get(0), s.get(99)), ("", ""));
        assert_eq!(s.len(), 2);
    }
}
