use anyhow::Result;
use arboard::Clipboard;

/// Copy text to clipboard then paste into the window identified by `target_hwnd`.
/// We restore focus to that window first — necessary because pressing a modifier
/// key like Right Alt can steal focus from the target before we get here.
pub fn paste(text: &str, target_hwnd: u64) -> Result<()> {
    let mut cb = Clipboard::new()?;
    cb.set_text(text.to_owned())?;

    // Restore focus to the window that was active when recording started
    if target_hwnd != 0 {
        focus_window(target_hwnd);
        // Let the window process the focus event before we send Ctrl+V
        std::thread::sleep(std::time::Duration::from_millis(120));
    } else {
        std::thread::sleep(std::time::Duration::from_millis(60));
    }

    send_ctrl_v();
    Ok(())
}

#[cfg(windows)]
fn focus_window(hwnd: u64) {
    use windows::Win32::Foundation::HWND as WinHWND;
    use windows::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE};

    let hwnd = WinHWND(hwnd as *mut std::ffi::c_void);
    unsafe {
        let _ = ShowWindow(hwnd, SW_RESTORE); // un-minimize if needed
        let _ = SetForegroundWindow(hwnd);
    }
}

#[cfg(windows)]
fn send_ctrl_v() {
    use windows::Win32::UI::Input::KeyboardAndMouse::{
        SendInput, INPUT, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP,
        VK_CONTROL, VK_V,
    };

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
fn focus_window(_hwnd: u64) {}

#[cfg(not(windows))]
fn send_ctrl_v() {}
