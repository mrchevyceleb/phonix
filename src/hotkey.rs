use crossbeam_channel::Sender;

#[derive(Debug)]
pub enum HotkeyEvent {
    /// Key pressed. Carries the HWND of the window that was focused at the
    /// moment of the keypress — so we can restore focus before pasting.
    RecordStart { target_hwnd: u64 },
    RecordStop,
}

/// Maps a human-readable key name (from config) to a Windows virtual key code.
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

/// Spawn a background thread that polls `GetAsyncKeyState` every 20ms.
/// On key-down, captures the foreground window so paste can restore focus.
pub fn start_polling(key_name: String, tx: Sender<HotkeyEvent>) {
    std::thread::Builder::new()
        .name("phonix-hotkey".into())
        .spawn(move || {
            let vk = vk_for_name(&key_name);
            let mut held = false;

            loop {
                let pressed = is_key_down(vk);

                if pressed && !held {
                    held = true;
                    let hwnd = get_foreground_window();
                    let _ = tx.send(HotkeyEvent::RecordStart { target_hwnd: hwnd });
                } else if !pressed && held {
                    held = false;
                    let _ = tx.send(HotkeyEvent::RecordStop);
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

#[cfg(not(windows))]
fn is_key_down(_vk: i32) -> bool { false }

#[cfg(not(windows))]
fn get_foreground_window() -> u64 { 0 }
