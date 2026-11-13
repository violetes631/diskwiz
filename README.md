<h1 align="center">DiskWiz</h1>

<p align="center">
  A fast disk cleaner and <a href="https://diskanalyzer.com/">WizTree</a>-style disk usage analyzer<br>
  for Linux and Windows (macOS best-effort). GTK4 + libadwaita, written in Rust.
</p>

<p align="center"><a href="README.ru.md">Русский</a></p>

## Features

**Disk analyzer**
- Multi-threaded scanner: ~260 000 files / 210 GiB in about 0.2 s on a warm NVMe.
- Real *allocated* sizes (like `du`), hard links counted once, other mounts / btrfs-subvolume aware.
- Expandable tree with size, share of parent and file counts.
- Interactive treemap (squarified, coloured by file type) — click to dive in, hover for details.
- "Biggest files" list, cancel at any time, read-only mounting of unmounted partitions (Linux, udisks).

**Cleaner** — shows how much each item would free *before* you press the button:

| | Linux | Windows | macOS |
|---|---|---|---|
| Application / browser caches | ✔ | ✔ | ✔ |
| Trash / Recycle Bin | ✔ | ✔ | ✔ |
| Temporary files, thumbnail cache | ✔ (in caches) | ✔ | |
| Package manager cache | dnf · apt · pacman · zypper | Windows Update, Delivery Optimization | Homebrew |
| Old kernels / unused packages | dnf · apt · pacman | | |
| systemd journal, crash dumps | ✔ | | |
| Flatpak runtimes, snap revisions | ✔ | | |
| Error reports, component store (DISM) | | ✔ | |

Items that need administrator rights are grouped separately and run through **polkit** (Linux),
**UAC** (Windows) or an admin prompt (macOS) — the GUI itself never runs as root. Only items that
make sense on your machine are shown (no `apt` entry on Fedora).

Interface languages: English, Русский (follows your system locale, override with `DISKWIZ_LANG=en|ru`).

## Install

### From source

Requirements: Rust ≥ 1.88, GTK ≥ 4.12, libadwaita ≥ 1.5.

```sh
# Fedora
sudo dnf install gtk4-devel libadwaita-devel
# Debian / Ubuntu
sudo apt install libgtk-4-dev libadwaita-1-dev pkg-config
# Arch
sudo pacman -S gtk4 libadwaita

git clone https://github.com/violetes631/diskwiz && cd diskwiz
make && sudo make install             # or: make install PREFIX=$HOME/.local
```

### Windows

Download the zip from the Releases page and run `diskwiz.exe`. To build it yourself, install
[MSYS2](https://www.msys2.org/), then in the *UCRT64* shell:

```sh
pacman -S mingw-w64-ucrt-x86_64-{gtk4,libadwaita,pkgconf,rust,gcc}
cargo build --release
```

### CLI only (no GTK needed)

```sh
cargo build --release --no-default-features
```

## Command line

```text
diskwiz                                   start the GUI
diskwiz scan [PATH] [--top N] [--threads N]   biggest folders and files under PATH
diskwiz list                              cleanup items and how much each would free
diskwiz clean ID... | --all               run cleanup items
```

## Configuration

Extra cache folders to clean go into `~/.config/diskwiz/caches.txt` (`%APPDATA%\diskwiz\caches.txt`
on Windows), one per line:

```text
# relative paths are inside ~/.cache (%LOCALAPPDATA% on Windows)
beamng-earth
# "~/" is your home directory, absolute paths work too
~/Games/shader-cache
```

Entries that would delete a filesystem root, your home directory or use `..` are ignored.

## What is (not) touched

DiskWiz only removes things that programs recreate or that are already deleted/unused: caches, trash,
temporary files, package downloads, unused runtimes, disabled snap revisions, and old kernels
**keeping the running one plus one spare**. Settings, profiles, documents and games are never touched.
Still — read the list before pressing *Clean*; every item has a switch.

## Development

```sh
cargo test                       # unit + i18n tests (GUI feature on)
cargo test --no-default-features # core only, no GTK
```

Layout: `src/scan.rs` (scanner), `src/treemap.rs` (layout), `src/cleaners/` (items per OS),
`src/platform/` (everything OS-specific), `src/gui/` (GTK front-end), `src/i18n/` (translations).

- **New cleanup item:** add a `CleanItem` to `src/cleaners/<os>.rs` and its strings to `src/i18n/ru.rs`.
- **New language:** add `src/i18n/xx.rs`, register it in `src/i18n.rs`; `cargo test` lists missing strings.
- The Windows and macOS code is compiled and tested in CI but has had far less real-world use than Linux.

## License

GPL-3.0-or-later — see [LICENSE](LICENSE).
