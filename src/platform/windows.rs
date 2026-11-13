//! Windows (drive letters, UAC through PowerShell).
//!
//! NOTE: this file is compiled only on Windows and is exercised by the Windows CI job.

use std::collections::HashSet;
use std::fs::Metadata;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
use windows_sys::Win32::Storage::FileSystem::{
    GetDiskFreeSpaceExW, GetDriveTypeW, GetLogicalDrives,
};
use windows_sys::Win32::System::Console::{ATTACH_PARENT_PROCESS, AttachConsole};

use super::{DiskEntry, Elevated};
use crate::util::human;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const DRIVE_REMOVABLE: u32 = 2;
const DRIVE_FIXED: u32 = 3;
const DRIVE_REMOTE: u32 = 4;

fn wide(s: &str) -> Vec<u16> {
    std::ffi::OsStr::new(s).encode_wide().chain(std::iter::once(0)).collect()
}

/// std has no stable allocated-size API on Windows; the logical length is a good approximation.
pub fn alloc_size(md: &Metadata) -> u64 {
    md.file_size()
}

pub fn hardlink_id(_md: &Metadata) -> Option<(u64, u64)> {
    None
}

pub fn disk_usage(path: &str) -> (u64, u64, u64) {
    let p = wide(path);
    let (mut avail, mut total, mut free) = (0u64, 0u64, 0u64);
    // SAFETY: `p` is NUL-terminated, the out-pointers are valid for the call.
    let ok = unsafe { GetDiskFreeSpaceExW(p.as_ptr(), &mut avail, &mut total, &mut free) };
    if ok == 0 { (0, 0, 0) } else { (total, total.saturating_sub(free), avail) }
}

pub fn skip_set(_root: &str) -> HashSet<PathBuf> {
    HashSet::new()
}

pub fn list_disks() -> Vec<DiskEntry> {
    // SAFETY: plain Win32 calls without pointers to our memory (other than NUL-terminated strings).
    let mask = unsafe { GetLogicalDrives() };
    let mut v = Vec::new();
    for i in 0..26u32 {
        if mask & (1 << i) == 0 {
            continue;
        }
        let root = format!("{}:\\", (b'A' + i as u8) as char);
        let kind = unsafe { GetDriveTypeW(wide(&root).as_ptr()) };
        let kind_name = match kind {
            DRIVE_FIXED => "Local disk",
            DRIVE_REMOVABLE => "Removable",
            DRIVE_REMOTE => "Network",
            _ => continue,
        };
        let (total, _, _) = disk_usage(&root);
        if total == 0 {
            continue; // empty card reader, disconnected share
        }
        v.push(DiskEntry {
            title: root.clone(),
            sub: format!("{}, {}", crate::i18n::lookup(crate::i18n::current(), kind_name), human(total)),
            dev: Some(root.clone()),
            mount: Some(root),
        });
    }
    v
}

/// Windows drives are always mounted.
pub fn mount_ro(dev: &str) -> Result<String, String> {
    Ok(dev.to_string())
}

pub fn unmount(_dev: &str) {}

pub fn open_path(p: &Path) {
    let _ = Command::new("explorer").arg(p).creation_flags(CREATE_NO_WINDOW).spawn();
}

pub fn os_locale() -> Option<String> {
    let mut buf = [0u16; 85];
    // SAFETY: buffer length is passed along; LOCALE_NAME_MAX_LENGTH is 85.
    let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
    (n > 1).then(|| String::from_utf16_lossy(&buf[..n as usize - 1]))
}

pub fn attach_console() {
    // SAFETY: no pointers involved.
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

/// PowerShell single-quote escaping.
fn ps_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "''"))
}

/// Re-launch `exe` through UAC. An elevated process cannot share pipes with us, so the helper is
/// told (`--out FILE`) to write its report to a temporary file which we read afterwards.
pub fn run_elevated(exe: &Path, args: &[String]) -> Elevated {
    let out = std::env::temp_dir().join(format!("diskwiz-{}.out", std::process::id()));
    let _ = std::fs::remove_file(&out);
    let mut full: Vec<String> = vec!["--out".into(), out.to_string_lossy().into_owned()];
    full.extend(args.iter().cloned());
    let arglist = full.iter().map(|a| ps_quote(a)).collect::<Vec<_>>().join(",");
    let script = format!(
        "Start-Process -FilePath {} -ArgumentList {} -Verb RunAs -Wait -WindowStyle Hidden",
        ps_quote(&exe.to_string_lossy()),
        arglist
    );
    let res = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", &script])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    let report = std::fs::read_to_string(&out);
    let _ = std::fs::remove_file(&out);
    match (res, report) {
        (Ok(_), Ok(text)) => Elevated::Done { stdout: text, stderr: String::new(), ok: true },
        // PowerShell failed and the helper never ran: the UAC prompt was declined.
        (Ok(_), Err(_)) => Elevated::Cancelled,
        (Err(e), _) => Elevated::Failed(format!("powershell: {e}")),
    }
}
