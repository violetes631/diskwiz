//! diskwiz entry point: GUI by default, a small CLI for headless use, and the hidden
//! `--root` helper that is started through polkit / UAC for administrator-level items.

// Release GUI builds on Windows must not open a console window; the CLI modes re-attach to the
// parent console in `platform::cli_init`.
#![cfg_attr(all(windows, not(debug_assertions), feature = "gui"), windows_subsystem = "windows")]

use std::process::ExitCode;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Instant;

use diskwiz::cleaners::{self, RootOutcome};
use diskwiz::i18n::{self, Lang, tr};
use diskwiz::scan::{self, Progress};
use diskwiz::tr_f;
use diskwiz::util::{human, spaced};
use diskwiz::{VERSION, platform};

const HELP: &str = "\
diskwiz — disk cleaner and disk usage analyzer

USAGE:
    diskwiz                          start the GUI
    diskwiz scan [PATH] [--top N] [--threads N]
                                     print the biggest folders and files under PATH (default: /)
    diskwiz list                     list cleanup items and how much each could free
    diskwiz clean ID... | --all      run cleanup items (administrator items ask for rights)
    diskwiz --version | --help

ENVIRONMENT:
    DISKWIZ_LANG=en|ru               override the interface language
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let first = args.first().map(String::as_str);

    // Hidden helper: run administrator-level items. Must be checked before anything else.
    if args.iter().any(|a| a == "--root") {
        platform::cli_init();
        return root_helper(&args);
    }
    match first {
        Some("-h" | "--help" | "help") => {
            platform::cli_init();
            print!("{HELP}");
            ExitCode::SUCCESS
        }
        Some("-V" | "--version") => {
            platform::cli_init();
            println!("diskwiz {VERSION}");
            ExitCode::SUCCESS
        }
        Some("scan" | "--scan") => {
            platform::cli_init();
            cli_scan(&args[1..])
        }
        Some("list") => {
            platform::cli_init();
            cli_list()
        }
        Some("clean") => {
            platform::cli_init();
            cli_clean(&args[1..])
        }
        Some(other) => {
            platform::cli_init();
            eprintln!("diskwiz: unknown argument '{other}'\n\n{HELP}");
            ExitCode::from(2)
        }
        None => start_gui(),
    }
}

#[cfg(feature = "gui")]
fn start_gui() -> ExitCode {
    ExitCode::from(diskwiz::gui::run().get())
}

#[cfg(not(feature = "gui"))]
fn start_gui() -> ExitCode {
    platform::cli_init();
    eprintln!("diskwiz was built without the `gui` feature.\n\n{HELP}");
    ExitCode::from(2)
}

/// `diskwiz --root [--lang xx] [--out FILE] ID...` — executes only known administrator items.
fn root_helper(args: &[String]) -> ExitCode {
    let mut ids = Vec::new();
    let mut out_file = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => {}
            "--lang" => {
                if let Some(l) = it.next().and_then(|c| Lang::from_code(c)) {
                    i18n::set(l);
                }
            }
            "--out" => out_file = it.next().cloned(),
            id => ids.push(id.to_string()),
        }
    }
    let mut report = String::new();
    for id in &ids {
        if let Some(item) = cleaners::find(id).filter(|i| i.root) {
            report.push_str(&format!("{id}: {}\n", (item.run)()));
        }
    }
    match out_file {
        Some(f) => {
            let _ = std::fs::write(f, report);
        }
        None => print!("{report}"),
    }
    ExitCode::SUCCESS
}

fn cli_scan(args: &[String]) -> ExitCode {
    let (mut path, mut top, mut threads) = ("/".to_string(), 10usize, scan::default_threads());
    if cfg!(windows) {
        path = "C:\\".into();
    }
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--top" => top = it.next().and_then(|v| v.parse().ok()).unwrap_or(top),
            "--threads" => threads = it.next().and_then(|v| v.parse().ok()).unwrap_or(threads),
            p => path = p.to_string(),
        }
    }
    let t0 = Instant::now();
    let prog = Arc::new(Progress::default());
    let tree = scan::scan_tree_with(&path, Arc::new(AtomicBool::new(false)), prog.clone(), threads, platform::skip_set(&path));
    let root = &tree.nodes[0];
    println!("{path}: {} files, {} in {:.1} s", spaced(root.nfiles), human(root.size), t0.elapsed().as_secs_f64());
    let denied = prog.denied.load(std::sync::atomic::Ordering::Relaxed);
    if denied > 0 {
        println!("{}", tr_f!("no access to {} items", denied));
    }
    println!("\n{}", tr("Biggest folders"));
    for &c in root.dirs.iter().take(top) {
        println!("  {:>10}  {}", human(tree.nodes[c as usize].size), tree.nodes[c as usize].name);
    }
    println!("\n{}", tr("Biggest files"));
    for (size, node, name) in scan::top_files(&tree, top) {
        println!("  {:>10}  {}", human(size), std::path::Path::new(&tree.path(node)).join(name).display());
    }
    ExitCode::SUCCESS
}

fn cli_list() -> ExitCode {
    println!("{:<12} {:<6} {:>10}  {}", "ID", tr("admin"), tr("size"), tr("what"));
    for it in cleaners::items() {
        let size = (it.size)().map(human).unwrap_or_else(|| "—".into());
        println!("{:<12} {:<6} {:>10}  {}", it.id, if it.root { "yes" } else { "" }, size, tr(it.title));
    }
    ExitCode::SUCCESS
}

fn cli_clean(args: &[String]) -> ExitCode {
    let wanted: Vec<&str> = if args.iter().any(|a| a == "--all") {
        cleaners::items().iter().map(|i| i.id).collect()
    } else {
        args.iter().map(String::as_str).collect()
    };
    if wanted.is_empty() {
        eprintln!("diskwiz clean: give item ids (see `diskwiz list`) or --all");
        return ExitCode::from(2);
    }
    if let Some(bad) = wanted.iter().find(|id| cleaners::find(id).is_none()) {
        eprintln!("diskwiz clean: unknown item '{bad}' (see `diskwiz list`)");
        return ExitCode::from(2);
    }
    let before = platform::disk_usage(&home_root()).2;
    let (root_ids, user_ids): (Vec<&str>, Vec<&str>) = wanted.iter().partition(|id| cleaners::find(id).is_some_and(|i| i.root));
    for id in user_ids {
        let item = cleaners::find(id).expect("checked above");
        println!("{id}: {}", (item.run)());
    }
    if !root_ids.is_empty() {
        match cleaners::run_root_items(&root_ids) {
            RootOutcome::Lines(l) => l.iter().for_each(|l| println!("{l}")),
            RootOutcome::Cancelled => println!("{}", tr("Authentication cancelled — administrator items skipped.")),
            RootOutcome::Failed(e) => {
                eprintln!("{e}");
                return ExitCode::FAILURE;
            }
        }
    }
    let after = platform::disk_usage(&home_root()).2;
    println!("{}", tr_f!("Done · freed {}", human(after.saturating_sub(before))));
    ExitCode::SUCCESS
}

fn home_root() -> String {
    if cfg!(windows) { "C:\\".into() } else { "/".into() }
}
