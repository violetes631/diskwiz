//! Tiny dependency-free i18n.
//!
//! The English source string *is* the key. `tr("Trash")` returns the translation for the
//! current language or the key itself. Placeholders are plain `{}`, filled by [`tr_f!`].
//!
//! Adding a language: create `src/i18n/xx.rs` with a `TABLE`, register it in [`table_for`].
//! `tests/i18n.rs` checks that every string passed to `tr(...)` / `tr_f!(...)` is translated.

use std::sync::OnceLock;

mod ru;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Lang {
    En,
    Ru,
}

impl Lang {
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::Ru => "ru",
        }
    }

    pub fn from_code(s: &str) -> Option<Lang> {
        let s = s.trim().to_ascii_lowercase();
        let s = s.split(['_', '-', '.', '@', ':']).next().unwrap_or("");
        match s {
            "en" | "c" | "posix" => Some(Lang::En),
            "ru" => Some(Lang::Ru),
            _ => None,
        }
    }
}

static LANG: OnceLock<Lang> = OnceLock::new();

/// Language chosen from `DISKWIZ_LANG`, then the usual POSIX variables, then the OS API.
pub fn detect() -> Lang {
    for var in ["DISKWIZ_LANG", "LC_ALL", "LC_MESSAGES", "LANGUAGE", "LANG"] {
        if let Ok(v) = std::env::var(var)
            && let Some(l) = v.split(':').find_map(Lang::from_code)
        {
            return l;
        }
    }
    crate::platform::os_locale().and_then(|l| Lang::from_code(&l)).unwrap_or(Lang::En)
}

/// Force a language (used by the elevated helper, whose environment is sanitised).
pub fn set(lang: Lang) {
    let _ = LANG.set(lang);
}

pub fn current() -> Lang {
    *LANG.get_or_init(detect)
}

/// The translation table of `lang` (`None` for English, which is the source language).
pub fn table_for(lang: Lang) -> Option<&'static [(&'static str, &'static str)]> {
    match lang {
        Lang::En => None,
        Lang::Ru => Some(ru::TABLE),
    }
}

/// Translate an English source string.
pub fn tr(s: &'static str) -> &'static str {
    lookup(current(), s)
}

pub fn lookup(lang: Lang, s: &'static str) -> &'static str {
    match table_for(lang) {
        Some(t) => t.iter().find(|(k, _)| *k == s).map(|(_, v)| *v).unwrap_or(s),
        None => s,
    }
}

/// Replace `{}` placeholders in `pattern` with `args`, in order.
pub fn fill(pattern: &str, args: &[String]) -> String {
    let mut out = String::with_capacity(pattern.len() + 16);
    let mut it = args.iter();
    let mut rest = pattern;
    while let Some(pos) = rest.find("{}") {
        out.push_str(&rest[..pos]);
        if let Some(a) = it.next() {
            out.push_str(a);
        }
        rest = &rest[pos + 2..];
    }
    out.push_str(rest);
    out
}

/// `tr_f!("Scanning… {} files · {}", n, size)` — translate, then fill placeholders.
#[macro_export]
macro_rules! tr_f {
    ($pat:literal $(, $arg:expr)* $(,)?) => {
        {
            let args = [$(($arg).to_string()),*];
            $crate::i18n::fill($crate::i18n::tr($pat), &args)
        }
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_replaces_in_order() {
        assert_eq!(fill("a {} b {} c", &["1".into(), "2".into()]), "a 1 b 2 c");
        assert_eq!(fill("no placeholders", &["x".into()]), "no placeholders");
        assert_eq!(fill("{} and {}", &["only".into()]), "only and ");
    }

    #[test]
    fn lang_codes() {
        assert_eq!(Lang::from_code("ru_RU.UTF-8"), Some(Lang::Ru));
        assert_eq!(Lang::from_code("en-US"), Some(Lang::En));
        assert_eq!(Lang::from_code("C"), Some(Lang::En));
        assert_eq!(Lang::from_code("de_DE"), None);
    }

    #[test]
    fn unknown_key_falls_back_to_english() {
        assert_eq!(lookup(Lang::Ru, "definitely not a key"), "definitely not a key");
    }
}
