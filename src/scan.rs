//! Multi-threaded directory scanner that builds a size tree (the engine behind the disk analyzer).
//!
//! Design notes:
//!  * Directories are the nodes of an arena (`Vec<Node>`); a child always has a larger index than
//!    its parent, so totals are computed with one reverse pass.
//!  * Files smaller than [`SMALL`] are not stored individually, only counted — this keeps memory
//!    flat on disks with millions of tiny files.
//!  * Sizes are *allocated* sizes (like `du`), hard links are counted once, symlinks are ignored.

use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashSet, VecDeque};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use crate::platform;

/// Files below this size are only counted, not listed.
pub const SMALL: u64 = 1024 * 1024;
pub const NO_PARENT: u32 = u32::MAX;

pub struct Node {
    pub name: String,
    pub parent: u32,
    /// Sub-directories, largest first (after the scan).
    pub dirs: Vec<u32>,
    /// Files ≥ [`SMALL`], largest first.
    pub fl: Vec<(String, u64)>,
    pub small_n: u64,
    pub small_size: u64,
    pub size: u64,
    pub nfiles: u64,
}

pub struct Tree {
    pub nodes: Vec<Node>,
    pub root_path: String,
}

impl Tree {
    /// Full path of node `idx`.
    pub fn path(&self, mut idx: u32) -> String {
        let mut parts = Vec::new();
        while self.nodes[idx as usize].parent != NO_PARENT {
            parts.push(self.nodes[idx as usize].name.as_str());
            idx = self.nodes[idx as usize].parent;
        }
        let mut p = PathBuf::from(&self.root_path);
        for part in parts.iter().rev() {
            p.push(part);
        }
        p.to_string_lossy().into_owned()
    }
}

/// Live counters, readable from another thread while a scan runs.
#[derive(Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub denied: AtomicU64,
}

/// Reasonable thread count: the work is I/O bound, so a bit more than the core count.
pub fn default_threads() -> usize {
    std::thread::available_parallelism().map(|n| n.get() * 2).unwrap_or(8).clamp(4, 16)
}

struct Shared {
    nodes: Mutex<Vec<Node>>,
    /// (pending jobs, number of jobs not yet finished)
    queue: Mutex<(VecDeque<(u32, PathBuf)>, usize)>,
    cv: Condvar,
    seen: Mutex<HashSet<(u64, u64)>>,
    skip: HashSet<PathBuf>,
    cancel: Arc<AtomicBool>,
    prog: Arc<Progress>,
}

fn new_node(name: String, parent: u32) -> Node {
    Node { name, parent, dirs: vec![], fl: vec![], small_n: 0, small_size: 0, size: 0, nfiles: 0 }
}

