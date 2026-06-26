use crate::{
    capture,
    config::Config,
    geometry::{Point, Rect},
    hotkeys::{self, HotkeyModifiers},
    input::{self, KeyOutcome, PointerRelease, TextStyle, ZoomDirection},
    logging,
    model::{AppState, Mode},
    render, x11,
};
use gdk_pixbuf::Pixbuf;
use glib::Propagation;
use gtk::prelude::*;
use std::{cell::RefCell, rc::Rc, time::Duration};

const LIVE_ZOOM_REFRESH: Duration = Duration::from_millis(100);

pub struct Overlay {
    window: gtk::ApplicationWindow,
    area: gtk::DrawingArea,
    state: Rc<RefCell<AppState>>,
    config: Config,
    background: Rc<RefCell<Option<Pixbuf>>>,
    clipboard: Rc<RefCell<Option<arboard::Clipboard>>>,
}

impl Overlay {
    pub fn new(app: &gtk::Application, config: Config) -> Self {
        let window = gtk::ApplicationWindow::builder()
            .application(app)
            .title("Zoomix Overlay")
            .decorated(false)
            .skip_taskbar_hint(true)
            .skip_pager_hint(true)
            .app_paintable(true)
            .build();
        window.fullscreen();
        window.set_keep_above(true);
        window.set_accept_focus(true);

        let area = gtk::DrawingArea::new();
        area.set_can_focus(true);
        window.add(&area);

        let initial = AppState {
            stroke_width: config.drawing.stroke_width,
            ..Default::default()
        };

        let overlay = Self {
            window,
            area,
            state: Rc::new(RefCell::new(initial)),
            config,
            background: Rc::new(RefCell::new(None)),
            clipboard: Rc::new(RefCell::new(None)),
        };
        overlay.connect_events();
        overlay.connect_live_zoom_refresh();
        overlay
    }

    pub fn activate(&self, mode: Mode) {
        self.clone_handles()
            .activate_mode_from(mode, ActivationSource::Global);
    }

    fn connect_events(&self) {
        self.area.add_events(
            gdk::EventMask::BUTTON_PRESS_MASK
                | gdk::EventMask::BUTTON_RELEASE_MASK
                | gdk::EventMask::POINTER_MOTION_MASK
                | gdk::EventMask::KEY_PRESS_MASK
                | gdk::EventMask::SCROLL_MASK,
        );

        let state = self.state.clone();
        let background = self.background.clone();
        self.area.connect_draw(move |area, cr| {
            let alloc = area.allocation();
            logging::verbose(format!(
                "overlay draw mode={:?} size={}x{} has_background={}",
                state.borrow().mode,
                alloc.width(),
                alloc.height(),
                background.borrow().is_some()
            ));
            render::draw_overlay(
                cr,
                background.borrow().as_ref(),
                &state.borrow(),
                alloc.width(),
                alloc.height(),
            );
            Propagation::Stop
        });

        let state = self.state.clone();
        self.area.connect_button_press_event(move |_, event| {
            let (x, y) = event.position();
            let mut state = state.borrow_mut();
            input::pointer_press(&mut state, Point::new(x as i32, y as i32));
            Propagation::Stop
        });

        let state = self.state.clone();
        let area = self.area.clone();
        self.area.connect_motion_notify_event(move |_, event| {
            let (x, y) = event.position();
            let mut state = state.borrow_mut();
            if input::pointer_move(&mut state, Point::new(x as i32, y as i32)) {
                area.queue_draw();
            }
            Propagation::Stop
        });

        let state = self.state.clone();
        let area = self.area.clone();
        let window = self.window.clone();
        let config = self.config.clone();
        let clipboard = self.clipboard.clone();
        self.area.connect_button_release_event(move |_, event| {
            let (x, y) = event.position();
            let mut state = state.borrow_mut();
            match input::pointer_release(&mut state, Point::new(x as i32, y as i32)) {
                PointerRelease::None => {}
                PointerRelease::Redraw => area.queue_draw(),
                PointerRelease::CaptureSnip(rect) => {
                    drop(state);
                    window.hide();
                    while gtk::events_pending() {
                        gtk::main_iteration_do(false);
                    }
                    schedule_snip_capture(config.clone(), clipboard.clone(), rect);
                    area.queue_draw();
                }
            }
            Propagation::Stop
        });

        let this = self.clone_handles();
        self.area.connect_key_press_event(move |_, event| {
            this.handle_key(event.keyval(), event.state());
            Propagation::Stop
        });

        let state = self.state.clone();
        let area = self.area.clone();
        self.area.connect_scroll_event(move |_, event| {
            let mut state = state.borrow_mut();
            match event.direction() {
                gdk::ScrollDirection::Up => input::scroll_zoom(&mut state, ZoomDirection::In),
                gdk::ScrollDirection::Down => input::scroll_zoom(&mut state, ZoomDirection::Out),
                _ => {}
            }
            area.queue_draw();
            Propagation::Stop
        });
    }

