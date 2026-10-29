//! UI strings for the Rust side (tray, notifications). They come from the same
//! `src/i18n/*.json` files as the panel, so there is one source of truth.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

const ES: &str = include_str!("../../src/i18n/es.json");
const EN: &str = include_str!("../../src/i18n/en.json");

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lang {
    Es,
    En,
}

impl Lang {
    /// Spanish if Windows' display language is Spanish, English otherwise.
    pub fn system() -> Self {
        #[cfg(windows)]
        {
            // The low 10 bits of a LANGID are the primary language.
            let id = unsafe { windows::Win32::Globalization::GetUserDefaultUILanguage() };
            if id & 0x3ff == 0x0a {
                return Self::Es;
            }
        }
        Self::En
    }
}

pub struct Strings {
    table: Map<String, Value>,
    fallback: Map<String, Value>,
}

fn parse(json: &str) -> Map<String, Value> {
    match serde_json::from_str(json) {
        Ok(Value::Object(map)) => map,
        _ => {
            log::error!("i18n: a strings file is not a JSON object");
            Map::new()
        }
    }
}

impl Strings {
    pub fn new(lang: Lang) -> Self {
        Self {
            table: parse(match lang {
                Lang::Es => ES,
                Lang::En => EN,
            }),
            fallback: parse(EN),
        }
    }

    /// The string for `key`, falling back to English and then to the key.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.table
            .get(key)
            .or_else(|| self.fallback.get(key))
            .and_then(Value::as_str)
            .unwrap_or(key)
    }

    /// `get` with `{name}` placeholders filled in.
    pub fn format(&self, key: &str, args: &[(&str, &str)]) -> String {
        args.iter()
            .fold(self.get(key).to_owned(), |s, (name, value)| {
                s.replace(&format!("{{{name}}}"), value)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_languages_have_the_same_keys() {
        let (es, en) = (parse(ES), parse(EN));
        let mut es_keys: Vec<_> = es.keys().collect();
        let mut en_keys: Vec<_> = en.keys().collect();
        es_keys.sort();
        en_keys.sort();
        assert_eq!(es_keys, en_keys);
        assert!(!es.is_empty());
    }

    #[test]
    fn formats_placeholders_and_falls_back_to_the_key() {
        let s = Strings::new(Lang::Es);
        assert_eq!(
            s.format("trayPlaying", &[("color", "Rosa"), ("volume", "40")]),
            "Rumor · Rosa · 40 %"
        );
        assert_eq!(s.get("noSuchKey"), "noSuchKey");
    }
}
