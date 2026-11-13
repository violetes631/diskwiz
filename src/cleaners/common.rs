//! Helpers used by several platforms.

use std::path::{Path, PathBuf};

use crate::util::{cache_dir, config_dir, home};

/// Parse the user's `caches.txt`: one path per line, `#` starts a comment.
/// Relative paths are relative to the cache directory, `~/` is the home directory.
/// Anything that would remove a filesystem root or the home directory itself is dropped.
pub fn parse_extra_caches(text: &str, cache: &Path, home: &Path) -> Vec<PathBuf> {
    text.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(|l| match l.strip_prefix("~/") {
            Some(rest) => home.join(rest),
            None => cache.join(l), // join() keeps absolute paths as they are
        })
        .filter(|p| !p.components().any(|c| matches!(c, std::path::Component::ParentDir)))
        .filter(|p| p != home && p != cache && !home.starts_with(p) && crate::util::depth(p) >= 2)
        .collect()
}

/// `~/.config/diskwiz/caches.txt` — extra cache folders the user wants cleaned.
pub fn extra_caches_file() -> PathBuf {
    config_dir().join("diskwiz").join("caches.txt")
}

/// Known cache folders (relative to the cache dir) plus the user's own, only those that exist.
pub fn existing_caches(known: &[&str]) -> Vec<PathBuf> {
    let cache = cache_dir();
    let mut v: Vec<PathBuf> = known.iter().map(|n| cache.join(n)).collect();
    if let Ok(text) = std::fs::read_to_string(extra_caches_file()) {
        v.extend(parse_extra_caches(&text, &cache, &home()));
    }
    v.retain(|p| p.exists());
    v.sort();
    v.dedup();
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_relative_absolute_and_home_paths() {
        let cache = Path::new("/home/u/.cache");
        let home = Path::new("/home/u");
        let text = "# comment\nspotify\n\n  beamng-earth  # inline\n~/Games/cache\n/var/tmp/foo\n";
        let v = parse_extra_caches(text, cache, home);
        assert_eq!(
            v,
            [
                PathBuf::from("/home/u/.cache/spotify"),
                PathBuf::from("/home/u/.cache/beamng-earth"),
                PathBuf::from("/home/u/Games/cache"),
                PathBuf::from("/var/tmp/foo"),
            ]
        );
    }

    #[test]
    fn rejects_dangerous_entries() {
        let cache = Path::new("/home/u/.cache");
        let home = Path::new("/home/u");
        let v = parse_extra_caches("/\n/usr\n/home\n/home/u\n~/\n../../etc\n.\n", cache, home);
        assert!(v.is_empty(), "{v:?}");
    }
}
