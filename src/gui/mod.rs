//! GTK4 + libadwaita front-end.

use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib};

use crate::i18n::tr;
use crate::{APP_ID, APP_NAME};

mod clean_page;
mod disk_page;

const CSS: &str = "
.disk-card { padding: 20px; border-radius: 16px; }
.disk-free { font-size: 34px; font-weight: 800; }
.disk-bar trough { min-height: 12px; border-radius: 6px; }
.disk-bar progress { min-height: 12px; border-radius: 6px; }
.disk-bar.full progress { background: @warning_bg_color; }
.size-pill { font-weight: 700; padding: 2px 10px; border-radius: 20px;
             background: alpha(@accent_bg_color, 0.15); color: @accent_color; }
.size-pill.empty { background: alpha(currentColor, 0.08); color: alpha(currentColor, 0.55); font-weight: 500; }
.clean-btn { font-weight: 700; padding: 12px 36px; }
.log { font-size: 12px; padding: 8px; }
";

pub(crate) fn margins(w: &impl IsA<gtk::Widget>, v: i32) {
    w.set_margin_top(v);
    w.set_margin_bottom(v);
    w.set_margin_start(v);
    w.set_margin_end(v);
}

/// Deliver messages from worker threads to the GTK main loop (no polling timer).
pub(crate) fn spawn_rx<T: 'static>(rx: async_channel::Receiver<T>, mut f: impl FnMut(T) + 'static) {
    glib::spawn_future_local(async move {
        while let Ok(m) = rx.recv().await {
            f(m);
        }
    });
}

/// Minimum interval between progress updates sent by the scan thread.
pub(crate) const PROGRESS_EVERY: Duration = Duration::from_millis(250);

fn build_ui(app: &adw::Application) {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(CSS);
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(&display, &provider, gtk::STYLE_PROVIDER_PRIORITY_APPLICATION);
    }

    let win = adw::ApplicationWindow::builder()
        .application(app)
        .title(APP_NAME)
        .default_width(1000)
        .default_height(780)
        .build();
    let toasts = adw::ToastOverlay::new();
    win.set_content(Some(&toasts));
    let view = adw::ToolbarView::new();
    toasts.set_child(Some(&view));

    let stack = adw::ViewStack::new();
    let header = adw::HeaderBar::new();
    let switcher = adw::ViewSwitcher::builder().stack(&stack).policy(adw::ViewSwitcherPolicy::Wide).build();
    header.set_title_widget(Some(&switcher));
    view.add_top_bar(&header);
    view.set_content(Some(&stack));

    let clean = clean_page::build(&toasts);
    header.pack_start(&clean.refresh_btn);
    stack.add_titled_with_icon(&clean.root, Some("clean"), tr("Cleanup"), "user-trash-symbolic");
    let disk = disk_page::build(&win);
    stack.add_titled_with_icon(&disk, Some("disk"), tr("Disk"), "drive-harddisk-symbolic");

    let rb = clean.refresh_btn.clone();
    stack.connect_visible_child_name_notify(move |s| rb.set_visible(s.visible_child_name().as_deref() == Some("clean")));

    win.present();
}

/// Start the application; returns the process exit code.
pub fn run() -> glib::ExitCode {
    glib::set_application_name(APP_NAME);
    let app = adw::Application::builder().application_id(APP_ID).build();
    app.connect_activate(build_ui);
    // We parse our own CLI before this point, so GTK must not see our arguments.
    app.run_with_args::<&str>(&[])
}
