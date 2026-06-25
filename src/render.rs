use crate::{
    geometry::{Point, Rect},
    model::{Annotation, AppState, Color, DrawTool, Mode},
};
use cairo::Context;
use gdk_pixbuf::Pixbuf;
use gtk::prelude::GdkContextExt;

pub fn draw_overlay(
    cr: &Context,
    background: Option<&Pixbuf>,
    state: &AppState,
    width: i32,
    height: i32,
) {
    if let Some(background) = background {
        draw_background(cr, background, state, width, height);
    } else {
        cr.set_source_rgba(0.0, 0.0, 0.0, 0.15);
        let _ = cr.paint();
    }

    draw_hud(cr, state);
    draw_status_message(cr, state, width, height);

    for annotation in &state.annotations {
        draw_annotation(cr, annotation);
    }
    draw_pending_text(cr, state);

    if state.current_points.len() > 1 {
        stroke_points(
            cr,
            &state.current_points,
            state.color,
            state.stroke_width,
            state.tool == DrawTool::Highlight,
        );
    }

    if let (Some(start), Some(current)) = (state.drag_start, state.drag_current) {
        let rect = Rect::from_points(start, current);
        match (state.mode, state.tool) {
            (Mode::Snip, _) => draw_selection(cr, rect),
            (_, DrawTool::Arrow) => {
                draw_arrow(cr, current, start, state.color, state.stroke_width);
            }
            (_, DrawTool::Line | DrawTool::Rectangle | DrawTool::Ellipse) => {
                draw_shape(cr, state.tool, rect, state.color, state.stroke_width);
            }
            _ => {}
        }
    }
}

fn draw_background(cr: &Context, background: &Pixbuf, state: &AppState, width: i32, height: i32) {
    let zoom = match state.mode {
        Mode::Zoom | Mode::LiveZoom | Mode::Draw | Mode::Text => state.zoom_factor.max(1.0),
        Mode::Snip | Mode::Idle => 1.0,
    };
    let center = if zoom > 1.0 {
        state.zoom_center
    } else {
        Point::new(width / 2, height / 2)
    };
    let min_x = width as f64 - background.width() as f64 * zoom;
    let min_y = height as f64 - background.height() as f64 * zoom;
    let x = (width as f64 / 2.0 - center.x as f64 * zoom).clamp(min_x.min(0.0), 0.0);
    let y = (height as f64 / 2.0 - center.y as f64 * zoom).clamp(min_y.min(0.0), 0.0);
    let _ = cr.save();
    cr.translate(x, y);
    cr.scale(zoom, zoom);
    cr.set_source_pixbuf(background, 0.0, 0.0);
    let _ = cr.paint();
    let _ = cr.restore();
}

pub fn draw_annotation(cr: &Context, annotation: &Annotation) {
    match annotation {
        Annotation::Stroke {
            points,
            color,
            width,
            highlight,
        } => stroke_points(cr, points, *color, *width, *highlight),
        Annotation::Shape {
            tool,
            rect,
            start,
            end,
            color,
            width,
        } => {
            if *tool == DrawTool::Arrow {
                draw_arrow(cr, *end, *start, *color, *width);
            } else {
                draw_shape(cr, *tool, *rect, *color, *width);
            }
        }
        Annotation::Text {
            at,
            text,
            color,
            font,
            size,
        } => draw_text(cr, *at, text, *color, font, *size),
    }
}

fn stroke_points(cr: &Context, points: &[Point], color: Color, width: f64, highlight: bool) {
    if points.len() < 2 {
        return;
    }
    set_color(
        cr,
        if highlight {
            Color {
                alpha: 0.45,
                ..color
            }
        } else {
            color
        },
    );
    cr.set_line_width(width);
    cr.set_line_cap(cairo::LineCap::Round);
    cr.set_line_join(cairo::LineJoin::Round);
    cr.move_to(points[0].x as f64, points[0].y as f64);
    for point in &points[1..] {
        cr.line_to(point.x as f64, point.y as f64);
    }
    let _ = cr.stroke();
}

fn draw_shape(cr: &Context, tool: DrawTool, rect: Rect, color: Color, width: f64) {
    set_color(cr, color);
    cr.set_line_width(width);
    match tool {
        DrawTool::Line => {
            cr.move_to(rect.x as f64, rect.y as f64);
            cr.line_to((rect.x + rect.width) as f64, (rect.y + rect.height) as f64);
            let _ = cr.stroke();
        }
        DrawTool::Rectangle => {
            cr.rectangle(
                rect.x as f64,
                rect.y as f64,
                rect.width as f64,
                rect.height as f64,
            );
            let _ = cr.stroke();
        }
        DrawTool::Ellipse => {
            let _ = cr.save();
            cr.translate(
                (rect.x + rect.width / 2) as f64,
                (rect.y + rect.height / 2) as f64,
            );
            cr.scale(
                rect.width.max(1) as f64 / 2.0,
                rect.height.max(1) as f64 / 2.0,
            );
            cr.arc(0.0, 0.0, 1.0, 0.0, std::f64::consts::TAU);
            let _ = cr.restore();
            let _ = cr.stroke();
        }
        DrawTool::Arrow => {}
        _ => {}
    }
}