fn scan_dir(sh: &Shared, idx: u32, path: &Path) {
    let mut subdirs: Vec<String> = Vec::new();
    let mut fl = Vec::new();
    let (mut small_n, mut small_size, mut files, mut bytes) = (0u64, 0u64, 0u64, 0u64);
    match fs::read_dir(path) {
        Err(_) => {
            sh.prog.denied.fetch_add(1, Ordering::Relaxed);
        }
        Ok(rd) => {
            for e in rd.flatten() {
                if sh.cancel.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(ft) = e.file_type() else {
                    sh.prog.denied.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                if ft.is_symlink() {
                    continue;
                }
                let name = e.file_name().to_string_lossy().into_owned();
                if ft.is_dir() {
                    if !sh.skip.is_empty() && sh.skip.contains(&e.path()) {
                        continue;
                    }
                    subdirs.push(name);
                    continue;
                }
                let Ok(md) = e.metadata() else {
                    sh.prog.denied.fetch_add(1, Ordering::Relaxed);
                    continue;
                };
                if let Some(id) = platform::hardlink_id(&md)
                    && !sh.seen.lock().unwrap().insert(id)
                {
                    continue;
                }
                let size = platform::alloc_size(&md);
                files += 1;
                bytes += size;
                if size >= SMALL {
                    fl.push((name, size));
                } else {
                    small_n += 1;
                    small_size += size;
                }
            }
        }
    }
    sh.prog.files.fetch_add(files, Ordering::Relaxed);
    sh.prog.bytes.fetch_add(bytes, Ordering::Relaxed);

    let mut ids = Vec::with_capacity(subdirs.len());
    {
        let mut nodes = sh.nodes.lock().unwrap();
        for name in &subdirs {
            nodes.push(new_node(name.clone(), idx));
            ids.push((nodes.len() - 1) as u32);
        }
        let n = &mut nodes[idx as usize];
        n.fl = fl;
        n.small_n = small_n;
        n.small_size = small_size;
        n.dirs = ids.clone();
    }
    if !ids.is_empty() {
        let mut q = sh.queue.lock().unwrap();
        for (id, name) in ids.iter().zip(&subdirs) {
            q.0.push_back((*id, path.join(name)));
        }
        q.1 += ids.len();
        sh.cv.notify_all();
    }
}

fn worker(sh: &Shared) {
    loop {
        let job = {
            let mut q = sh.queue.lock().unwrap();
            loop {
                if let Some(j) = q.0.pop_front() {
                    break Some(j);
                }
                if q.1 == 0 {
                    break None;
                }
                q = sh.cv.wait(q).unwrap();
            }
        };
        let Some((idx, path)) = job else { return };
        if !sh.cancel.load(Ordering::Relaxed) {
            scan_dir(sh, idx, &path);
        }
        let mut q = sh.queue.lock().unwrap();
        q.1 -= 1;
        if q.1 == 0 {
            sh.cv.notify_all();
        }
    }
}

/// Scan `root` with [`default_threads`] workers.
pub fn scan_tree(root: &str, cancel: Arc<AtomicBool>, prog: Arc<Progress>) -> Tree {
    scan_tree_with(root, cancel, prog, default_threads(), platform::skip_set(root))
}

/// Scan `root` with explicit parallelism and a set of directories that must not be entered.
pub fn scan_tree_with(root: &str, cancel: Arc<AtomicBool>, prog: Arc<Progress>, threads: usize, skip: HashSet<PathBuf>) -> Tree {
    let sh = Arc::new(Shared {
        nodes: Mutex::new(vec![new_node(root.to_string(), NO_PARENT)]),
        queue: Mutex::new((VecDeque::from([(0u32, PathBuf::from(root))]), 1)),
        cv: Condvar::new(),
        seen: Mutex::new(HashSet::new()),
        skip,
        cancel,
        prog,
    });
    let handles: Vec<_> = (0..threads.max(1))
        .map(|_| {
            let sh = sh.clone();
            std::thread::spawn(move || worker(&sh))
        })
        .collect();
    for h in handles {
        let _ = h.join();
    }
    let mut nodes = std::mem::take(&mut *sh.nodes.lock().unwrap());
    // Children always have a larger index than their parent — walk backwards.
    for i in (0..nodes.len()).rev() {
        let mut ds = std::mem::take(&mut nodes[i].dirs);
        ds.sort_by_key(|&c| Reverse(nodes[c as usize].size));
        let sub_size: u64 = ds.iter().map(|&c| nodes[c as usize].size).sum();
        let sub_files: u64 = ds.iter().map(|&c| nodes[c as usize].nfiles).sum();
        let n = &mut nodes[i];
        n.fl.sort_by_key(|f| Reverse(f.1));
        n.size = n.fl.iter().map(|f| f.1).sum::<u64>() + n.small_size + sub_size;
        n.nfiles = n.fl.len() as u64 + n.small_n + sub_files;
        n.dirs = ds;
    }
    Tree { nodes, root_path: root.to_string() }
}

/// The `limit` biggest files as `(size, parent node, name)`, biggest first.
pub fn top_files(tree: &Tree, limit: usize) -> Vec<(u64, u32, String)> {
    let mut heap: BinaryHeap<Reverse<(u64, u32, String)>> = BinaryHeap::with_capacity(limit + 1);
    for (i, n) in tree.nodes.iter().enumerate() {
        for (name, size) in &n.fl {
            if heap.len() < limit {
                heap.push(Reverse((*size, i as u32, name.clone())));
            } else if heap.peek().is_some_and(|r| *size > r.0.0) {
                heap.pop();
                heap.push(Reverse((*size, i as u32, name.clone())));
            }
        }
    }
    let mut v: Vec<_> = heap.into_iter().map(|r| r.0).collect();
    v.sort_by_key(|t| Reverse(t.0));
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(p: &Path) -> Tree {
        scan_tree_with(&p.to_string_lossy(), Arc::default(), Arc::default(), 4, HashSet::new())
    }

    fn write(p: &Path, bytes: usize) {
        fs::create_dir_all(p.parent().unwrap()).unwrap();
        fs::write(p, vec![7u8; bytes]).unwrap();
    }

    #[test]
    fn totals_counts_and_ordering() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("big.bin"), 3 * 1024 * 1024);
        write(&r.join("a/medium.bin"), 2 * 1024 * 1024);
        write(&r.join("a/b/tiny1"), 10);
        write(&r.join("a/b/tiny2"), 10);
        write(&r.join("empty_dir_parent/x/tiny"), 1);

        let t = scan(r);
        let root = &t.nodes[0];
        assert_eq!(root.nfiles, 5);
        assert!(root.size >= 5 * 1024 * 1024);
        // big.bin (≥ SMALL) is listed, tiny files are only counted
        assert_eq!(root.fl.len(), 1);
        assert_eq!(root.fl[0].0, "big.bin");
        // sub-directories are sorted by size, largest first
        let sizes: Vec<u64> = root.dirs.iter().map(|&c| t.nodes[c as usize].size).collect();
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]));
        assert_eq!(t.nodes[root.dirs[0] as usize].name, "a");
        // a node's size is the sum of its parts
        for n in &t.nodes {
            let parts: u64 =
                n.fl.iter().map(|f| f.1).sum::<u64>() + n.small_size + n.dirs.iter().map(|&c| t.nodes[c as usize].size).sum::<u64>();
            assert_eq!(n.size, parts);
        }
    }

    #[test]
    fn path_reconstruction() {
        let d = tempfile::tempdir().unwrap();
        write(&d.path().join("x/y/z/f"), 1);
        let t = scan(d.path());
        let z = t.nodes.iter().position(|n| n.name == "z").unwrap() as u32;
        assert_eq!(Path::new(&t.path(z)), d.path().join("x/y/z"));
    }

    #[cfg(unix)]
    #[test]
    fn hard_links_and_symlinks_counted_once() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("orig"), 2 * 1024 * 1024);
        fs::hard_link(r.join("orig"), r.join("link")).unwrap();
        std::os::unix::fs::symlink(r.join("orig"), r.join("sym")).unwrap();
        let t = scan(r);
        assert_eq!(t.nodes[0].nfiles, 1);
    }

    #[test]
    fn skip_set_is_respected() {
        let d = tempfile::tempdir().unwrap();
        let r = d.path();
        write(&r.join("keep/f"), 1);
        write(&r.join("skipme/f"), 1);
        let skip = HashSet::from([r.join("skipme")]);
        let t = scan_tree_with(&r.to_string_lossy(), Arc::default(), Arc::default(), 2, skip);
        assert_eq!(t.nodes[0].nfiles, 1);
    }

    #[test]
    fn cancel_stops_early_and_does_not_hang() {
        let d = tempfile::tempdir().unwrap();
        for i in 0..20 {
            write(&d.path().join(format!("d{i}/f")), 1);
        }
        let cancel = Arc::new(AtomicBool::new(true));
        let t = scan_tree_with(&d.path().to_string_lossy(), cancel, Arc::default(), 4, HashSet::new());
        assert!(t.nodes[0].nfiles <= 20);
    }

    #[test]
    fn top_files_keeps_the_largest() {
        let d = tempfile::tempdir().unwrap();
        for (i, mb) in [1usize, 5, 3, 4, 2].iter().enumerate() {
            write(&d.path().join(format!("d{i}/f{mb}")), mb * 1024 * 1024);
        }
        let t = scan(d.path());
        let top = top_files(&t, 3);
        let names: Vec<&str> = top.iter().map(|(_, _, n)| n.as_str()).collect();
        assert_eq!(names, ["f5", "f4", "f3"]);
    }

    #[test]
    fn unreadable_root_counts_as_denied() {
        let prog = Arc::new(Progress::default());
        let t = scan_tree_with("/definitely/not/here", Arc::default(), prog.clone(), 2, HashSet::new());
        assert_eq!(t.nodes[0].nfiles, 0);
        assert_eq!(prog.denied.load(Ordering::Relaxed), 1);
    }
}
