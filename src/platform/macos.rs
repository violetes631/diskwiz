//! macOS (best effort — not regularly tested).

use std::path::Path;
use std::process::Command;

use super::{DiskEntry, Elevated};
use crate::util::human;

pub use super::unix::{
    alloc_size, disk_usage, hardlink_id, mount_ro, open_path, os_locale, skip_set, unmount,
};

pub fn list_disks() -> Vec<DiskEntry> {
    let mut v = vec![DiskEntry {
        title: "Macintosh HD · /".into(),
        sub: human(super::disk_usage("/").0),
        dev: None,
        mount: Some("/".into()),
    }];
    if let Ok(rd) = std::fs::read_dir("/Volumes") {
        for e in rd.flatten() {
            let p = e.path();
            // /Volumes/Macintosh HD is a symlink to "/".
            if p.is_dir() && !std::fs::symlink_metadata(&p).map(|m| m.file_type().is_symlink()).unwrap_or(false) {
                let mp = p.to_string_lossy().into_owned();
                v.push(DiskEntry {
                    title: format!("{} · {mp}", e.file_name().to_string_lossy()),
                    sub: human(super::disk_usage(&mp).0),
                    dev: None,
                    mount: Some(mp),
                });
            }
        }
    }
    v
}

pub fn run_elevated(exe: &Path, args: &[String]) -> Elevated {
    let q = |s: &str| format!("'{}'", s.replace('\'', "'\\''"));
    let mut cmd = q(&exe.to_string_lossy());
    for a in args {
        cmd.push(' ');
        cmd.push_str(&q(a));
    }
    let script = format!("do shell script \"{}\" with administrator privileges", cmd.replace('\\', "\\\\").replace('"', "\\\""));
    match Command::new("osascript").args(["-e", &script]).output() {
        Ok(o) if o.status.success() => Elevated::Done {
            stdout: String::from_utf8_lossy(&o.stdout).into_owned(),
            stderr: String::new(),
            ok: true,
        },
        Ok(o) => {
            let err = String::from_utf8_lossy(&o.stderr).into_owned();
            if err.contains("-128") { Elevated::Cancelled } else { Elevated::Done { stdout: String::new(), stderr: err, ok: false } }
        }
        Err(e) => Elevated::Failed(format!("osascript: {e}")),
    }
}
