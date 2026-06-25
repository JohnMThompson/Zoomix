use chrono::Local;
use std::{
    fs::{File, OpenOptions},
    io::Write,
    path::PathBuf,
    sync::{Mutex, OnceLock},
};

static LOG_FILE: OnceLock<Mutex<File>> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static VERBOSE: OnceLock<bool> = OnceLock::new();

pub fn init() -> anyhow::Result<PathBuf> {
    let verbose = verbose_from_env();
    let _ = VERBOSE.set(verbose);

    let dir = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("zoomix");
    std::fs::create_dir_all(&dir)?;
    let path = dir.join("zoomix.log");
    let file = OpenOptions::new().create(true).append(true).open(&path)?;
    let _ = LOG_PATH.set(path.clone());
    let _ = LOG_FILE.set(Mutex::new(file));
    info("logging initialized");
    if verbose {
        info("verbose logging enabled");
    }
    Ok(path)
}

pub fn path() -> Option<&'static PathBuf> {
    LOG_PATH.get()
}

pub fn info(message: impl AsRef<str>) {
    write("INFO", message.as_ref());
}

pub fn verbose(message: impl AsRef<str>) {
    if is_verbose() {
        write("DEBUG", message.as_ref());
    }
}

pub fn error(message: impl AsRef<str>) {
    write("ERROR", message.as_ref());
}

fn is_verbose() -> bool {
    *VERBOSE.get_or_init(verbose_from_env)
}

fn verbose_from_env() -> bool {
    std::env::var("ZOOMIX_VERBOSE_LOG")
        .map(|value| {
            matches!(
                value.to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
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
