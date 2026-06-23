use crate::model::Color;
use anyhow::Context;
use serde::{Deserialize, Serialize};
use std::{fs, path::PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub hotkeys: Hotkeys,
    pub drawing: Drawing,
    pub screenshots: Screenshots,
    pub start_hidden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
    pub zoom: String,
    pub live_zoom: String,
    pub draw: String,
    pub snip: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Drawing {
    pub stroke_width: f64,
    pub font: String,
    pub font_size: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Screenshots {
    pub directory: PathBuf,
    pub copy_to_clipboard: bool,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            hotkeys: Hotkeys::default(),
            drawing: Drawing::default(),
            screenshots: Screenshots::default(),
            start_hidden: true,
        }
    }
}

impl Default for Hotkeys {
    fn default() -> Self {
        Self {
            zoom: "Ctrl+1".into(),
            live_zoom: "Ctrl+4".into(),
            draw: "Ctrl+2".into(),
            snip: "Ctrl+3".into(),
        }
    }
}

impl Default for Drawing {
    fn default() -> Self {
        Self {
            stroke_width: 4.0,
            font: "Sans".into(),
            font_size: 24.0,
        }
    }
}

impl Default for Screenshots {
    fn default() -> Self {
        Self {
            directory: dirs::picture_dir()
                .unwrap_or_else(|| dirs::home_dir().unwrap_or_else(|| PathBuf::from(".")))
                .join("Zoomix"),
            copy_to_clipboard: true,
        }
    }
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let path = Self::path()?;
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw =
            fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        let mut config: Self =
            toml::from_str(&raw).with_context(|| format!("parsing {}", path.display()))?;
        config.expand_paths();
        Ok(config)
    }

    pub fn path() -> anyhow::Result<PathBuf> {
        Ok(dirs::config_dir()
            .context("no XDG config directory found")?
            .join("zoomix")
            .join("config.toml"))
    }

    pub fn ensure_parent_dirs(&self) -> anyhow::Result<()> {
        if let Some(parent) = Self::path()?.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::create_dir_all(&self.screenshots.directory)?;
        Ok(())
    }

    fn expand_paths(&mut self) {
        let text = self.screenshots.directory.to_string_lossy().into_owned();
        if text == "~" {
            if let Some(home) = dirs::home_dir() {
                self.screenshots.directory = home;
            }
        } else if let Some(rest) = text.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                self.screenshots.directory = home.join(rest);
            }
        }
    }
}

pub fn color_for_key(ch: char) -> Option<Color> {
    match ch.to_ascii_lowercase() {
        'r' => Some(Color::RED),
        'g' => Some(Color::GREEN),
        'b' => Some(Color::BLUE),
        'y' => Some(Color::YELLOW),
        'k' => Some(Color::BLACK),
        'w' => Some(Color::WHITE),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_hotkeys_match_v1_plan() {
        let config = Config::default();
        assert_eq!(config.hotkeys.zoom, "Ctrl+1");
        assert_eq!(config.hotkeys.live_zoom, "Ctrl+4");
        assert_eq!(config.hotkeys.draw, "Ctrl+2");
        assert_eq!(config.hotkeys.snip, "Ctrl+3");
    }

    #[test]
    fn expands_home_relative_screenshot_dir() {
        let mut config = Config::default();
        config.screenshots.directory = PathBuf::from("~/Pictures/Zoomix");
        config.expand_paths();
        assert!(!config
            .screenshots
            .directory
            .to_string_lossy()
            .starts_with('~'));
    }
}
