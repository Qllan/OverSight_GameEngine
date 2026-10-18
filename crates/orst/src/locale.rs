//! Translations: a table of texts per language, from JSON files named
//! after their language (Locale/en.json, Locale/ru.json). Objects in a
//! file nest keys with dots:
//!
//! ```json
//! {"menu": {"play": "Играть", "quit": "Выйти"}, "title": "Сад"}
//! ```
//!
//! gives `menu.play`, `menu.quit` and `title`. A UiText whose text starts
//! with `@` shows the text of that key in the current language (`@@`
//! starts a text with a plain `@`).

use serde_json::Value as Json;
use std::borrow::Cow;
use std::collections::{BTreeMap, HashMap};
use std::path::Path;

/// The language a missing text comes from when there is one
pub const FALLBACK: &str = "en";

#[derive(Default)]
pub struct Locale {
    language: String,
    tables: BTreeMap<String, HashMap<String, String>>,
}

impl Locale {
    /// Put the texts of `json` in the table of `language` (texts already
    /// there stay unless the file has them too); how many texts it had
    pub fn add(&mut self, language: &str, json: &str) -> Result<usize, String> {
        if language.is_empty() {
            return Err("a language needs a name".into());
        }
        let value: Json = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let Json::Object(_) = value else {
            return Err("a language file is a JSON object of texts".into());
        };
        let table = self.tables.entry(language.to_string()).or_default();
        let mut count = 0;
        flatten("", &value, table, &mut count);
        if self.language.is_empty() {
            self.language = self.default_language().to_string();
        }
        Ok(count)
    }

    /// Every *.json in `dir`, each named after its language; what went
    /// wrong, file by file. The current language stays if it is still
    /// there.
    pub fn load_dir(&mut self, dir: &Path) -> Vec<String> {
        let mut errors = Vec::new();
        let Ok(entries) = std::fs::read_dir(dir) else {
            return errors;
        };
        let mut files: Vec<_> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case("json"))
            })
            .collect();
        files.sort();
        let current = std::mem::take(&mut self.language);
        self.tables.clear();
        for path in files {
            let Some(language) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            let result = std::fs::read_to_string(&path)
                .map_err(|e| e.to_string())
                .and_then(|text| self.add(language, &text));
            if let Err(e) = result {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        if !self.set_language(&current) {
            self.language = self.default_language().to_string();
        }
        errors
    }

    pub fn languages(&self) -> Vec<&str> {
        self.tables.keys().map(|k| k.as_str()).collect()
    }

    /// The current language; "" when there are none
    pub fn language(&self) -> &str {
        &self.language
    }

    /// Switch to a loaded language; false (and no change) for others
    pub fn set_language(&mut self, language: &str) -> bool {
        if !self.tables.contains_key(language) {
            return false;
        }
        self.language = language.to_string();
        true
    }

    /// English when there is English, else the first by name
    fn default_language(&self) -> &str {
        if self.tables.contains_key(FALLBACK) {
            return FALLBACK;
        }
        self.tables.keys().next().map_or("", |k| k.as_str())
    }

    /// The text of `key` in the current language, else in the fallback
    /// one, else the key itself (so a missing text shows what is missing)
    pub fn tr<'a>(&'a self, key: &'a str) -> &'a str {
        [self.language.as_str(), self.default_language()]
            .iter()
            .find_map(|l| self.tables.get(*l)?.get(key))
            .map_or(key, |s| s.as_str())
    }

    /// What a UiText shows for `text`: the translation of an `@key`
    pub fn text<'a>(&'a self, text: &'a str) -> Cow<'a, str> {
        match text.strip_prefix('@') {
            Some(rest) if rest.starts_with('@') => Cow::Borrowed(rest),
            Some(key) => Cow::Borrowed(self.tr(key)),
            None => Cow::Borrowed(text),
        }
    }
}

fn flatten(prefix: &str, value: &Json, table: &mut HashMap<String, String>, count: &mut usize) {
    match value {
        Json::Object(map) => {
            for (k, v) in map {
                let key = match prefix {
                    "" => k.clone(),
                    p => format!("{p}.{k}"),
                };
                flatten(&key, v, table, count);
            }
        }
        Json::String(s) => {
            table.insert(prefix.to_string(), s.clone());
            *count += 1;
        }
        Json::Null => {}
        other => {
            table.insert(prefix.to_string(), other.to_string());
            *count += 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_find_their_text_in_the_language() {
        let mut l = Locale::default();
        assert_eq!(l.tr("menu.play"), "menu.play", "no texts: the key");
        let ru = r#"{"menu": {"play": "Играть"}, "title": "Сад"}"#;
        assert_eq!(l.add("ru", ru), Ok(2));
        assert_eq!(l.language(), "ru", "the only language");
        l.add("en", r#"{"menu": {"play": "Play", "quit": "Quit"}}"#)
            .unwrap();
        assert_eq!(l.languages(), ["en", "ru"]);
        assert_eq!(l.tr("menu.play"), "Играть");
        assert_eq!(l.tr("menu.quit"), "Quit", "missing: from English");
        assert!(l.set_language("en") && !l.set_language("de"));
        assert_eq!((l.language(), l.tr("title")), ("en", "title"));
        assert_eq!(l.text("@menu.play"), "Play");
        assert_eq!(l.text("@@home"), "@home");
        assert_eq!(l.text("plain"), "plain");
        assert!(l.add("de", "[1]").is_err());
    }

    #[test]
    fn a_folder_of_languages_loads_again_in_place() {
        let dir = std::env::temp_dir().join(format!("orst_locale_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("en.json"), r#"{"hi": "Hello"}"#).unwrap();
        std::fs::write(dir.join("ru.json"), r#"{"hi": "Привет"}"#).unwrap();
        std::fs::write(dir.join("bad.json"), "{").unwrap();
        let mut l = Locale::default();
        let errors = l.load_dir(&dir);
        assert!(
            errors.len() == 1 && errors[0].contains("bad.json"),
            "{errors:?}"
        );
        assert_eq!(l.language(), "en");
        l.set_language("ru");
        std::fs::write(dir.join("ru.json"), r#"{"hi": "Здравствуй"}"#).unwrap();
        l.load_dir(&dir);
        assert_eq!((l.language(), l.tr("hi")), ("ru", "Здравствуй"));
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
