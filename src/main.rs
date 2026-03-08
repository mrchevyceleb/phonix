// Hide the console window in release builds — it's a GUI app
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod cleanup;
mod config;
mod hotkey;
mod overlay;
mod paste;
mod server;
mod sound;
mod store;
mod whisper;

use std::sync::{Arc, Mutex};

use app::{AppEvent, PhonixApp, SharedFlags};
use audio::AudioRecorder;
use config::Config;
use crossbeam_channel::bounded;
use store::Store;
use tokio::runtime::Runtime;

fn main() -> eframe::Result<()> {
    let config = Config::load();
    let store = Arc::new(Mutex::new(Store::load()));

    let flags = Arc::new(Mutex::new(SharedFlags {
        long_dictate_active: false,
        auto_paste: config.auto_paste,
    }));

    // Channels
    let (event_tx, event_rx) = bounded::<AppEvent>(32);
    let (cmd_tx, _cmd_rx) = bounded::<()>(8);
    let (hotkey_tx, hotkey_rx) = bounded::<hotkey::HotkeyEvent>(8);

    // ── Local Whisper server (auto-start when provider = Local) ───────────────
    // Keep _whisper_server alive until the app exits — Drop kills the process.
    let _whisper_server = maybe_start_local_server(&config, &event_tx);

    // ── Hotkey polling thread ─────────────────────────────────────────────────
    hotkey::start_polling(config.record_key.clone(), hotkey_tx);

    // ── Pipeline thread ───────────────────────────────────────────────────────
    {
        let _config = config.clone(); // retained for potential future use
        let flags = Arc::clone(&flags);
        let event_tx = event_tx.clone();

        std::thread::Builder::new()
            .name("phonix-pipeline".into())
            .spawn(move || {
                let rt = Runtime::new().expect("tokio runtime");
                let mut recorder = AudioRecorder::new();
                let mut sample_rate = 44100u32;
                let mut recording = false;
                let mut target_hwnd: u64 = 0;
                let mut pre_roll_len: usize = 0;
                let mut long_dictate_at_start = false;

                // Open the mic once at startup so the pre-roll buffer is
                // already warm when the user first presses the hotkey.
                match recorder.open() {
                    Ok(sr) => sample_rate = sr,
                    Err(e) => {
                        let _ = event_tx.send(AppEvent::Error(format!("Mic error: {e}")));
                    }
                }

                loop {
                    // Drain hotkey events
                    while let Ok(ev) = hotkey_rx.try_recv() {
                        match ev {
                            hotkey::HotkeyEvent::RecordStart { target_hwnd: hwnd } if !recording => {
                                recording = true;
                                target_hwnd = hwnd;
                                pre_roll_len = recorder.start();
                                // Capture long-dictate state NOW so it's correct even
                                // if the user toggles Stop before transcription finishes
                                long_dictate_at_start = flags.lock().unwrap().long_dictate_active;
                                let _ = event_tx.send(AppEvent::RecordingStarted);
                            }
                            hotkey::HotkeyEvent::RecordStop if recording => {
                                recording = false;
                                let samples = recorder.stop();
                                let _ = event_tx.send(AppEvent::RecordingStopped);

                                // Spawn async task for transcribe + cleanup + paste
                                // Always reload from disk so settings changes take effect immediately
                                let cfg = Config::load();
                                let hwnd = target_hwnd;
                                let tx = event_tx.clone();
                                let flags = Arc::clone(&flags);
                                let prl = pre_roll_len;
                                let for_ld = long_dictate_at_start;

                                rt.spawn(async move {
                                    // Guard: ignore clips where actual speech is shorter than 0.5s
                                    // (subtract pre-roll since that's ambient noise, not speech)
                                    let speech_samples = samples.len().saturating_sub(prl);
                                    if speech_samples < (sample_rate / 2) as usize {
                                        let _ = tx.send(AppEvent::StatusUpdate(
                                            "Too short — try again".into(),
                                        ));
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                        let _ = tx.send(AppEvent::StatusUpdate(
                                            "Ready — hold key to dictate".into(),
                                        ));
                                        return;
                                    }

                                    let _ = tx.send(AppEvent::StatusUpdate("Transcribing…".into()));

                                    let raw = match whisper::transcribe(samples, sample_rate, &cfg).await {
                                        Ok(r) => r,
                                        Err(e) => {
                                            let _ = tx.send(AppEvent::Error(e.to_string()));
                                            return;
                                        }
                                    };

                                    if raw.is_empty() {
                                        let _ = tx.send(AppEvent::StatusUpdate("No speech detected".into()));
                                        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                                        let _ = tx.send(AppEvent::StatusUpdate(
                                            "Ready — hold key to dictate".into(),
                                        ));
                                        return;
                                    }

                                    let text = if cfg.cleanup_enabled {
                                        let _ = tx.send(AppEvent::StatusUpdate("Cleaning up…".into()));
                                        cleanup::cleanup(&raw, &cfg).await
                                    } else {
                                        raw.clone()
                                    };

                                    // Auto-paste unless in long dictate mode
                                    let do_paste = {
                                        let f = flags.lock().unwrap();
                                        f.auto_paste
                                    } && !for_ld;

                                    if do_paste {
                                        if let Err(e) = paste::paste(&text, hwnd) {
                                            eprintln!("[phonix/paste] {e}");
                                        }
                                    }

                                    let _ = tx.send(AppEvent::Transcribed { text, raw, for_long_dictate: for_ld });
                                });
                            }
                            _ => {}
                        }
                    }

                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            })
            .expect("failed to spawn pipeline thread");
    }

    // ── Recording overlay (native always-on-top window) ─────────────────────
    let rec_overlay = overlay::Overlay::new();

    // ── System tray ───────────────────────────────────────────────────────────
    let tray = build_tray();

    // ── egui window ───────────────────────────────────────────────────────────
    let store_for_app = Arc::clone(&store);
    let flags_for_app = Arc::clone(&flags);
    let config_for_app = config.clone();

    eframe::run_native(
        "Phonix",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Phonix")
                .with_inner_size([460.0, 620.0])
                .with_min_inner_size([360.0, 400.0])
                .with_icon(load_icon()),
            ..Default::default()
        },
        Box::new(move |cc| {
            Ok(Box::new(PhonixApp::new(
                cc,
                store_for_app,
                config_for_app,
                flags_for_app,
                event_rx,
                cmd_tx,
                tray,
                rec_overlay,
            )))
        }),
    )
}

