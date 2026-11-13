# Changelog

## 0.2.0 — first public version
- Renamed to **DiskWiz**; split into a GUI-free core library and a GTK front-end (`gui` feature).
- Cross-platform: Linux, Windows (drive letters, UAC, Windows cleanup items), macOS (best effort).
- Cleaner supports dnf, apt, pacman, zypper, Flatpak and snap; items are detected at runtime.
- English + Russian interface, with a test that every string is translated.
- User-configurable cache list (`caches.txt`) instead of hard-coded application names.
- `statvfs` instead of spawning `df`; GTK messages delivered through async channels, no polling timer.
- CLI: `scan`, `list`, `clean`.
- Unit tests for the scanner, treemap, parsers and path-safety rules.
