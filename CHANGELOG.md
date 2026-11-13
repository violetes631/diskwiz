# Changelog

## Unreleased
- Safety: the "dangerous path" rule now also protects top-level Windows folders (`C:\Windows`,
  `C:\Users`, `D:\Data`, …) and any folder that contains the home directory; it used to count the drive prefix as a path segment.
- Fixed administrator cleanup on Windows when the user name or temp path contains a space (the report path was split into two arguments, so the run was reported as cancelled).
- `diskwiz scan` now rejects unknown options and invalid `--top` / `--threads` values instead of ignoring them.
- AppStream metadata (`share/metainfo`) so software centers can show DiskWiz; `make install` installs it.
- Releases include `SHA256SUMS`; CI checks the desktop entry, AppStream file, clippy warnings and the minimum Rust version.

## 0.2.0 — first public version
- Renamed to **DiskWiz**; split into a GUI-free core library and a GTK front-end (`gui` feature).
- Cross-platform: Linux, Windows (drive letters, UAC, Windows cleanup items), macOS (best effort).
- Cleaner supports dnf, apt, pacman, zypper, Flatpak and snap; items are detected at runtime.
- English + Russian interface, with a test that every string is translated.
- User-configurable cache list (`caches.txt`) instead of hard-coded application names.
- `statvfs` instead of spawning `df`; GTK messages delivered through async channels, no polling timer.
- CLI: `scan`, `list`, `clean`.
- Unit tests for the scanner, treemap, parsers and path-safety rules.
