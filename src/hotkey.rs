use crossbeam_channel::Sender;

#[derive(Debug)]
pub enum HotkeyEvent {
    /// Key pressed. Carries the HWND of the window that was focused at the
    /// moment of the keypress — so we can restore focus before pasting.
    RecordStart { target_hwnd: u64 },
    RecordStop,
}

/// All supported push-to-talk keys as `(config_name, display_label)`.
/// Left variants come first in each group so the UI reads naturally.
pub const SUPPORTED_KEYS: &[(&str, &str)] = &[
    ("LeftAlt", "Left Alt"),
    ("RightAlt", "Right Alt"),
    ("LeftCtrl", "Left Ctrl"),
    ("RightCtrl", "Right Ctrl"),
    ("LeftShift", "Left Shift"),
    ("RightShift", "Right Shift"),
    ("CapsLock", "Caps Lock"),
    ("ScrollLock", "Scroll Lock"),
    ("F13", "F13"),
    ("F14", "F14"),
    ("F15", "F15"),
    ("F16", "F16"),
];

/// Groups of key indices into SUPPORTED_KEYS for UI layout.
/// Each entry is `(group_label, start_index, end_index_exclusive)`.
pub const KEY_GROUPS: &[(&str, usize, usize)] = &[
    ("Alt", 0, 2),
    ("Ctrl", 2, 4),
    ("Shift", 4, 6),
    ("Locks", 6, 8),
    ("Function", 8, 12),
];

/// Maps a human-readable key name (from config) to a Windows virtual key code.
#[cfg(windows)]
fn vk_for_name(name: &str) -> i32 {
    match name.to_lowercase().replace(['-', '_', ' '], "").as_str() {
        "rightalt" | "ralt" | "altgr" => 0xA5, // VK_RMENU
        "leftalt" | "lalt" => 0xA4,             // VK_LMENU
        "rightctrl" | "rightcontrol" | "rctrl" => 0xA3, // VK_RCONTROL
        "leftctrl" | "leftcontrol" | "lctrl" => 0xA2,   // VK_LCONTROL
        "rightshift" | "rshift" => 0xA1,
        "leftshift" | "lshift" => 0xA0,
        "capslock" => 0x14,
        "scrolllock" => 0x91,
        "f13" => 0x7C,
        "f14" => 0x7D,
        "f15" => 0x7E,
        "f16" => 0x7F,
        _ => {
            eprintln!("[phonix/hotkey] unknown key '{}', defaulting to RightAlt", name);
            0xA5
        }
    }
}

/// Maps a human-readable key name (from config) to a macOS CGKeyCode.
#[cfg(target_os = "macos")]
fn vk_for_name(name: &str) -> i32 {
    match name.to_lowercase().replace(['-', '_', ' '], "").as_str() {
        "rightalt" | "ralt" | "altgr" | "rightoption" | "roption" => 0x3D,
        "leftalt" | "lalt" | "leftoption" | "loption" => 0x3A,
        "rightctrl" | "rightcontrol" | "rctrl" => 0x3E,
        "leftctrl" | "leftcontrol" | "lctrl" => 0x3B,
        "rightshift" | "rshift" => 0x3C,
        "leftshift" | "lshift" => 0x38,
        "capslock" => 0x39,
        "scrolllock" => 0x69, // No macOS equivalent, map to F13
        "f13" => 0x69,
        "f14" => 0x6B,
        "f15" => 0x71,
        "f16" => 0x6A,
        _ => {
            eprintln!("[phonix/hotkey] unknown key '{}', defaulting to F13", name);
            0x69 // F13
        }
    }
}

/// Fallback vk_for_name for unsupported platforms.
#[cfg(not(any(windows, target_os = "macos")))]
fn vk_for_name(name: &str) -> i32 {
    eprintln!("[phonix/hotkey] unsupported platform, key '{}' ignored", name);
    0
}

/// Check which supported key is currently pressed. Returns the config name if any.
/// Kept for backward compatibility; prefer `detect_pressed_combo` for combo support.
#[allow(dead_code)]
pub fn detect_pressed_key() -> Option<&'static str> {
    for &(config_name, _) in supported_keys() {
        let vk = vk_for_name(config_name);
        if is_key_down(vk) {
            return Some(config_name);
        }
    }
    None
}

