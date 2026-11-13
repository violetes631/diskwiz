//! Windows cleanup items.
//!
//! NOTE: compiled only on Windows (exercised by the Windows CI job).

use std::path::PathBuf;

use super::common::existing_caches;
use super::{CleanItem, remove_all};
use crate::i18n::tr;
use crate::util::{cache_dir, list_dir, paths_size, run};

fn env_path(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

fn windir() -> PathBuf {
    env_path("SystemRoot").unwrap_or_else(|| PathBuf::from("C:\\Windows"))
}

fn user_temp() -> Vec<PathBuf> {
    list_dir(&std::env::temp_dir())
}

/// Chromium-family cache folders inside `%LOCALAPPDATA%`.
fn browser_caches() -> Vec<PathBuf> {
    let local = cache_dir();
    let mut v = Vec::new();
    for base in ["Google\\Chrome", "Microsoft\\Edge", "BraveSoftware\\Brave-Browser", "Chromium"] {
        let user_data = local.join(base).join("User Data");
        for profile in list_dir(&user_data) {
            let name = profile.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
            if name == "Default" || name.starts_with("Profile ") {
                for sub in ["Cache", "Code Cache", "GPUCache"] {
                    v.push(profile.join(sub));
                }
            }
        }
    }
    for p in list_dir(&local.join("Mozilla\\Firefox\\Profiles")) {
        v.push(p.join("cache2"));
    }
    v.extend(existing_caches(&[]));
    v.retain(|p| p.exists());
    v
}

fn thumbnail_cache() -> Vec<PathBuf> {
    let dir = cache_dir().join("Microsoft\\Windows\\Explorer");
    list_dir(&dir)
        .into_iter()
        .filter(|p| p.file_name().is_some_and(|n| n.to_string_lossy().to_ascii_lowercase().starts_with("thumbcache_")))
        .collect()
}

/// `X:\$Recycle.Bin\<SID>` of every fixed drive that the current user can read.
fn recycle_bins() -> Vec<PathBuf> {
    (b'A'..=b'Z')
        .map(|c| PathBuf::from(format!("{}:\\$Recycle.Bin", c as char)))
        .filter(|p| p.is_dir())
        .flat_map(|p| list_dir(&p))
        .filter(|p| p.is_dir())
        .collect()
}

fn update_cache() -> Vec<PathBuf> {
    list_dir(&windir().join("SoftwareDistribution\\Download"))
}

fn delivery_optimization() -> Vec<PathBuf> {
    list_dir(&windir().join("ServiceProfiles\\NetworkService\\AppData\\Local\\Microsoft\\Windows\\DeliveryOptimization\\Cache"))
}

fn system_temp() -> Vec<PathBuf> {
    list_dir(&windir().join("Temp"))
}

fn error_reports() -> Vec<PathBuf> {
    let base = env_path("ProgramData").unwrap_or_else(|| PathBuf::from("C:\\ProgramData"));
    list_dir(&base.join("Microsoft\\Windows\\WER"))
}

fn powershell(cmd: &str) -> bool {
    run("powershell", &["-NoProfile", "-NonInteractive", "-Command", cmd]).ok
}

pub fn items() -> Vec<CleanItem> {
    vec![
        CleanItem {
            id: "temp",
            title: "Temporary files",
            hint: "%TEMP% — files in use are skipped",
            icon: "folder-download-symbolic",
            root: false,
            size: || Some(paths_size(&user_temp())),
            run: || {
                remove_all(&user_temp());
                tr("temporary files removed").into()
            },
        },
        CleanItem {
            id: "cache",
            title: "Browser caches",
            hint: "Chrome, Edge, Brave, Firefox — recreated automatically",
            icon: "web-browser-symbolic",
            root: false,
            size: || Some(paths_size(&browser_caches())),
            run: || {
                remove_all(&browser_caches());
                tr("application caches removed").into()
            },
        },
        CleanItem {
            id: "thumbnails",
            title: "Thumbnail cache",
            hint: "Explorer thumbnails — rebuilt on demand",
            icon: "image-x-generic-symbolic",
            root: false,
            size: || Some(paths_size(&thumbnail_cache())),
            run: || {
                remove_all(&thumbnail_cache());
                tr("thumbnail cache removed").into()
            },
        },
        CleanItem {
            id: "trash",
            title: "Recycle Bin",
            hint: "Deleted files in the Recycle Bin",
            icon: "user-trash-symbolic",
            root: false,
            size: || Some(paths_size(&recycle_bins())),
            run: || {
                if powershell("Clear-RecycleBin -Force -ErrorAction SilentlyContinue") { tr("trash emptied").into() } else { tr("done").into() }
            },
        },
        CleanItem {
            id: "winupdate",
            title: "Windows Update downloads",
            hint: "SoftwareDistribution\\Download",
            icon: "system-software-install-symbolic",
            root: true,
            size: || Some(paths_size(&update_cache())),
            run: || {
                run("net", &["stop", "wuauserv"]);
                remove_all(&update_cache());
                run("net", &["start", "wuauserv"]);
                tr("Windows Update cache removed").into()
            },
        },
        CleanItem {
            id: "deliveryopt",
            title: "Delivery Optimization cache",
            hint: "Windows Update peer-to-peer cache",
            icon: "folder-download-symbolic",
            root: true,
            size: || Some(paths_size(&delivery_optimization())),
            run: || {
                remove_all(&delivery_optimization());
                tr("Delivery Optimization cache removed").into()
            },
        },
        CleanItem {
            id: "systemtemp",
            title: "System temporary files",
            hint: "C:\\Windows\\Temp",
            icon: "folder-symbolic",
            root: true,
            size: || Some(paths_size(&system_temp())),
            run: || {
                remove_all(&system_temp());
                tr("temporary files removed").into()
            },
        },
        CleanItem {
            id: "wer",
            title: "Error reports",
            hint: "Windows Error Reporting archives",
            icon: "dialog-warning-symbolic",
            root: true,
            size: || Some(paths_size(&error_reports())),
            run: || {
                remove_all(&error_reports());
                tr("error reports removed").into()
            },
        },
        CleanItem {
            id: "winsxs",
            title: "Component store cleanup",
            hint: "DISM /StartComponentCleanup (can take a few minutes)",
            icon: "computer-symbolic",
            root: true,
            size: || None,
            run: || {
                if run("Dism.exe", &["/Online", "/Cleanup-Image", "/StartComponentCleanup"]).ok {
                    tr("component store cleaned").into()
                } else {
                    tr("DISM failed").into()
                }
            },
        },
    ]
}
