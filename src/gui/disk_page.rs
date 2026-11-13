//! "Disk" tab: pick a disk or folder, scan it, browse the tree, look at the treemap.

use std::cell::RefCell;
use std::cmp::Reverse;
use std::path::Path;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use adw::prelude::*;
use gtk::{gio, glib};

use super::{PROGRESS_EVERY, margins, spawn_rx};
use crate::i18n::tr;
use crate::platform::{self, DiskEntry};
use crate::scan::{self, NO_PARENT, Progress, Tree};
use crate::tr_f;
use crate::treemap::{self, Rect, TmItem, TmKind};
use crate::util::{human, spaced};

const MAX_CHILDREN: usize = 1500;
const TOP_FILES: usize = 500;

/// One row of the tree / list.
struct Row {
    name: String,
    size: u64,
    files: u64,
    is_dir: bool,
    node: Option<u32>, // for folders
    parent: u32,       // folder this row lives in
    frac: f64,
    path: String, // for "Biggest files"
}

enum Msg {
    Status(String),
    Done(Tree, Vec<(u64, u32, String)>, u64, bool),
    Failed(String),
}

struct TmState {
    tree: Option<Rc<Tree>>,
    cur: u32,
    rects: Vec<(Rect, TmItem)>,
    hover: Option<usize>,
    /// (width, height, directory, tree generation) the cached layout was computed for.
    laid: (i32, i32, u32, u32),
    generation: u32,
}

fn row_of(obj: &glib::Object) -> Option<glib::BoxedAnyObject> {
    obj.clone().downcast::<glib::BoxedAnyObject>().ok()
}

fn column(
    title: &str,
    expand: bool,
    width: i32,
    setup: impl Fn(&gtk::ListItem) + 'static,
    bind: impl Fn(&gtk::ListItem, &glib::BoxedAnyObject, &gtk::TreeListRow) + 'static,
) -> gtk::ColumnViewColumn {
    let f = gtk::SignalListItemFactory::new();
    f.connect_setup(move |_, o| setup(o.downcast_ref::<gtk::ListItem>().unwrap()));
    f.connect_bind(move |_, o| {
        let li = o.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some(row) = li.item().and_downcast::<gtk::TreeListRow>() {
            if let Some(bo) = row.item().and_downcast::<glib::BoxedAnyObject>() {
                bind(li, &bo, &row);
            }
        }
    });
    let c = gtk::ColumnViewColumn::new(Some(title), Some(f));
    c.set_expand(expand);
    if width > 0 {
        c.set_fixed_width(width);
    }
    c
}

fn simple_col(title: &str, expand: bool, width: i32, right: bool, f: impl Fn(&Row) -> String + 'static) -> gtk::ColumnViewColumn {
    let fac = gtk::SignalListItemFactory::new();
    fac.connect_setup(move |_, o| {
        let l = gtk::Label::builder()
            .xalign(if right { 1.0 } else { 0.0 })
            .ellipsize(gtk::pango::EllipsizeMode::Middle)
            .build();
        o.downcast_ref::<gtk::ListItem>().unwrap().set_child(Some(&l));
    });
    fac.connect_bind(move |_, o| {
        let li = o.downcast_ref::<gtk::ListItem>().unwrap();
        if let Some(bo) = li.item().and_downcast::<glib::BoxedAnyObject>() {
            let l = li.child().and_downcast::<gtk::Label>().unwrap();
            let text = f(&bo.borrow::<Row>());
            l.set_tooltip_text(Some(&text));
            l.set_label(&text);
        }
    });
    let c = gtk::ColumnViewColumn::new(Some(title), Some(fac));
    c.set_expand(expand);
    if width > 0 {
        c.set_fixed_width(width);
    }
    c
}

