/// Play a system sound on record start.
pub fn play_start() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::MB_OK;
        unsafe {
            let _ = MessageBeep(MB_OK);
        }
    }
}

/// Play a different system sound on record stop.
pub fn play_stop() {
    #[cfg(windows)]
    {
        use windows::Win32::System::Diagnostics::Debug::MessageBeep;
        use windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE;
        unsafe {
            // MB_ICONASTERISK = 0x40 — plays the "asterisk" system sound
            let _ = MessageBeep(MESSAGEBOX_STYLE(0x00000040));
        }
    }
}
