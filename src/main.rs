// Hide the console window in release builds — it's a GUI app
#![cfg_attr(all(not(debug_assertions), windows), windows_subsystem = "windows")]

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
mod update;
mod whisper;

use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use app::{AppEvent, PhonixApp, PipelineCmd, SharedFlags};
use audio::AudioRecorder;
use config::Config;
use crossbeam_channel::bounded;
use store::Store;
use tokio::runtime::Runtime;

/// Debug logger that writes to %APPDATA%/phonix/Phonix/config/debug.log.
/// GUI apps have no stderr, so this is the only way to see what's happening.
fn dbg_log(msg: &str) {
    use std::io::Write;
    if let Ok(appdata) = std::env::var("APPDATA") {
        let path = std::path::PathBuf::from(appdata)
            .join("phonix").join("Phonix").join("config").join("debug.log");
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
            let now = chrono::Local::now().format("%H:%M:%S%.3f");
            let _ = writeln!(f, "[{now}] {msg}");
        }
    }
}

fn main() -> eframe::Result<()> {
    let config = Config::load();
    let store = Arc::new(Mutex::new(Store::load()));

    let flags = Arc::new(Mutex::new(SharedFlags {
        long_dictate_active: false,
        auto_paste: config.auto_paste,
        paste_in_progress: false,
    }));

    // Channels
    let (event_tx, event_rx) = bounded::<AppEvent>(32);
    let (cmd_tx, cmd_rx) = bounded::<PipelineCmd>(8);
    let (hotkey_tx, hotkey_rx) = bounded::<hotkey::HotkeyEvent>(8);

    // ── Check for updates ───────────────────────────────────────────────────────
    update::check_for_updates(event_tx.clone());

    // ── Local Whisper server (auto-start when provider = Local) ───────────────
    // Keep _whisper_server alive until the app exits — Drop kills the process.
    // server_ready is false until the health poll confirms the server is up.
    // Non-local providers are always "ready".
    let server_ready = Arc::new(AtomicBool::new(
        config.whisper_provider != config::WhisperProvider::Local,
    ));
    let _whisper_server = maybe_start_local_server(&config, &event_tx, &server_ready);

    // ── macOS Accessibility permission check ─────────────────────────────────
    #[cfg(target_os = "macos")]
    {
        if !hotkey::check_accessibility() {
            hotkey::prompt_accessibility();
            let _ = event_tx.try_send(AppEvent::Error(
                "Accessibility permission required. Grant it in System Settings > Privacy & Security > Accessibility, then restart Phonix.".into(),
            ));
        }
    }

    // ── Recording overlay (native always-on-top window) ─────────────────────
    // Created before the pipeline so both the pipeline thread and the UI can
    // set overlay state. The overlay polls an AtomicU8 so set_state is safe
    // from any thread.
    let rec_overlay = overlay::Overlay::new();
    let shared_overlay: Arc<Option<overlay::Overlay>> = Arc::new(rec_overlay);

    // ── Paste guard ──────────────────────────────────────────────────────────
    // Shared AtomicBool that suppresses hotkey events during (and briefly
    // after) paste operations. SetForegroundWindow generates ghost Alt
    // keypresses that the 500ms cooldown can't catch because the cooldown
    // starts at RecordStop, long before the async transcription finishes.
    let paste_guard = Arc::new(AtomicBool::new(false));

    // ── Hotkey polling thread ─────────────────────────────────────────────────
    hotkey::start_polling(config.record_key.clone(), hotkey_tx, Arc::clone(&paste_guard));

    // ── Pipeline thread ───────────────────────────────────────────────────────
    {
        let _config = config.clone(); // retained for potential future use
        let flags = Arc::clone(&flags);
        let event_tx = event_tx.clone();
        let cmd_rx = cmd_rx;
        let pipeline_overlay = Arc::clone(&shared_overlay);
        let server_ready = Arc::clone(&server_ready);
        let paste_guard = Arc::clone(&paste_guard);
        std::thread::Builder::new()
            .name("phonix-pipeline".into())
            .spawn(move || {
                let rt = Runtime::new().expect("tokio runtime");
                let http_client = reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(120))
                    .pool_max_idle_per_host(2)
                    .build()
                    .expect("http client");
                let mut recorder = AudioRecorder::new();
                let mut sample_rate = 44100u32;
                let mut recording = false;
                let mut target_hwnd: u64 = 0;
                let mut pre_roll_len: usize = 0;
                let mut long_dictate_at_start = false;
                // Cooldown: ignore RecordStart within 2s of last completed
                // transcription+paste. Prevents ghost double-recordings.
                // Stores epoch millis of last completion (0 = never).
                let last_done = Arc::new(AtomicU64::new(0));

                // Helper: set overlay state from the pipeline thread
                let set_overlay = |state: u8| {
                    if let Some(ref ov) = *pipeline_overlay {
                        ov.set_state(state);
                    }
                };

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
                                           flags: &Arc<Mutex<SharedFlags>>,
                                           overlay: &Arc<Option<overlay::Overlay>>,
                                           client: &reqwest::Client,
                                           paste_guard: &Arc<AtomicBool>,
                                           last_done: &Arc<AtomicU64>| {
                    let tx = event_tx.clone();
                    let flags = Arc::clone(flags);
                    let ov = Arc::clone(overlay);
                    let paste_guard = Arc::clone(paste_guard);
                    let last_done = Arc::clone(last_done);
                    let prl = pre_roll_len;
                    let hwnd = target_hwnd;
                    let for_ld = long_dictate;
                    let cfg = Config::load();
                    let client = client.clone();

                    rt.spawn(async move {
                        let hide_overlay = || {
                            if let Some(ref o) = *ov {
                                o.set_state(overlay::STATE_HIDDEN);
                            }
                        };
                        let ready_msg = format!(
                            "Ready \u{2014} hold {} to dictate",
                            hotkey::format_hotkey_display(&cfg.record_key),
                        );

                        // Guard: ignore clips where actual speech is shorter than 0.5s
                        let speech_samples = samples.len().saturating_sub(prl);
                        if speech_samples < (sample_rate / 2) as usize {
                            hide_overlay();
                            let _ = tx.try_send(AppEvent::StatusUpdate(
                                "Too short \u{2014} try again".into(),
                            ));
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            let _ = tx.try_send(AppEvent::StatusUpdate(ready_msg));
                            return;
                        }

                        let _ = tx.try_send(AppEvent::StatusUpdate("Transcribing\u{2026}".into()));

                        let raw = match whisper::transcribe(samples, sample_rate, &cfg, &client).await {
                            Ok(r) => r,
                            Err(e) => {
                                hide_overlay();
                                let _ = tx.try_send(AppEvent::Error(e.to_string()));
                                return;
                            }
                        };

                        if raw.is_empty() {
                            hide_overlay();
                            let _ = tx.try_send(AppEvent::StatusUpdate("No speech detected".into()));
                            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            let _ = tx.try_send(AppEvent::StatusUpdate(ready_msg));
                            return;
                        }

                        let text = if cfg.cleanup_enabled {
                            if let Some(ref o) = *ov {
                                o.set_state(overlay::STATE_CLEANING);
                            }
                            let _ = tx.try_send(AppEvent::StatusUpdate("Cleaning up\u{2026}".into()));
                            let result = cleanup::cleanup(&raw, &cfg, &client).await;
                            if let Some(warning) = result.warning {
                                let _ = tx.try_send(AppEvent::StatusUpdate(warning));
                                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                            }
                            result.text
                        } else {
                            raw.clone()
                        };

                        dbg_log(&format!("raw={:?}", &raw));
                        dbg_log(&format!("cleaned={:?}", &text));

                        // Auto-paste unless in long dictate mode
                        let do_paste = {
                            let f = flags.lock().unwrap();
                            f.auto_paste
                        } && !for_ld;

                        if do_paste {
                            // Activate paste guard BEFORE pasting — tells the hotkey
                            // thread to suppress any ghost keypresses triggered by
                            // SetForegroundWindow / SendInput.
                            paste_guard.store(true, Ordering::Release);
                            {
                                let mut f = flags.lock().unwrap();
                                f.paste_in_progress = true;
                            }
                            dbg_log(&format!("paste: starting, text_len={}, text={:?}", text.len(), &text));
                            if let Err(e) = paste::paste(&text, hwnd) {
                                dbg_log(&format!("paste: error {e}"));
                            }
                            // Brief delay after paste so ghost key-up events from
                            // SendInput settle before we re-enable hotkey detection.
                            tokio::time::sleep(std::time::Duration::from_millis(300)).await;
                            {
                                let mut f = flags.lock().unwrap();
                                f.paste_in_progress = false;
                            }
                            paste_guard.store(false, Ordering::Release);
                            dbg_log("paste: done, guard cleared");
                        }

                        hide_overlay();
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                        last_done.store(now_ms, Ordering::Release);
                        dbg_log(&format!("transcription complete, last_done={now_ms}"));
                        let _ = tx.try_send(AppEvent::Transcribed { text, raw, for_long_dictate: for_ld });
                    });
                };

                loop {
                    // Drain hotkey events
                    while let Ok(ev) = hotkey_rx.try_recv() {
                        match ev {
                            hotkey::HotkeyEvent::RecordStart { target_hwnd: hwnd } if !recording => {
                                dbg_log(&format!("RecordStart hwnd={hwnd}"));
                                // Block recording while local server is still loading
                                if !server_ready.load(Ordering::Relaxed) {
                                    dbg_log("  blocked: server not ready");
                                    let _ = event_tx.try_send(AppEvent::StatusUpdate(
                                        "Server still loading, please wait...".into(),
                                    ));
                                    continue;
                                }
                                // Block recording while paste is in progress (prevents
                                // ghost triggers from SetForegroundWindow synthetic Alt events)
                                if flags.lock().unwrap().paste_in_progress {
                                    dbg_log("  blocked: paste_in_progress");
                                    continue;
                                }
                                // Ignore recordings within 2s of last completed transcription.
                                // Catches ghost double-fires that bypass other guards.
                                {
                                    let done_ms = last_done.load(Ordering::Relaxed);
                                    if done_ms > 0 {
                                        let now_ms = std::time::SystemTime::now()
                                            .duration_since(std::time::UNIX_EPOCH).unwrap().as_millis() as u64;
                                        let elapsed = now_ms.saturating_sub(done_ms);
                                        if elapsed < 2000 {
                                            dbg_log(&format!("  blocked: {elapsed}ms since last transcription"));
                                            continue;
                                        }
                                    }
                                }
                                dbg_log("  -> recording started");
                                // Auto-reconnect if the audio stream died
                                if let Some(sr) = recorder.ensure_stream() {
                                    sample_rate = sr;
                                }
                                recording = true;
                                target_hwnd = hwnd;
                                pre_roll_len = recorder.start();
                                long_dictate_at_start = flags.lock().unwrap().long_dictate_active;
                                set_overlay(overlay::STATE_RECORDING);
                                sound::play_start_with_preset(&Config::load().sound_preset);
                                let _ = event_tx.try_send(AppEvent::RecordingStarted);
                            }
                            hotkey::HotkeyEvent::RecordStop if recording => {
                                dbg_log(&format!("RecordStop (was recording, samples pending)"));
                                recording = false;
                                let samples = recorder.stop();
                                dbg_log(&format!("  samples={} pre_roll={pre_roll_len}", samples.len()));
                                set_overlay(overlay::STATE_TRANSCRIBING);
                                sound::play_stop_with_preset(&Config::load().sound_preset);
                                let _ = event_tx.try_send(AppEvent::RecordingStopped);
                                spawn_transcription(
                                    &rt, samples, sample_rate, pre_roll_len,
                                    target_hwnd, long_dictate_at_start,
                                    &event_tx, &flags, &pipeline_overlay, &http_client,
                                    &paste_guard, &last_done,
                                );
                            }
                            _ => {}
                        }
                    }

                    // Drain UI commands (Long Dictate Start/Stop button)
                    while let Ok(cmd) = cmd_rx.try_recv() {
                        match cmd {
                            PipelineCmd::StartRecording if !recording => {
                                if !server_ready.load(Ordering::Relaxed) {
                                    let _ = event_tx.try_send(AppEvent::StatusUpdate(
                                        "Server still loading, please wait...".into(),
                                    ));
                                    continue;
                                }
                                if let Some(sr) = recorder.ensure_stream() {
                                    sample_rate = sr;
                                }
                                recording = true;
                                target_hwnd = 0; // Long Dictate never pastes
                                pre_roll_len = recorder.start();
                                long_dictate_at_start = true;
                                set_overlay(overlay::STATE_RECORDING);
                                sound::play_start_with_preset(&Config::load().sound_preset);
                                let _ = event_tx.try_send(AppEvent::RecordingStarted);
                            }
                            PipelineCmd::StopRecording if recording => {
                                recording = false;
                                let samples = recorder.stop();
                                set_overlay(overlay::STATE_TRANSCRIBING);
                                sound::play_stop_with_preset(&Config::load().sound_preset);
                                let _ = event_tx.try_send(AppEvent::RecordingStopped);
                                spawn_transcription(
                                    &rt, samples, sample_rate, pre_roll_len,
                                    target_hwnd, long_dictate_at_start,
                                    &event_tx, &flags, &pipeline_overlay, &http_client,
                                    &paste_guard, &last_done,
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

    // ── System tray ───────────────────────────────────────────────────────────
    let (tray, tray_menu_ids) = build_tray();

    // ── egui window ───────────────────────────────────────────────────────────
    let store_for_app = Arc::clone(&store);
    let flags_for_app = Arc::clone(&flags);
    let config_for_app = config.clone();
    let overlay_for_app = Arc::clone(&shared_overlay);
    let event_tx_for_app = event_tx.clone();

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
                event_tx_for_app,
                cmd_tx,
                tray,
                tray_menu_ids,
                overlay_for_app,
            )))
        }),
    )
}

// ── Local server management ───────────────────────────────────────────────────

fn maybe_start_local_server(
    config: &Config,
    event_tx: &crossbeam_channel::Sender<AppEvent>,
    server_ready: &Arc<AtomicBool>,
) -> Option<Arc<Mutex<server::WhisperServer>>> {
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

    // Brief pause to let the OS release CUDA contexts and free port 8080
    // after killing stale processes.
    std::thread::sleep(std::time::Duration::from_millis(500));

    let mut srv = server::WhisperServer::new();
    let model_arg = config.local_model_size.arg();
    if let Err(e) = srv.start(&server_py, Some(model_arg)) {
        let _ = event_tx.try_send(AppEvent::Error(e));
        return None;
    }

    // Wrap server in Arc<Mutex> so the health-poll thread can check for early exit
    let srv = Arc::new(Mutex::new(srv));

    // Health-poll in background — updates status when ready
    let tx = event_tx.clone();
    let srv_poll = Arc::clone(&srv);
    let model_label = config.local_model_size.arg().to_string();
    let ready_msg = format!(
        "Ready \u{2014} hold {} to dictate",
        hotkey::format_hotkey_display(&config.record_key),
    );
    let ready_flag = Arc::clone(server_ready);
    std::thread::spawn(move || {
        let start = std::time::Instant::now();
        let mut last_status_secs = 0u64;
        let mut warned_slow = false;
        loop {
            // Check if the server process crashed
            if let Ok(mut s) = srv_poll.lock() {
                if let Some(err) = s.check_early_exit() {
                    let _ = tx.try_send(AppEvent::Error(err));
                    return;
                }
            }
            if server::is_server_ready_public() {
                ready_flag.store(true, Ordering::Relaxed);
                let _ = tx.try_send(AppEvent::StatusUpdate(ready_msg));
                return;
            }
            let elapsed = start.elapsed().as_secs();
            // After 120s, show a warning but keep trying (model loading can be slow)
            if elapsed > 120 && !warned_slow {
                warned_slow = true;
                let _ = tx.try_send(AppEvent::StatusUpdate(
                    format!("Still loading Whisper ({model_label})... this may take a few minutes on first run"),
                ));
            }
            // Update status every 5 seconds so user knows it's still loading
            if elapsed >= 5 && elapsed / 5 != last_status_secs / 5 {
                last_status_secs = elapsed;
                if !warned_slow {
                    let _ = tx.try_send(AppEvent::StatusUpdate(
                        format!("Loading Whisper ({model_label})... {elapsed}s"),
                    ));
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(400));
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
