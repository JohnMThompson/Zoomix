use crate::geometry::{Point, Rect};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Idle,
    Zoom,
    LiveZoom,
    Draw,
    Text,
    Snip,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DrawTool {
    Pen,
    Line,
    Rectangle,
    Ellipse,
    Arrow,
    Highlight,
    Eraser,
}

pub const DEFAULT_ZOOM_FACTOR: f64 = 1.5;

#[derive(Debug, Clone, PartialEq)]
pub enum Annotation {
    Stroke {
        points: Vec<Point>,
        color: Color,
        width: f64,
        highlight: bool,
    },
    Shape {
        tool: DrawTool,
        rect: Rect,
        start: Point,
        end: Point,
        color: Color,
        width: f64,
    },
    Text {
        at: Point,
        text: String,
        color: Color,
        font: String,
        size: f64,
    },
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Color {
    pub red: f64,
    pub green: f64,
    pub blue: f64,
    pub alpha: f64,
}

impl Color {
    pub const RED: Self = Self::rgb(1.0, 0.0, 0.0);
    pub const YELLOW: Self = Self::rgba(1.0, 0.90, 0.0, 0.45);
    pub const GREEN: Self = Self::rgb(0.1, 0.8, 0.1);
    pub const BLUE: Self = Self::rgb(0.1, 0.35, 1.0);
    pub const BLACK: Self = Self::rgb(0.0, 0.0, 0.0);
    pub const WHITE: Self = Self::rgb(1.0, 1.0, 1.0);

    pub const fn rgb(red: f64, green: f64, blue: f64) -> Self {
        Self::rgba(red, green, blue, 1.0)
    }

    pub const fn rgba(red: f64, green: f64, blue: f64, alpha: f64) -> Self {
        Self {
            red,
            green,
            blue,
            alpha,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AppState {
    pub mode: Mode,
    pub mode_before_snip: Mode,
    pub tool: DrawTool,
    pub color: Color,
    pub stroke_width: f64,
    pub zoom_factor: f64,
    pub zoom_center: Point,
    pub annotations: Vec<Annotation>,
    pub current_points: Vec<Point>,
    pub drag_start: Option<Point>,
    pub drag_current: Option<Point>,
    pub pending_text: String,
    pub text_anchor: Option<Point>,
    pub status_message: Option<String>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            mode: Mode::Idle,
            mode_before_snip: Mode::Idle,
            tool: DrawTool::Pen,
            color: Color::RED,
            stroke_width: 4.0,
            zoom_factor: DEFAULT_ZOOM_FACTOR,
            zoom_center: Point::new(0, 0),
            annotations: Vec::new(),
            current_points: Vec::new(),
            drag_start: None,
            drag_current: None,
            pending_text: String::new(),
            text_anchor: None,
            status_message: None,
        }
    }
}

impl AppState {
    pub fn clear_interaction(&mut self) {
        self.current_points.clear();
        self.drag_start = None;
        self.drag_current = None;
        self.pending_text.clear();
        self.text_anchor = None;
    }

    pub fn reset_overlay(&mut self) {
        self.mode = Mode::Idle;
        self.mode_before_snip = Mode::Idle;
        self.clear_interaction();
        self.annotations.clear();
        self.status_message = None;
    }
}
