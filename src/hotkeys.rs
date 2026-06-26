use crate::{config::Hotkeys, logging, model::Mode};
use anyhow::{anyhow, Context};
use std::sync::mpsc::Sender;
use std::{
    ffi::CString,
    os::raw::c_int,
    ptr,
    sync::{LazyLock, Mutex},
    thread,
};
use x11::{keysym, xlib};

static X_GRAB_ERRORS: LazyLock<Mutex<Vec<XGrabError>>> = LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct XGrabError {
    error_code: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeySpec {
    pub mode: Mode,
    pub key: String,
    pub modifiers: HotkeyModifiers,
    pub display: String,
}

impl HotkeySpec {
    fn keysym(&self) -> anyhow::Result<u64> {
        key_name_to_keysym(&self.key).ok_or_else(|| anyhow!("unsupported hotkey key {}", self.key))
    }

    fn x11_modifiers(&self) -> u32 {
        self.modifiers.x11_mask()
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HotkeyModifiers {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub super_key: bool,
}

impl HotkeyModifiers {
    pub fn from_gdk(modifiers: gdk::ModifierType) -> Self {
        Self {
            ctrl: modifiers.contains(gdk::ModifierType::CONTROL_MASK),
            alt: modifiers.contains(gdk::ModifierType::MOD1_MASK),
            shift: modifiers.contains(gdk::ModifierType::SHIFT_MASK),
            super_key: modifiers.contains(gdk::ModifierType::SUPER_MASK)
                || modifiers.contains(gdk::ModifierType::MOD4_MASK),
        }
    }

    fn x11_mask(self) -> u32 {
        let mut mask = 0;
        if self.ctrl {
            mask |= xlib::ControlMask;
        }
        if self.alt {
            mask |= xlib::Mod1Mask;
        }
        if self.shift {
            mask |= xlib::ShiftMask;
        }
        if self.super_key {
            mask |= xlib::Mod4Mask;
        }
        mask
    }
}

pub fn specs(config: &Hotkeys) -> anyhow::Result<Vec<HotkeySpec>> {
    Ok(vec![
        parse_spec(Mode::Zoom, &config.zoom).context("parsing zoom hotkey")?,
        parse_spec(Mode::LiveZoom, &config.live_zoom).context("parsing live_zoom hotkey")?,
        parse_spec(Mode::Draw, &config.draw).context("parsing draw hotkey")?,
        parse_spec(Mode::Snip, &config.snip).context("parsing snip hotkey")?,
    ])
}

pub fn mode_for_event(
    config: &Hotkeys,
    key_name: &str,
    modifiers: HotkeyModifiers,
) -> Option<Mode> {
    specs(config).ok()?.into_iter().find_map(|spec| {
        (spec.modifiers == modifiers && key_matches(&spec.key, key_name)).then_some(spec.mode)
    })
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
    let specs = specs(&config)?;
    unsafe {
        let display = xlib::XOpenDisplay(ptr::null());
        if display.is_null() {
            return Err(anyhow!("could not open X display"));
        }
        logging::info("hotkey listener connected to X display");
        let root = xlib::XDefaultRootWindow(display);

        let mut grabs = Vec::new();
        for spec in &specs {
            let keysym = spec.keysym()?;
            let keycode = xlib::XKeysymToKeycode(display, keysym);
            if keycode == 0 {
                continue;
            }
            let modifiers = spec.x11_modifiers();
            let mut registered_any = false;
            for lock_mask in ignored_modifier_combinations() {
                let effective_modifiers = modifiers | lock_mask;
                if grab_key_checked(display, root, keycode as i32, effective_modifiers) {
                    registered_any = true;
                } else {
                    let message = format!(
                        "hotkey grab unavailable for {} with X11 modifiers 0x{effective_modifiers:x}; another client may already own this shortcut",
                        spec.display
                    );
                    logging::error(&message);
                    eprintln!("zoomix {message}");
                }
            }
            if !registered_any {
                logging::error(format!(
                    "hotkey not registered: {} -> {:?} (keycode {keycode})",
                    spec.display, spec.mode
                ));
                continue;
            }
            let message = format!(
                "hotkey registered: {} -> {:?} (keycode {keycode})",
                spec.display, spec.mode
            );
            logging::info(&message);
            eprintln!("zoomix {message}");
            grabs.push((keycode, modifiers, spec.mode));
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
            let event_modifiers = xkey.state & !ignored_modifier_mask();
            if let Some((_, _, mode)) = grabs.iter().find(|(keycode, modifiers, _)| {
                *keycode == xkey.keycode as u8 && *modifiers == event_modifiers
            }) {
                logging::info(format!("x11 hotkey matched -> {mode:?}"));
                let _ = sender.send(*mode);
            }
        }
    }
}

unsafe fn grab_key_checked(
    display: *mut xlib::Display,
    root: xlib::Window,
    keycode: c_int,
    modifiers: u32,
) -> bool {
    {
        let mut errors = X_GRAB_ERRORS.lock().expect("x11 grab error mutex poisoned");
        errors.clear();
    }

    let previous_handler = xlib::XSetErrorHandler(Some(record_x_grab_error));
    xlib::XGrabKey(
        display,
        keycode,
        modifiers,
        root,
        xlib::False,
        xlib::GrabModeAsync,
        xlib::GrabModeAsync,
    );
    xlib::XSync(display, xlib::False);
    xlib::XSetErrorHandler(previous_handler);

    let errors = X_GRAB_ERRORS.lock().expect("x11 grab error mutex poisoned");
    !errors
        .iter()
        .any(|error| error.error_code == xlib::BadAccess)
}

unsafe extern "C" fn record_x_grab_error(
    _display: *mut xlib::Display,
    event: *mut xlib::XErrorEvent,
) -> c_int {
    if !event.is_null() {
        let event = *event;
        if let Ok(mut errors) = X_GRAB_ERRORS.lock() {
            errors.push(XGrabError {
                error_code: event.error_code,
            });
        }
    }
    0
}

fn parse_spec(mode: Mode, text: &str) -> anyhow::Result<HotkeySpec> {
    let parts = text
        .split('+')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>();
    let Some((key, modifiers)) = parts.split_last() else {
        return Err(anyhow!("empty hotkey"));
    };

    let mut parsed = HotkeyModifiers::default();
    for modifier in modifiers {
        match modifier.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => parsed.ctrl = true,
            "alt" | "option" => parsed.alt = true,
            "shift" => parsed.shift = true,
            "super" | "meta" | "win" | "windows" | "mod4" => parsed.super_key = true,
            unknown => return Err(anyhow!("unknown hotkey modifier {unknown}")),
        }
    }

    Ok(HotkeySpec {
        mode,
        key: (*key).to_string(),
        modifiers: parsed,
        display: text.to_string(),
    })
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

fn ignored_modifier_mask() -> u32 {
    xlib::LockMask | xlib::Mod2Mask | xlib::Mod3Mask | xlib::Mod5Mask
}

fn key_matches(configured: &str, event: &str) -> bool {
    let configured = configured.to_ascii_lowercase();
    let event = event.to_ascii_lowercase();
    configured == event || shifted_digit_alias(&configured) == Some(event.as_str())
}

fn shifted_digit_alias(key: &str) -> Option<&'static str> {
    match key {
        "1" => Some("exclam"),
        "2" => Some("at"),
        "3" => Some("numbersign"),
        "4" => Some("dollar"),
        "5" => Some("percent"),
        "6" => Some("asciicircum"),
        "7" => Some("ampersand"),
        "8" => Some("asterisk"),
        "9" => Some("parenleft"),
        "0" => Some("parenright"),
        _ => None,
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_default_hotkeys_with_control_modifier() {
        let specs = specs(&Hotkeys::default()).expect("default hotkeys parse");

        assert_eq!(
            specs[0],
            HotkeySpec {
                mode: Mode::Zoom,
                key: "1".to_string(),
                modifiers: HotkeyModifiers {
                    ctrl: true,
                    ..Default::default()
                },
                display: "Ctrl+1".to_string(),
            }
        );
        assert_eq!(specs[1].mode, Mode::LiveZoom);
        assert_eq!(specs[2].mode, Mode::Draw);
        assert_eq!(specs[3].mode, Mode::Snip);
    }

    #[test]
    fn parses_non_default_modifier_sets() {
        let config = Hotkeys {
            zoom: "Alt+Shift+Z".to_string(),
            ..Default::default()
        };

        let specs = specs(&config).expect("custom hotkeys parse");

        assert_eq!(specs[0].key, "Z");
        assert_eq!(
            specs[0].modifiers,
            HotkeyModifiers {
                alt: true,
                shift: true,
                ..Default::default()
            }
        );
    }

    #[test]
    fn mode_lookup_uses_configured_hotkeys() {
        let config = Hotkeys {
            draw: "Alt+D".to_string(),
            ..Default::default()
        };

        assert_eq!(
            mode_for_event(
                &config,
                "D",
                HotkeyModifiers {
                    alt: true,
                    ..Default::default()
                },
            ),
            Some(Mode::Draw)
        );
        assert_eq!(
            mode_for_event(
                &config,
                "2",
                HotkeyModifiers {
                    ctrl: true,
                    ..Default::default()
                },
            ),
            None
        );
    }

    #[test]
    fn shifted_digit_key_names_match_configured_number_keys() {
        let config = Hotkeys {
            zoom: "Ctrl+Shift+1".to_string(),
            ..Default::default()
        };

        assert_eq!(
            mode_for_event(
                &config,
                "exclam",
                HotkeyModifiers {
                    ctrl: true,
                    shift: true,
                    ..Default::default()
                },
            ),
            Some(Mode::Zoom)
        );
    }
}
