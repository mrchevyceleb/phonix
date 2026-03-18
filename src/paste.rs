use anyhow::Result;

/// Paste text into the window that was focused when recording started.
///
/// Strategy:
///   1. Restore focus to `target_hwnd` (the window active at key-press time)
///   2. Flush all modifier keys (prevents ghost Ctrl/Alt state)
///   3. Copy text to clipboard, then simulate Ctrl+V
pub fn paste(text: &str, target_hwnd: u64) -> Result<()> {
    if target_hwnd != 0 {
        focus_window(target_hwnd);
        // Give the window time to process the focus event
        std::thread::sleep(std::time::Duration::from_millis(150));
    }

    type_text(text);
    Ok(())
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(windows)]
fn focus_window(hwnd: u64) {
    use windows::Win32::Foundation::HWND as WinHWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        IsIconic, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let hwnd = WinHWND(hwnd as *mut std::ffi::c_void);
    unsafe {
        // Only restore if the window is minimized. Calling SW_RESTORE on a
        // maximized window (e.g. VS Code) would un-maximize it.
        if IsIconic(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_RESTORE);
        }
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(windows)]
fn release_modifiers() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        GetAsyncKeyState, SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    // Only release modifier keys that are actually held down right now.
    // Sending blanket key-ups for ALL modifiers (the old approach) would
    // cancel the user's real Ctrl/Alt/Shift presses system-wide.
    let modifier_vks: [u16; 8] = [0xA4, 0xA5, 0xA2, 0xA3, 0xA0, 0xA1, 0x5B, 0x5C];

    let key_ups: Vec<INPUT> = modifier_vks.iter().filter(|&&vk| unsafe {
        (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0
    }).map(|&vk| INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: VIRTUAL_KEY(vk),
                wScan: 0,
                dwFlags: KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0),
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }).collect();

    if !key_ups.is_empty() {
        unsafe {
            let _ = SendInput(&key_ups, std::mem::size_of::<INPUT>() as i32);
        }
    }
    std::thread::sleep(std::time::Duration::from_millis(50));
}

#[cfg(windows)]
fn type_text(text: &str) {
    use arboard::Clipboard;
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT,
        KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    release_modifiers();

    // Save current clipboard, set our text, Ctrl+V, then restore.
    // This is far more reliable than character-by-character SendInput,
    // which drops/reorders characters when the target app is busy.
    let old_clipboard = Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok());

    let Ok(mut cb) = Clipboard::new() else { return };
    if cb.set_text(text).is_err() { return; }
    drop(cb);

    std::thread::sleep(std::time::Duration::from_millis(30));

    // Simulate Ctrl+V via SendInput
    let vk_control = VIRTUAL_KEY(0xA2); // VK_LCONTROL
    let vk_v = VIRTUAL_KEY(0x56);       // VK_V

    let inputs = [
        // Ctrl down
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk_control,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        // V down
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk_v,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        // V up
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk_v,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        // Ctrl up
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: vk_control,
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
    ];

    unsafe {
        let _ = SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }

    // Wait for the target app to process the paste
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Restore original clipboard contents
    if let Some(old_text) = old_clipboard {
        if let Ok(mut cb) = Clipboard::new() {
            let _ = cb.set_text(&old_text);
        }
    }
}

// ── macOS implementation ─────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn focus_window(pid: u64) {
    use objc::{class, msg_send, sel, sel_impl};
    unsafe {
        let cls = class!(NSRunningApplication);
        let app: *mut objc::runtime::Object =
            msg_send![cls, runningApplicationWithProcessIdentifier: pid as i32];
        if !app.is_null() {
            // NSApplicationActivateIgnoringOtherApps = 1 << 1
            let _: bool = msg_send![app, activateWithOptions: 0x2u64];
        }
    }
}

#[cfg(target_os = "macos")]
fn type_text(text: &str) {
    use arboard::Clipboard;
    use core_graphics::event::{CGEvent, CGEventFlags, CGKeyCode};
    use core_graphics::event_source::{CGEventSource, CGEventSourceStateID};

    // Save current clipboard contents
    let old_clipboard = Clipboard::new().ok().and_then(|mut cb| cb.get_text().ok());

    // Set clipboard to new text
    if let Ok(mut cb) = Clipboard::new() {
        if cb.set_text(text).is_err() {
            return;
        }
    } else {
        return;
    }

    // Small delay for clipboard to settle
    std::thread::sleep(std::time::Duration::from_millis(20));

    // Simulate Cmd+V
    let source = CGEventSource::new(CGEventSourceStateID::HIDSystemState).unwrap();
    let v_keycode: CGKeyCode = 9; // 'v' key on macOS

    if let Ok(key_down) = CGEvent::new_keyboard_event(source.clone(), v_keycode, true) {
        key_down.set_flags(CGEventFlags::CGEventFlagCommand);
        key_down.post(core_graphics::event::CGEventTapLocation::HID);
    }
    if let Ok(key_up) = CGEvent::new_keyboard_event(source, v_keycode, false) {
        key_up.set_flags(CGEventFlags::CGEventFlagCommand);
        key_up.post(core_graphics::event::CGEventTapLocation::HID);
    }

    // Wait for target app to read the clipboard. 200ms accounts for
    // slower Electron apps and heavy systems.
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Restore original clipboard
    if let Some(old_text) = old_clipboard {
        if let Ok(mut cb) = Clipboard::new() {
            let _ = cb.set_text(&old_text);
        }
    }
}

// ── Fallback stubs (Linux, etc.) ─────────────────────────────────────────────

#[cfg(not(any(windows, target_os = "macos")))]
fn focus_window(_hwnd: u64) {}

#[cfg(not(any(windows, target_os = "macos")))]
fn type_text(_text: &str) {}
