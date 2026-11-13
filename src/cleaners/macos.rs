//! macOS cleanup items (best effort — not regularly tested).

use std::path::PathBuf;

use super::{CleanItem, remove_all};
use crate::i18n::tr;
use crate::util::{cache_dir, home, list_dir, paths_size, run, which};

fn trash_paths() -> Vec<PathBuf> {
    list_dir(&home().join(".Trash"))
}

fn app_caches() -> Vec<PathBuf> {
    let c = cache_dir();
    ["Firefox", "Google/Chrome", "com.spotify.client", "pip", "Homebrew"]
        .iter()
        .map(|n| c.join(n))
        .chain([home().join("Library/Developer/Xcode/DerivedData")])
        .filter(|p| p.exists())
        .collect()
}

pub fn items() -> Vec<CleanItem> {
    let mut v = vec![
        CleanItem {
            id: "cache",
            title: "Application caches",
            hint: "Browsers, Spotify, Xcode DerivedData… — recreated automatically",
            icon: "folder-download-symbolic",
            root: false,
            size: || Some(paths_size(&app_caches())),
            run: || {
                remove_all(&app_caches());
                tr("application caches removed").into()
            },
        },
        CleanItem {
            id: "trash",
            title: "Trash",
            hint: "Deleted files in your trash",
            icon: "user-trash-symbolic",
            root: false,
            size: || Some(paths_size(&trash_paths())),
            run: || {
                remove_all(&trash_paths());
                tr("trash emptied").into()
            },
        },
    ];
    if which("brew") {
        v.push(CleanItem {
            id: "brew",
            title: "Homebrew downloads",
            hint: "brew cleanup",
            icon: "system-software-install-symbolic",
            root: false,
            size: || None,
            run: || {
                let o = run("brew", &["cleanup", "--prune=all"]);
                o.stdout.lines().last().map(str::to_string).unwrap_or_else(|| tr("done").into())
            },
        });
    }
    v
}
