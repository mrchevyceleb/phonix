use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam_channel::Sender;
use egui::{Align, Color32, Layout, RichText, ScrollArea, TextEdit, Vec2};

use crate::store::{Entry, Store};
use crate::config::Config;

// ── Events flowing FROM pipeline TO UI ────────────────────────────────────────

pub enum AppEvent {
    RecordingStarted,
    RecordingStopped,
    Transcribed { text: String, raw: String },
    StatusUpdate(String),
    Error(String),
}

// ── Shared state pipeline needs to read from UI ───────────────────────────────

pub struct SharedFlags {
    pub long_dictate_active: bool,
    pub auto_paste: bool,
}

// ── App ───────────────────────────────────────────────────────────────────────

pub struct PhonixApp {
    pub store: Arc<Mutex<Store>>,
    pub config: Config,
    pub flags: Arc<Mutex<SharedFlags>>,

    // UI state
    status: String,
    is_recording: bool,
    active_tab: Tab,
    long_dictate_text: String,
    copy_flash: Option<(String, Instant)>, // entry id that was just copied
    settings_saved_flash: Option<Instant>,

    // Channel to receive events from the pipeline
    event_rx: crossbeam_channel::Receiver<AppEvent>,
    // Channel to send ad-hoc commands (e.g. from settings save)
    #[allow(dead_code)]
    cmd_tx: Sender<()>,

    // System tray (lives on the main thread)
    tray: Option<tray_icon::TrayIcon>,

    // Native always-on-top recording overlay
    overlay: Option<crate::overlay::Overlay>,
}

#[derive(PartialEq)]
enum Tab {
    History,
    LongDictate,
    Settings,
}

impl PhonixApp {
    pub fn new(
        cc: &eframe::CreationContext,
        store: Arc<Mutex<Store>>,
        config: Config,
        flags: Arc<Mutex<SharedFlags>>,
        event_rx: crossbeam_channel::Receiver<AppEvent>,
        cmd_tx: Sender<()>,
        tray: Option<tray_icon::TrayIcon>,
        overlay: Option<crate::overlay::Overlay>,
    ) -> Self {
        // Dark theme
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        Self {
            store,
            config,
            flags,
            status: "Ready — hold key to dictate".to_string(),
            is_recording: false,
            active_tab: Tab::History,
            long_dictate_text: String::new(),
            copy_flash: None,
            settings_saved_flash: None,
            event_rx,
            cmd_tx,
            tray,
            overlay,
        }
    }
}

impl eframe::App for PhonixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Always repaint periodically so pipeline events (recording state,
        // transcription results) are processed even when there's no user input.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        // ── Poll events from pipeline ─────────────────────────────────────────
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AppEvent::RecordingStarted => {
                    self.is_recording = true;
                    self.status = "Recording…".into();
                    self.set_tray_recording(true);
                    if let Some(ref ov) = self.overlay {
                        ov.show();
                    }
                    if self.config.sound_enabled {
                        crate::sound::play_start();
                    }
                }
                AppEvent::RecordingStopped => {
                    self.is_recording = false;
                    self.status = "Transcribing…".into();
                    self.set_tray_recording(false);
                    if let Some(ref ov) = self.overlay {
                        ov.hide();
                    }
                    if self.config.sound_enabled {
                        crate::sound::play_stop();
                    }
                }
                AppEvent::Transcribed { text, raw } => {
                    self.status = "Ready — hold key to dictate".into();
                    // Append to long dictate area if that mode is on
                    if self.flags.lock().unwrap().long_dictate_active {
                        if !self.long_dictate_text.is_empty() {
                            self.long_dictate_text.push(' ');
                        }
                        self.long_dictate_text.push_str(&text);
                        self.active_tab = Tab::LongDictate;
                    }
                    self.store.lock().unwrap().push(Entry::new(text, raw));
                }
                AppEvent::StatusUpdate(s) => self.status = s,
                AppEvent::Error(e) => self.status = format!("Error: {e}"),
            }
        }

        // ── Poll tray events ──────────────────────────────────────────────────
        if let Ok(_event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            // Any click on tray icon → show + focus the window
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }

        // Intercept window close → hide to tray instead
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // Keep repainting while recording so the status dot animates
        if self.is_recording {
            ctx.request_repaint_after(std::time::Duration::from_millis(200));
        }

        // Clear copy flash after 2s
        if let Some((_, t)) = &self.copy_flash {
            if t.elapsed().as_secs() >= 2 {
                self.copy_flash = None;
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            }
        }

        // Clear settings-saved flash after 2s
        if let Some(t) = self.settings_saved_flash {
            if t.elapsed().as_secs() < 2 {
                ctx.request_repaint_after(std::time::Duration::from_millis(200));
            } else {
                self.settings_saved_flash = None;
            }
        }

        // ── Render ────────────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            self.render_header(ui);
            ui.separator();
            self.render_tabs(ui);
            ui.separator();

            match self.active_tab {
                Tab::History => self.render_history(ui),
                Tab::LongDictate => self.render_long_dictate(ui),
                Tab::Settings => self.render_settings(ui),
            }
        });
    }
}

