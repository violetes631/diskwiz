//! Small helpers shared by the whole crate.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::i18n::{Lang, lookup};
use crate::platform;

/// 1536 -> "1.5 KiB".
pub fn human(n: u64) -> String {
    human_in(n, crate::i18n::current())
}

pub fn human_in(n: u64, lang: Lang) -> String {
    let units = ["B", "KiB", "MiB", "GiB", "TiB"].map(|u| lookup(lang, u));
    let mut v = n as f64;
    let mut i = 0;
    while v >= 1024.0 && i < units.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 { format!("{} {}", n, units[0]) } else { format!("{:.1} {}", v, units[i]) }
}

/// 1234567 -> "1 234 567".
pub fn spaced(n: u64) -> String {
    let s = n.to_string();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i) % 3 == 0 {
            out.push(' ');
        }
        out.push(c);
    }
    out
}

pub fn home() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "C:\\" } else { "/" }))
}

fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from).filter(|p| p.is_absolute())
}

/// Per-user cache directory (`$XDG_CACHE_HOME`, `%LOCALAPPDATA%`, `~/Library/Caches`).
pub fn cache_dir() -> PathBuf {
    if cfg!(windows) {
        env_dir("LOCALAPPDATA").unwrap_or_else(|| home().join("AppData\\Local"))
    } else if cfg!(target_os = "macos") {
        home().join("Library/Caches")
    } else {
        env_dir("XDG_CACHE_HOME").unwrap_or_else(|| home().join(".cache"))
    }
}

/// Per-user data directory (`$XDG_DATA_HOME`, `%LOCALAPPDATA%`, `~/Library/Application Support`).
pub fn data_dir() -> PathBuf {
    if cfg!(windows) {
        env_dir("LOCALAPPDATA").unwrap_or_else(|| home().join("AppData\\Local"))
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        env_dir("XDG_DATA_HOME").unwrap_or_else(|| home().join(".local/share"))
    }
}

/// Per-user configuration directory.
pub fn config_dir() -> PathBuf {
    if cfg!(windows) {
        env_dir("APPDATA").unwrap_or_else(|| home().join("AppData\\Roaming"))
    } else if cfg!(target_os = "macos") {
        home().join("Library/Application Support")
    } else {
        env_dir("XDG_CONFIG_HOME").unwrap_or_else(|| home().join(".config"))
    }
}

pub struct Output {
    pub ok: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Run a command and capture its output. The locale is forced to `C` so that anything we
/// parse (apt, journalctl, snap, …) has a stable format.
pub fn run(cmd: &str, args: &[&str]) -> Output {
    match Command::new(cmd).args(args).env("LC_ALL", "C").env("LANG", "C").output() {
        Ok(o) => Output {
            ok: o.status.success(),
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
        },
        Err(e) => Output { ok: false, stdout: String::new(), stderr: e.to_string() },
    }
}

/// Is `cmd` an executable on `PATH` (plus the sbin dirs a non-root PATH often lacks)?
pub fn which(cmd: &str) -> bool {
    let mut dirs: Vec<PathBuf> =
        std::env::var_os("PATH").map(|p| std::env::split_paths(&p).collect()).unwrap_or_default();
    if cfg!(unix) {
        dirs.extend(["/usr/sbin", "/sbin", "/usr/local/sbin"].map(PathBuf::from));
    }
    let names: Vec<String> =
        if cfg!(windows) { vec![format!("{cmd}.exe"), format!("{cmd}.cmd"), cmd.to_string()] } else { vec![cmd.to_string()] };
    dirs.iter().any(|d| names.iter().any(|n| d.join(n).is_file()))
}

/// Allocated size of a path (recursively), like `du`. Symlinks are not followed.
pub fn path_size(p: &Path) -> u64 {
    let Ok(md) = fs::symlink_metadata(p) else { return 0 };
    let mut total = platform::alloc_size(&md);
    if md.is_dir() {
        if let Ok(rd) = fs::read_dir(p) {
            for e in rd.flatten() {
                total += match e.metadata() {
                    Ok(m) if m.is_dir() => path_size(&e.path()),
                    Ok(m) => platform::alloc_size(&m),
                    Err(_) => 0,
                };
            }
        }
    }
    total
}

pub fn paths_size(v: &[PathBuf]) -> u64 {
    v.iter().map(|p| path_size(p)).sum()
}

/// Remove a file or a whole directory tree; errors (busy files, permissions) are ignored.
pub fn remove_path(p: &Path) {
    match fs::symlink_metadata(p) {
        Ok(md) if md.is_dir() => {
            let _ = fs::remove_dir_all(p);
        }
        Ok(_) => {
            let _ = fs::remove_file(p);
        }
        Err(_) => {}
    }
}

/// Immediate children of `dir` (empty if it cannot be read).
pub fn list_dir(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir).map(|rd| rd.flatten().map(|e| e.path()).collect()).unwrap_or_default()
}

/// Is deleting `p` obviously a terrible idea (filesystem root, home, a drive root)?
pub fn is_dangerous_target(p: &Path) -> bool {
    p.parent().is_none() || p == home() || p.components().count() < 3
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn human_formats() {
        let h = |n| human_in(n, Lang::En);
        assert_eq!(h(0), "0 B");
        assert_eq!(h(1023), "1023 B");
        assert_eq!(h(1024), "1.0 KiB");
        assert_eq!(h(1536), "1.5 KiB");
        assert_eq!(h(5 * 1024 * 1024 * 1024), "5.0 GiB");
        assert_eq!(human_in(1024, Lang::Ru), "1.0 КиБ");
    }

    #[test]
    fn spaced_groups_thousands() {
        assert_eq!(spaced(0), "0");
        assert_eq!(spaced(999), "999");
        assert_eq!(spaced(1000), "1 000");
        assert_eq!(spaced(1234567), "1 234 567");
    }

    #[test]
    fn dangerous_targets() {
        assert!(is_dangerous_target(Path::new("/")));
        assert!(is_dangerous_target(Path::new("/usr")));
        assert!(!is_dangerous_target(Path::new("/var/cache/foo")));
    }

    #[test]
    fn path_size_counts_nested_files() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir(d.path().join("a")).unwrap();
        std::fs::write(d.path().join("a/f"), vec![1u8; 100_000]).unwrap();
        assert!(path_size(d.path()) >= 100_000);
        remove_path(&d.path().join("a"));
        assert!(!d.path().join("a").exists());
    }
}
