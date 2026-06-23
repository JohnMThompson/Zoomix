use crate::geometry::Rect;
use anyhow::{anyhow, Context};
use chrono::Local;
use gdk::prelude::*;
use gdk_pixbuf::Pixbuf;
use std::path::{Path, PathBuf};

pub fn capture_root() -> anyhow::Result<Pixbuf> {
    let root = gdk::Window::default_root_window();
    let width = root.width();
    let height = root.height();
    root.pixbuf(0, 0, width, height)
        .ok_or_else(|| anyhow!("failed to capture root window"))
}

pub fn crop(pixbuf: &Pixbuf, rect: Rect) -> anyhow::Result<Pixbuf> {
    if rect.is_empty() {
        return Err(anyhow!("empty snip selection"));
    }
    let x = rect.x.clamp(0, pixbuf.width());
    let y = rect.y.clamp(0, pixbuf.height());
    let width = rect.width.min(pixbuf.width() - x);
    let height = rect.height.min(pixbuf.height() - y);
    if width <= 0 || height <= 0 {
        return Err(anyhow!("snip selection is outside the captured screen"));
    }
    Ok(Pixbuf::new_subpixbuf(pixbuf, x, y, width, height))
}

pub fn save_png(pixbuf: &Pixbuf, dir: &Path) -> anyhow::Result<PathBuf> {
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let filename = format!("zoomix-{}.png", Local::now().format("%Y%m%d-%H%M%S"));
    let path = dir.join(filename);
    pixbuf
        .savev(
            path.to_str()
                .ok_or_else(|| anyhow!("screenshot path is not valid UTF-8"))?,
            "png",
            &[],
        )
        .with_context(|| format!("saving {}", path.display()))?;
    Ok(path)
}

pub fn copy_to_clipboard(pixbuf: &Pixbuf) -> anyhow::Result<arboard::Clipboard> {
    let png = pixbuf.save_to_bufferv("png", &[])?;
    let image = image::load_from_memory(&png)?.into_rgba8();
    let (width, height) = image.dimensions();
    let mut clipboard = arboard::Clipboard::new()?;
    clipboard.set_image(arboard::ImageData {
        width: width as usize,
        height: height as usize,
        bytes: std::borrow::Cow::Owned(image.into_raw()),
    })?;
    Ok(clipboard)
}
