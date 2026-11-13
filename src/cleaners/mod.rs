//! Cleanup items ("what can be removed").
//!
//! Every item has an id, texts (English source strings, translated by the GUI), a size probe and
//! an action. Items are built per platform and only the ones that make sense on *this* machine
//! are returned (no `apt` entry on Fedora, no `snap` entry without snapd, …).
//!
//! Adding an item: add a [`CleanItem`] to the platform's `items()` function (and its strings to
//! `src/i18n/ru.rs`; `cargo test` tells you if you forget).

use std::path::PathBuf;
use std::sync::OnceLock;

use crate::util::{is_dangerous_target, remove_path};

mod common;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

pub struct CleanItem {
    pub id: &'static str,
    pub title: &'static str,
    pub hint: &'static str,
    /// Symbolic icon name (GTK icon theme).
    pub icon: &'static str,
    /// Needs administrator rights.
    pub root: bool,
    /// Reclaimable bytes; `None` when they cannot be known beforehand.
    pub size: fn() -> Option<u64>,
    /// Do the cleaning; returns a one-line human-readable result.
    pub run: fn() -> String,
}

fn build() -> Vec<CleanItem> {
    let mut v = Vec::new();
    #[cfg(target_os = "linux")]
    v.extend(linux::items());
    #[cfg(target_os = "macos")]
    v.extend(macos::items());
    #[cfg(windows)]
    v.extend(windows::items());
    v
}

/// All cleanup items available on this machine: user-level first, then administrator-level.
pub fn items() -> &'static [CleanItem] {
    static ITEMS: OnceLock<Vec<CleanItem>> = OnceLock::new();
    ITEMS.get_or_init(|| {
        let mut v = build();
        v.sort_by_key(|i| i.root); // stable: keeps the declared order inside each group
        v
    })
}

pub fn find(id: &str) -> Option<&'static CleanItem> {
    items().iter().find(|i| i.id == id)
}

pub enum RootOutcome {
    /// One `id: result` line per item, plus anything the helper printed to stderr.
    Lines(Vec<String>),
    /// Authentication was dismissed or failed.
    Cancelled,
    Failed(String),
}

/// Run administrator-level items by relaunching this executable with elevated rights
/// (polkit on Linux, UAC on Windows, an admin prompt on macOS).
pub fn run_root_items(ids: &[&str]) -> RootOutcome {
    use crate::platform::{self, Elevated};
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => return RootOutcome::Failed(e.to_string()),
    };
    let mut args = vec!["--root".to_string(), "--lang".to_string(), crate::i18n::current().code().to_string()];
    args.extend(ids.iter().filter(|id| find(id).is_some_and(|i| i.root)).map(|s| s.to_string()));
    match platform::run_elevated(&exe, &args) {
        Elevated::Cancelled => RootOutcome::Cancelled,
        Elevated::Failed(e) => RootOutcome::Failed(e),
        Elevated::Done { stdout, stderr, ok } => {
            let mut lines: Vec<String> = stdout.lines().map(String::from).collect();
            if !ok && !stderr.trim().is_empty() {
                lines.push(stderr.trim().to_string());
            }
            RootOutcome::Lines(lines)
        }
    }
}

/// Remove every path in `paths`, refusing anything that looks like a filesystem root or a home dir.
pub(crate) fn remove_all(paths: &[PathBuf]) {
    for p in paths {
        if !is_dangerous_target(p) {
            remove_path(p);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_user_items_come_first() {
        let it = items();
        let mut ids: Vec<_> = it.iter().map(|i| i.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), it.len(), "duplicate item ids");
        let first_root = it.iter().position(|i| i.root).unwrap_or(it.len());
        assert!(it[first_root..].iter().all(|i| i.root));
    }

    #[test]
    fn find_works() {
        for i in items() {
            assert_eq!(find(i.id).unwrap().id, i.id);
        }
        assert!(find("nope").is_none());
    }

    #[test]
    fn remove_all_refuses_dangerous_paths() {
        let d = tempfile::tempdir().unwrap();
        let f = d.path().join("x/y/file");
        std::fs::create_dir_all(f.parent().unwrap()).unwrap();
        std::fs::write(&f, b"1").unwrap();
        remove_all(&[PathBuf::from("/"), PathBuf::from("/usr"), f.clone()]);
        assert!(!f.exists());
        #[cfg(unix)]
        assert!(std::path::Path::new("/usr").exists());
    }
}
