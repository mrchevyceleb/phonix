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

use app::{AppEvent, PhonixApp, PipelineCmd, SharedFlags};
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
    let (cmd_tx, cmd_rx) = bounded::<PipelineCmd>(8);
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
        let cmd_rx = cmd_rx;

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
                        let _ = event_tx.try_send(AppEvent::Error(format!("Mic error: {e}")));
                    }
                }

                // Helper: spawn the transcription task on the tokio runtime
                let spawn_transcription = |rt: &Runtime,
                                           samples: Vec<f32>,
                                           sample_rate: u32,
                                           pre_roll_len: usize,
                                           target_hwnd: u64,
                                           long_dictate: bool,
                                           event_tx: &crossbeam_channel::Sender<AppEvent>,
                                           flags: &Arc<Mutex<SharedFlags>>| {
                    let tx = event_tx.clone();
                    let flags = Arc::clone(flags);
                    let prl = pre_roll_len;
                    let hwnd = target_hwnd;
                    let for_ld = long_dictate;
                    let cfg = Config::load();

                    rt.spawn(async move {
                        // Guard: ignore clips where actual speech is shorter than 0.5s
                        let speech_samples = samples.len().saturating_sub(prl);
                        if speech_samples < (sample_rate / 2) as usize {
                            let _ = tx.try_send(AppEvent::StatusUpdate(
                                "Too short \u{2014} try again".into(),
                            ));
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            let _ = tx.try_send(AppEvent::StatusUpdate(
                                "Ready \u{2014} hold key to dictate".into(),
                            ));
                            return;
                        }

                        let _ = tx.try_send(AppEvent::StatusUpdate("Transcribing\u{2026}".into()));

                        let raw = match whisper::transcribe(samples, sample_rate, &cfg).await {
                            Ok(r) => r,
                            Err(e) => {
                                let _ = tx.try_send(AppEvent::Error(e.to_string()));
                                return;
                            }
                        };

                        if raw.is_empty() {
                            let _ = tx.try_send(AppEvent::StatusUpdate("No speech detected".into()));
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            let _ = tx.try_send(AppEvent::StatusUpdate(
                                "Ready \u{2014} hold key to dictate".into(),
                            ));
                            return;
                        }

                        let text = if cfg.cleanup_enabled {
                            let _ = tx.try_send(AppEvent::StatusUpdate("Cleaning up\u{2026}".into()));
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

                        let _ = tx.try_send(AppEvent::Transcribed { text, raw, for_long_dictate: for_ld });
                    });
                };

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
                                let _ = event_tx.try_send(AppEvent::RecordingStarted);
                            }
                            hotkey::HotkeyEvent::RecordStop if recording => {
                                recording = false;
                                let samples = recorder.stop();
                                let _ = event_tx.try_send(AppEvent::RecordingStopped);
                                spawn_transcription(
                                    &rt, samples, sample_rate, pre_roll_len,
                                    target_hwnd, long_dictate_at_start,
                                    &event_tx, &flags,
                                );
                            }
                            _ => {}
                        }
                    }

                    // Drain UI commands (Long Dictate Start/Stop button)
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            PipelineCmd::StartRecording if !recording => {
                                recording = true;
                                target_hwnd = 0; // Long Dictate never pastes
                                pre_roll_len = recorder.start();
                                long_dictate_at_start = true;
                                let _ = event_tx.try_send(AppEvent::RecordingStarted);
                            }
                            PipelineCmd::StopRecording if recording => {
                                recording = false;
                                let samples = recorder.stop();
                                let _ = event_tx.try_send(AppEvent::RecordingStopped);
                                spawn_transcription(
                                    &rt, samples, sample_rate, pre_roll_len,
                                    target_hwnd, long_dictate_at_start,
                                    &event_tx, &flags,
                                );
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
    let (tray, tray_menu_ids) = build_tray();

    // ── egui window ───────────────────────────────────────────────────────────
    let store_for_app = Arc::clone(&store);
    let flags_for_app = Arc::clone(&flags);
    let config_for_app = config.clone();

    eframe::run_native(
        "Phonix",
        eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_title("Phonix")
                .with_inner_size([500.0, 660.0])
                .with_min_inner_size([380.0, 440.0])
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
                tray_menu_ids,
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
            let _ = event_tx.try_send(AppEvent::Error(
                "whisper-server/server.py not found next to phonix.exe".into(),
            ));
            return None;
        }
    };

    let _ = event_tx.try_send(AppEvent::StatusUpdate(
        "Starting local Whisper server…".into(),
    ));

    // Kill any zombie whisper-server processes from previous runs
    server::WhisperServer::kill_stale();

    let mut srv = server::WhisperServer::new();
    if let Err(e) = srv.start(&server_py) {
        let _ = event_tx.try_send(AppEvent::Error(e));
        return None;
    }

    // Health-poll in background — updates status when ready
    let tx = event_tx.clone();
    let srv_ref = server::WhisperServer::new(); // dummy ref just for the wait helper
    std::thread::spawn(move || {
        match srv_ref.wait_until_ready(std::time::Duration::from_secs(60)) {
            Ok(_) => {
                let _ = tx.try_send(AppEvent::StatusUpdate("Ready — hold key to dictate".into()));
            }
            Err(e) => {
                let _ = tx.try_send(AppEvent::Error(e));
            }
        }
    });

    Some(srv)
}

