//! "Cleanup" tab: a list of switches, sizes, and one big button.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;

use super::{margins, spawn_rx};
use crate::cleaners::{self, RootOutcome};
use crate::i18n::tr;
use crate::platform;
use crate::tr_f;

/// A closure that can be replaced after creation (the scan button re-runs itself).
type SharedFn = Rc<RefCell<Option<Rc<dyn Fn()>>>>;
use crate::util::human;

struct CleanRow {
    sw: gtk::Switch,
    pill: gtk::Label,
    spin: gtk::Spinner,
}

pub struct CleanPage {
    pub root: gtk::ScrolledWindow,
    pub refresh_btn: gtk::Button,
}

enum Msg {
    Size(usize, Option<u64>),
    ScanDone,
    Log(String),
    Done(u64),
}

fn root_disk() -> &'static str {
    if cfg!(windows) { "C:\\" } else { "/" }
}

pub fn build(toasts: &adw::ToastOverlay) -> CleanPage {
    let items = cleaners::items();

    let scroll = gtk::ScrolledWindow::builder().hscrollbar_policy(gtk::PolicyType::Never).build();
    let clamp = adw::Clamp::builder().maximum_size(620).tightening_threshold(480).build();
    scroll.set_child(Some(&clamp));
    let page = gtk::Box::new(gtk::Orientation::Vertical, 18);
    margins(&page, 18);
    clamp.set_child(Some(&page));

    // --- disk card
    let card = gtk::Box::new(gtk::Orientation::Vertical, 8);
    card.add_css_class("card");
    card.add_css_class("disk-card");
    let cap = gtk::Label::builder().label(tr("Free disk space")).xalign(0.0).build();
    cap.add_css_class("dim-label");
    let free_lbl = gtk::Label::builder().xalign(0.0).build();
    free_lbl.add_css_class("disk-free");
    let bar = gtk::ProgressBar::new();
    bar.add_css_class("disk-bar");
    let used_lbl = gtk::Label::builder().xalign(0.0).build();
    used_lbl.add_css_class("dim-label");
    used_lbl.add_css_class("caption");
    for w in [cap.upcast_ref::<gtk::Widget>(), free_lbl.upcast_ref(), bar.upcast_ref(), used_lbl.upcast_ref()] {
        card.append(w);
    }
    page.append(&card);

    let refresh_free = Rc::new({
        let (free_lbl, bar, used_lbl) = (free_lbl.clone(), bar.clone(), used_lbl.clone());
        move || {
            let (total, used, avail) = platform::disk_usage(root_disk());
            free_lbl.set_label(&human(avail));
            let frac = if total > 0 { used as f64 / total as f64 } else { 0.0 };
            bar.set_fraction(frac);
            if frac > 0.85 {
                bar.add_css_class("full")
            } else {
                bar.remove_css_class("full")
            }
            used_lbl.set_label(&tr_f!("Used {} of {} · {}%", human(used), human(total), format!("{:.0}", frac * 100.0)));
        }
    });
    refresh_free();

    // --- rows. Row order = `items` order (user items first, then administrator items).
    let sizes: Rc<RefCell<Vec<Option<u64>>>> = Rc::new(RefCell::new(vec![None; items.len()]));
    let working = Rc::new(RefCell::new(false));
    let clean_btn = gtk::Button::builder().label(tr("Clean")).halign(gtk::Align::Center).build();
    clean_btn.add_css_class("suggested-action");
    clean_btn.add_css_class("pill");
    clean_btn.add_css_class("clean-btn");

    let rows: Rc<RefCell<Vec<CleanRow>>> = Rc::new(RefCell::new(Vec::new()));
    let update_btn = {
        let (rows, sizes, clean_btn, working) = (rows.clone(), sizes.clone(), clean_btn.clone(), working.clone());
        Rc::new(move || {
            let (rows, sizes) = (rows.borrow(), sizes.borrow());
            let mut any = false;
            let mut total = 0u64;
            for (i, r) in rows.iter().enumerate() {
                if r.sw.is_active() {
                    any = true;
                    total += sizes[i].unwrap_or(0);
                }
            }
            clean_btn.set_sensitive(any && !*working.borrow());
            clean_btn.set_label(&if total > 0 { tr_f!("Clean · {}", human(total)) } else { tr("Clean").to_string() });
        })
    };

    let make_group = |title: &str, root: bool| -> Option<adw::PreferencesGroup> {
        if !items.iter().any(|i| i.root == root) {
            return None;
        }
        let g = adw::PreferencesGroup::builder().title(title).build();
        for it in items.iter().filter(|i| i.root == root) {
            let row = adw::ActionRow::builder().title(tr(it.title)).subtitle(tr(it.hint)).build();
            row.add_prefix(&gtk::Image::builder().icon_name(it.icon).pixel_size(22).build());
            let spin = gtk::Spinner::builder().spinning(true).valign(gtk::Align::Center).build();
            let pill = gtk::Label::builder().valign(gtk::Align::Center).visible(false).build();
            pill.add_css_class("size-pill");
            let sw = gtk::Switch::builder().active(true).valign(gtk::Align::Center).build();
            let ub = update_btn.clone();
            sw.connect_active_notify(move |_| ub());
            row.add_suffix(&spin);
            row.add_suffix(&pill);
            row.add_suffix(&sw);
            row.set_activatable_widget(Some(&sw));
            g.add(&row);
            rows.borrow_mut().push(CleanRow { sw, pill, spin });
        }
        Some(g)
    };
    if let Some(g) = make_group(tr("No password needed"), false) {
        page.append(&g);
    }
    let admin_title =
        if cfg!(windows) { tr("System · administrator rights required") } else { tr("System · administrator password required") };
    if let Some(g) = make_group(admin_title, true) {
        page.append(&g);
    }

    page.append(&clean_btn);
    let safe = gtk::Label::builder().label(tr("Settings, profiles, games and personal files are not touched.")).wrap(true).build();
    safe.add_css_class("dim-label");
    safe.add_css_class("caption");
    page.append(&safe);

    let exp = gtk::Expander::new(Some(tr("Details")));
    let log = gtk::TextView::builder().editable(false).monospace(true).wrap_mode(gtk::WrapMode::WordChar).build();
    log.add_css_class("log");
    let lf = gtk::Frame::new(None);
    lf.set_child(Some(&log));
    exp.set_child(Some(&lf));
    page.append(&exp);

    let refresh_btn = gtk::Button::builder().icon_name("view-refresh-symbolic").tooltip_text(tr("Rescan")).build();

    let say = {
        let log = log.clone();
        Rc::new(move |msg: &str| {
            let buf = log.buffer();
            buf.insert(&mut buf.end_iter(), &format!("{msg}\n"));
        })
    };

    // --- size scan
    let scan: SharedFn = Rc::new(RefCell::new(None));
    {
        let (rows, sizes, working, update_btn, refresh_btn2) =
            (rows.clone(), sizes.clone(), working.clone(), update_btn.clone(), refresh_btn.clone());
        let f: Rc<dyn Fn()> = Rc::new(move || {
            *working.borrow_mut() = true;
            refresh_btn2.set_sensitive(false);
            *sizes.borrow_mut() = vec![None; cleaners::items().len()];
            for r in rows.borrow().iter() {
                r.pill.set_visible(false);
                r.spin.set_visible(true);
                r.spin.set_spinning(true);
            }
            update_btn();
            let (tx, rx) = async_channel::unbounded::<Msg>();
            std::thread::spawn(move || {
                for (i, it) in cleaners::items().iter().enumerate() {
                    let _ = tx.send_blocking(Msg::Size(i, (it.size)()));
                }
                let _ = tx.send_blocking(Msg::ScanDone);
            });
            let (rows, sizes, working, update_btn, refresh_btn2) =
                (rows.clone(), sizes.clone(), working.clone(), update_btn.clone(), refresh_btn2.clone());
            spawn_rx(rx, move |m| match m {
                Msg::Size(i, s) => {
                    let rows = rows.borrow();
                    let r = &rows[i];
                    r.spin.set_spinning(false);
                    r.spin.set_visible(false);
                    r.pill.set_visible(true);
                    r.pill.remove_css_class("empty");
                    match s {
                        None => {
                            r.pill.set_label("—");
                            r.pill.add_css_class("empty");
                        }
                        Some(0) => {
                            r.pill.set_label(tr("clean"));
                            r.pill.add_css_class("empty");
                            r.sw.set_active(false);
                        }
                        Some(v) => {
                            r.pill.set_label(&human(v));
                            r.sw.set_active(true);
                        }
                    }
                    sizes.borrow_mut()[i] = s;
                }
                Msg::ScanDone => {
                    *working.borrow_mut() = false;
                    refresh_btn2.set_sensitive(true);
                    update_btn();
                }
                _ => {}
            });
        });
        *scan.borrow_mut() = Some(f);
    }
    {
        let scan = scan.clone();
        refresh_btn.connect_clicked(move |_| {
            let f = scan.borrow().clone();
            if let Some(f) = f {
                f();
            }
        });
    }

    // --- cleaning
    {
        let (rows, working, update_btn, refresh_btn2, scan, say, toasts, refresh_free) = (
            rows.clone(),
            working.clone(),
            update_btn.clone(),
            refresh_btn.clone(),
            scan.clone(),
            say.clone(),
            toasts.clone(),
            refresh_free,
        );
        clean_btn.connect_clicked(move |_| {
            let chosen: Vec<usize> = rows.borrow().iter().enumerate().filter(|(_, r)| r.sw.is_active()).map(|(i, _)| i).collect();
            if chosen.is_empty() {
                return;
            }
            *working.borrow_mut() = true;
            refresh_btn2.set_sensitive(false);
            update_btn();
            let before = platform::disk_usage(root_disk()).2;
            let (tx, rx) = async_channel::unbounded::<Msg>();
            std::thread::spawn(move || {
                let items = cleaners::items();
                for &i in chosen.iter().filter(|&&i| !items[i].root) {
                    let _ = tx.send_blocking(Msg::Log(format!("{}: {}", items[i].id, (items[i].run)())));
                }
                let root_ids: Vec<&str> = chosen.iter().filter(|&&i| items[i].root).map(|&i| items[i].id).collect();
                if !root_ids.is_empty() {
                    match cleaners::run_root_items(&root_ids) {
                        RootOutcome::Lines(l) => l.into_iter().for_each(|l| {
                            let _ = tx.send_blocking(Msg::Log(l));
                        }),
                        RootOutcome::Cancelled => {
                            let _ = tx.send_blocking(Msg::Log(tr("Authentication cancelled — administrator items skipped.").into()));
                        }
                        RootOutcome::Failed(e) => {
                            let _ = tx.send_blocking(Msg::Log(e));
                        }
                    }
                }
                let after = platform::disk_usage(root_disk()).2;
                let _ = tx.send_blocking(Msg::Done(after.saturating_sub(before)));
            });
            let (say, toasts, working, refresh_btn2, update_btn, scan, refresh_free) = (
                say.clone(),
                toasts.clone(),
                working.clone(),
                refresh_btn2.clone(),
                update_btn.clone(),
                scan.clone(),
                refresh_free.clone(),
            );
            spawn_rx(rx, move |m| match m {
                Msg::Log(l) => say(&l),
                Msg::Done(freed) => {
                    refresh_free();
                    toasts.add_toast(adw::Toast::new(&tr_f!("Done · freed {}", human(freed))));
                    *working.borrow_mut() = false;
                    refresh_btn2.set_sensitive(true);
                    update_btn();
                    let f = scan.borrow().clone();
                    if let Some(f) = f {
                        f();
                    }
                }
                _ => {}
            });
        });
    }

    let first = scan.borrow().clone();
    if let Some(f) = first {
        f();
    }
    CleanPage { root: scroll, refresh_btn }
}
