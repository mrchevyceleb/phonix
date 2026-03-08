/// Play an audible tone on record start (background thread, non-blocking).
pub fn play_start() {
    #[cfg(windows)]
    std::thread::spawn(|| {
        use windows::Win32::System::Diagnostics::Debug::Beep;
        unsafe {
            let _ = Beep(880, 120); // A5 — short high chirp
        }
    });
}

/// Play an audible tone on record stop (background thread, non-blocking).
pub fn play_stop() {
    #[cfg(windows)]
    std::thread::spawn(|| {
        use windows::Win32::System::Diagnostics::Debug::Beep;
        unsafe {
            let _ = Beep(440, 120); // A4 — short low chirp
        }
    });
}
