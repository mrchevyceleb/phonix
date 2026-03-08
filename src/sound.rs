/// Play a system sound on record start (async, non-blocking).
pub fn play_start() {
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
        unsafe {
            // "SystemExclamation" — a short attention sound
            let _ = PlaySoundW(
                w!("SystemExclamation"),
                None,
                SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}

/// Play a system sound on record stop (async, non-blocking).
pub fn play_stop() {
    #[cfg(windows)]
    {
        use windows::core::w;
        use windows::Win32::Media::Audio::{PlaySoundW, SND_ALIAS, SND_ASYNC, SND_NODEFAULT};
        unsafe {
            // "SystemAsterisk" — a softer notification sound
            let _ = PlaySoundW(
                w!("SystemAsterisk"),
                None,
                SND_ALIAS | SND_ASYNC | SND_NODEFAULT,
            );
        }
    }
}
