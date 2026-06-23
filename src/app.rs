use crate::{config::Config, hotkeys, logging, overlay::Overlay, x11};
use gtk::prelude::*;
use std::rc::Rc;
use std::sync::mpsc;
use std::time::Duration;

pub struct ZoomixApp;

impl ZoomixApp {
    pub fn launch(app: &gtk::Application) -> anyhow::Result<()> {
        logging::info("app launch requested");
        x11::assert_x11_available()?;
        let config = Config::load().unwrap_or_default();
        config.ensure_parent_dirs()?;
        if let Some(path) = logging::path() {
            logging::info(format!("log file path: {}", path.display()));
        }

        let overlay = Rc::new(Overlay::new(app, config.clone()));
        let (sender, receiver) = mpsc::channel();
        hotkeys::spawn_listener(config.hotkeys.clone(), sender);

        let overlay_for_receiver = overlay.clone();
        glib::timeout_add_local(Duration::from_millis(25), move || {
            for mode in receiver.try_iter() {
                logging::info(format!("main loop received hotkey mode {mode:?}"));
                overlay_for_receiver.activate(mode);
            }
            glib::ControlFlow::Continue
        });

        install_tray_window(app, overlay, &config)?;
        Ok(())
    }
}

fn install_tray_window(
    app: &gtk::Application,
    overlay: Rc<Overlay>,
    config: &Config,
) -> anyhow::Result<()> {
    let window = gtk::ApplicationWindow::builder()
        .application(app)
        .title("Zoomix")
        .default_width(300)
        .default_height(150)
        .build();
    let box_ = gtk::Box::new(gtk::Orientation::Vertical, 8);
    box_.set_margin_top(12);
    box_.set_margin_bottom(12);
    box_.set_margin_start(12);
    box_.set_margin_end(12);
    let label = gtk::Label::new(Some(
        "Zoomix is running. Use Ctrl+1 zoom, Ctrl+4 live zoom, Ctrl+2 draw, Ctrl+3 snip.",
    ));
    label.set_line_wrap(true);
    box_.pack_start(&label, true, true, 0);

    let row = gtk::Box::new(gtk::Orientation::Horizontal, 6);
    let draw = gtk::Button::with_label("Draw");
    let zoom = gtk::Button::with_label("Zoom");
    let snip = gtk::Button::with_label("Snip");
    row.pack_start(&zoom, true, true, 0);
    row.pack_start(&draw, true, true, 0);
    row.pack_start(&snip, true, true, 0);
    box_.pack_start(&row, false, false, 0);
    window.add(&box_);

    let ov = overlay.clone();
    let control_window = window.clone();
    zoom.connect_clicked(move |_| {
        control_window.hide();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        ov.activate(crate::model::Mode::Zoom);
    });
    let ov = overlay.clone();
    let control_window = window.clone();
    draw.connect_clicked(move |_| {
        control_window.hide();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        ov.activate(crate::model::Mode::Draw);
    });
    let control_window = window.clone();
    snip.connect_clicked(move |_| {
        control_window.hide();
        while gtk::events_pending() {
            gtk::main_iteration_do(false);
        }
        overlay.activate(crate::model::Mode::Snip);
    });

    if !config.start_hidden {
        window.show_all();
    }
    Ok(())
}
