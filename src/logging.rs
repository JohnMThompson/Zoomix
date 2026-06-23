use chrono::Local;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn init() -> anyhow::Result<PathBuf> {
    let dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("zoomix");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("zoomix.log");
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let _ = LOG_PATH.set(path.clone());
    let _ = LOG_FILE.set(Mutex::new(file));
    info("logging initialized");
    Ok(path)
}

pub fn path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}

pub fn info(message: impl AsRef<str>) {
    write("INFO", message.as_ref());
}

pub fn error(message: impl AsRef<str>) {
    write("ERROR", message.as_ref());
}

fn write(level: &str, message: &str) {
    let Some(file) = LOG_FILE.get() else {
        return;
    };
    let Ok(mut file) = file.lock() else {
        return;
    };
    let _ = writeln!(
        file,
        "{} {level} {message}",
        Local::now().format("%Y-%m-%d %H:%M:%S%.3f")
    );
}
