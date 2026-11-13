//! Every user-visible string must have a translation in every language.
//!
//! The test scans the sources for `tr("…")`, `tr_f!("…")` and the `title:` / `hint:` fields of
//! cleanup items, so forgetting a translation fails `cargo test`.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;

use diskwiz::i18n::{Lang, table_for};

/// `(path, contents)` of every source file except the translation tables.
fn rust_files(dir: &Path, out: &mut Vec<(String, String)>) {
    for e in fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("ru.rs") {
            out.push((p.to_string_lossy().replace('\\', "/"), fs::read_to_string(&p).unwrap()));
        }
    }
}

/// Read a Rust string literal at the start of `s` (after optional whitespace).
fn literal(s: &str) -> Option<String> {
    let s = s.trim_start().strip_prefix('"')?;
    let mut out = String::new();
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                '"' => out.push('"'),
                '\\' => out.push('\\'),
                other => {
                    out.push('\\');
                    out.push(other);
                }
            },
            c => out.push(c),
        }
    }
    None
}

fn used_keys() -> BTreeSet<String> {
    let mut files = Vec::new();
    rust_files(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    let mut keys: BTreeSet<String> = BTreeSet::new();
    for (path, src) in &files {
        for pat in ["tr(", "tr_f!(", "title: ", "hint: "] {
            // `title:` / `hint:` are only translatable inside cleanup item definitions
            if pat.ends_with(": ") && !path.contains("/cleaners/") {
                continue;
            }
            for (pos, _) in src.match_indices(pat) {
                // `pat` must start a token: not the tail of `str(`, `fn tr(`, `lookup(` …
                let prev = src[..pos].chars().next_back();
                if pat.starts_with("tr") && prev.is_some_and(|c| c.is_alphanumeric() || c == '_') {
                    continue;
                }
                if let Some(l) = literal(&src[pos + pat.len()..]) {
                    keys.insert(l);
                }
            }
        }
    }
    // strings that reach `lookup()` through variables
    keys.extend(["Local disk", "Removable", "Network", "B", "KiB", "MiB", "GiB", "TiB"].map(String::from));
    keys
}

#[test]
fn every_used_string_is_translated() {
    let keys = used_keys();
    assert!(keys.len() > 50, "scanner found only {} keys — is it broken?", keys.len());
    for lang in [Lang::Ru] {
        let table = table_for(lang).unwrap();
        let missing: Vec<_> = keys.iter().filter(|k| !table.iter().any(|(t, _)| t == k)).collect();
        assert!(missing.is_empty(), "missing {} translations:\n{:#?}", lang.code(), missing);
    }
}

#[test]
fn no_stale_or_duplicate_translations() {
    let keys = used_keys();
    for lang in [Lang::Ru] {
        let table = table_for(lang).unwrap();
        let mut seen = BTreeSet::new();
        for (k, v) in table {
            assert!(seen.insert(*k), "duplicate key in {}: {k:?}", lang.code());
            assert!(!v.is_empty(), "empty translation for {k:?}");
            assert_eq!(k.matches("{}").count(), v.matches("{}").count(), "placeholder mismatch: {k:?}");
        }
        let stale: Vec<_> = table.iter().map(|(k, _)| *k).filter(|k| !keys.contains(*k)).collect();
        assert!(stale.is_empty(), "unused {} translations: {stale:#?}", lang.code());
    }
}
