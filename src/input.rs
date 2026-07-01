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
    HideOverlay { clear_background: bool },
    CaptureSnip(Rect),
}

#[derive(Debug, Clone, PartialEq)]
pub struct TextStyle {
    pub font: String,
    pub size: f64,
}

pub fn mode_is_active(current: Mode, requested: Mode) -> bool {
    current == requested || (requested == Mode::Draw && current == Mode::Text)
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
        Mode::Snip if previous_mode == Mode::Idle => {
            state.zoom_factor = 1.0;
            state.zoom_center = Point::new(0, 0);
            state.tool = DrawTool::Rectangle;
        }
        Mode::Snip => {
            state.tool = DrawTool::Rectangle;
        }
        _ => {}
    }

    ActivationEffect {
        capture_background: matches!(mode, Mode::Zoom | Mode::LiveZoom) || !has_background,
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

    if state.mode == Mode::Text {
        state.text_anchor = Some(point);
        state.drag_start = None;
        state.drag_current = None;
        state.current_points.clear();
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
    if matches!(
        state.mode,
        Mode::Idle | Mode::Zoom | Mode::LiveZoom | Mode::Text
    ) || state.drag_start.is_none()
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
    if matches!(
        state.mode,
        Mode::Idle | Mode::Zoom | Mode::LiveZoom | Mode::Text
    ) {
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
        DrawTool::Eraser if state.current_points.len() > 1 => {
            let points = std::mem::take(&mut state.current_points);
            erase_intersecting_annotations(
                &mut state.annotations,
                &points,
                state.stroke_width * 3.0,
            );
            None
        }
        DrawTool::Pen | DrawTool::Highlight if state.current_points.len() > 1 => {
            Some(Annotation::Stroke {
                points: std::mem::take(&mut state.current_points),
                color: state.color,
                width: state.stroke_width,
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
    state.current_points.clear();
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
            KeyOutcome::HideOverlay {
                clear_background: true,
            }
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
        .text_anchor
        .or(state.drag_current)
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
    state.text_anchor = None;
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

fn erase_intersecting_annotations(
    annotations: &mut Vec<Annotation>,
    eraser_points: &[Point],
    eraser_width: f64,
) {
    let radius = eraser_width / 2.0;
    annotations.retain(|annotation| !annotation_intersects_path(annotation, eraser_points, radius));
}

fn annotation_intersects_path(
    annotation: &Annotation,
    eraser_points: &[Point],
    radius: f64,
) -> bool {
    match annotation {
        Annotation::Stroke { points, width, .. } => {
            paths_intersect(eraser_points, points, radius + width / 2.0)
        }
        Annotation::Shape {
            tool,
            rect,
            start,
            end,
            width,
            ..
        } => match tool {
            DrawTool::Line | DrawTool::Arrow => {
                path_intersects_segment(eraser_points, *start, *end, radius + width / 2.0)
            }
            DrawTool::Rectangle | DrawTool::Ellipse => {
                path_intersects_rect(eraser_points, *rect, radius + width / 2.0)
            }
            _ => false,
        },
        Annotation::Text { at, .. } => eraser_points
            .iter()
            .any(|point| point_distance_sq(*point, *at) <= radius * radius),
    }
}

fn paths_intersect(a: &[Point], b: &[Point], threshold: f64) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    if a.len() == 1 || b.len() == 1 {
        return a.iter().any(|left| {
            b.iter()
                .any(|right| point_distance_sq(*left, *right) <= threshold * threshold)
        });
    }

    a.windows(2).any(|left| {
        b.windows(2)
            .any(|right| segments_within(left[0], left[1], right[0], right[1], threshold))
    })
}

fn path_intersects_segment(path: &[Point], start: Point, end: Point, threshold: f64) -> bool {
    if path.is_empty() {
        return false;
    }
    if path.len() == 1 {
        return point_to_segment_distance_sq(path[0], start, end) <= threshold * threshold;
    }
    path.windows(2)
        .any(|segment| segments_within(segment[0], segment[1], start, end, threshold))
}

fn path_intersects_rect(path: &[Point], rect: Rect, threshold: f64) -> bool {
    if path
        .iter()
        .any(|point| point_in_expanded_rect(*point, rect, threshold))
    {
        return true;
    }

    let top_left = Point::new(rect.x, rect.y);
    let top_right = Point::new(rect.x + rect.width, rect.y);
    let bottom_left = Point::new(rect.x, rect.y + rect.height);
    let bottom_right = Point::new(rect.x + rect.width, rect.y + rect.height);
    [
        (top_left, top_right),
        (top_right, bottom_right),
        (bottom_right, bottom_left),
        (bottom_left, top_left),
    ]
    .into_iter()
    .any(|(start, end)| path_intersects_segment(path, start, end, threshold))
}

fn point_in_expanded_rect(point: Point, rect: Rect, padding: f64) -> bool {
    let padding = padding.ceil() as i32;
    point.x >= rect.x - padding
        && point.x <= rect.x + rect.width + padding
        && point.y >= rect.y - padding
        && point.y <= rect.y + rect.height + padding
}

fn segments_within(a: Point, b: Point, c: Point, d: Point, threshold: f64) -> bool {
    segments_intersect(a, b, c, d)
        || point_to_segment_distance_sq(a, c, d) <= threshold * threshold
        || point_to_segment_distance_sq(b, c, d) <= threshold * threshold
        || point_to_segment_distance_sq(c, a, b) <= threshold * threshold
        || point_to_segment_distance_sq(d, a, b) <= threshold * threshold
}

fn segments_intersect(a: Point, b: Point, c: Point, d: Point) -> bool {
    let o1 = orientation(a, b, c);
    let o2 = orientation(a, b, d);
    let o3 = orientation(c, d, a);
    let o4 = orientation(c, d, b);

    if o1 == 0 && point_on_segment(c, a, b)
        || o2 == 0 && point_on_segment(d, a, b)
        || o3 == 0 && point_on_segment(a, c, d)
        || o4 == 0 && point_on_segment(b, c, d)
    {
        return true;
    }

    (o1 > 0) != (o2 > 0) && (o3 > 0) != (o4 > 0)
}

fn orientation(a: Point, b: Point, c: Point) -> i64 {
    let value = (b.y - a.y) as i64 * (c.x - b.x) as i64 - (b.x - a.x) as i64 * (c.y - b.y) as i64;
    value.signum()
}

fn point_on_segment(point: Point, start: Point, end: Point) -> bool {
    point.x >= start.x.min(end.x)
        && point.x <= start.x.max(end.x)
        && point.y >= start.y.min(end.y)
        && point.y <= start.y.max(end.y)
}

fn point_to_segment_distance_sq(point: Point, start: Point, end: Point) -> f64 {
    let px = point.x as f64;
    let py = point.y as f64;
    let sx = start.x as f64;
    let sy = start.y as f64;
    let ex = end.x as f64;
    let ey = end.y as f64;
    let dx = ex - sx;
    let dy = ey - sy;
    let len_sq = dx * dx + dy * dy;
    if len_sq == 0.0 {
        return point_distance_sq(point, start);
    }
    let t = (((px - sx) * dx + (py - sy) * dy) / len_sq).clamp(0.0, 1.0);
    let x = sx + t * dx;
    let y = sy + t * dy;
    let x_diff = px - x;
    let y_diff = py - y;
    x_diff * x_diff + y_diff * y_diff
}

fn point_distance_sq(a: Point, b: Point) -> f64 {
    let x = (a.x - b.x) as f64;
    let y = (a.y - b.y) as f64;
    x * x + y * y
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
    fn activate_snip_from_draw_reuses_background_and_annotations() {
        let annotation = Annotation::Shape {
            tool: DrawTool::Rectangle,
            rect: Rect::new(10, 20, 30, 40),
            start: Point::new(10, 20),
            end: Point::new(40, 60),
            color: Color::RED,
            width: 4.0,
        };
        let mut state = AppState {
            mode: Mode::Draw,
            zoom_factor: 2.5,
            zoom_center: Point::new(320, 240),
            annotations: vec![annotation.clone()],
            ..Default::default()
        };

        let effect = activate_mode(&mut state, Mode::Snip, Point::new(9, 9), true);

        assert!(!effect.capture_background);
        assert_eq!(state.mode, Mode::Snip);
        assert_eq!(state.zoom_factor, 2.5);
        assert_eq!(state.zoom_center, Point::new(320, 240));
        assert_eq!(state.annotations, vec![annotation]);
    }

    #[test]
    fn activate_snip_from_idle_uses_unzoomed_view() {
        let mut state = AppState {
            zoom_factor: 3.0,
            zoom_center: Point::new(320, 240),
            ..Default::default()
        };

        let effect = activate_mode(&mut state, Mode::Snip, Point::new(9, 9), false);

        assert!(effect.capture_background);
        assert_eq!(state.zoom_factor, 1.0);
        assert_eq!(state.zoom_center, Point::new(0, 0));
    }

    #[test]
    fn mode_hotkeys_toggle_the_active_mode_family() {
        assert!(mode_is_active(Mode::Zoom, Mode::Zoom));
        assert!(mode_is_active(Mode::LiveZoom, Mode::LiveZoom));
        assert!(mode_is_active(Mode::Draw, Mode::Draw));
        assert!(mode_is_active(Mode::Text, Mode::Draw));
        assert!(mode_is_active(Mode::Snip, Mode::Snip));
        assert!(!mode_is_active(Mode::Draw, Mode::Zoom));
        assert!(!mode_is_active(Mode::Idle, Mode::Snip));
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
            text_anchor: Some(Point::new(12, 14)),
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
    fn text_mode_prioritizes_text_keys_over_shortcuts() {
        assert_eq!(
            key_to_action(Mode::Text, "r", false),
            Some(KeyAction::InsertText("r".to_string()))
        );
        assert_eq!(
            key_to_action(Mode::Text, "g", false),
            Some(KeyAction::InsertText("g".to_string()))
        );
        assert_eq!(
            key_to_action(Mode::Text, "BackSpace", false),
            Some(KeyAction::Backspace)
        );
        assert_eq!(
            key_to_action(Mode::Draw, "g", false),
            Some(KeyAction::SetColor(Color::GREEN))
        );
    }

    #[test]
    fn text_mode_click_sets_anchor_without_drawing() {
        let mut state = AppState {
            mode: Mode::Text,
            tool: DrawTool::Pen,
            ..Default::default()
        };

        pointer_press(&mut state, Point::new(40, 50));
        assert!(!pointer_move(&mut state, Point::new(80, 90)));
        assert_eq!(
            pointer_release(&mut state, Point::new(80, 90)),
            PointerRelease::None
        );

        assert_eq!(state.text_anchor, Some(Point::new(40, 50)));
        assert!(state.current_points.is_empty());
        assert!(state.annotations.is_empty());
    }

    #[test]
    fn eraser_removes_intersected_strokes_without_adding_black_stroke() {
        let mut state = AppState {
            mode: Mode::Draw,
            tool: DrawTool::Eraser,
            annotations: vec![
                Annotation::Stroke {
                    points: vec![Point::new(10, 10), Point::new(100, 10)],
                    color: Color::RED,
                    width: 4.0,
                    highlight: false,
                },
                Annotation::Stroke {
                    points: vec![Point::new(10, 100), Point::new(100, 100)],
                    color: Color::BLUE,
                    width: 4.0,
                    highlight: false,
                },
            ],
            ..Default::default()
        };

        pointer_press(&mut state, Point::new(50, 0));
        pointer_move(&mut state, Point::new(50, 20));
        assert_eq!(
            pointer_release(&mut state, Point::new(50, 20)),
            PointerRelease::Redraw
        );

        assert_eq!(state.annotations.len(), 1);
        assert!(matches!(
            state.annotations[0],
            Annotation::Stroke { color, .. } if color == Color::BLUE
        ));
    }

    #[test]
    fn eraser_removes_intersected_shapes() {
        let mut state = AppState {
            mode: Mode::Draw,
            tool: DrawTool::Eraser,
            annotations: vec![Annotation::Shape {
                tool: DrawTool::Rectangle,
                rect: Rect::new(20, 20, 40, 30),
                start: Point::new(20, 20),
                end: Point::new(60, 50),
                color: Color::GREEN,
                width: 4.0,
            }],
            ..Default::default()
        };

        pointer_press(&mut state, Point::new(40, 0));
        pointer_move(&mut state, Point::new(40, 100));
        pointer_release(&mut state, Point::new(40, 100));

        assert!(state.annotations.is_empty());
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

    #[test]
    fn escape_requests_overlay_hide_and_background_clear() {
        let mut state = AppState {
            mode: Mode::Zoom,
            ..Default::default()
        };

        assert_eq!(
            apply_key_action(&mut state, KeyAction::Escape, &text_style()),
            KeyOutcome::HideOverlay {
                clear_background: true
            }
        );
        assert_eq!(state.mode, Mode::Idle);
    }
}
