use crate::{
    config::color_for_key,
    geometry::{Point, Rect},
    model::{Annotation, AppState, Color, DrawTool, Mode, DEFAULT_ZOOM_FACTOR},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivationEffect {
    pub capture_background: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PointerRelease {
    None,
    Redraw,
    CaptureSnip(Rect),
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyAction {
    Escape,
    Undo,
    Clear,
    ZoomBy(f64),
    SetTool(DrawTool),
    CycleTool,
    EnterText,
    Submit,
    Backspace,
    InsertText(String),
    SetColor(Color),
}

#[derive(Debug, Clone, PartialEq)]
pub enum KeyOutcome {
    None,
    Redraw,
    HideOverlay,
    CaptureSnip(Rect),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font: String,
    pub size: f64,
}

pub fn activate_mode(
    state: &mut AppState,
    mode: Mode,
    pointer: Point,
    has_background: bool,
) -> ActivationEffect {
    let previous_mode = state.mode;
    state.mode = mode;
    state.clear_interaction();
    state.status_message = None;

    match mode {
        Mode::Zoom | Mode::LiveZoom if previous_mode == Mode::Idle => {
            state.zoom_factor = DEFAULT_ZOOM_FACTOR;
            state.zoom_center = pointer;
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

    ActivationEffect {
        capture_background: matches!(mode, Mode::Zoom | Mode::LiveZoom | Mode::Snip)
            || !has_background,
    }
}

pub fn capture_failed(state: &mut AppState, mode: Mode, error: impl AsRef<str>) {
    state.reset_overlay();
    state.status_message = Some(format!(
        "Could not capture the screen for {}: {}",
        mode_label(mode),
        error.as_ref()
    ));
}

pub fn pointer_press(state: &mut AppState, point: Point) {
    if matches!(state.mode, Mode::Idle | Mode::Zoom | Mode::LiveZoom) {
        return;
    }

    state.drag_start = Some(point);
    state.drag_current = Some(point);
    if is_freehand_tool(state.tool) {
        state.current_points.clear();
        state.current_points.push(point);
    }
}

pub fn pointer_move(state: &mut AppState, point: Point) -> bool {
    if matches!(state.mode, Mode::Idle | Mode::Zoom | Mode::LiveZoom) || state.drag_start.is_none()
    {
        return false;
    }

    state.drag_current = Some(point);
    if is_freehand_tool(state.tool) {
        state.current_points.push(point);
    }
    true
}

pub fn pointer_release(state: &mut AppState, point: Point) -> PointerRelease {
    if matches!(state.mode, Mode::Idle | Mode::Zoom | Mode::LiveZoom) {
        return PointerRelease::None;
    }

    if state.mode == Mode::Snip {
        let rect = Rect::from_points(state.drag_start.unwrap_or(point), point);
        if rect.is_empty() {
            state.drag_start = None;
            state.drag_current = None;
            return PointerRelease::Redraw;
        }

        state.reset_overlay();
        return PointerRelease::CaptureSnip(rect);
    }

    let annotation = match state.tool {
        DrawTool::Pen | DrawTool::Highlight | DrawTool::Eraser
            if state.current_points.len() > 1 =>
        {
            let color = if state.tool == DrawTool::Eraser {
                Color::BLACK
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
                rect: Rect::from_points(start, point),
                start,
                end: point,
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
    PointerRelease::Redraw
}

pub fn key_to_action(mode: Mode, name: &str, ctrl: bool) -> Option<KeyAction> {
    if ctrl {
        return None;
    }

    if mode == Mode::Text {
        return match name {
            "Escape" => Some(KeyAction::Escape),
            "BackSpace" => Some(KeyAction::Backspace),
            "Return" => Some(KeyAction::Submit),
            "space" => Some(KeyAction::InsertText(" ".to_string())),
            name if name.len() == 1 => Some(KeyAction::InsertText(name.to_string())),
            _ => None,
        };
    }

    match name {
        "Escape" => Some(KeyAction::Escape),
        "BackSpace" | "z" | "Z" => Some(KeyAction::Undo),
        "Delete" | "c" | "C" => Some(KeyAction::Clear),
        "plus" | "equal" => Some(KeyAction::ZoomBy(0.25)),
        "minus" => Some(KeyAction::ZoomBy(-0.25)),
        "R" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetColor(Color::RED))
        }
        "p" | "P" | "1" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetTool(DrawTool::Pen))
        }
        "r" | "2" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetTool(DrawTool::Rectangle))
        }
        "a" | "A" | "3" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetTool(DrawTool::Arrow))
        }
        "l" | "L" | "4" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetTool(DrawTool::Line))
        }
        "e" | "E" | "5" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetTool(DrawTool::Ellipse))
        }
        "h" | "H" | "6" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetTool(DrawTool::Highlight))
        }
        "x" | "X" | "7" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::SetTool(DrawTool::Eraser))
        }
        "Tab" | "space" if !matches!(mode, Mode::Zoom | Mode::LiveZoom) => {
            Some(KeyAction::CycleTool)
        }
        "t" | "T" => Some(KeyAction::EnterText),
        "Return" if mode == Mode::Snip => Some(KeyAction::Submit),
        _ => name
            .chars()
            .next()
            .and_then(color_for_key)
            .map(KeyAction::SetColor),
    }
}

