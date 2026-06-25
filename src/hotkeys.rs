use crate::{config::Hotkeys, logging, model::Mode};
use anyhow::anyhow;
use std::sync::mpsc::Sender;
use std::{ffi::CString, ptr, thread};
use x11::{keysym, xlib};

#[derive(Debug, Clone)]
pub struct HotkeySpec {
    pub mode: Mode,
    pub key: String,
}

impl HotkeySpec {
    fn keysym(&self) -> anyhow::Result<u64> {
        key_name_to_keysym(&self.key).ok_or_else(|| anyhow!("unsupported hotkey key {}", self.key))
    }
}

pub fn specs(config: &Hotkeys) -> Vec<HotkeySpec> {
    vec![
        HotkeySpec {
            mode: Mode::Zoom,
            key: config.hotkeys_key(&config.zoom),
        },
        HotkeySpec {
            mode: Mode::LiveZoom,
            key: config.hotkeys_key(&config.live_zoom),
        },
        HotkeySpec {
            mode: Mode::Draw,
            key: config.hotkeys_key(&config.draw),
        },
        HotkeySpec {
            mode: Mode::Snip,
            key: config.hotkeys_key(&config.snip),
        },
    ]
}

trait HotkeyKey {
    fn hotkeys_key(&self, text: &str) -> String;
}

impl HotkeyKey for Hotkeys {
    fn hotkeys_key(&self, text: &str) -> String {
        text.split('+')
            .next_back()
            .unwrap_or(text)
            .trim()
            .to_string()
    }
}

pub fn spawn_listener(config: Hotkeys, sender: Sender<Mode>) {
    thread::spawn(move || {
        logging::info("hotkey listener thread starting");
        if let Err(err) = listen(config, sender) {
            logging::error(format!("hotkeys disabled: {err:#}"));
            eprintln!("zoomix hotkeys disabled: {err:#}");
        }
    });
}

fn listen(config: Hotkeys, sender: Sender<Mode>) -> anyhow::Result<()> {
    let specs = specs(&config);
    unsafe {
        let display = xlib::XOpenDisplay(ptr::null());
        if display.is_null() {
            return Err(anyhow!("could not open X display"));
        }
        logging::info("hotkey listener connected to X display");
        let root = xlib::XDefaultRootWindow(display);
        let modifiers = xlib::ControlMask;

        let mut grabs = Vec::new();
        for spec in &specs {
            let keysym = spec.keysym()?;
            let keycode = xlib::XKeysymToKeycode(display, keysym);
            if keycode == 0 {
                continue;
            }
            for lock_mask in ignored_modifier_combinations() {
                xlib::XGrabKey(
                    display,
                    keycode as i32,
                    modifiers | lock_mask,
                    root,
                    xlib::False,
                    xlib::GrabModeAsync,
                    xlib::GrabModeAsync,
                );
            }
            let message = format!(
                "hotkey registered: Ctrl+{} -> {:?} (keycode {keycode})",
                spec.key, spec.mode
            );
            logging::info(&message);
            eprintln!("zoomix {message}");
            grabs.push((keycode, spec.mode));
        }
        xlib::XSync(display, xlib::False);

        loop {
            let mut event: xlib::XEvent = std::mem::zeroed();
            xlib::XNextEvent(display, &mut event);
            if event.get_type() != xlib::KeyPress {
                if event.get_type() == xlib::KeyRelease {
                    let xkey = event.key;
                    logging::verbose(format!(
                        "x11 keyrelease keycode={} state=0x{:x}",
                        xkey.keycode, xkey.state
                    ));
                }
                continue;
            }
            let xkey = event.key;
            logging::verbose(format!(
                "x11 keypress keycode={} state=0x{:x}",
                xkey.keycode, xkey.state
            ));
            if let Some((_, mode)) = grabs
                .iter()
                .find(|(keycode, _)| *keycode == xkey.keycode as u8)
            {
                logging::info(format!("x11 hotkey matched -> {mode:?}"));
                let _ = sender.send(*mode);
            }
        }
    }
}

fn ignored_modifier_combinations() -> Vec<u32> {
    let ignored = [
        xlib::LockMask,
        xlib::Mod2Mask,
        xlib::Mod3Mask,
        xlib::Mod5Mask,
    ];
    let mut combinations = Vec::with_capacity(1 << ignored.len());
    for bits in 0..(1 << ignored.len()) {
        let mut mask = 0;
        for (idx, modifier) in ignored.iter().enumerate() {
            if bits & (1 << idx) != 0 {
                mask |= modifier;
            }
        }
        combinations.push(mask);
    }
    combinations
}

fn key_name_to_keysym(key: &str) -> Option<u64> {
    match key.to_ascii_lowercase().as_str() {
        "1" => Some(keysym::XK_1.into()),
        "2" => Some(keysym::XK_2.into()),
        "3" => Some(keysym::XK_3.into()),
        "4" => Some(keysym::XK_4.into()),
        "z" => Some(keysym::XK_z.into()),
        "d" => Some(keysym::XK_d.into()),
        "s" => Some(keysym::XK_s.into()),
        "escape" | "esc" => Some(keysym::XK_Escape.into()),
        _ => CString::new(key)
            .ok()
            .map(|name| unsafe { xlib::XStringToKeysym(name.as_ptr()) })
            .filter(|v| *v != 0),
    }
}
