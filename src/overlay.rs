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
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

const MAGNIFIER_APPLICATION_SCHEMA: &str = "org.cinnamon.desktop.a11y.applications";
const MAGNIFIER_SCHEMA: &str = "org.cinnamon.desktop.a11y.magnifier";

pub struct Overlay {
    window: gtk::ApplicationWindow,
    area: gtk::DrawingArea,
    state: Rc<RefCell<AppState>>,
    config: Config,
    background: Rc<RefCell<Option<Pixbuf>>>,
    clipboard: Rc<RefCell<Option<arboard::Clipboard>>>,
    magnifier: Rc<CinnamonMagnifier>,
    live_zoom_active: Arc<AtomicBool>,
}

impl Overlay {
    pub fn new(app: &gtk::Application, config: Config, live_zoom_active: Arc<AtomicBool>) -> Self {
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
        if let Some(screen) = gtk::prelude::WidgetExt::screen(&window) {
            if let Some(visual) = screen.rgba_visual() {
                window.set_visual(Some(&visual));
            }
        }

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
            magnifier: Rc::new(CinnamonMagnifier::new()),
            live_zoom_active,
        };
        overlay.connect_events();
        overlay
    }

    pub fn activate(&self, mode: Mode) {
        self.clone_handles()
            .activate_mode_from(mode, ActivationSource::Global);
    }

    pub fn adjust_live_zoom(&self, direction: ZoomDirection) {
        let mut state = self.state.borrow_mut();
        if state.mode != Mode::LiveZoom {
            return;
        }
        input::scroll_zoom(&mut state, direction);
        self.magnifier.set_factor(state.zoom_factor);
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
            if state.borrow().mode == Mode::LiveZoom {
                cr.set_operator(cairo::Operator::Source);
                cr.set_source_rgba(0.0, 0.0, 0.0, 0.0);
                let _ = cr.paint();
                return Propagation::Stop;
            }
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
        let background = self.background.clone();
        let clipboard = self.clipboard.clone();
        self.area.connect_button_release_event(move |_, event| {
            let (x, y) = event.position();
            let mut state = state.borrow_mut();
            let snip_source =
                (state.mode == Mode::Snip).then(|| capture_snip_source(&background, &state, &area));
            match input::pointer_release(&mut state, Point::new(x as i32, y as i32)) {
                PointerRelease::None => {}
                PointerRelease::Redraw => area.queue_draw(),
                PointerRelease::CaptureSnip(rect) => {
                    let resume_overlay = state.mode != Mode::Idle;
                    drop(state);
                    save_snip_result(
                        &config,
                        &clipboard,
                        snip_source.unwrap_or_else(missing_snip_source),
                        rect,
                    );
                    finish_snip_ui(&window, &area, &background, resume_overlay);
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
        let magnifier = self.magnifier.clone();
        self.area.connect_scroll_event(move |_, event| {
            let mut state = state.borrow_mut();
            match event.direction() {
                gdk::ScrollDirection::Up => input::scroll_zoom(&mut state, ZoomDirection::In),
                gdk::ScrollDirection::Down => input::scroll_zoom(&mut state, ZoomDirection::Out),
                _ => {}
            }
            if state.mode == Mode::LiveZoom {
                magnifier.set_factor(state.zoom_factor);
            }
            area.queue_draw();
            Propagation::Stop
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
            magnifier: self.magnifier.clone(),
            live_zoom_active: self.live_zoom_active.clone(),
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
    magnifier: Rc<CinnamonMagnifier>,
    live_zoom_active: Arc<AtomicBool>,
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
            let state = self.state.borrow();
            let snip_source = (state.mode == Mode::Snip)
                .then(|| capture_snip_source(&self.background, &state, &self.area));
            drop(state);
            let mut state = self.state.borrow_mut();
            (
                input::apply_key_action(&mut state, action, &text_style),
                snip_source,
            )
        };

        match outcome.0 {
            KeyOutcome::None => {}
            KeyOutcome::Redraw => self.area.queue_draw(),
            KeyOutcome::HideOverlay { clear_background } => {
                self.magnifier.restore();
                if clear_background {
                    *self.background.borrow_mut() = None;
                }
                self.window.hide();
            }
            KeyOutcome::CaptureSnip(rect) => {
                let resume_overlay = self.state.borrow().mode != Mode::Idle;
                save_snip_result(
                    &self.config,
                    &self.clipboard,
                    outcome.1.unwrap_or_else(missing_snip_source),
                    rect,
                );
                finish_snip_ui(&self.window, &self.area, &self.background, resume_overlay);
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
        let current_mode = self.state.borrow().mode;
        if input::mode_is_active(current_mode, mode) {
            logging::info(format!(
                "overlay {} toggle off requested: {current_mode:?}",
                source.label()
            ));
            self.magnifier.restore();
            self.live_zoom_active.store(false, Ordering::Release);
            self.state.borrow_mut().reset_overlay();
            *self.background.borrow_mut() = None;
            self.window.hide();
            flush_gtk_events();
            return;
        }

        if source.hides_before_capture(mode) && self.background.borrow().is_none() {
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
            if previous_mode == Mode::LiveZoom && mode != Mode::LiveZoom {
                self.magnifier.restore();
                self.live_zoom_active.store(false, Ordering::Release);
            }
            if mode == Mode::LiveZoom {
                input::activate_mode(&mut state, mode, pointer, false);
                *self.background.borrow_mut() = None;
                self.magnifier.enable(state.zoom_factor);
                self.live_zoom_active.store(true, Ordering::Release);
            } else {
                activate_state_with_capture(&mut state, &self.background, mode, pointer);
            }
        }

        if mode == Mode::LiveZoom {
            self.window.hide();
            flush_gtk_events();
            logging::info("live zoom active with application input pass-through");
            return;
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
        self == Self::Local && mode == Mode::Snip
    }
}

struct CinnamonMagnifier {
    applications: gio::Settings,
    magnifier: gio::Settings,
    previous: RefCell<Option<(bool, f64)>>,
}

impl CinnamonMagnifier {
    fn new() -> Self {
        Self {
            applications: gio::Settings::new(MAGNIFIER_APPLICATION_SCHEMA),
            magnifier: gio::Settings::new(MAGNIFIER_SCHEMA),
            previous: RefCell::new(None),
        }
    }

    fn enable(&self, factor: f64) {
        if self.previous.borrow().is_none() {
            *self.previous.borrow_mut() = Some((
                self.applications.boolean("screen-magnifier-enabled"),
                self.magnifier.double("mag-factor"),
            ));
        }
        self.set_factor(factor);
        if let Err(err) = self
            .applications
            .set_boolean("screen-magnifier-enabled", true)
        {
            logging::error(format!("could not enable Cinnamon magnifier: {err}"));
        }
    }

    fn set_factor(&self, factor: f64) {
        if let Err(err) = self.magnifier.set_double("mag-factor", factor.max(1.0)) {
            logging::error(format!("could not set Cinnamon magnifier factor: {err}"));
        }
    }

    fn restore(&self) {
        let Some((enabled, factor)) = self.previous.borrow_mut().take() else {
            return;
        };
        if let Err(err) = self.magnifier.set_double("mag-factor", factor) {
            logging::error(format!(
                "could not restore Cinnamon magnifier factor: {err}"
            ));
        }
        if let Err(err) = self
            .applications
            .set_boolean("screen-magnifier-enabled", enabled)
        {
            logging::error(format!("could not restore Cinnamon magnifier state: {err}"));
        }
    }
}

impl Drop for CinnamonMagnifier {
    fn drop(&mut self) {
        self.restore();
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

fn flush_gtk_events() {
    if let Some(display) = gdk::Display::default() {
        display.flush();
    }
    while gtk::events_pending() {
        gtk::main_iteration_do(false);
    }
}

fn capture_snip_source(
    background: &Rc<RefCell<Option<Pixbuf>>>,
    state: &AppState,
    area: &gtk::DrawingArea,
) -> anyhow::Result<Pixbuf> {
    let background = background.borrow();
    let background = background
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("snip background is unavailable"))?;
    let allocation = area.allocation();
    render::capture_overlay(background, state, allocation.width(), allocation.height())
}

fn finish_snip_ui(
    window: &gtk::ApplicationWindow,
    area: &gtk::DrawingArea,
    background: &Rc<RefCell<Option<Pixbuf>>>,
    resume_overlay: bool,
) {
    if resume_overlay {
        area.queue_draw();
    } else {
        window.hide();
        *background.borrow_mut() = None;
        flush_gtk_events();
    }
}

fn save_snip_result(
    config: &Config,
    clipboard_store: &Rc<RefCell<Option<arboard::Clipboard>>>,
    source: anyhow::Result<Pixbuf>,
    rect: Rect,
) {
    if let Err(err) =
        source.and_then(|source| save_snip_rect(config, clipboard_store, &source, rect))
    {
        logging::error(format!("snip failed: {err:#}"));
        eprintln!("zoomix snip failed: {err:#}");
    }
}

fn missing_snip_source() -> anyhow::Result<Pixbuf> {
    Err(anyhow::anyhow!("snip source was not prepared"))
}

fn save_snip_rect(
    config: &Config,
    clipboard_store: &Rc<RefCell<Option<arboard::Clipboard>>>,
    source: &Pixbuf,
    rect: Rect,
) -> anyhow::Result<()> {
    let pixbuf = capture::crop(source, rect)?;
    let path = capture::save_png(&pixbuf, &config.screenshots.directory)?;
    if config.screenshots.copy_to_clipboard {
        *clipboard_store.borrow_mut() = Some(capture::copy_to_clipboard(&pixbuf)?);
    }
    eprintln!("zoomix saved {}", path.display());
    Ok(())
}
