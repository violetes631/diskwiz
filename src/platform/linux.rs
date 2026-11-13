//! Linux: /proc/self/mountinfo, lsblk, udisks, polkit.

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{DiskEntry, Elevated};
use crate::i18n::tr;
use crate::util::{human, run};

pub use super::unix::{alloc_size, disk_usage, hardlink_id, open_path, os_locale};

const REAL_FS: [&str; 14] = [
    "btrfs", "ext2", "ext3", "ext4", "xfs", "f2fs", "ntfs", "ntfs3", "fuseblk", "vfat", "exfat", "zfs", "jfs",
    "reiserfs",
];

/// Decode the `\040`-style octal escapes used in /proc/self/mountinfo.
fn unescape_mount(s: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == b'\\' && i + 3 < b.len() && b[i + 1..=i + 3].iter().all(|c| (b'0'..=b'7').contains(c)) {
            out.push((b[i + 1] - b'0') * 64 + (b[i + 2] - b'0') * 8 + (b[i + 3] - b'0'));
            i += 4;
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// `[(mount point, fstype, source)]` parsed from mountinfo text.
fn parse_mountinfo(text: &str) -> Vec<(String, String, String)> {
    text.lines()
        .filter_map(|line| {
            let (l, r) = line.split_once(" - ")?;
            let lf: Vec<&str> = l.split_whitespace().collect();
            let rf: Vec<&str> = r.split_whitespace().collect();
            (lf.len() > 4 && rf.len() >= 2).then(|| (unescape_mount(lf[4]), rf[0].to_string(), rf[1].to_string()))
        })
        .collect()
}

fn read_mounts() -> Vec<(String, String, String)> {
    parse_mountinfo(&fs::read_to_string("/proc/self/mountinfo").unwrap_or_default())
}

/// Parse one line of `lsblk -P`: `KEY="value" KEY2="value2" …`.
fn parse_pairs(line: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    let mut rest = line;
    while let Some(eq) = rest.find("=\"") {
        let key = rest[..eq].trim().to_string();
        let after = &rest[eq + 2..];
        let Some(end) = after.find('"') else { break };
        m.insert(key, after[..end].to_string());
        rest = &after[end + 1..];
    }
    m
}

pub fn list_disks() -> Vec<DiskEntry> {
    let out = run("lsblk", &["-P", "-b", "-o", "PATH,FSTYPE,SIZE,LABEL,UUID"]).stdout;
    let devs: Vec<_> = out.lines().map(parse_pairs).collect();
    let find = |path: &str| devs.iter().find(|d| d.get("PATH").map(|s| s.as_str()) == Some(path));

    let mut by_src: Vec<(String, Vec<String>)> = Vec::new();
    for (mp, fs, src) in &read_mounts() {
        if REAL_FS.contains(&fs.as_str()) && src.starts_with("/dev/") {
            match by_src.iter_mut().find(|(s, _)| s == src) {
                Some((_, v)) => v.push(mp.clone()),
                None => by_src.push((src.clone(), vec![mp.clone()])),
            }
        }
    }

    let mut entries = Vec::new();
    let mut seen_uuid = HashSet::new();
    for (src, mps) in &by_src {
        let Some(mp) = mps.iter().min_by_key(|m| m.len()).cloned() else { continue };
        let d = find(src);
        if let Some(u) = d.and_then(|d| d.get("UUID")).filter(|u| !u.is_empty()) {
            seen_uuid.insert(u.clone());
        }
        let (total, _, _) = disk_usage(&mp);
        let label = d
            .and_then(|d| d.get("LABEL"))
            .filter(|l| !l.is_empty())
            .cloned()
            .unwrap_or_else(|| src.trim_start_matches("/dev/").to_string());
        let fsname = d.and_then(|d| d.get("FSTYPE")).cloned().unwrap_or_default();
        entries.push(DiskEntry {
            title: format!("{label} · {mp}"),
            sub: format!("{}, {}, {}", src.trim_start_matches("/dev/"), fsname, human(total)),
            dev: Some(src.clone()),
            mount: Some(mp),
        });
    }
    for d in &devs {
        let path = d.get("PATH").cloned().unwrap_or_default();
        let fsname = d.get("FSTYPE").cloned().unwrap_or_default();
        let uuid = d.get("UUID").cloned().unwrap_or_default();
        if REAL_FS.contains(&fsname.as_str()) && !by_src.iter().any(|(s, _)| *s == path) && !seen_uuid.contains(&uuid) {
            let size: u64 = d.get("SIZE").and_then(|s| s.parse().ok()).unwrap_or(0);
            let label = d
                .get("LABEL")
                .filter(|l| !l.is_empty())
                .cloned()
                .unwrap_or_else(|| path.trim_start_matches("/dev/").to_string());
            entries.push(DiskEntry {
                title: format!("{label} · {}", tr("not mounted")),
                sub: format!("{}, {}, {}", path.trim_start_matches("/dev/"), fsname, human(size)),
                dev: Some(path),
                mount: None,
            });
        }
    }
    entries.sort_by_key(|e| (e.mount.is_none(), e.mount.clone().unwrap_or_default()));
    entries
}

/// Mount a partition read-only through udisks; returns the mount point.
pub fn mount_ro(dev: &str) -> Result<String, String> {
    let o = run("udisksctl", &["mount", "-b", dev, "-o", "ro"]);
    if o.ok
        && let Some(pos) = o.stdout.find(" at ")
    {
        return Ok(o.stdout[pos + 4..].trim().trim_end_matches('.').to_string());
    }
    Err(if o.stderr.trim().is_empty() { o.stdout } else { o.stderr }.trim().to_string())
}

pub fn unmount(dev: &str) {
    run("udisksctl", &["unmount", "-b", dev]);
}

/// Pure part of [`skip_set`]: mount points inside `root` that live on another source.
fn skip_from(mounts: &[(String, String, String)], root: &str) -> HashSet<PathBuf> {
    let src = mounts
        .iter()
        .filter(|(mp, _, _)| root == mp || root.starts_with(&format!("{}/", mp.trim_end_matches('/'))))
        .max_by_key(|(mp, _, _)| mp.len())
        .map(|(_, _, s)| s.clone());
    let Some(src) = src else { return HashSet::new() };
    let prefix = format!("{}/", root.trim_end_matches('/'));
    mounts
        .iter()
        .filter(|(mp, _, s)| mp != root && mp.starts_with(&prefix) && *s != src)
        .map(|(mp, _, _)| PathBuf::from(mp))
        .collect()
}

pub fn skip_set(root: &str) -> HashSet<PathBuf> {
    skip_from(&read_mounts(), root)
}

pub fn run_elevated(exe: &Path, args: &[String]) -> Elevated {
    match Command::new("pkexec").arg(exe).args(args).output() {
        // 126: dialog dismissed, 127: not authorized / authentication failed.
        Ok(o) if matches!(o.status.code(), Some(126) | Some(127)) => Elevated::Cancelled,
        Ok(o) => Elevated::Done {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&o.stderr).into_owned(),
            ok: o.status.success(),
        },
        Err(e) => Elevated::Failed(format!("pkexec: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unescapes_octal() {
        assert_eq!(unescape_mount("/mnt/my\\040disk"), "/mnt/my disk");
        assert_eq!(unescape_mount("/plain"), "/plain");
        assert_eq!(unescape_mount("/trailing\\04"), "/trailing\\04");
    }

    #[test]
    fn parses_mountinfo_and_skips_foreign_mounts() {
        let text = "\
26 1 0:24 /root / rw,relatime shared:1 - btrfs /dev/nvme0n1p3 rw
27 26 0:24 /home /home rw,relatime shared:2 - btrfs /dev/nvme0n1p3 rw
30 26 259:1 / /boot/efi rw,relatime shared:3 - vfat /dev/nvme0n1p1 rw
31 26 259:2 / /mnt/my\\040usb rw - ext4 /dev/sdb1 rw";
        let m = parse_mountinfo(text);
        assert_eq!(m.len(), 4);
        assert_eq!(m[3].0, "/mnt/my usb");
        let skip = skip_from(&m, "/");
        // btrfs subvolume /home is the same source → scanned; other devices are skipped.
        assert!(!skip.contains(Path::new("/home")));
        assert!(skip.contains(Path::new("/boot/efi")));
        assert!(skip.contains(Path::new("/mnt/my usb")));
    }

    #[test]
    fn parses_lsblk_pairs() {
        let m = parse_pairs("PATH=\"/dev/sda1\" FSTYPE=\"ext4\" SIZE=\"1000\" LABEL=\"\" UUID=\"abc\"");
        assert_eq!(m["PATH"], "/dev/sda1");
        assert_eq!(m["LABEL"], "");
        assert_eq!(m["UUID"], "abc");
    }
}
