use crate::{
    capture,
    config::{color_for_key, Config},
    geometry::{Point, Rect},
    logging,
    model::{Annotation, AppState, DrawTool, Mode, DEFAULT_ZOOM_FACTOR},
    render, x11,
};
use gdk_pixbuf::Pixbuf;
use glib::Propagation;
use gtk::prelude::*;
use std::{cell::RefCell, rc::Rc, time::Duration};

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
        overlay
    }

    pub fn activate(&self, mode: Mode) {
        logging::info(format!("overlay activate requested: {mode:?}"));
        if matches!(mode, Mode::Zoom | Mode::LiveZoom | Mode::Snip)
            || self.background.borrow().is_none()
        {
            *self.background.borrow_mut() = capture::capture_root().ok();
        }

        {
            let mut state = self.state.borrow_mut();
            let previous_mode = state.mode;
            logging::info(format!(
                "overlay mode transition: {previous_mode:?} -> {mode:?}"
            ));
            state.mode = mode;
            state.clear_interaction();
            match mode {
                Mode::Zoom | Mode::LiveZoom if previous_mode == Mode::Idle => {
                    state.zoom_factor = DEFAULT_ZOOM_FACTOR;
                    state.zoom_center = x11::pointer_position().unwrap_or(state.zoom_center);
                }
                Mode::Draw if previous_mode == Mode::Idle => {
                    state.zoom_factor = 1.0;
                    state.zoom_center = Point::new(0, 0);
                    state.tool = DrawTool::Pen;
                }
                Mode::Snip => {
                    state.zoom_factor = 1.0;
                    state.zoom_center = Point::new(0, 0);
                    state.tool = DrawTool::Rectangle;
                }
                _ => {}
            }
        }
        self.window.show_all();
        self.window.fullscreen();
        self.window.set_keep_above(true);
        self.window.present();
        self.area.grab_focus();
        self.area.queue_draw();
        let area = self.area.clone();
        glib::idle_add_local_once(move || {
            logging::info("overlay idle redraw after global activation");
            area.queue_draw();
        });
        logging::info(format!("overlay presented for mode {mode:?}"));
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
            logging::info(format!(
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
            if matches!(state.mode, Mode::Zoom | Mode::LiveZoom) {
                return Propagation::Stop;
            }
            let point = Point::new(x as i32, y as i32);
            state.drag_start = Some(point);
            state.drag_current = Some(point);
            if matches!(
                state.tool,
                DrawTool::Pen | DrawTool::Highlight | DrawTool::Eraser
            ) {
                state.current_points.clear();
                state.current_points.push(point);
            }
            Propagation::Stop
        });

        let state = self.state.clone();
        let area = self.area.clone();
        self.area.connect_motion_notify_event(move |_, event| {
            let (x, y) = event.position();
            let mut state = state.borrow_mut();
            if matches!(state.mode, Mode::Zoom | Mode::LiveZoom) {
                return Propagation::Stop;
            }
            if state.drag_start.is_none() {
                return Propagation::Stop;
            }
            let point = Point::new(x as i32, y as i32);
            state.drag_current = Some(point);
            if matches!(
                state.tool,
                DrawTool::Pen | DrawTool::Highlight | DrawTool::Eraser
            ) {
                state.current_points.push(point);
            }
            area.queue_draw();
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
            if matches!(state.mode, Mode::Zoom | Mode::LiveZoom) {
                return Propagation::Stop;
            }
            let current = Point::new(x as i32, y as i32);
            if state.mode == Mode::Snip {
                let rect = Rect::from_points(state.drag_start.unwrap_or(current), current);
                if rect.is_empty() {
                    state.drag_start = None;
                    state.drag_current = None;
                    area.queue_draw();
                    return Propagation::Stop;
                }
                state.reset_overlay();
                drop(state);
                window.hide();
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
                schedule_snip_capture(config.clone(), clipboard.clone(), rect);
                area.queue_draw();
                return Propagation::Stop;
            }

            let annotation = match state.tool {
                DrawTool::Pen | DrawTool::Highlight | DrawTool::Eraser
                    if state.current_points.len() > 1 =>
                {
                    let color = if state.tool == DrawTool::Eraser {
                        crate::model::Color::BLACK
                    } else {
                        state.color
                    };
                    Some(Annotation::Stroke {
                        points: std::mem::take(&mut state.current_points),
                        color,
                        width: if state.tool == DrawTool::Eraser {
                            state.stroke_width * 3.0
                        } else {
                            state.stroke_width
                        },
                        highlight: state.tool == DrawTool::Highlight,
                    })
                }
                DrawTool::Line | DrawTool::Rectangle | DrawTool::Ellipse | DrawTool::Arrow => {
                    state.drag_start.map(|start| Annotation::Shape {
                        tool: state.tool,
                        rect: Rect::from_points(start, current),
                        start,
                        end: current,
                        color: state.color,
                        width: state.stroke_width,
                    })
                }
                _ => None,
            };
            if let Some(annotation) = annotation {
                state.annotations.push(annotation);
            }
            state.drag_start = None;
            state.drag_current = None;
            area.queue_draw();
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
                gdk::ScrollDirection::Up => state.zoom_factor = (state.zoom_factor + 0.25).min(8.0),
                gdk::ScrollDirection::Down => {
                    state.zoom_factor = (state.zoom_factor - 0.25).max(1.0)
                }
                _ => {}
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
        logging::info(format!(
            "overlay keypress name={name} modifiers={modifiers:?}"
        ));
        if modifiers.contains(gdk::ModifierType::CONTROL_MASK) {
            match name.as_str() {
                "1" => {
                    logging::info("overlay Ctrl+1 -> Zoom");
                    self.activate_mode(Mode::Zoom);
                    return;
                }
                "2" => {
                    logging::info("overlay Ctrl+2 -> Draw");
                    self.activate_mode(Mode::Draw);
                    return;
                }
                "3" => {
                    logging::info("overlay Ctrl+3 -> Snip");
                    self.activate_mode(Mode::Snip);
                    return;
                }
                "4" => {
                    logging::info("overlay Ctrl+4 -> LiveZoom");
                    self.activate_mode(Mode::LiveZoom);
                    return;
                }
                _ => {}
            }
        }

        let mut state = self.state.borrow_mut();
        match name.as_str() {
            "Escape" => {
                state.reset_overlay();
                self.window.hide();
            }
            "BackSpace" | "z" | "Z" => {
                state.annotations.pop();
                self.area.queue_draw();
            }
            "Delete" | "c" | "C" => {
                state.annotations.clear();
                self.area.queue_draw();
            }
            "plus" | "equal" => {
                state.zoom_factor = (state.zoom_factor + 0.25).min(8.0);
                self.area.queue_draw();
            }
            "minus" => {
                state.zoom_factor = (state.zoom_factor - 0.25).max(1.0);
                self.area.queue_draw();
            }
            "R" if !matches!(state.mode, Mode::Text | Mode::Zoom | Mode::LiveZoom) => {
                state.color = crate::model::Color::RED;
            }
            "p" | "P" | "1" if !matches!(state.mode, Mode::Zoom | Mode::LiveZoom) => {
                self.set_tool(&mut state, DrawTool::Pen)
            }
            "r" | "2" if !matches!(state.mode, Mode::Text | Mode::Zoom | Mode::LiveZoom) => {
                self.set_tool(&mut state, DrawTool::Rectangle)
            }
            "a" | "A" | "3" if !matches!(state.mode, Mode::Zoom | Mode::LiveZoom) => {
                self.set_tool(&mut state, DrawTool::Arrow)
            }
            "l" | "L" | "4" if !matches!(state.mode, Mode::Zoom | Mode::LiveZoom) => {
                self.set_tool(&mut state, DrawTool::Line)
            }
            "e" | "E" | "5" if !matches!(state.mode, Mode::Zoom | Mode::LiveZoom) => {
                self.set_tool(&mut state, DrawTool::Ellipse)
            }
            "h" | "H" | "6" if !matches!(state.mode, Mode::Zoom | Mode::LiveZoom) => {
                self.set_tool(&mut state, DrawTool::Highlight)
            }
            "x" | "X" | "7" if !matches!(state.mode, Mode::Zoom | Mode::LiveZoom) => {
                self.set_tool(&mut state, DrawTool::Eraser)
            }
            "Tab" | "space" if !matches!(state.mode, Mode::Zoom | Mode::LiveZoom) => {
                let next = match state.tool {
                    DrawTool::Pen => DrawTool::Rectangle,
                    DrawTool::Rectangle => DrawTool::Arrow,
                    DrawTool::Arrow => DrawTool::Line,
                    DrawTool::Line => DrawTool::Ellipse,
                    DrawTool::Ellipse => DrawTool::Highlight,
                    DrawTool::Highlight => DrawTool::Eraser,
                    DrawTool::Eraser => DrawTool::Pen,
                };
                self.set_tool(&mut state, next);
            }
            "t" | "T" => state.mode = Mode::Text,
            "Return" if state.mode == Mode::Text => {
                let at = state
                    .drag_current
                    .or(state.drag_start)
                    .unwrap_or(Point::new(80, 80));
                let text = std::mem::take(&mut state.pending_text);
                if !text.is_empty() {
                    let color = state.color;
                    state.annotations.push(Annotation::Text {
                        at,
                        text,
                        color,
                        font: self.config.drawing.font.clone(),
                        size: self.config.drawing.font_size,
                    });
                }
                state.mode = Mode::Draw;
                self.area.queue_draw();
            }
            "Return" if state.mode == Mode::Snip => {
                let rect = Rect::from_points(
                    state.drag_start.unwrap_or(Point::new(0, 0)),
                    state.drag_current.unwrap_or(Point::new(0, 0)),
                );
                if rect.is_empty() {
                    state.drag_start = None;
                    state.drag_current = None;
                    self.area.queue_draw();
                    return;
                }
                state.reset_overlay();
                drop(state);
                self.window.hide();
                while gtk::events_pending() {
                    gtk::main_iteration_do(false);
                }
                schedule_snip_capture(self.config.clone(), self.clipboard.clone(), rect);
            }
            _ => {
                if let Some(ch) = name.chars().next().and_then(color_for_key) {
                    state.color = ch;
                } else if state.mode == Mode::Text && name.len() == 1 {
                    state.pending_text.push_str(&name);
                }
            }
        }
    }

    fn set_tool(&self, state: &mut AppState, tool: DrawTool) {
        if matches!(state.mode, Mode::Zoom | Mode::LiveZoom) {
            state.mode = Mode::Draw;
        }
        state.tool = tool;
        state.clear_interaction();
        self.area.queue_draw();
    }

    fn activate_mode(&self, mode: Mode) {
        logging::info(format!("overlay local activate requested: {mode:?}"));
        if mode == Mode::Snip {
            self.window.hide();
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        }

        if matches!(mode, Mode::Zoom | Mode::LiveZoom | Mode::Snip)
            || self.background.borrow().is_none()
        {
            *self.background.borrow_mut() = capture::capture_root().ok();
        }

        {
            let mut state = self.state.borrow_mut();
            let previous_mode = state.mode;
            state.mode = mode;
            state.clear_interaction();
            match mode {
                Mode::Zoom | Mode::LiveZoom if previous_mode == Mode::Idle => {
                    state.zoom_factor = DEFAULT_ZOOM_FACTOR;
                    state.zoom_center = x11::pointer_position().unwrap_or(state.zoom_center);
                }
                Mode::Draw if previous_mode == Mode::Idle => {
                    state.zoom_factor = 1.0;
                    state.zoom_center = Point::new(0, 0);
                    state.tool = DrawTool::Pen;
                }
                Mode::Snip => {
                    state.zoom_factor = 1.0;
                    state.zoom_center = Point::new(0, 0);
                    state.tool = DrawTool::Rectangle;
                }
                _ => {}
            }
        }

        self.window.show_all();
        self.window.fullscreen();
        self.window.set_keep_above(true);
        self.window.present();
        self.area.grab_focus();
        self.area.queue_draw();
        let area = self.area.clone();
        glib::idle_add_local_once(move || {
            logging::info("overlay idle redraw after local activation");
            area.queue_draw();
        });
        logging::info(format!("overlay locally presented for mode {mode:?}"));
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