// ── Local server management ───────────────────────────────────────────────────

fn maybe_start_local_server(
    config: &Config,
    event_tx: &crossbeam_channel::Sender<AppEvent>,
) -> Option<server::WhisperServer> {
    use config::WhisperProvider;

    if config.whisper_provider != WhisperProvider::Local {
        return None;
    }

    let server_py = match server::find_server_py() {
        Some(p) => p,
        None => {
            let _ = event_tx.send(AppEvent::Error(
                "whisper-server/server.py not found next to phonix.exe".into(),
            ));
            return None;
        }
    };

    let _ = event_tx.send(AppEvent::StatusUpdate(
        "Starting local Whisper server…".into(),
    ));

    // Kill any zombie whisper-server processes from previous runs
    server::WhisperServer::kill_stale();

    let mut srv = server::WhisperServer::new();
    if let Err(e) = srv.start(&server_py) {
        let _ = event_tx.send(AppEvent::Error(e));
        return None;
    }

    // Health-poll in background — updates status when ready
    let tx = event_tx.clone();
    let srv_ref = server::WhisperServer::new(); // dummy ref just for the wait helper
    std::thread::spawn(move || {
        match srv_ref.wait_until_ready(std::time::Duration::from_secs(60)) {
            Ok(_) => {
                let _ = tx.send(AppEvent::StatusUpdate("Ready — hold key to dictate".into()));
            }
            Err(e) => {
                let _ = tx.send(AppEvent::Error(e));
            }
        }
    });

    Some(srv)
}

// ── Tray icon ─────────────────────────────────────────────────────────────────

fn build_tray() -> Option<tray_icon::TrayIcon> {
    use tray_icon::{
        menu::{Menu, MenuItem},
        TrayIconBuilder,
    };

    let menu = Menu::new();
    let _ = menu.append(&MenuItem::new("Open Phonix", true, None));
    let _ = menu.append(&MenuItem::new("Quit", true, None));

    TrayIconBuilder::new()
        .with_tooltip("Phonix — voice dictation")
        .with_icon(make_tray_icon_rgb(100, 180, 255))
        .with_menu(Box::new(menu))
        .build()
        .ok()
}

/// Generate a simple 32x32 RGBA colored dot as the tray icon.
pub fn make_tray_icon_rgb(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let cx = x as f32 - size as f32 / 2.0;
            let cy = y as f32 - size as f32 / 2.0;
            let dist = (cx * cx + cy * cy).sqrt();
            let i = ((y * size + x) * 4) as usize;
            if dist < 13.0 {
                rgba[i] = r;
                rgba[i + 1] = g;
                rgba[i + 2] = b;
                rgba[i + 3] = 255;
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, size, size).expect("tray icon")
}

/// Same icon data used for the app window title bar.
fn load_icon() -> egui::IconData {
    let size = 32u32;
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let cx = x as f32 - size as f32 / 2.0;
            let cy = y as f32 - size as f32 / 2.0;
            let dist = (cx * cx + cy * cy).sqrt();
            let i = ((y * size + x) * 4) as usize;
            if dist < 13.0 {
                rgba[i] = 100;
                rgba[i + 1] = 180;
                rgba[i + 2] = 255;
                rgba[i + 3] = 255;
            }
        }
    }
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}