impl PhonixApp {
    // ── Header ────────────────────────────────────────────────────────────────

    fn render_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let (dot_color, dot) = if self.is_recording {
                (Color32::from_rgb(255, 70, 70), "●")
            } else {
                (Color32::from_rgb(80, 200, 100), "●")
            };
            ui.colored_label(dot_color, dot);
            ui.label(RichText::new(&self.status).strong());
        });
    }

    // ── Tabs ──────────────────────────────────────────────────────────────────

    fn render_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut self.active_tab, Tab::History, "History");
            ui.selectable_value(&mut self.active_tab, Tab::LongDictate, "Long Dictate");
            ui.selectable_value(&mut self.active_tab, Tab::Settings, "Settings");
        });
    }

    // ── History ───────────────────────────────────────────────────────────────

    fn render_history(&mut self, ui: &mut egui::Ui) {
        let entries = self.store.lock().unwrap().entries.clone();

        if entries.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("No recordings yet.\n\nHold your record key and speak.")
                        .color(Color32::GRAY),
                );
            });
            return;
        }

        // Toolbar
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} recordings", entries.len()))
                    .small()
                    .color(Color32::GRAY),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Clear all").clicked() {
                    self.store.lock().unwrap().clear();
                }
            });
        });
        ui.add_space(4.0);

        let mut to_delete: Option<String> = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &entries {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        // Row: timestamp + buttons
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(entry.timestamp.format("%b %d  %H:%M").to_string())
                                    .small()
                                    .color(Color32::GRAY),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                // Delete
                                if ui
                                    .small_button(RichText::new("✕").color(Color32::GRAY))
                                    .on_hover_text("Delete")
                                    .clicked()
                                {
                                    to_delete = Some(entry.id.clone());
                                }

                                // Copy
                                let flashing = self
                                    .copy_flash
                                    .as_ref()
                                    .map(|(id, _)| id == &entry.id)
                                    .unwrap_or(false);

                                let copy_label = if flashing { "✓ Copied" } else { "Copy" };
                                if ui.small_button(copy_label).clicked() {
                                    if let Ok(mut cb) = arboard::Clipboard::new() {
                                        let _ = cb.set_text(entry.text.clone());
                                        self.copy_flash =
                                            Some((entry.id.clone(), Instant::now()));
                                    }
                                }
                            });
                        });

                        // Entry text (truncated in list)
                        let preview = truncate(&entry.text, 280);
                        ui.label(preview);
                    });
                    ui.add_space(4.0);
                }
            });

        if let Some(id) = to_delete {
            self.store.lock().unwrap().remove(&id);
        }
    }

    // ── Long Dictate ──────────────────────────────────────────────────────────

    fn render_long_dictate(&mut self, ui: &mut egui::Ui) {
        // Read the flag quickly, don't hold the lock during rendering
        let is_active = self.flags.lock().unwrap().long_dictate_active;

        ui.horizontal(|ui| {
            let (btn_text, btn_color) = if is_active {
                ("⏹  Stop", Color32::from_rgb(255, 80, 80))
            } else {
                ("🎙  Start", Color32::from_rgb(80, 180, 100))
            };

            if ui
                .add(egui::Button::new(RichText::new(btn_text).color(btn_color)))
                .on_hover_text(if is_active {
                    "Stop long dictate mode"
                } else {
                    "Hold your record key to dictate — text accumulates here"
                })
                .clicked()
            {
                self.flags.lock().unwrap().long_dictate_active = !is_active;
            }

            ui.separator();

            if ui.button("📋  Copy All").clicked() && !self.long_dictate_text.is_empty() {
                if let Ok(mut cb) = arboard::Clipboard::new() {
                    let _ = cb.set_text(self.long_dictate_text.clone());
                }
            }

            if ui.button("Clear").clicked() {
                self.long_dictate_text.clear();
            }

            if is_active {
                ui.separator();
                ui.colored_label(Color32::from_rgb(255, 100, 100), "● Live");
            }
        });

        ui.add_space(8.0);

        let available = ui.available_size();
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add_sized(
                    Vec2::new(available.x, (available.y - 20.0).max(100.0)),
                    TextEdit::multiline(&mut self.long_dictate_text)
                        .hint_text(
                            "Hold your record key to speak. \
                             Text accumulates here — copy when done.",
                        )
                        .font(egui::TextStyle::Body),
                );
            });
    }

    // ── Settings ──────────────────────────────────────────────────────────────

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Recording");
            egui::Grid::new("g_rec")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Record key");
                    ui.text_edit_singleline(&mut self.config.record_key);
                    ui.end_row();

                    ui.label("Auto-paste");
                    ui.checkbox(
                        &mut self.config.auto_paste,
                        "Paste into active window on transcription",
                    );
                    ui.end_row();

                    ui.label("Sound effect");
                    ui.checkbox(
                        &mut self.config.sound_enabled,
                        "Beep on record start / stop",
                    );
                    ui.end_row();
                });

            ui.add_space(12.0);
            ui.heading("Whisper  (speech → text)");
            ui.add_space(4.0);

            // Provider picker
            egui::Grid::new("g_wh_provider")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    use crate::config::WhisperProvider;

                    ui.label("Provider");
                    ui.horizontal(|ui| {
                        for provider in [WhisperProvider::Groq, WhisperProvider::OpenAI, WhisperProvider::Local] {
                            let selected = self.config.whisper_provider == provider;
                            let label = provider.label();
                            if ui.selectable_label(selected, label).clicked() {
                                self.config.whisper_provider = provider;
                                // Clear overrides so defaults kick in
                                self.config.whisper_url_override.clear();
                                self.config.whisper_model_override.clear();
                            }
                        }
                    });
                    ui.end_row();

                    // API key (hidden for local)
                    if self.config.whisper_provider.needs_key() {
                        ui.label("API Key");
                        ui.add(
                            TextEdit::singleline(&mut self.config.whisper_api_key)
                                .password(true)
                                .hint_text("paste your API key here"),
                        );
                        ui.end_row();
                    }

                    // Show resolved URL + model as read-only hints
                    ui.label("Endpoint");
                    ui.label(
                        RichText::new(self.config.whisper_url())
                            .small()
                            .color(Color32::GRAY),
                    );
                    ui.end_row();

                    ui.label("Model");
                    ui.label(
                        RichText::new(self.config.whisper_model())
                            .small()
                            .color(Color32::GRAY),
                    );
                    ui.end_row();
                });

            // Advanced overrides (collapsed by default)
            egui::CollapsingHeader::new(
                RichText::new("Advanced — override URL / model").small(),
            )
            .default_open(false)
            .show(ui, |ui| {
                egui::Grid::new("g_wh_adv")
                    .num_columns(2)
                    .spacing([12.0, 8.0])
                    .show(ui, |ui| {
                        ui.label("URL override");
                        ui.add(
                            TextEdit::singleline(&mut self.config.whisper_url_override)
                                .hint_text("leave blank to use provider default"),
                        );
                        ui.end_row();

                        ui.label("Model override");
                        ui.add(
                            TextEdit::singleline(&mut self.config.whisper_model_override)
                                .hint_text("leave blank to use provider default"),
                        );
                        ui.end_row();
                    });
            });

            ui.add_space(12.0);
            ui.heading("Cleanup  (text → polished text)");
            ui.label(
                RichText::new("Uses your local LM Studio model to remove filler words, fix sentences, etc.")
                    .small()
                    .color(Color32::GRAY),
            );
            ui.add_space(4.0);
            egui::Grid::new("g_cl")
                .num_columns(2)
                .spacing([12.0, 8.0])
                .show(ui, |ui| {
                    ui.label("Enable cleanup");
                    ui.checkbox(&mut self.config.cleanup_enabled, "");
                    ui.end_row();

                    ui.label("LM Studio URL");
                    ui.text_edit_singleline(&mut self.config.cleanup_url);
                    ui.end_row();

                    ui.label("API Key");
                    ui.add(
                        TextEdit::singleline(&mut self.config.cleanup_api_key)
                            .password(true)
                            .hint_text("lm-studio"),
                    );
                    ui.end_row();

                    ui.label("Model name");
                    ui.text_edit_singleline(&mut self.config.cleanup_model);
                    ui.end_row();
                });

            ui.add_space(16.0);
            ui.horizontal(|ui| {
                if ui.button("Save").clicked() {
                    match self.config.save() {
                        Ok(_) => self.settings_saved_flash = Some(Instant::now()),
                        Err(e) => self.status = format!("Save failed: {e}"),
                    }
                    // Push updated auto_paste into shared flags
                    self.flags.lock().unwrap().auto_paste = self.config.auto_paste;
                }

                if let Some(t) = self.settings_saved_flash {
                    if t.elapsed().as_secs() < 2 {
                        ui.colored_label(Color32::from_rgb(80, 200, 100), "✓ Saved");
                    }
                }
            });

            ui.add_space(8.0);
            ui.label(
                RichText::new("Record key changes take effect after restart.")
                    .small()
                    .color(Color32::GRAY),
            );
        });
    }

    // ── Tray icon ─────────────────────────────────────────────────────────

    fn set_tray_recording(&self, recording: bool) {
        if let Some(ref tray) = self.tray {
            let icon = if recording {
                crate::make_tray_icon_rgb(255, 70, 70) // red
            } else {
                crate::make_tray_icon_rgb(100, 180, 255) // blue
            };
            let _ = tray.set_icon(Some(icon));
        }
    }

}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        let cut = s.floor_char_boundary(max);
        format!("{}…", &s[..cut])
    }
}