/// Detect all currently-pressed supported keys and return them as a `+`-joined
/// combo string (e.g. `"LeftCtrl+LeftShift"`). Keys are sorted in SUPPORTED_KEYS
/// order for consistency. Returns None if no supported key is pressed.
pub fn detect_pressed_combo() -> Option<String> {
    let pressed: Vec<&str> = supported_keys()
        .iter()
        .filter(|&&(name, _)| is_key_down(vk_for_name(name)))
        .map(|&(name, _)| name)
        .collect();
    if pressed.is_empty() {
        None
    } else {
        Some(pressed.join("+"))
    }
}

/// Check whether a `+`-separated key string is valid (each part is a known key name).
pub fn is_valid_key_combo(key_str: &str) -> bool {
    key_str.split('+').all(|part| {
        let trimmed = part.trim();
        SUPPORTED_KEYS.iter().any(|&(name, _)| name == trimmed)
    })
}

/// Format a `+`-separated config key string into a display-friendly label.
pub fn format_hotkey_display(record_key: &str) -> String {
    record_key
        .split('+')
        .map(|k| {
            let k = k.trim();
            supported_keys()
                .iter()
                .find(|&&(name, _)| name == k)
                .map(|&(_, label)| label)
                .unwrap_or(k)
        })
        .collect::<Vec<_>>()
        .join(" + ")
}

/// Spawn a background thread that polls `GetAsyncKeyState` every 20ms.
/// On key-down, captures the foreground window so paste can restore focus.
///
/// `paste_guard` is set by the pipeline thread during (and briefly after)
/// paste operations. While true, all key events are suppressed so ghost
/// keypresses from SetForegroundWindow/SendInput don't trigger recordings.
pub fn start_polling(
    key_name: String,
    tx: Sender<HotkeyEvent>,
    paste_guard: std::sync::Arc<std::sync::atomic::AtomicBool>,
) {
    use std::sync::atomic::Ordering;

    std::thread::Builder::new()
        .name("phonix-hotkey".into())
        .spawn(move || {
            let vks: Vec<i32> = key_name
                .split('+')
                .map(|k| vk_for_name(k.trim()))
                .collect();
            let mut held = false;
            // Cooldown after RecordStop to ignore ghost keypresses caused by
            // SetForegroundWindow / SendInput during paste (prevents double-fire).
            let mut cooldown_until: Option<std::time::Instant> = None;

            loop {
                let pressed = vks.iter().all(|&vk| is_key_down(vk));

                // Suppress all events while paste is in progress (or cooling down).
                // Do NOT reset `held` here -- we want to preserve the held/released
                // state so that when the guard clears we don't see a stale key-down
                // as a new press. Instead, track the physical state silently.
                if paste_guard.load(Ordering::Relaxed) {
                    // Track physical state but don't emit events
                    held = pressed;
                    std::thread::sleep(std::time::Duration::from_millis(20));
                    continue;
                }

                // Skip events during cooldown (also track physical state silently)
                if let Some(deadline) = cooldown_until {
                    if std::time::Instant::now() < deadline {
                        held = pressed;
                        std::thread::sleep(std::time::Duration::from_millis(20));
                        continue;
                    }
                    cooldown_until = None;
                }

                if pressed && !held {
                    held = true;
                    let hwnd = get_foreground_window();
                    let _ = tx.try_send(HotkeyEvent::RecordStart { target_hwnd: hwnd });
                } else if !pressed && held {
                    held = false;
                    let _ = tx.try_send(HotkeyEvent::RecordStop);
                    // 500ms cooldown: paste takes ~150ms focus + typing time.
                    // Any ghost keypress from SetForegroundWindow happens within
                    // the first ~100ms, so 500ms is safely beyond that.
                    cooldown_until = Some(std::time::Instant::now() + std::time::Duration::from_millis(500));
                }

                std::thread::sleep(std::time::Duration::from_millis(20));
            }
        })
        .expect("failed to spawn hotkey thread");
}

