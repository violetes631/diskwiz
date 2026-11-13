//! Treemap layout (squarified) and colouring — pure maths, no GUI types.

use std::cmp::Reverse;
use std::hash::{Hash, Hasher};

use crate::scan::Tree;
use crate::tr_f;

/// Maximum number of rectangles drawn for one directory.
pub const MAX_ITEMS: usize = 400;

#[derive(Clone, Debug)]
pub enum TmKind {
    Dir(u32),
    File,
    Small,
}

#[derive(Clone, Debug)]
pub struct TmItem {
    pub kind: TmKind,
    pub name: String,
    pub size: u64,
}

pub type Rect = (f64, f64, f64, f64);

/// Squarified treemap (Bruls et al.). `areas` must be sorted in descending order and sum to
/// `w * h`; returns one rectangle `(x, y, w, h)` per area, in the same order.
pub fn squarify(areas: &[f64], mut x: f64, mut y: f64, mut w: f64, mut h: f64) -> Vec<Rect> {
    /// Worst aspect ratio of a row laid along a side of length `side`.
    fn worst(sum: f64, max: f64, min: f64, side: f64) -> f64 {
        (side * side * max / (sum * sum)).max(sum * sum / (side * side * min))
    }
    let n = areas.len();
    let mut rects = Vec::with_capacity(n);
    let mut i = 0;
    while i < n {
        if w <= 0.5 || h <= 0.5 {
            rects.extend(std::iter::repeat_n((x, y, 0.0, 0.0), n - i));
            break;
        }
        let side = w.min(h);
        // Grow the row while the worst aspect ratio does not get worse. Areas are descending,
        // so the first is the max and the last added is the min — no re-scan needed.
        let (mut sum, max) = (areas[i], areas[i]);
        let mut j = i + 1;
        while j < n {
            let next_sum = sum + areas[j];
            if worst(next_sum, max, areas[j], side) <= worst(sum, max, areas[j - 1], side) {
                sum = next_sum;
                j += 1;
            } else {
                break;
            }
        }
        let row = &areas[i..j];
        if w >= h {
            let cw = sum / h;
            let mut yy = y;
            for a in row {
                let rh = a / cw;
                rects.push((x, yy, cw, rh));
                yy += rh;
            }
            x += cw;
            w -= cw;
        } else {
            let rh = sum / w;
            let mut xx = x;
            for a in row {
                let cw = a / rh;
                rects.push((xx, y, cw, rh));
                xx += cw;
            }
            y += rh;
            h -= rh;
        }
        i = j;
    }
    rects
}

/// Rectangles to show for directory `cur`: sub-directories, big files and one "small files" block.
pub fn tm_items(tree: &Tree, cur: u32) -> Vec<TmItem> {
    let d = &tree.nodes[cur as usize];
    let mut items: Vec<TmItem> = d
        .dirs
        .iter()
        .map(|&c| TmItem { kind: TmKind::Dir(c), name: tree.nodes[c as usize].name.clone(), size: tree.nodes[c as usize].size })
        .collect();
    items.extend(d.fl.iter().map(|(n, s)| TmItem { kind: TmKind::File, name: n.clone(), size: *s }));
    if d.small_n > 0 {
        items.push(TmItem { kind: TmKind::Small, name: tr_f!("{} small files", d.small_n), size: d.small_size });
    }
    items.retain(|i| i.size > 0);
    items.sort_by_key(|i| Reverse(i.size));
    items.truncate(MAX_ITEMS);
    items
}

/// Lay out `items` (sorted by size) inside a `w × h` box.
pub fn layout(items: Vec<TmItem>, w: f64, h: f64) -> Vec<(Rect, TmItem)> {
    let total: f64 = items.iter().map(|i| i.size as f64).sum();
    if total <= 0.0 {
        return Vec::new();
    }
    let areas: Vec<f64> = items.iter().map(|i| i.size as f64 * w * h / total).collect();
    squarify(&areas, 0.0, 0.0, w, h).into_iter().zip(items).collect()
}

pub fn hsv(h: f64, s: f64, v: f64) -> (f64, f64, f64) {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match (i as i64).rem_euclid(6) {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    }
}

