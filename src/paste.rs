use anyhow::Result;
use arboard::Clipboard;

/// Copy text to clipboard, then simulate Ctrl+V into the active window.
/// Works in terminals, browsers, editors, email clients — anywhere that
/// accepts keyboard input.
pub fn paste(text: &str) -> Result<()> {
    let mut cb = Clipboard::new()?;
    cb.set_text(text.to_owned())?;

    // Small settle time so the clipboard write completes before SendInput fires
    std::thread::sleep(std::time::Duration::from_millis(60));

    send_ctrl_v();
    Ok(())
}

#[cfg(windows)]
fn send_ctrl_v() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, VK_CONTROL, VK_V,
    };

    // In the windows crate, "key down" = KEYBD_EVENT_FLAGS(0) — no flag needed.
    // "key up" = KEYEVENTF_KEYUP
    let key_down = KEYBD_EVENT_FLAGS(0);

    let inputs: [INPUT; 4] = [
        make_key(VK_CONTROL, key_down),
        make_key(VK_V, key_down),
        make_key(VK_V, KEYEVENTF_KEYUP),
        make_key(VK_CONTROL, KEYEVENTF_KEYUP),
    ];

    unsafe {
        SendInput(&inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

#[cfg(windows)]
fn make_key(
    vk: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY,
    flags: windows::Win32::UI::Input::KeyboardAndMouse::KEYBD_EVENT_FLAGS,
) -> windows::Win32::UI::Input::KeyboardAndMouse::INPUT {
    use windows::Win32::UI::Input::KeyboardAndMouse::*;
    INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT {
                wVk: vk,
                wScan: 0,
                dwFlags: flags,
                time: 0,
                dwExtraInfo: 0,
            },
        },
    }
}

#[cfg(not(windows))]
fn send_ctrl_v() {
    // TODO: Linux (xdotool) / macOS (CGEventPost)
}