// ── Tray icon ─────────────────────────────────────────────────────────────────

/// IDs returned alongside the tray icon so the app can match menu events.
pub struct TrayMenuIds {
    pub open: tray_icon::menu::MenuId,
    pub quit: tray_icon::menu::MenuId,
}

fn build_tray() -> (Option<tray_icon::TrayIcon>, Option<TrayMenuIds>) {
    use tray_icon::{
        menu::{Menu, MenuItem},
        TrayIconBuilder,
    };

    let menu = Menu::new();
    let open_item = MenuItem::new("Open Phonix", true, None);
    let quit_item = MenuItem::new("Quit", true, None);
    let _ = menu.append(&open_item);
    let _ = menu.append(&quit_item);

    let ids = TrayMenuIds {
        open: open_item.id().clone(),
        quit: quit_item.id().clone(),
    };

    match TrayIconBuilder::new()
        .with_tooltip("Phonix — voice dictation")
        .with_icon(make_tray_icon_rgb(100, 180, 255))
        .with_menu(Box::new(menu))
        .build()
        .ok()
    {
        Some(tray) => (Some(tray), Some(ids)),
        None => (None, None),
    }
}

/// Generate RGBA pixel data for a microphone-in-circle icon.
fn generate_mic_icon(bg_r: u8, bg_g: u8, bg_b: u8, size: u32) -> Vec<u8> {
    let mut rgba = vec![0u8; (size * size * 4) as usize];
    let s = size as f32 / 32.0;
    let center = size as f32 / 2.0;
    let circle_r = 13.0 * s;

    for y in 0..size {
        for x in 0..size {
            let px = x as f32 + 0.5;
            let py = y as f32 + 0.5;
            let dx = px - center;
            let dy = py - center;
            let dist = (dx * dx + dy * dy).sqrt();
            let i = ((y * size + x) * 4) as usize;

            // Anti-aliased circle edge
            let alpha = ((circle_r - dist + 0.5) * 255.0).clamp(0.0, 255.0) as u8;
            if alpha == 0 {
                continue;
            }

            // Normalise to 32px base coordinates, relative to center
            let nx = dx / s;
            let ny = dy / s;

            // --- Mic head (capsule) ---
            let head_cy: f32 = -2.5;
            let head_hw: f32 = 2.5;
            let head_hh: f32 = 4.0;
            let in_head = {
                let rx = nx.abs();
                let ry = (ny - head_cy).abs();
                if ry <= head_hh - head_hw {
                    rx <= head_hw
                } else {
                    let oy = ry - (head_hh - head_hw);
                    rx * rx + oy * oy <= head_hw * head_hw
                }
            };

            // --- U-shaped arc around mic ---
            let arc_cy: f32 = -0.5;
            let arc_r: f32 = 4.2;
            let arc_thick: f32 = 1.4;
            let arc_dist = (nx * nx + (ny - arc_cy).powi(2)).sqrt();
            let in_arc = (arc_dist - arc_r).abs() <= arc_thick / 2.0 && ny >= arc_cy;

            // --- Stem ---
            let stem_top = arc_cy + arc_r;
            let stem_bottom = stem_top + 2.5;
            let in_stem = nx.abs() <= 0.7 && ny >= stem_top && ny <= stem_bottom;

            // --- Base ---
            let in_base = nx.abs() <= 2.8 && (ny - stem_bottom).abs() <= 0.7;

            if in_head || in_arc || in_stem || in_base {
                rgba[i] = 255;
                rgba[i + 1] = 255;
                rgba[i + 2] = 255;
                rgba[i + 3] = alpha;
            } else {
                rgba[i] = bg_r;
                rgba[i + 1] = bg_g;
                rgba[i + 2] = bg_b;
                rgba[i + 3] = alpha;
            }
        }
    }
    rgba
}

/// Tray icon: microphone in a colored circle.
pub fn make_tray_icon_rgb(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    let size = 32u32;
    let rgba = generate_mic_icon(r, g, b, size);
    tray_icon::Icon::from_rgba(rgba, size, size).expect("tray icon")
}

/// Window icon: microphone in a blue circle.
fn load_icon() -> egui::IconData {
    let size = 64u32;
    let rgba = generate_mic_icon(100, 180, 255, size);
    egui::IconData {
        rgba,
        width: size,
        height: size,
    }
}
