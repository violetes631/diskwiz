//! Linux cleanup items: user caches, trash, Flatpak, systemd journal and core dumps, plus the
//! package manager of the distribution (dnf / apt / pacman / zypper) and snap.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use super::common::existing_caches;
use super::{CleanItem, remove_all};
use crate::i18n::tr;
use crate::tr_f;
use crate::util::{data_dir, list_dir, path_size, paths_size, run, which};

const JOURNAL_KEEP_MB: u64 = 200;
const ARCH: &str = std::env::consts::ARCH;

/// Cache folders (inside `$XDG_CACHE_HOME`) that applications recreate on their own.
/// Add your own in `~/.config/diskwiz/caches.txt`.
const KNOWN_CACHES: &[&str] = &[
    "thumbnails",
    "mozilla",
    "chromium",
    "google-chrome",
    "BraveSoftware",
    "microsoft-edge",
    "spotify",
    "pip",
    "mesa_shader_cache",
    "nvidia",
];

pub fn items() -> Vec<CleanItem> {
    let mut v = vec![
        CleanItem {
            id: "cache",
            title: "Application caches",
            hint: "Browsers, thumbnails, Spotify, pip… — recreated automatically",
            icon: "folder-download-symbolic",
            root: false,
            size: || Some(paths_size(&existing_caches(KNOWN_CACHES))),
            run: || {
                remove_all(&existing_caches(KNOWN_CACHES));
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
    if which("flatpak") {
        v.push(CleanItem {
            id: "flatpak",
            title: "Unused Flatpak runtimes",
            hint: "flatpak uninstall --unused",
            icon: "package-x-generic-symbolic",
            root: false,
            size: || None,
            run: || {
                let o = run("flatpak", &["uninstall", "--unused", "-y"]);
                o.stdout.lines().last().map(str::to_string).unwrap_or_else(|| tr("done").into())
            },
        });
    }
    if Path::new("/var/lib/systemd/coredump").is_dir() {
        v.push(CleanItem {
            id: "coredump",
            title: "Crash dumps",
            hint: "/var/lib/systemd/coredump",
            icon: "dialog-warning-symbolic",
            root: true,
            size: || Some(paths_size(&list_dir(Path::new("/var/lib/systemd/coredump")))),
            run: || {
                remove_all(&list_dir(Path::new("/var/lib/systemd/coredump")));
                tr("crash dumps removed").into()
            },
        });
    }
    if which("journalctl") {
        v.push(CleanItem {
            id: "journal",
            title: "systemd journal",
            hint: "Shrink to 200 MB",
            icon: "text-x-generic-symbolic",
            root: true,
            size: || Some(journal_reclaimable()),
            run: || {
                let o = run("journalctl", &[&format!("--vacuum-size={JOURNAL_KEEP_MB}M")]);
                format!("{}{}", o.stderr.trim(), o.stdout.trim()).lines().last().map(str::to_string).unwrap_or_else(|| tr("done").into())
            },
        });
    }
    v.extend(package_manager_items());
    if which("snap") {
        v.push(CleanItem {
            id: "snap",
            title: "Old snap revisions",
            hint: "Disabled revisions of snap packages",
            icon: "package-x-generic-symbolic",
            root: true,
            size: || Some(old_snaps().iter().map(|(n, r)| fs::metadata(format!("/var/lib/snapd/snaps/{n}_{r}.snap")).map(|m| m.len()).unwrap_or(0)).sum()),
            run: clean_snaps,
        });
    }
    v
}

fn trash_paths() -> Vec<PathBuf> {
    let base = data_dir().join("Trash");
    let mut v = list_dir(&base.join("files"));
    v.extend(list_dir(&base.join("info")));
    v
}

// ---------------------------------------------------------------- package managers

fn package_manager_items() -> Vec<CleanItem> {
    if which("dnf") {
        vec![
            CleanItem {
                id: "pkgcache",
                title: "Package cache (dnf, PackageKit)",
                hint: "Downloaded package metadata",
                icon: "system-software-install-symbolic",
                root: true,
                size: || Some(paths_size(&dnf_cache_paths())),
                run: || {
                    run("dnf", &["clean", "all"]);
                    remove_all(&list_dir(Path::new("/var/cache/PackageKit")));
                    tr("dnf cache cleaned").into()
                },
            },
            CleanItem {
                id: "kernels",
                title: "Old kernels",
                hint: "Keeps the running kernel and one spare",
                icon: "computer-symbolic",
                root: true,
                size: || Some(old_kernels().iter().map(|v| path_size(Path::new(&format!("/usr/lib/modules/{v}.{ARCH}")))).sum()),
                run: clean_kernels,
            },
        ]
    } else if which("apt-get") {
        vec![
            CleanItem {
                id: "pkgcache",
                title: "Package cache (apt)",
                hint: "Downloaded .deb files",
                icon: "system-software-install-symbolic",
                root: true,
                size: || Some(path_size(Path::new("/var/cache/apt/archives"))),
                run: || {
                    let o = run("apt-get", &["clean"]);
                    if o.ok { tr("apt cache cleaned").into() } else { o.stderr.trim().to_string() }
                },
            },
            CleanItem {
                id: "autoremove",
                title: "Unused packages and old kernels",
                hint: "apt autoremove --purge",
                icon: "computer-symbolic",
                root: true,
                size: || Some(apt_autoremove_size()),
                run: || {
                    let o = run("apt-get", &["autoremove", "--purge", "-y"]);
                    if o.ok { tr("unused packages removed").into() } else { o.stderr.trim().to_string() }
                },
            },
        ]
    } else if which("pacman") {
        vec![
            CleanItem {
                id: "pkgcache",
                title: "Package cache (pacman)",
                hint: "Old downloaded packages",
                icon: "system-software-install-symbolic",
                root: true,
                size: || Some(path_size(Path::new("/var/cache/pacman/pkg"))),
                run: || {
                    let o = if which("paccache") { run("paccache", &["-rk1"]) } else { run("pacman", &["-Sc", "--noconfirm"]) };
                    if o.ok { tr("pacman cache cleaned").into() } else { o.stderr.trim().to_string() }
                },
            },
            CleanItem {
                id: "orphans",
                title: "Orphaned packages",
                hint: "pacman -Qdtq",
                icon: "computer-symbolic",
                root: true,
                size: || None,
                run: || {
                    let names: Vec<String> = run("pacman", &["-Qdtq"]).stdout.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect();
                    if names.is_empty() {
                        return tr("no orphaned packages").into();
                    }
                    let mut args = vec!["-Rns", "--noconfirm"];
                    args.extend(names.iter().map(String::as_str));
                    let o = run("pacman", &args);
                    if o.ok { tr_f!("removed: {}", names.join(", ")) } else { o.stderr.trim().to_string() }
                },
            },
        ]
    } else if which("zypper") {
        vec![CleanItem {
            id: "pkgcache",
            title: "Package cache (zypper)",
            hint: "Downloaded packages and metadata",
            icon: "system-software-install-symbolic",
            root: true,
            size: || Some(path_size(Path::new("/var/cache/zypp"))),
            run: || {
                let o = run("zypper", &["--non-interactive", "clean", "--all"]);
                if o.ok { tr("zypper cache cleaned").into() } else { o.stderr.trim().to_string() }
            },
        }]
    } else {
        Vec::new()
    }
}

fn dnf_cache_paths() -> Vec<PathBuf> {
    let mut v = list_dir(Path::new("/var/cache/libdnf5"));
    v.extend(list_dir(Path::new("/var/cache/dnf")));
    v.extend(list_dir(Path::new("/var/cache/PackageKit")));
    v
}

/// "After this operation, 1,234 MB disk space will be freed." → bytes.
fn parse_apt_freed(text: &str) -> u64 {
    let Some(line) = text.lines().find(|l| l.contains("disk space will be freed")) else { return 0 };
    let mut words = line.split_whitespace().skip_while(|w| *w != "operation,").skip(1);
    let Some(num) = words.next() else { return 0 };
    let num: f64 = num.replace(',', "").parse().unwrap_or(0.0);
    let mult = match words.next().unwrap_or("") {
        "B" => 1.0,
        "kB" | "KB" => 1024.0,
        "MB" => 1024.0 * 1024.0,
        "GB" => 1024.0 * 1024.0 * 1024.0,
        _ => return 0,
    };
    (num * mult) as u64
}

fn apt_autoremove_size() -> u64 {
    parse_apt_freed(&run("apt-get", &["-s", "autoremove"]).stdout)
}

// ---------------------------------------------------------------- kernels (dnf)

fn version_key(v: &str) -> Vec<u64> {
    let mut out = Vec::new();
    let mut cur = String::new();
    for c in v.chars() {
        if c.is_ascii_digit() {
            cur.push(c);
        } else if !cur.is_empty() {
            out.push(cur.parse().unwrap_or(0));
            cur.clear();
        }
    }
    if !cur.is_empty() {
        out.push(cur.parse().unwrap_or(0));
    }
    out
}

/// Of `installed` keep the running kernel and the newest other one; return the rest.
fn pick_old_kernels(mut installed: Vec<String>, running: &str) -> Vec<String> {
    installed.sort_by_key(|v| version_key(v));
    installed.dedup();
    let mut keep: HashSet<String> = HashSet::from([running.to_string()]);
    for v in installed.iter().rev() {
        if keep.len() >= 2 {
            break;
        }
        keep.insert(v.clone());
    }
    installed.into_iter().filter(|v| !keep.contains(v)).collect()
}

fn old_kernels() -> Vec<String> {
    let o = run("rpm", &["-q", "--qf", "%{VERSION}-%{RELEASE}\\n", "kernel-core"]);
    let vers: Vec<String> = o.stdout.lines().map(|l| l.trim().to_string()).filter(|l| !l.is_empty() && !l.contains(' ')).collect();
    let running = fs::read_to_string("/proc/sys/kernel/osrelease").unwrap_or_default();
    let running = running.trim().strip_suffix(&format!(".{ARCH}")).unwrap_or(running.trim());
    pick_old_kernels(vers, running)
}

fn clean_kernels() -> String {
    let vers = old_kernels();
    if vers.is_empty() {
        return tr("no old kernels").into();
    }
    let pk: Vec<String> = vers.iter().map(|v| format!("kernel-core-{v}")).collect();
    let mut args = vec!["remove", "-y"];
    args.extend(pk.iter().map(String::as_str));
    let o = run("dnf", &args);
    if o.ok { tr_f!("kernels removed: {}", vers.join(", ")) } else { tr_f!("dnf error: {}", o.stderr.trim()) }
}

// ---------------------------------------------------------------- snap

/// `(name, revision)` of disabled snap revisions.
fn old_snaps() -> Vec<(String, String)> {
    parse_snap_list(&run("snap", &["list", "--all"]).stdout)
}

fn parse_snap_list(text: &str) -> Vec<(String, String)> {
    text.lines()
        .skip(1)
        .filter_map(|l| {
            let p: Vec<&str> = l.split_whitespace().collect();
            (p.len() >= 6 && p.last().is_some_and(|n| n.contains("disabled"))).then(|| (p[0].to_string(), p[2].to_string()))
        })
        .collect()
}

fn clean_snaps() -> String {
    let mut done = Vec::new();
    for (n, r) in old_snaps() {
        if run("snap", &["remove", &n, &format!("--revision={r}")]).ok {
            done.push(format!("{n} r{r}"));
        }
    }
    if done.is_empty() { tr("no old snaps").into() } else { tr_f!("removed: {}", done.join(", ")) }
}

// ---------------------------------------------------------------- journal

/// Parse `journalctl --disk-usage` ("… take up 1.2G in the file system.") and subtract what we keep.
fn parse_journal_reclaimable(text: &str) -> u64 {
    let Some(pos) = text.find("take up ") else { return 0 };
    let tok = text[pos + 8..].split_whitespace().next().unwrap_or("");
    let (num, unit) = tok.split_at(tok.len().saturating_sub(1));
    let mult: f64 = match unit {
        "B" => 1.0,
        "K" => 1024.0,
        "M" => 1024.0 * 1024.0,
        "G" => 1024.0 * 1024.0 * 1024.0,
        "T" => 1024f64.powi(4),
        _ => return 0,
    };
    let total = (num.parse::<f64>().unwrap_or(0.0) * mult) as u64;
    total.saturating_sub(JOURNAL_KEEP_MB * 1024 * 1024)
}

fn journal_reclaimable() -> u64 {
    parse_journal_reclaimable(&run("journalctl", &["--disk-usage"]).stdout)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn journal_parsing() {
        let t = "Archived and active journals take up 1.2G in the file system.";
        assert_eq!(parse_journal_reclaimable(t), (1.2 * 1024.0 * 1024.0 * 1024.0) as u64 - 200 * 1024 * 1024);
        assert_eq!(parse_journal_reclaimable("take up 100.0M in the file system."), 0);
        assert_eq!(parse_journal_reclaimable("garbage"), 0);
    }

    #[test]
    fn apt_parsing() {
        let t = "Reading package lists...\nAfter this operation, 1,234 MB disk space will be freed.\n";
        assert_eq!(parse_apt_freed(t), 1234 * 1024 * 1024);
        assert_eq!(parse_apt_freed("After this operation, 512 kB disk space will be freed."), 512 * 1024);
        assert_eq!(parse_apt_freed("0 upgraded, 0 newly installed"), 0);
    }

    #[test]
    fn kernel_selection_keeps_running_and_newest_spare() {
        let installed: Vec<String> = ["6.8.5-300.fc40", "6.9.7-200.fc40", "6.10.2-100.fc40", "6.9.10-200.fc40"].map(String::from).to_vec();
        // running an old one: keep it and the newest
        let old = pick_old_kernels(installed.clone(), "6.8.5-300.fc40");
        assert_eq!(old, ["6.9.7-200.fc40", "6.9.10-200.fc40"]);
        // running the newest: keep it and the next newest
        let old = pick_old_kernels(installed, "6.10.2-100.fc40");
        assert_eq!(old, ["6.8.5-300.fc40", "6.9.7-200.fc40"]);
        // numeric, not lexicographic: 6.10 > 6.9
        assert!(version_key("6.10.2") > version_key("6.9.10"));
    }

    #[test]
    fn snap_parsing() {
        let t = "Name   Version Rev  Tracking Publisher Notes\n\
                 core22 20240111 1122 latest/stable canonical** base,disabled\n\
                 core22 20240111 1380 latest/stable canonical** base\n";
        assert_eq!(parse_snap_list(t), [("core22".to_string(), "1122".to_string())]);
    }
}