#[cfg(windows)]
fn is_key_down(vk: i32) -> bool {
    use windows::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;
    unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 }
}

#[cfg(windows)]
fn get_foreground_window() -> u64 {
    use windows::Win32::UI::WindowsAndMessaging::GetForegroundWindow;
    unsafe { GetForegroundWindow().0 as u64 }
}

#[cfg(target_os = "macos")]
fn is_key_down(vk: i32) -> bool {
    // CGEventSourceKeyState with HIDSystemState (1) polls physical key state
    extern "C" {
        fn CGEventSourceKeyState(state_id: u32, key: u16) -> bool;
    }
    unsafe { CGEventSourceKeyState(1, vk as u16) }
}

#[cfg(target_os = "macos")]
fn get_foreground_window() -> u64 {
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let workspace: *mut objc::runtime::Object = msg_send![class!(NSWorkspace), sharedWorkspace];
        let app: *mut objc::runtime::Object = msg_send![workspace, frontmostApplication];
        if app.is_null() {
            return 0;
        }
        let pid: i32 = msg_send![app, processIdentifier];
        pid as u64
    }
}

#[cfg(not(any(windows, target_os = "macos")))]
fn is_key_down(_vk: i32) -> bool { false }

#[cfg(not(any(windows, target_os = "macos")))]
fn get_foreground_window() -> u64 { 0 }

/// Check if the app has Accessibility permission (macOS only).
/// Returns true on non-macOS platforms.
#[cfg(target_os = "macos")]
pub fn check_accessibility() -> bool {
    extern "C" {
        fn AXIsProcessTrusted() -> bool;
    }
    unsafe { AXIsProcessTrusted() }
}

/// Prompt the user for Accessibility permission (macOS only).
/// Shows the system dialog asking to grant permission.
#[cfg(target_os = "macos")]
pub fn prompt_accessibility() {
    use core_foundation::base::TCFType;
    use core_foundation::boolean::CFBoolean;
    use core_foundation::dictionary::CFDictionary;
    use core_foundation::string::CFString;

    extern "C" {
        fn AXIsProcessTrustedWithOptions(options: core_foundation::base::CFTypeRef) -> bool;
    }

    let key = CFString::new("AXTrustedCheckOptionPrompt");
    let value = CFBoolean::true_value();
    let options = CFDictionary::from_CFType_pairs(&[(key.as_CFType(), value.as_CFType())]);
    unsafe {
        AXIsProcessTrustedWithOptions(options.as_CFTypeRef());
    }
}

#[cfg(not(target_os = "macos"))]
pub fn check_accessibility() -> bool { true }

#[cfg(not(target_os = "macos"))]
pub fn prompt_accessibility() {}

/// Platform-filtered supported keys. Hides ScrollLock on macOS.
pub fn supported_keys() -> &'static [(&'static str, &'static str)] {
    #[cfg(target_os = "macos")]
    {
        const MACOS_KEYS: &[(&str, &str)] = &[
            ("LeftAlt", "Left Option"),
            ("RightAlt", "Right Option"),
            ("LeftCtrl", "Left Control"),
            ("RightCtrl", "Right Control"),
            ("LeftShift", "Left Shift"),
            ("RightShift", "Right Shift"),
            ("CapsLock", "Caps Lock"),
            ("F13", "F13"),
            ("F14", "F14"),
            ("F15", "F15"),
            ("F16", "F16"),
        ];
        MACOS_KEYS
    }
    #[cfg(not(target_os = "macos"))]
    {
        SUPPORTED_KEYS
    }
}

/// Platform-filtered key groups for UI layout.
pub fn key_groups() -> &'static [(&'static str, usize, usize)] {
    #[cfg(target_os = "macos")]
    {
        const MACOS_GROUPS: &[(&str, usize, usize)] = &[
            ("Option", 0, 2),
            ("Control", 2, 4),
            ("Shift", 4, 6),
            ("Other", 6, 7),    // CapsLock only
            ("Function", 7, 11),
        ];
        MACOS_GROUPS
    }
    #[cfg(not(target_os = "macos"))]
    {
        KEY_GROUPS
    }
}
