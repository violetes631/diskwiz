//! diskwiz — disk cleaner and WizTree-style disk usage analyzer.
//!
//! The crate is split in two layers:
//!  * the *core* (`util`, `i18n`, `platform`, `scan`, `treemap`, `cleaners`) has no GUI
//!    dependencies and is usable from the CLI and from tests;
//!  * the optional `gui` module (feature `gui`, on by default) is GTK4 + libadwaita.

pub mod cleaners;
pub mod i18n;
pub mod platform;
pub mod scan;
pub mod treemap;
pub mod util;

#[cfg(feature = "gui")]
pub mod gui;

/// Reverse-DNS application id (desktop file, D-Bus name, GTK application id).
pub const APP_ID: &str = "io.github.violetes631.Diskwiz";
pub const APP_NAME: &str = "DiskWiz";
pub const VERSION: &str = env!("CARGO_PKG_VERSION");
