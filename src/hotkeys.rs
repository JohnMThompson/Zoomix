use crate::{config::Hotkeys, logging, model::Mode};
use anyhow::{anyhow, Context};
use std::sync::mpsc::Sender;
use std::{
    collections::HashSet,
    ffi::CString,
    os::raw::c_int,
    ptr,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, LazyLock, Mutex,
    },
    thread,
    time::Duration,
};
use x11::{keysym, xinput2, xlib};

static X_GRAB_ERRORS: LazyLock<Mutex<Vec<XGrabError>>> = LazyLock::new(|| Mutex::new(Vec::new()));

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct XGrabError {
    error_code: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GlobalAction {
    Activate(Mode),
    LiveZoomIn,
    LiveZoomOut,
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

pub fn spawn_listener(
    config: Hotkeys,
    sender: Sender<GlobalAction>,
    live_zoom_active: Arc<AtomicBool>,
) {
    thread::spawn(move || {
        logging::info("hotkey listener thread starting");
        if let Err(err) = listen(config, sender, live_zoom_active) {
            logging::error(format!("hotkeys disabled: {err:#}"));
            eprintln!("zoomix hotkeys disabled: {err:#}");
        }
    });
}

fn listen(
    config: Hotkeys,
    sender: Sender<GlobalAction>,
    live_zoom_active: Arc<AtomicBool>,
) -> anyhow::Result<()> {
    let specs = specs(&config)?;
    unsafe {
        let display = xlib::XOpenDisplay(ptr::null());
        if display.is_null() {
            return Err(anyhow!("could not open X display"));
        }
        logging::info("hotkey listener connected to X display");
        let root = xlib::XDefaultRootWindow(display);

        let xi2_opcode = xinput2_opcode(display);
        let mut grabs = Vec::new();
        for spec in &specs {
            let keysym = spec.keysym()?;
            let keycode = xlib::XKeysymToKeycode(display, keysym);
            if keycode == 0 {
                continue;
            }
            let modifiers = spec.x11_modifiers();
            let core_registered =
                grab_core_key(display, root, keycode as i32, modifiers, &spec.display);
            let xi2_registered = xi2_opcode.is_none()
                || grab_xinput2_key(display, root, keycode as i32, modifiers, &spec.display);
            let registered_any = core_registered && xi2_registered;
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

        let mut pressed_keys = HashSet::new();
        let mut wheel_grabbed = false;
        let mut wheel_uses_xinput2 = false;
        loop {
            let live_zoom = live_zoom_active.load(Ordering::Acquire);
            if live_zoom != wheel_grabbed {
                if live_zoom {
                    wheel_uses_xinput2 =
                        set_live_zoom_wheel_grabs(display, root, xi2_opcode.is_some(), true);
                } else {
                    set_live_zoom_wheel_grabs(display, root, wheel_uses_xinput2, false);
                    wheel_uses_xinput2 = false;
                }
                wheel_grabbed = live_zoom;
            }

            if xlib::XPending(display) == 0 {
                thread::sleep(Duration::from_millis(10));
                continue;
            }

            let mut event: xlib::XEvent = std::mem::zeroed();
            xlib::XNextEvent(display, &mut event);
            if event.get_type() == xlib::GenericEvent {
                if let Some(opcode) = xi2_opcode {
                    handle_xinput2_event(
                        display,
                        &event,
                        opcode,
                        &grabs,
                        &sender,
                        &mut pressed_keys,
                        wheel_grabbed && wheel_uses_xinput2,
                    );
                }
                continue;
            }
            // The core grab is also installed to suppress legacy events, but XI2 is
            // the single activation source so one physical keypress cannot fire twice.
            if xi2_opcode.is_some() && matches!(event.get_type(), xlib::KeyPress | xlib::KeyRelease)
            {
                continue;
            }
            if wheel_uses_xinput2
                && matches!(event.get_type(), xlib::ButtonPress | xlib::ButtonRelease)
            {
                continue;
            }
            if event.get_type() == xlib::ButtonPress && wheel_grabbed {
                send_wheel_action(event.button.button, &sender);
                continue;
            }
            if event.get_type() != xlib::KeyPress {
                if event.get_type() == xlib::KeyRelease {
                    let xkey = event.key;
                    pressed_keys.remove(&(xkey.keycode as u8));
                    logging::verbose(format!(
                        "x11 keyrelease keycode={} state=0x{:x}",
                        xkey.keycode, xkey.state
                    ));
                }
                continue;
            }
            let xkey = event.key;
            if !pressed_keys.insert(xkey.keycode as u8) {
                continue;
            }
            logging::verbose(format!(
                "x11 keypress keycode={} state=0x{:x}",
                xkey.keycode, xkey.state
            ));
            let event_modifiers = xkey.state & !ignored_modifier_mask();
            if let Some((_, _, mode)) = grabs.iter().find(|(keycode, modifiers, _)| {
                *keycode == xkey.keycode as u8 && *modifiers == event_modifiers
            }) {
                logging::info(format!("x11 hotkey matched -> {mode:?}"));
                let _ = sender.send(GlobalAction::Activate(*mode));
            }
        }
    }
}

unsafe fn xinput2_opcode(display: *mut xlib::Display) -> Option<c_int> {
    let extension = CString::new("XInputExtension").expect("static string contains no NUL");
    let mut opcode = 0;
    let mut event = 0;
    let mut error = 0;
    if xlib::XQueryExtension(
        display,
        extension.as_ptr(),
        &mut opcode,
        &mut event,
        &mut error,
    ) == xlib::False
    {
        logging::info("XInput2 unavailable; using core X11 hotkey grabs");
        return None;
    }

    let mut major = 2;
    let mut minor = 0;
    if xinput2::XIQueryVersion(display, &mut major, &mut minor) != xlib::Success as c_int {
        logging::info("XInput2 version negotiation failed; using core X11 hotkey grabs");
        return None;
    }
    logging::info(format!("hotkey listener using XInput {major}.{minor}"));
    Some(opcode)
}

unsafe fn set_live_zoom_wheel_grabs(
    display: *mut xlib::Display,
    root: xlib::Window,
    use_xinput2: bool,
    enabled: bool,
) -> bool {
    let modifiers = xlib::ControlMask | xlib::ShiftMask;
    let combinations = ignored_modifier_combinations()
        .into_iter()
        .map(|lock_mask| modifiers | lock_mask)
        .collect::<Vec<_>>();

    let mut xinput2_registered = use_xinput2;
    for button in [xlib::Button4, xlib::Button5] {
        for effective_modifiers in &combinations {
            if enabled {
                xlib::XGrabButton(
                    display,
                    button,
                    *effective_modifiers,
                    root,
                    xlib::False,
                    (xlib::ButtonPressMask | xlib::ButtonReleaseMask) as u32,
                    xlib::GrabModeAsync,
                    xlib::GrabModeAsync,
                    0,
                    0,
                );
            } else {
                xlib::XUngrabButton(display, button, *effective_modifiers, root);
            }
        }

        if use_xinput2 {
            xinput2_registered &=
                set_xinput2_wheel_grab(display, root, button as c_int, &combinations, enabled);
        }
    }
    if enabled && use_xinput2 && !xinput2_registered {
        for button in [xlib::Button4, xlib::Button5] {
            set_xinput2_wheel_grab(display, root, button as c_int, &combinations, false);
        }
        logging::error("XInput2 live zoom wheel grabs unavailable; using core X11");
    }
    xlib::XSync(display, xlib::False);
    logging::info(format!(
        "live zoom wheel grabs {}",
        if enabled { "enabled" } else { "disabled" }
    ));
    enabled && xinput2_registered
}

unsafe fn set_xinput2_wheel_grab(
    display: *mut xlib::Display,
    root: xlib::Window,
    button: c_int,
    combinations: &[u32],
    enabled: bool,
) -> bool {
    let mut grab_modifiers = combinations
        .iter()
        .map(|modifiers| xinput2::XIGrabModifiers {
            modifiers: *modifiers as c_int,
            status: xinput2::XIGrabSuccess,
        })
        .collect::<Vec<_>>();

    if enabled {
        let mut event_bits = [0_u8; 1];
        xinput2::XISetMask(&mut event_bits, xinput2::XI_ButtonPress);
        xinput2::XISetMask(&mut event_bits, xinput2::XI_ButtonRelease);
        let mut event_mask = xinput2::XIEventMask {
            deviceid: xinput2::XIAllMasterDevices,
            mask_len: event_bits.len() as c_int,
            mask: event_bits.as_mut_ptr(),
        };
        let status = xinput2::XIGrabButton(
            display,
            xinput2::XIAllMasterDevices,
            button,
            root,
            0,
            xinput2::XIGrabModeAsync,
            xinput2::XIGrabModeAsync,
            xlib::False,
            &mut event_mask,
            grab_modifiers.len() as c_int,
            grab_modifiers.as_mut_ptr(),
        );
        status == xlib::Success as c_int
            && grab_modifiers
                .iter()
                .all(|modifier| modifier.status == xinput2::XIGrabSuccess)
    } else {
        xinput2::XIUngrabButton(
            display,
            xinput2::XIAllMasterDevices,
            button,
            root,
            grab_modifiers.len() as c_int,
            grab_modifiers.as_mut_ptr(),
        );
        true
    }
}

unsafe fn grab_xinput2_key(
    display: *mut xlib::Display,
    root: xlib::Window,
    keycode: c_int,
    modifiers: u32,
    display_name: &str,
) -> bool {
    let mut event_bits = [0_u8; 1];
    xinput2::XISetMask(&mut event_bits, xinput2::XI_KeyPress);
    xinput2::XISetMask(&mut event_bits, xinput2::XI_KeyRelease);
    let mut event_mask = xinput2::XIEventMask {
        deviceid: xinput2::XIAllMasterDevices,
        mask_len: event_bits.len() as c_int,
        mask: event_bits.as_mut_ptr(),
    };
    let mut grab_modifiers = ignored_modifier_combinations()
        .into_iter()
        .map(|lock_mask| xinput2::XIGrabModifiers {
            modifiers: (modifiers | lock_mask) as c_int,
            status: xinput2::XIGrabSuccess,
        })
        .collect::<Vec<_>>();

    let status = xinput2::XIGrabKeycode(
        display,
        xinput2::XIAllMasterDevices,
        keycode,
        root,
        xinput2::XIGrabModeAsync,
        xinput2::XIGrabModeAsync,
        xlib::False,
        &mut event_mask,
        grab_modifiers.len() as c_int,
        grab_modifiers.as_mut_ptr(),
    );
    xlib::XSync(display, xlib::False);
    if status != xlib::Success as c_int {
        let message = format!("XInput2 hotkey grab failed for {display_name}: status {status}");
        logging::error(&message);
        eprintln!("zoomix {message}");
        return false;
    }

    let successful = grab_modifiers
        .iter()
        .filter(|modifier| modifier.status == xinput2::XIGrabSuccess)
        .count();
    if successful != grab_modifiers.len() {
        let message = format!(
            "XInput2 hotkey grab unavailable for {display_name} in {} modifier states",
            grab_modifiers.len() - successful
        );
        logging::error(&message);
        eprintln!("zoomix {message}");
    }
    successful > 0
}

unsafe fn handle_xinput2_event(
    display: *mut xlib::Display,
    event: &xlib::XEvent,
    opcode: c_int,
    grabs: &[(u8, u32, Mode)],
    sender: &Sender<GlobalAction>,
    pressed_keys: &mut HashSet<u8>,
    wheel_grabbed: bool,
) {
    let mut cookie = xlib::XGenericEventCookie::from(*event);
    if cookie.extension != opcode || xlib::XGetEventData(display, &mut cookie) != xlib::True {
        return;
    }

    if matches!(cookie.evtype, xinput2::XI_ButtonPress) && wheel_grabbed {
        let event_data = &*(cookie.data as *const xinput2::XIDeviceEvent);
        send_wheel_action(event_data.detail as u32, sender);
    } else if matches!(cookie.evtype, xinput2::XI_KeyPress | xinput2::XI_KeyRelease) {
        let event_data = &*(cookie.data as *const xinput2::XIDeviceEvent);
        let keycode = event_data.detail as u8;
        if cookie.evtype == xinput2::XI_KeyRelease {
            pressed_keys.remove(&keycode);
        } else if pressed_keys.insert(keycode) {
            let event_modifiers = event_data.mods.effective as u32 & !ignored_modifier_mask();
            logging::verbose(format!(
                "xinput2 keypress keycode={} state=0x{:x}",
                event_data.detail, event_modifiers
            ));
            if let Some((_, _, mode)) = grabs.iter().find(|(grabbed, modifiers, _)| {
                *grabbed == keycode && *modifiers == event_modifiers
            }) {
                logging::info(format!("xinput2 hotkey matched -> {mode:?}"));
                let _ = sender.send(GlobalAction::Activate(*mode));
            }
        }
    }
    xlib::XFreeEventData(display, &mut cookie);
}

fn send_wheel_action(button: u32, sender: &Sender<GlobalAction>) {
    if let Some(action) = wheel_action(button) {
        let _ = sender.send(action);
    }
}

fn wheel_action(button: u32) -> Option<GlobalAction> {
    match button {
        xlib::Button4 => Some(GlobalAction::LiveZoomIn),
        xlib::Button5 => Some(GlobalAction::LiveZoomOut),
        _ => None,
    }
}

unsafe fn grab_core_key(
    display: *mut xlib::Display,
    root: xlib::Window,
    keycode: c_int,
    modifiers: u32,
    display_name: &str,
) -> bool {
    let mut registered_any = false;
    for lock_mask in ignored_modifier_combinations() {
        let effective_modifiers = modifiers | lock_mask;
        if grab_key_checked(display, root, keycode, effective_modifiers) {
            registered_any = true;
        } else {
            let message = format!(
                "hotkey grab unavailable for {display_name} with X11 modifiers 0x{effective_modifiers:x}; another client may already own this shortcut"
            );
            logging::error(&message);
            eprintln!("zoomix {message}");
        }
    }
    registered_any
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
    fn parses_default_hotkeys_with_ctrl_shift_modifiers() {
        let specs = specs(&Hotkeys::default()).expect("default hotkeys parse");

        assert_eq!(
            specs[0],
            HotkeySpec {
                mode: Mode::Zoom,
                key: "1".to_string(),
                modifiers: HotkeyModifiers {
                    ctrl: true,
                    shift: true,
                    ..Default::default()
                },
                display: "Ctrl+Shift+1".to_string(),
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
    fn maps_vertical_wheel_buttons_to_live_zoom_actions() {
        assert_eq!(wheel_action(xlib::Button4), Some(GlobalAction::LiveZoomIn));
        assert_eq!(wheel_action(xlib::Button5), Some(GlobalAction::LiveZoomOut));
        assert_eq!(wheel_action(xlib::Button1), None);
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