fn draw_arrow(cr: &Context, start: Point, end: Point, color: Color, width: f64) {
    set_color(cr, color);
    cr.set_line_width(width);
    cr.move_to(start.x as f64, start.y as f64);
    cr.line_to(end.x as f64, end.y as f64);
    let angle = ((end.y - start.y) as f64).atan2((end.x - start.x) as f64);
    let head = 18.0;
    for offset in [std::f64::consts::FRAC_PI_6, -std::f64::consts::FRAC_PI_6] {
        cr.move_to(end.x as f64, end.y as f64);
        cr.line_to(
            end.x as f64 - head * (angle + offset).cos(),
            end.y as f64 - head * (angle + offset).sin(),
        );
    }
    let _ = cr.stroke();
}

fn draw_text(cr: &Context, at: Point, text: &str, color: Color, font: &str, size: f64) {
    set_color(cr, color);
    let layout = pangocairo::create_layout(cr);
    let desc = pango::FontDescription::from_string(&format!("{font} {size}"));
    layout.set_font_description(Some(&desc));
    layout.set_text(text);
    cr.move_to(at.x as f64, at.y as f64);
    pangocairo::show_layout(cr, &layout);
}

fn draw_pending_text(cr: &Context, state: &AppState) {
    if state.mode != Mode::Text || state.pending_text.is_empty() {
        return;
    }

    let at = state.text_anchor.unwrap_or(Point::new(80, 80));
    draw_text(cr, at, &state.pending_text, state.color, "Sans", 24.0);
}

fn draw_hud(cr: &Context, state: &AppState) {
    let label = match state.mode {
        Mode::Idle => return,
        Mode::Zoom => format!("Zoom {:.0}%", state.zoom_factor * 100.0),
        Mode::LiveZoom => format!("Live Zoom {:.0}%", state.zoom_factor * 100.0),
        Mode::Draw => format!("Draw: {}", tool_name(state.tool)),
        Mode::Text => "Text".to_string(),
        Mode::Snip => "Snip: drag to capture".to_string(),
    };

    let layout = pangocairo::create_layout(cr);
    let desc = pango::FontDescription::from_string("Sans 14");
    layout.set_font_description(Some(&desc));
    layout.set_text(&label);
    let (text_width, text_height) = layout.pixel_size();
    let padding = 10.0;
    let x = 16.0;
    let y = 16.0;

    cr.set_source_rgba(0.0, 0.0, 0.0, 0.72);
    cr.rectangle(
        x,
        y,
        text_width as f64 + padding * 2.0,
        text_height as f64 + padding * 2.0,
    );
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.95);
    cr.move_to(x + padding, y + padding);
    pangocairo::show_layout(cr, &layout);
}

fn draw_status_message(cr: &Context, state: &AppState, width: i32, height: i32) {
    let Some(message) = &state.status_message else {
        return;
    };

    let layout = pangocairo::create_layout(cr);
    let desc = pango::FontDescription::from_string("Sans Bold 16");
    layout.set_font_description(Some(&desc));
    layout.set_width((width.saturating_sub(160) * pango::SCALE).max(1));
    layout.set_wrap(pango::WrapMode::WordChar);
    layout.set_text(message);
    let (text_width, text_height) = layout.pixel_size();
    let padding = 16.0;
    let box_width = text_width as f64 + padding * 2.0;
    let box_height = text_height as f64 + padding * 2.0;
    let x = ((width as f64 - box_width) / 2.0).max(16.0);
    let y = ((height as f64 - box_height) / 2.0).max(16.0);

    cr.set_source_rgba(0.05, 0.05, 0.05, 0.88);
    cr.rectangle(x, y, box_width, box_height);
    let _ = cr.fill();

    cr.set_source_rgba(1.0, 1.0, 1.0, 0.96);
    cr.move_to(x + padding, y + padding);
    pangocairo::show_layout(cr, &layout);
}

fn tool_name(tool: DrawTool) -> &'static str {
    match tool {
        DrawTool::Pen => "Pen",
        DrawTool::Line => "Line",
        DrawTool::Rectangle => "Rectangle",
        DrawTool::Ellipse => "Ellipse",
        DrawTool::Arrow => "Arrow",
        DrawTool::Highlight => "Highlight",
        DrawTool::Eraser => "Eraser",
    }
}

fn draw_selection(cr: &Context, rect: Rect) {
    cr.set_source_rgba(0.1, 0.35, 1.0, 0.18);
    cr.rectangle(
        rect.x as f64,
        rect.y as f64,
        rect.width as f64,
        rect.height as f64,
    );
    let _ = cr.fill_preserve();
    cr.set_source_rgba(0.1, 0.35, 1.0, 0.95);
    cr.set_line_width(2.0);
    let _ = cr.stroke();
}

fn set_color(cr: &Context, color: Color) {
    cr.set_source_rgba(color.red, color.green, color.blue, color.alpha);
}