/// Children of directory `node` as tree rows, biggest first, capped at [`MAX_CHILDREN`].
fn child_rows(tree: &Tree, node: u32) -> Vec<Row> {
    let d = &tree.nodes[node as usize];
    let total = d.size.max(1) as f64;
    let mut rows: Vec<Row> = d
        .dirs
        .iter()
        .map(|&c| {
            let n = &tree.nodes[c as usize];
            Row { name: n.name.clone(), size: n.size, files: n.nfiles, is_dir: true, node: Some(c), parent: node, frac: n.size as f64 / total, path: String::new() }
        })
        .collect();
    rows.extend(d.fl.iter().map(|(n, s)| Row {
        name: n.clone(),
        size: *s,
        files: 1,
        is_dir: false,
        node: None,
        parent: node,
        frac: *s as f64 / total,
        path: String::new(),
    }));
    if d.small_n > 0 {
        rows.push(Row {
            name: tr_f!("<{} files < 1 MiB>", d.small_n),
            size: d.small_size,
            files: d.small_n,
            is_dir: false,
            node: None,
            parent: node,
            frac: d.small_size as f64 / total,
            path: String::new(),
        });
    }
    rows.sort_by_key(|r| Reverse(r.size));
    if rows.len() > MAX_CHILDREN {
        let rest = rows.split_off(MAX_CHILDREN);
        let rs: u64 = rest.iter().map(|r| r.size).sum();
        rows.push(Row {
            name: tr_f!("… {} more items", rest.len()),
            size: rs,
            files: rest.iter().map(|r| r.files).sum(),
            is_dir: false,
            node: None,
            parent: node,
            frac: rs as f64 / total,
            path: String::new(),
        });
    }
    rows
}

fn draw_treemap(st: &TmState, cr: &gtk::cairo::Context) {
    cr.set_source_rgb(0.12, 0.12, 0.14);
    let _ = cr.paint();
    cr.select_font_face("Sans", gtk::cairo::FontSlant::Normal, gtk::cairo::FontWeight::Normal);
    cr.set_font_size(11.0);
    for (idx, ((x, y, rw, rh), it)) in st.rects.iter().enumerate() {
        let (x, y, rw, rh) = (*x, *y, *rw, *rh);
        if rw < 1.5 || rh < 1.5 {
            continue;
        }
        let (r, g, b) = treemap::color_for(&it.name, &it.kind);
        let pat = gtk::cairo::LinearGradient::new(x, y, x + rw, y + rh);
        pat.add_color_stop_rgb(0.0, (r + 0.18).min(1.0), (g + 0.18).min(1.0), (b + 0.18).min(1.0));
        pat.add_color_stop_rgb(1.0, r * 0.78, g * 0.78, b * 0.78);
        cr.rectangle(x, y, rw, rh);
        let _ = cr.set_source(&pat);
        let _ = cr.fill();
        cr.rectangle(x + 0.5, y + 0.5, rw - 1.0, rh - 1.0);
        if st.hover == Some(idx) {
            cr.set_source_rgb(1.0, 1.0, 1.0);
            cr.set_line_width(2.5);
        } else {
            cr.set_source_rgba(0.0, 0.0, 0.0, 0.55);
            cr.set_line_width(1.0);
        }
        let _ = cr.stroke();
        if rw > 56.0 && rh > 18.0 {
            let _ = cr.save();
            cr.rectangle(x + 2.0, y + 2.0, rw - 4.0, rh - 4.0);
            cr.clip();
            cr.set_source_rgb(0.05, 0.05, 0.08);
            cr.move_to(x + 5.0, y + 14.0);
            let _ = cr.show_text(&it.name);
            if rh > 34.0 {
                cr.move_to(x + 5.0, y + 28.0);
                let _ = cr.show_text(&human(it.size));
            }
            let _ = cr.restore();
        }
    }
}

