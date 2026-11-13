//! Pieces shared by every unix-like system.

use std::collections::HashSet;
use std::ffi::CString;
use std::fs::Metadata;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::{DiskEntry, Elevated};

pub fn alloc_size(md: &Metadata) -> u64 {
    md.blocks() * 512
}

pub fn hardlink_id(md: &Metadata) -> Option<(u64, u64)> {
    (md.nlink() > 1).then(|| (md.dev(), md.ino()))
}

/// Same numbers `df` shows: size = blocks, used = blocks − free, avail = what a normal user may use.
pub fn disk_usage(path: &str) -> (u64, u64, u64) {
    let Ok(c) = CString::new(Path::new(path).as_os_str().as_bytes()) else { return (0, 0, 0) };
    let mut st: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated string and `st` is a properly sized out-parameter.
    if unsafe { libc::statvfs(c.as_ptr(), &mut st) } != 0 {
        return (0, 0, 0);
    }
    let frsize = st.f_frsize as u64;
    let total = st.f_blocks as u64 * frsize;
    let used = (st.f_blocks as u64).saturating_sub(st.f_bfree as u64) * frsize;
    let avail = st.f_bavail as u64 * frsize;
    (total, used, avail)
}

/// Generic unix fallback: nothing is skipped.
#[allow(dead_code)]
pub fn skip_set(_root: &str) -> HashSet<PathBuf> {
    HashSet::new()
}

#[allow(dead_code)]
pub fn list_disks() -> Vec<DiskEntry> {
    vec![DiskEntry { title: "/".into(), sub: String::new(), dev: None, mount: Some("/".into()) }]
}

#[allow(dead_code)]
pub fn mount_ro(_dev: &str) -> Result<String, String> {
    Err("mounting is not supported on this system".into())
}

#[allow(dead_code)]
pub fn unmount(_dev: &str) {}

pub fn open_path(p: &Path) {
    let opener = if cfg!(target_os = "macos") { "open" } else { "xdg-open" };
    let _ = Command::new(opener).arg(p).spawn();
}

#[allow(dead_code)]
pub fn run_elevated(_exe: &Path, _args: &[String]) -> Elevated {
    Elevated::Failed("privilege elevation is not supported on this system".into())
}

/// Rust ignores SIGPIPE, which turns `| head` into a panic inside `println!`.
pub fn restore_sigpipe() {
    // SAFETY: resetting a signal disposition to the default is always sound.
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }
}

pub fn os_locale() -> Option<String> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disk_usage_of_root_is_sane() {
        let (total, used, avail) = disk_usage("/");
        assert!(total > 0);
        assert!(used <= total);
        assert!(avail <= total);
    }

    #[test]
    fn disk_usage_of_missing_path_is_zero() {
        assert_eq!(disk_usage("/definitely/not/here"), (0, 0, 0));
    }
}
