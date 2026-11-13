# Contributing

Bug reports, translations and pull requests are welcome.

## Build and test

```sh
cargo build                       # GUI; needs GTK 4 and libadwaita development files
cargo test                        # unit tests + translation completeness
cargo build --no-default-features # CLI only, no GTK (what CI runs on Windows/macOS)
```

## Rules of the road

- Keep `scan`, `treemap`, `cleaners`, `util` and `platform` free of GTK; only `src/gui` may use it.
- Anything that deletes files must go through `cleaners::remove_all` / `util::is_dangerous_target`.
  Add a test for every new path rule.
- Every new UI string needs an entry in `src/i18n/ru.rs` (the English text is the key);
  `cargo test` fails if one is missing. To add a language, add a `Lang` variant in `src/i18n.rs`.
- Adding a cleanup item: add a `CleanItem` in the platform's `items()` in `src/cleaners/`.
- Windows and macOS code cannot be built on Linux; CI is the check, so keep pull requests small.

By contributing you agree that your work is licensed under GPL-3.0-or-later.