pub fn build(window: &adw::ApplicationWindow) -> gtk::Box {
    let page = gtk::Box::new(gtk::Orientation::Vertical, 8);
    margins(&page, 12);

    let entries: Rc<RefCell<Vec<DiskEntry>>> = Rc::new(RefCell::new(Vec::new()));
    let model = gtk::StringList::new(&[]);
    let drop = gtk::DropDown::new(Some(model.clone()), gtk::Expression::NONE);
    drop.set_hexpand(true);
    let reload_btn = gtk::Button::builder().icon_name("view-refresh-symbolic").tooltip_text(tr("Refresh disk list")).build();
    let folder_btn = gtk::Button::with_label(tr("Folder…"));
    let scan_btn = gtk::Button::with_label(tr("Scan"));
    scan_btn.add_css_class("suggested-action");
    let bar = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    for w in [drop.upcast_ref::<gtk::Widget>(), reload_btn.upcast_ref(), folder_btn.upcast_ref(), scan_btn.upcast_ref()] {
        bar.append(w);
    }
    page.append(&bar);

    let spin = gtk::Spinner::new();
    let status = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::End).build();
    status.add_css_class("dim-label");
    let open_btn = gtk::Button::builder().label(tr("Open in file manager")).sensitive(false).build();
    let st = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    st.append(&spin);
    st.append(&status);
    st.append(&open_btn);
    page.append(&st);

    let stack = gtk::Stack::builder().vexpand(true).build();
    let switcher = gtk::StackSwitcher::builder().stack(&stack).halign(gtk::Align::Center).build();
    page.append(&switcher);
    page.append(&stack);

    // ---- state
    let tm = Rc::new(RefCell::new(TmState { tree: None, cur: 0, rects: vec![], hover: None, laid: (0, 0, 0, 0), generation: 0 }));
    let tree_cell: Rc<RefCell<Option<Rc<Tree>>>> = Rc::new(RefCell::new(None));

    // ---- tree
    let root_store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let tc = tree_cell.clone();
    let tree_model = gtk::TreeListModel::new(root_store.clone(), false, false, move |obj| {
        let bo = row_of(obj)?;
        let node = bo.borrow::<Row>().node?;
        let tree = tc.borrow().clone()?;
        let d = &tree.nodes[node as usize];
        if d.dirs.is_empty() && d.fl.is_empty() && d.small_n == 0 {
            return None;
        }
        let store = gio::ListStore::new::<glib::BoxedAnyObject>();
        for r in child_rows(&tree, node) {
            store.append(&glib::BoxedAnyObject::new(r));
        }
        Some(store.upcast())
    });
    let sel = gtk::SingleSelection::new(Some(tree_model.clone()));

    let lbl_setup = |li: &gtk::ListItem| li.set_child(Some(&gtk::Label::builder().xalign(1.0).build()));

    let view = gtk::ColumnView::new(Some(sel.clone()));
    view.set_show_row_separators(true);
    view.append_column(&column(
        tr("Name"),
        true,
        0,
        |li| {
            let exp = gtk::TreeExpander::new();
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            b.append(&gtk::Image::new());
            b.append(&gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build());
            exp.set_child(Some(&b));
            li.set_child(Some(&exp));
        },
        |li, bo, row| {
            let exp = li.child().and_downcast::<gtk::TreeExpander>().unwrap();
            exp.set_list_row(Some(row));
            let b = exp.child().and_downcast::<gtk::Box>().unwrap();
            let r = bo.borrow::<Row>();
            b.first_child().and_downcast::<gtk::Image>().unwrap().set_icon_name(Some(if r.is_dir { "folder-symbolic" } else { "text-x-generic-symbolic" }));
            b.last_child().and_downcast::<gtk::Label>().unwrap().set_label(&r.name);
        },
    ));
    view.append_column(&column(tr("Size"), false, 90, lbl_setup, |li, bo, _| {
        li.child().and_downcast::<gtk::Label>().unwrap().set_label(&human(bo.borrow::<Row>().size))
    }));
    view.append_column(&column(
        tr("% of parent"),
        false,
        130,
        |li| {
            let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
            b.append(&gtk::ProgressBar::builder().valign(gtk::Align::Center).hexpand(true).build());
            let l = gtk::Label::builder().xalign(1.0).width_chars(5).build();
            l.add_css_class("caption");
            b.append(&l);
            li.set_child(Some(&b));
        },
        |li, bo, _| {
            let b = li.child().and_downcast::<gtk::Box>().unwrap();
            let r = bo.borrow::<Row>();
            b.first_child().and_downcast::<gtk::ProgressBar>().unwrap().set_fraction(r.frac);
            b.last_child().and_downcast::<gtk::Label>().unwrap().set_label(&format!("{:.0}%", r.frac * 100.0));
        },
    ));
    view.append_column(&column(tr("Files"), false, 80, lbl_setup, |li, bo, _| {
        li.child().and_downcast::<gtk::Label>().unwrap().set_label(&spaced(bo.borrow::<Row>().files))
    }));
    let tree_scroll = gtk::ScrolledWindow::builder().vexpand(true).child(&view).build();

    // ---- treemap
    let area = gtk::DrawingArea::builder().hexpand(true).vexpand(true).content_height(260).build();
    let up_btn = gtk::Button::builder().icon_name("go-up-symbolic").tooltip_text(tr("Up one level")).sensitive(false).build();
    let path_lbl = gtk::Label::builder().xalign(0.0).hexpand(true).ellipsize(gtk::pango::EllipsizeMode::Start).build();
    path_lbl.add_css_class("heading");
    let hover_lbl = gtk::Label::builder().xalign(0.0).ellipsize(gtk::pango::EllipsizeMode::End).build();
    hover_lbl.add_css_class("dim-label");
    hover_lbl.add_css_class("caption");

    {
        let tm = tm.clone();
        area.set_draw_func(move |_, cr, w, h| {
            let mut st = tm.borrow_mut();
            let Some(tree) = st.tree.clone() else { return };
            let key = (w, h, st.cur, st.generation);
            if st.laid != key || st.rects.is_empty() {
                st.rects = treemap::layout(treemap::tm_items(&tree, st.cur), w as f64, h as f64);
                st.laid = key;
            }
            draw_treemap(&st, cr);
        });
    }

    // go into a folder
    let go: Rc<dyn Fn(u32)> = {
        let (tm, area, path_lbl, up_btn) = (tm.clone(), area.clone(), path_lbl.clone(), up_btn.clone());
        Rc::new(move |node: u32| {
            let mut st = tm.borrow_mut();
            let Some(tree) = st.tree.clone() else { return };
            st.cur = node;
            st.rects.clear();
            st.hover = None;
            path_lbl.set_label(&format!("{} — {}", tree.path(node), human(tree.nodes[node as usize].size)));
            up_btn.set_sensitive(tree.nodes[node as usize].parent != NO_PARENT);
            area.queue_draw();
        })
    };
    {
        let (tm, go) = (tm.clone(), go.clone());
        up_btn.connect_clicked(move |_| {
            let parent = {
                let st = tm.borrow();
                st.tree.as_ref().map(|t| t.nodes[st.cur as usize].parent)
            };
            if let Some(p) = parent.filter(|&p| p != NO_PARENT) {
                go(p);
            }
        });
    }
    let at = |st: &TmState, px: f64, py: f64| -> Option<usize> {
        st.rects.iter().position(|((x, y, w, h), _)| px >= *x && px < x + w && py >= *y && py < y + h)
    };
    {
        let (tm, go) = (tm.clone(), go.clone());
        let click = gtk::GestureClick::new();
        click.set_button(1);
        click.connect_pressed(move |_, _, x, y| {
            let target = {
                let st = tm.borrow();
                at(&st, x, y).and_then(|i| match st.rects[i].1.kind {
                    TmKind::Dir(n) if st.rects[i].1.size > 0 => Some(n),
                    _ => None,
                })
            };
            if let Some(n) = target {
                go(n);
            }
        });
        area.add_controller(click);
    }
    {
        let (tm_h, area2, hover_lbl2) = (tm.clone(), area.clone(), hover_lbl.clone());
        let motion = gtk::EventControllerMotion::new();
        let set_hover = Rc::new(move |i: Option<usize>| {
            let mut st = tm_h.borrow_mut();
            if st.hover != i {
                st.hover = i;
                match (i, st.tree.clone()) {
                    (Some(i), Some(tree)) => {
                        let it = &st.rects[i].1;
                        let tot = tree.nodes[st.cur as usize].size.max(1) as f64;
                        hover_lbl2.set_label(&format!("{} — {} ({:.1}%)", it.name, human(it.size), it.size as f64 * 100.0 / tot));
                    }
                    _ => hover_lbl2.set_label(""),
                }
                area2.queue_draw();
            }
        });
        let (tm2, sh) = (tm.clone(), set_hover.clone());
        motion.connect_motion(move |_, x, y| {
            let i = at(&tm2.borrow(), x, y);
            sh(i);
        });
        let sh = set_hover.clone();
        motion.connect_leave(move |_| sh(None));
        area.add_controller(motion);
    }

    let tm_box = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    top.append(&up_btn);
    top.append(&path_lbl);
    let frame = gtk::Frame::new(None);
    frame.set_child(Some(&area));
    tm_box.append(&top);
    tm_box.append(&frame);
    tm_box.append(&hover_lbl);

    let paned = gtk::Paned::builder()
        .orientation(gtk::Orientation::Vertical)
        .position(300)
        .shrink_start_child(false)
        .shrink_end_child(false)
        .start_child(&tree_scroll)
        .end_child(&tm_box)
        .build();
    stack.add_titled(&paned, Some("tree"), tr("Tree and map"));

    // selection in the tree -> treemap
    {
        let (go, tm, open_btn) = (go.clone(), tm.clone(), open_btn.clone());
        sel.connect_selected_notify(move |s| {
            let Some(row) = s.selected_item().and_downcast::<gtk::TreeListRow>() else { return };
            open_btn.set_sensitive(true);
            let Some(bo) = row.item().and_downcast::<glib::BoxedAnyObject>() else { return };
            let node = {
                let r = bo.borrow::<Row>();
                r.node.unwrap_or(r.parent)
            };
            let (cur, has) = {
                let st = tm.borrow();
                (st.cur, st.tree.is_some())
            };
            if has && node != cur {
                go(node);
            }
        });
    }
    {
        let (sel, tree_cell) = (sel.clone(), tree_cell.clone());
        open_btn.connect_clicked(move |_| {
            let Some(row) = sel.selected_item().and_downcast::<gtk::TreeListRow>() else { return };
            let Some(bo) = row.item().and_downcast::<glib::BoxedAnyObject>() else { return };
            let node = {
                let r = bo.borrow::<Row>();
                r.node.unwrap_or(r.parent)
            };
            if let Some(t) = tree_cell.borrow().as_ref() {
                platform::open_path(Path::new(&t.path(node)));
            }
        });
    }

    // ---- biggest files
    let top_store = gio::ListStore::new::<glib::BoxedAnyObject>();
    let top_sel = gtk::SingleSelection::new(Some(top_store.clone()));
    let top_view = gtk::ColumnView::new(Some(top_sel));
    top_view.set_show_row_separators(true);
    top_view.append_column(&simple_col(tr("Size"), false, 100, true, |r| human(r.size)));
    top_view.append_column(&simple_col(tr("File"), true, 0, false, |r| r.path.clone()));
    let top_scroll = gtk::ScrolledWindow::builder().vexpand(true).child(&top_view).build();
    stack.add_titled(&top_scroll, Some("top"), tr("Biggest files"));

    // ---- disks
    let reload = Rc::new({
        let (entries, model) = (entries.clone(), model.clone());
        move || {
            let list = platform::list_disks();
            let labels: Vec<String> = list.iter().map(|e| if e.sub.is_empty() { e.title.clone() } else { format!("{}   ({})", e.title, e.sub) }).collect();
            let refs: Vec<&str> = labels.iter().map(|s| s.as_str()).collect();
            model.splice(0, model.n_items(), &refs);
            *entries.borrow_mut() = list;
        }
    });
    reload();
    {
        let reload = reload.clone();
        reload_btn.connect_clicked(move |_| reload());
    }

    // ---- scanning
    let scanning = Rc::new(RefCell::new(false));
    let cancel = Rc::new(RefCell::new(Arc::new(AtomicBool::new(false))));
    let start_scan: Rc<dyn Fn(DiskEntry)> = {
        let (scanning, cancel, scan_btn, spin, status) = (scanning.clone(), cancel.clone(), scan_btn.clone(), spin.clone(), status.clone());
        let (tm, tree_cell, root_store, tree_model, top_store, go) =
            (tm.clone(), tree_cell.clone(), root_store.clone(), tree_model.clone(), top_store.clone(), go.clone());
        Rc::new(move |entry: DiskEntry| {
            *scanning.borrow_mut() = true;
            scan_btn.set_label(tr("Cancel"));
            spin.start();
            status.set_label(tr("Preparing…"));
            let flag = Arc::new(AtomicBool::new(false));
            *cancel.borrow_mut() = flag.clone();
            let prog = Arc::new(Progress::default());
            let (tx, rx) = async_channel::unbounded::<Msg>();
            {
                let (flag, prog) = (flag.clone(), prog.clone());
                std::thread::spawn(move || {
                    let mut mounted_here: Option<String> = None;
                    let path = match &entry.mount {
                        Some(p) => p.clone(),
                        None => {
                            let _ = tx.send_blocking(Msg::Status(tr("Mounting the disk read-only (a password may be required)…").into()));
                            match platform::mount_ro(entry.dev.as_deref().unwrap_or("")) {
                                Ok(p) => {
                                    mounted_here = entry.dev.clone();
                                    p
                                }
                                Err(e) => {
                                    let _ = tx.send_blocking(Msg::Failed(e));
                                    return;
                                }
                            }
                        }
                    };
                    let result = std::thread::scope(|s| {
                        let h = s.spawn(|| scan::scan_tree(&path, flag.clone(), prog.clone()));
                        while !h.is_finished() {
                            let _ = tx.send_blocking(Msg::Status(tr_f!(
                                "Scanning… {} files · {}",
                                spaced(prog.files.load(Ordering::Relaxed)),
                                human(prog.bytes.load(Ordering::Relaxed))
                            )));
                            std::thread::sleep(PROGRESS_EVERY);
                        }
                        h.join().ok()
                    });
                    match result {
                        Some(tree) => {
                            let top = scan::top_files(&tree, TOP_FILES);
                            let _ = tx.send_blocking(Msg::Done(tree, top, prog.denied.load(Ordering::Relaxed), flag.load(Ordering::Relaxed)));
                        }
                        None => {
                            let _ = tx.send_blocking(Msg::Failed(tr("the scanner crashed").into()));
                        }
                    }
                    if let Some(dev) = mounted_here {
                        platform::unmount(&dev);
                    }
                });
            }
            let (scanning, scan_btn, spin, status) = (scanning.clone(), scan_btn.clone(), spin.clone(), status.clone());
            let (tm, tree_cell, root_store, tree_model, top_store, go) =
                (tm.clone(), tree_cell.clone(), root_store.clone(), tree_model.clone(), top_store.clone(), go.clone());
            spawn_rx(rx, move |m| match m {
                Msg::Status(s) => status.set_label(&s),
                Msg::Failed(e) => {
                    *scanning.borrow_mut() = false;
                    scan_btn.set_label(tr("Scan"));
                    spin.stop();
                    status.set_label(&tr_f!("Error: {}", e));
                }
                Msg::Done(tree, top, denied, cancelled) => {
                    *scanning.borrow_mut() = false;
                    scan_btn.set_label(tr("Scan"));
                    spin.stop();
                    let tree = Rc::new(tree);
                    let root = &tree.nodes[0];
                    let mut line = tr_f!("{}: {} files, {}", tree.root_path, spaced(root.nfiles), human(root.size));
                    if denied > 0 {
                        line.push_str(" · ");
                        line.push_str(&tr_f!("no access to {} items", denied));
                    }
                    if cancelled {
                        line.push(' ');
                        line.push_str(tr("(cancelled)"));
                    }
                    status.set_label(&line);
                    *tree_cell.borrow_mut() = Some(tree.clone());
                    {
                        let mut st = tm.borrow_mut();
                        st.tree = Some(tree.clone());
                        st.cur = 0;
                        st.generation = st.generation.wrapping_add(1);
                        st.rects.clear();
                    }
                    root_store.remove_all();
                    root_store.append(&glib::BoxedAnyObject::new(Row {
                        name: tree.root_path.clone(),
                        size: root.size,
                        files: root.nfiles,
                        is_dir: true,
                        node: Some(0),
                        parent: 0,
                        frac: 1.0,
                        path: String::new(),
                    }));
                    if let Some(r) = tree_model.row(0) {
                        r.set_expanded(true);
                    }
                    top_store.remove_all();
                    for (size, node, name) in top {
                        let p = Path::new(&tree.path(node)).join(&name).to_string_lossy().into_owned();
                        top_store.append(&glib::BoxedAnyObject::new(Row { name, size, files: 1, is_dir: false, node: None, parent: node, frac: 0.0, path: p }));
                    }
                    go(0);
                }
            });
        })
    };
    {
        let (scanning, cancel, status, entries, drop, start_scan) =
            (scanning.clone(), cancel.clone(), status.clone(), entries.clone(), drop.clone(), start_scan.clone());
        scan_btn.connect_clicked(move |_| {
            if *scanning.borrow() {
                cancel.borrow().store(true, Ordering::Relaxed);
                status.set_label(tr("Cancelling…"));
                return;
            }
            let e = entries.borrow().get(drop.selected() as usize).cloned();
            if let Some(e) = e {
                start_scan(e);
            }
        });
    }
    {
        let (window, start_scan) = (window.clone(), start_scan.clone());
        folder_btn.connect_clicked(move |_| {
            let dlg = gtk::FileDialog::builder().title(tr("Choose a folder to analyze")).build();
            let start_scan = start_scan.clone();
            dlg.select_folder(Some(&window), gio::Cancellable::NONE, move |res| {
                if let Some(p) = res.ok().and_then(|f| f.path()) {
                    let p = p.to_string_lossy().into_owned();
                    start_scan(DiskEntry { title: p.clone(), sub: String::new(), dev: None, mount: Some(p) });
                }
            });
        });
    }

    page
}
