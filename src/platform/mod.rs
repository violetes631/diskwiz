//! Everything that differs between operating systems lives behind this module.
//!
//! * `unix`    — sizes via `st_blocks`, `statvfs`, hard-link ids, `xdg-open` / `open`
//! * `linux`   — mountinfo, lsblk, udisks, polkit (`pkexec`)
//! * `macos`   — volumes, `osascript` elevation
//! * `windows` — drive letters, `GetDiskFreeSpaceExW`, UAC elevation via PowerShell

use std::collections::HashSet;
use std::fs::Metadata;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(unix)]
mod unix;
#[cfg(windows)]
mod windows;

#[cfg(target_os = "linux")]
use linux as sys;
#[cfg(target_os = "macos")]
use macos as sys;
#[cfg(windows)]
use windows as sys;
// Fallback for other unixes (BSD, …): generic unix pieces only.
#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
use unix as sys;

/// A disk / volume / mount point the analyzer can scan.
#[derive(Clone, Debug)]
pub struct DiskEntry {
    pub title: String,
    pub sub: String,
    /// Block device or drive id (`/dev/sda1`, `C:\`).
    pub dev: Option<String>,
    /// Where it is mounted; `None` means it has to be mounted (read-only) first.
    pub mount: Option<String>,
}

/// Result of running something with administrator rights.
pub enum Elevated {
    Done {
        stdout: String,
        stderr: String,
        ok: bool,
    },
    /// The user dismissed / failed the authentication dialog.
    Cancelled,
    Failed(String),
}

// ---- file system facts ------------------------------------------------------------------

/// Bytes a file really occupies on disk (blocks × 512 on unix, logical length on Windows).
pub fn alloc_size(md: &Metadata) -> u64 {
    sys::alloc_size(md)
}

/// Identity of a multiply-linked file, so hard links are only counted once.
pub fn hardlink_id(md: &Metadata) -> Option<(u64, u64)> {
    sys::hardlink_id(md)
}

/// `(total, used, available)` bytes of the filesystem holding `path`; zeros on failure.
pub fn disk_usage(path: &str) -> (u64, u64, u64) {
    sys::disk_usage(path)
}

/// Mount points below `root` that belong to a *different* filesystem and must not be entered.
pub fn skip_set(root: &str) -> HashSet<PathBuf> {
    sys::skip_set(root)
}

// ---- disks ------------------------------------------------------------------------------

pub fn list_disks() -> Vec<DiskEntry> {
    sys::list_disks()
}

/// Mount `dev` read-only and return the mount point. Only meaningful on Linux.
pub fn mount_ro(dev: &str) -> Result<String, String> {
    sys::mount_ro(dev)
}

pub fn unmount(dev: &str) {
    sys::unmount(dev)
}

// ---- desktop integration ------------------------------------------------------------------

/// Open a file or folder in the system file manager.
pub fn open_path(p: &Path) {
    sys::open_path(p)
}

/// Run this executable again with administrator rights and wait for it.
pub fn run_elevated(exe: &Path, args: &[String]) -> Elevated {
    sys::run_elevated(exe, args)
}

/// UI locale reported by the OS, e.g. `ru-RU` (used when no `LANG`-style variable is set).
pub fn os_locale() -> Option<String> {
    sys::os_locale()
}

/// Prepare the process for command-line use: make `println!` visible when a GUI-subsystem binary
/// is started from a terminal (Windows) and die quietly on a closed pipe (`diskwiz list | head`).
pub fn cli_init() {
    #[cfg(windows)]
    windows::attach_console();
    #[cfg(unix)]
    unix::restore_sigpipe();
}