pub fn apply_key_action(
    state: &mut AppState,
    action: KeyAction,
    text_style: &TextStyle,
) -> KeyOutcome {
    match action {
        KeyAction::Escape => {
            state.reset_overlay();
            KeyOutcome::HideOverlay
        }
        KeyAction::Undo => {
            state.annotations.pop();
            KeyOutcome::Redraw
        }
        KeyAction::Clear => {
            state.annotations.clear();
            KeyOutcome::Redraw
        }
        KeyAction::ZoomBy(delta) => {
            state.zoom_factor = (state.zoom_factor + delta).clamp(1.0, 8.0);
            KeyOutcome::Redraw
        }
        KeyAction::SetTool(tool) => {
            set_tool(state, tool);
            KeyOutcome::Redraw
        }
        KeyAction::CycleTool => {
            let next = next_tool(state.tool);
            set_tool(state, next);
            KeyOutcome::Redraw
        }
        KeyAction::EnterText => {
            state.mode = Mode::Text;
            KeyOutcome::Redraw
        }
        KeyAction::Submit if state.mode == Mode::Text => {
            submit_text(state, text_style);
            KeyOutcome::Redraw
        }
        KeyAction::Submit if state.mode == Mode::Snip => {
            let rect = Rect::from_points(
                state.drag_start.unwrap_or(Point::new(0, 0)),
                state.drag_current.unwrap_or(Point::new(0, 0)),
            );
            if rect.is_empty() {
                state.drag_start = None;
                state.drag_current = None;
                KeyOutcome::Redraw
            } else {
                state.reset_overlay();
                KeyOutcome::CaptureSnip(rect)
            }
        }
        KeyAction::Submit => KeyOutcome::None,
        KeyAction::Backspace if state.mode == Mode::Text => {
            state.pending_text.pop();
            KeyOutcome::Redraw
        }
        KeyAction::Backspace => KeyOutcome::None,
        KeyAction::InsertText(text) if state.mode == Mode::Text => {
            state.pending_text.push_str(&text);
            KeyOutcome::Redraw
        }
        KeyAction::InsertText(_) => KeyOutcome::None,
        KeyAction::SetColor(color) => {
            state.color = color;
            KeyOutcome::Redraw
        }
    }
}

pub fn set_tool(state: &mut AppState, tool: DrawTool) {
    if matches!(state.mode, Mode::Zoom | Mode::LiveZoom) {
        state.mode = Mode::Draw;
    }
    state.tool = tool;
    state.clear_interaction();
}