    fn connect_live_zoom_refresh(&self) {
        let state = self.state.clone();
        let background = self.background.clone();
        let window = self.window.clone();
        let area = self.area.clone();

        glib::timeout_add_local(LIVE_ZOOM_REFRESH, move || {
            if state.borrow().mode != Mode::LiveZoom {
                return glib::ControlFlow::Continue;
            }

            if let Ok(pointer) = x11::pointer_position() {
                input::update_live_zoom_center(&mut state.borrow_mut(), pointer);
            }

            match capture_root_with_overlay_hidden(&window, &area) {
                Ok(pixbuf) => {
                    *background.borrow_mut() = Some(pixbuf);
                    area.queue_draw();
                }
                Err(err) => {
                    let message = format!("{err:#}");
                    logging::error(format!("live zoom capture failed: {message}"));
                    eprintln!("zoomix live zoom capture failed: {message}");
                    {
                        let mut state = state.borrow_mut();
                        input::capture_failed(&mut state, Mode::LiveZoom, message);
                    }
                    *background.borrow_mut() = None;
                    area.queue_draw();
                }
            }

            glib::ControlFlow::Continue
        });
    }

    fn clone_handles(&self) -> OverlayHandles {
        OverlayHandles {
            window: self.window.clone(),
            area: self.area.clone(),
            state: self.state.clone(),
            config: self.config.clone(),
            background: self.background.clone(),
            clipboard: self.clipboard.clone(),
        }
    }
}

#[derive(Clone)]
struct OverlayHandles {
    window: gtk::ApplicationWindow,
    area: gtk::DrawingArea,
    state: Rc<RefCell<AppState>>,
    config: Config,
    background: Rc<RefCell<Option<Pixbuf>>>,
    clipboard: Rc<RefCell<Option<arboard::Clipboard>>>,
}

impl OverlayHandles {
    fn handle_key(&self, keyval: gdk::keys::Key, modifiers: gdk::ModifierType) {
        let name = keyval.name().map(|s| s.to_string()).unwrap_or_default();
        logging::verbose(format!(
            "overlay keypress name={name} modifiers={modifiers:?}"
        ));

        let hotkey_modifiers = HotkeyModifiers::from_gdk(modifiers);
        if let Some(mode) = hotkeys::mode_for_event(&self.config.hotkeys, &name, hotkey_modifiers) {
            logging::info(format!("overlay configured hotkey -> {mode:?}"));
            self.activate_mode(mode);
            return;
        }

        let ctrl = hotkey_modifiers.ctrl;
        let mode = self.state.borrow().mode;
        let Some(action) = input::key_to_action(mode, &name, ctrl) else {
            return;
        };

        let text_style = TextStyle {
            font: self.config.drawing.font.clone(),
            size: self.config.drawing.font_size,
        };
        let outcome = {
            let mut state = self.state.borrow_mut();
            input::apply_key_action(&mut state, action, &text_style)
        };

        match outcome {
            KeyOutcome::None => {}
            KeyOutcome::Redraw => self.area.queue_draw(),
            KeyOutcome::HideOverlay { clear_background } => {
                if clear_background {
                    *self.background.borrow_mut() = None;
                }
                self.window.hide();
            }
            KeyOutcome::CaptureSnip(rect) => {
                self.window.hide();
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
                schedule_snip_capture(self.config.clone(), self.clipboard.clone(), rect);
                self.area.queue_draw();
            }
        }
    }

    fn activate_mode(&self, mode: Mode) {
        self.activate_mode_from(mode, ActivationSource::Local);
    }