/// Hue per file type, like WizTree: video red, audio magenta, images green, archives orange…
fn hue_for_ext(ext: &str) -> Option<f64> {
    Some(match ext {
        "mp4" | "mkv" | "avi" | "mov" | "webm" | "wmv" | "flv" => 0.00,
        "mp3" | "flac" | "wav" | "ogg" | "m4a" | "opus" => 0.83,
        "jpg" | "jpeg" | "png" | "gif" | "webp" | "bmp" | "svg" | "raw" | "heic" => 0.33,
        "zip" | "rar" | "7z" | "tar" | "gz" | "xz" | "zst" | "bz2" | "iso" | "img" => 0.09,
        "pdf" | "doc" | "docx" | "odt" | "txt" | "xls" | "xlsx" | "ppt" | "pptx" => 0.58,
        "py" | "js" | "ts" | "c" | "cpp" | "h" | "rs" | "go" | "java" | "json" | "html" | "css" | "so" | "o" | "a" | "pak"
        | "dll" => 0.48,
        _ => return None,
    })
}

pub fn color_for(name: &str, kind: &TmKind) -> (f64, f64, f64) {
    match kind {
        TmKind::Small => (0.55, 0.55, 0.58),
        TmKind::Dir(_) => {
            let mut h = std::collections::hash_map::DefaultHasher::new();
            name.hash(&mut h);
            hsv((h.finish() % 1000) as f64 / 1000.0, 0.40, 0.72)
        }
        TmKind::File => {
            let ext = name.rsplit_once('.').map(|(_, e)| e.to_lowercase()).unwrap_or_default();
            match hue_for_ext(&ext) {
                Some(h) => hsv(h, 0.62, 0.86),
                None => hsv(0.66, 0.18, 0.86),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn areas_for(sizes: &[f64], w: f64, h: f64) -> Vec<f64> {
        let total: f64 = sizes.iter().sum();
        sizes.iter().map(|s| s * w * h / total).collect()
    }

    #[test]
    fn rectangles_tile_the_box_exactly() {
        let sizes = [50.0, 30.0, 10.0, 5.0, 3.0, 1.0, 1.0];
        let (w, h) = (800.0, 500.0);
        let rects = squarify(&areas_for(&sizes, w, h), 0.0, 0.0, w, h);
        assert_eq!(rects.len(), sizes.len());
        let area: f64 = rects.iter().map(|r| r.2 * r.3).sum();
        assert!((area - w * h).abs() < 1e-6, "area {area}");
        for (x, y, rw, rh) in &rects {
            assert!(*x >= -1e-9 && *y >= -1e-9 && x + rw <= w + 1e-6 && y + rh <= h + 1e-6);
        }
    }

    #[test]
    fn each_rect_keeps_its_share_of_the_area() {
        let sizes = [8.0, 4.0, 2.0, 1.0];
        let areas = areas_for(&sizes, 100.0, 100.0);
        let rects = squarify(&areas, 0.0, 0.0, 100.0, 100.0);
        for (a, r) in areas.iter().zip(&rects) {
            assert!((a - r.2 * r.3).abs() < 1e-6);
        }
    }

    #[test]
    fn rectangles_do_not_overlap() {
        let sizes: Vec<f64> = (1..=40).rev().map(|v| v as f64).collect();
        let rects = squarify(&areas_for(&sizes, 640.0, 480.0), 0.0, 0.0, 640.0, 480.0);
        for (i, a) in rects.iter().enumerate() {
            for b in &rects[i + 1..] {
                let ox = (a.0 + a.2).min(b.0 + b.2) - a.0.max(b.0);
                let oy = (a.1 + a.3).min(b.1 + b.3) - a.1.max(b.1);
                assert!(ox <= 1e-6 || oy <= 1e-6, "overlap {a:?} {b:?}");
            }
        }
    }

    #[test]
    fn degenerate_box_yields_zero_rects() {
        let r = squarify(&[1.0, 1.0], 0.0, 0.0, 0.1, 100.0);
        assert_eq!(r.len(), 2);
        assert!(r.iter().all(|r| r.2 == 0.0));
        assert!(squarify(&[], 0.0, 0.0, 10.0, 10.0).is_empty());
    }

    #[test]
    fn colors_are_valid_and_deterministic() {
        for k in [TmKind::File, TmKind::Small, TmKind::Dir(1)] {
            let (r, g, b) = color_for("movie.MKV", &k);
            assert!([r, g, b].iter().all(|c| (0.0..=1.0).contains(c)));
            assert_eq!(color_for("movie.MKV", &k), color_for("movie.MKV", &k));
        }
        assert_ne!(color_for("a.mp4", &TmKind::File), color_for("a.png", &TmKind::File));
        assert_eq!(color_for("a.MP4", &TmKind::File), color_for("a.mp4", &TmKind::File));
    }
}