pub fn scroll_zoom(state: &mut AppState, direction: ZoomDirection) {
    let delta = match direction {
        ZoomDirection::In => 0.25,
        ZoomDirection::Out => -0.25,
    };
    state.zoom_factor = (state.zoom_factor + delta).clamp(1.0, 8.0);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ZoomDirection {
    In,
    Out,
}

fn submit_text(state: &mut AppState, text_style: &TextStyle) {
    let at = state
        .drag_current
        .or(state.drag_start)
        .unwrap_or(Point::new(80, 80));
    let text = std::mem::take(&mut state.pending_text);
    if !text.is_empty() {
        state.annotations.push(Annotation::Text {
            at,
            text,
            color: state.color,
            font: text_style.font.clone(),
            size: text_style.size,
        });
    }
    state.mode = Mode::Draw;
}

fn is_freehand_tool(tool: DrawTool) -> bool {
    matches!(tool, DrawTool::Pen | DrawTool::Highlight | DrawTool::Eraser)
}

fn next_tool(tool: DrawTool) -> DrawTool {
    match tool {
        DrawTool::Pen => DrawTool::Rectangle,
        DrawTool::Rectangle => DrawTool::Arrow,
        DrawTool::Arrow => DrawTool::Line,
        DrawTool::Line => DrawTool::Ellipse,
        DrawTool::Ellipse => DrawTool::Highlight,
        DrawTool::Highlight => DrawTool::Eraser,
        DrawTool::Eraser => DrawTool::Pen,
    }
}

fn mode_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Idle => "idle",
        Mode::Zoom => "zoom",
        Mode::LiveZoom => "live zoom",
        Mode::Draw => "draw",
        Mode::Text => "text",
        Mode::Snip => "snip",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_style() -> TextStyle {
        TextStyle {
            font: "Sans".to_string(),
            size: 24.0,
        }
    }

    #[test]
    fn activate_draw_from_idle_resets_zoom_and_tool() {
        let mut state = AppState {
            zoom_factor: 3.0,
            zoom_center: Point::new(20, 30),
            tool: DrawTool::Arrow,
            status_message: Some("old status".to_string()),
            ..Default::default()
        };

        let effect = activate_mode(&mut state, Mode::Draw, Point::new(9, 9), false);

        assert_eq!(state.mode, Mode::Draw);
        assert_eq!(state.zoom_factor, 1.0);
        assert_eq!(state.zoom_center, Point::new(0, 0));
        assert_eq!(state.tool, DrawTool::Pen);
        assert_eq!(state.status_message, None);
        assert!(effect.capture_background);
    }

    #[test]
    fn activate_draw_from_zoom_preserves_zoom_context() {
        let mut state = AppState {
            mode: Mode::Zoom,
            zoom_factor: 2.0,
            zoom_center: Point::new(100, 120),
            ..Default::default()
        };

        activate_mode(&mut state, Mode::Draw, Point::new(9, 9), true);

        assert_eq!(state.mode, Mode::Draw);
        assert_eq!(state.zoom_factor, 2.0);
        assert_eq!(state.zoom_center, Point::new(100, 120));
    }

    #[test]
    fn freehand_drag_creates_stroke() {
        let mut state = AppState {
            mode: Mode::Draw,
            tool: DrawTool::Pen,
            ..Default::default()
        };

        pointer_press(&mut state, Point::new(1, 1));
        assert!(pointer_move(&mut state, Point::new(4, 4)));
        assert_eq!(
            pointer_release(&mut state, Point::new(8, 8)),
            PointerRelease::Redraw
        );

        assert_eq!(state.annotations.len(), 1);
        assert!(matches!(
            state.annotations[0],
            Annotation::Stroke { ref points, .. } if points.len() == 2
        ));
    }

    #[test]
    fn snip_release_reports_empty_or_capture_decision() {
        let mut state = AppState {
            mode: Mode::Snip,
            ..Default::default()
        };

        pointer_press(&mut state, Point::new(10, 10));
        assert_eq!(
            pointer_release(&mut state, Point::new(10, 10)),
            PointerRelease::Redraw
        );

        pointer_press(&mut state, Point::new(10, 10));
        assert_eq!(
            pointer_release(&mut state, Point::new(30, 35)),
            PointerRelease::CaptureSnip(Rect::new(10, 10, 20, 25))
        );
        assert_eq!(state.mode, Mode::Idle);
    }

    #[test]
    fn text_actions_edit_and_submit_annotation() {
        let mut state = AppState {
            mode: Mode::Text,
            color: Color::BLUE,
            drag_current: Some(Point::new(12, 14)),
            ..Default::default()
        };

        apply_key_action(
            &mut state,
            KeyAction::InsertText("abc".to_string()),
            &text_style(),
        );
        apply_key_action(&mut state, KeyAction::Backspace, &text_style());
        assert_eq!(state.pending_text, "ab");

        assert_eq!(
            apply_key_action(&mut state, KeyAction::Submit, &text_style()),
            KeyOutcome::Redraw
        );

        assert_eq!(state.mode, Mode::Draw);
        assert_eq!(state.pending_text, "");
        assert!(matches!(
            state.annotations[0],
            Annotation::Text { at, ref text, color, .. }
                if at == Point::new(12, 14) && text == "ab" && color == Color::BLUE
        ));
    }

    #[test]
    fn capture_failure_resets_mode_and_preserves_user_visible_status() {
        let mut state = AppState {
            mode: Mode::Zoom,
            annotations: vec![Annotation::Stroke {
                points: vec![Point::new(1, 1), Point::new(2, 2)],
                color: Color::RED,
                width: 4.0,
                highlight: false,
            }],
            ..Default::default()
        };

        capture_failed(&mut state, Mode::Zoom, "failed to capture root window");

        assert_eq!(state.mode, Mode::Idle);
        assert!(state.annotations.is_empty());
        assert_eq!(
            state.status_message,
            Some(
                "Could not capture the screen for zoom: failed to capture root window".to_string()
            )
        );
    }
}