    fn activate_mode_from(&self, mode: Mode, source: ActivationSource) {
        logging::info(format!(
            "overlay {} activate requested: {mode:?}",
            source.label()
        ));
        if source.hides_before_capture(mode) {
            self.window.hide();
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        }

        let pointer = x11::pointer_position().unwrap_or_default();
        {
            let mut state = self.state.borrow_mut();
            let previous_mode = state.mode;
            logging::info(format!(
                "overlay mode transition: {previous_mode:?} -> {mode:?}"
            ));
            activate_state_with_capture(&mut state, &self.background, mode, pointer);
        }

        present_overlay_window(&self.window, &self.area, source.activation_label());
        logging::info(format!(
            "overlay {} presented for mode {mode:?}",
            source.label()
        ));
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivationSource {
    Global,
    Local,
}

impl ActivationSource {
    fn label(self) -> &'static str {
        match self {
            Self::Global => "global",
            Self::Local => "local",
        }
    }

    fn activation_label(self) -> &'static str {
        match self {
            Self::Global => "global activation",
            Self::Local => "local activation",
        }
    }

    fn hides_before_capture(self, mode: Mode) -> bool {
        self == Self::Local && matches!(mode, Mode::Snip | Mode::LiveZoom)
    }
}

fn activate_state_with_capture(
    state: &mut AppState,
    background: &Rc<RefCell<Option<Pixbuf>>>,
    mode: Mode,
    pointer: Point,
) {
    let effect = input::activate_mode(state, mode, pointer, background.borrow().is_some());
    if !effect.capture_background {
        return;
    }

    match capture::capture_root() {
        Ok(pixbuf) => {
            *background.borrow_mut() = Some(pixbuf);
        }
        Err(err) => {
            *background.borrow_mut() = None;
            let message = format!("{err:#}");
            logging::error(format!(
                "capture failed during {mode:?} activation: {message}"
            ));
            eprintln!("zoomix capture failed: {message}");
            input::capture_failed(state, mode, message);
        }
    }
}

fn present_overlay_window(
    window: &gtk::ApplicationWindow,
    area: &gtk::DrawingArea,
    activation_source: &'static str,
) {
    window.show_all();
    window.fullscreen();
    window.set_keep_above(true);
    window.set_focus(Some(area));
    window.present();
    area.grab_focus();
    area.queue_draw();

    flush_gtk_events();

    let area = area.clone();
    glib::idle_add_local_once(move || {
        logging::verbose(format!("overlay idle redraw after {activation_source}"));
        area.queue_draw();
    });
}

fn capture_root_with_overlay_hidden(
    window: &gtk::ApplicationWindow,
    area: &gtk::DrawingArea,
) -> anyhow::Result<Pixbuf> {
    let was_visible = window.is_visible();
    if was_visible {
        window.hide();
        flush_gtk_events();
    }

    let result = capture::capture_root();

    if was_visible {
        restore_overlay_window(window, area);
        flush_gtk_events();
    }

    result
}

fn restore_overlay_window(window: &gtk::ApplicationWindow, area: &gtk::DrawingArea) {
    window.show_all();
    window.fullscreen();
    window.set_keep_above(true);
    window.set_focus(Some(area));
    window.present();
    area.grab_focus();
}

fn flush_gtk_events() {
    if let Some(display) = gdk::Display::default() {
        display.flush();
    }
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
}

fn schedule_snip_capture(
    config: Config,
    clipboard_store: Rc<RefCell<Option<arboard::Clipboard>>>,
    rect: Rect,
) {
    logging::info(format!(
        "scheduling snip capture after overlay hide: x={} y={} width={} height={}",
        rect.x, rect.y, rect.width, rect.height
    ));
    glib::timeout_add_local_once(Duration::from_millis(120), move || {
        if let Err(err) = save_snip_rect(&config, &clipboard_store, rect) {
            logging::error(format!("snip failed: {err:#}"));
            eprintln!("zoomix snip failed: {err:#}");
        }
    });
}

fn save_snip_rect(
    config: &Config,
    clipboard_store: &Rc<RefCell<Option<arboard::Clipboard>>>,
    rect: Rect,
) -> anyhow::Result<()> {
    let root = capture::capture_root()?;
    let pixbuf = capture::crop(&root, rect)?;
    let path = capture::save_png(&pixbuf, &config.screenshots.directory)?;
    if config.screenshots.copy_to_clipboard {
        *clipboard_store.borrow_mut() = Some(capture::copy_to_clipboard(&pixbuf)?);
    }
    eprintln!("zoomix saved {}", path.display());
    Ok(())
}
