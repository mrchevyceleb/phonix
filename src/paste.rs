use anyhow::Result;

/// Paste text into the window that was focused when recording started.
///
/// Strategy:
///   1. Restore focus to `target_hwnd` (the window active at key-press time)
///   2. Clipboard + Shift+Insert (avoids Ctrl, which causes ghost double-paste
///      when LeftCtrl is part of the hotkey combo)
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
fn type_text(text: &str) {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, INPUT_0, INPUT_KEYBOARD, KEYBDINPUT, KEYBD_EVENT_FLAGS,
        KEYEVENTF_KEYUP, VIRTUAL_KEY,
    };

    // Clipboard + Shift+Insert paste. Completely avoids Ctrl, which causes
    // ghost double-paste when LeftCtrl is part of the hotkey combo.
    // Shift+Insert is the universal Windows paste shortcut that predates
    // Ctrl+V and works in virtually every app.

    // Save clipboard, set our text
    let old_clipboard = arboard::Clipboard::new()
        .ok()
        .and_then(|mut cb| cb.get_text().ok());

    if let Ok(mut cb) = arboard::Clipboard::new() {
        if cb.set_text(text).is_err() {
            return;
        }
    } else {
        return;
    }

    std::thread::sleep(std::time::Duration::from_millis(50));

    // Shift+Insert (no Ctrl involved at all)
    let vk_shift: u16 = 0xA0;  // VK_LSHIFT
    let vk_insert: u16 = 0x2D; // VK_INSERT

    let inputs: [INPUT; 4] = [
        // Shift down
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk_shift),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        // Insert down
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk_insert),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        // Insert up
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk_insert),
                    wScan: 0,
                    dwFlags: KEYBD_EVENT_FLAGS(KEYEVENTF_KEYUP.0),
                    time: 0,
                    dwExtraInfo: 0,
                },
            },
        },
        // Shift up
        INPUT {
            r#type: INPUT_KEYBOARD,
            Anonymous: INPUT_0 {
                ki: KEYBDINPUT {
                    wVk: VIRTUAL_KEY(vk_shift),
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

    // Wait for target app to process the paste
    std::thread::sleep(std::time::Duration::from_millis(200));

    // Restore original clipboard
    if let Some(old_text) = old_clipboard {
        if let Ok(mut cb) = arboard::Clipboard::new() {
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
