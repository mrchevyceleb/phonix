use std::sync::{Arc, Mutex};
use std::time::Instant;

use crossbeam_channel::Sender;
use egui::{Align, Color32, Layout, RichText, ScrollArea, TextEdit, Vec2};

use crate::store::{Entry, Store};
use crate::config::Config;
use crate::hotkey::{SUPPORTED_KEYS, KEY_GROUPS};

// â”€â”€ Theme â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

struct Theme;

impl Theme {
    const BG_BASE:        Color32 = Color32::from_rgb(18, 18, 25);
    const BG_PANEL:       Color32 = Color32::from_rgb(22, 22, 30);
    const BG_CARD:        Color32 = Color32::from_rgb(28, 28, 38);
    const BG_ELEVATED:    Color32 = Color32::from_rgb(35, 35, 48);
    const BG_INPUT:       Color32 = Color32::from_rgb(20, 20, 28);
    const BORDER:         Color32 = Color32::from_rgb(45, 45, 60);
    const BORDER_SUBTLE:  Color32 = Color32::from_rgb(50, 50, 65);
    const TEXT_PRIMARY:    Color32 = Color32::from_rgb(220, 220, 235);
    const TEXT_SECONDARY:  Color32 = Color32::from_rgb(130, 130, 155);
    const TEXT_MUTED:      Color32 = Color32::from_rgb(80, 80, 100);
    const TEXT_HEADING:    Color32 = Color32::from_rgb(200, 200, 220);
    const ACCENT:         Color32 = Color32::from_rgb(100, 140, 255);
    const SUCCESS:        Color32 = Color32::from_rgb(80, 200, 120);
    const DANGER:         Color32 = Color32::from_rgb(255, 80, 80);
}

// â”€â”€ Events flowing FROM pipeline TO UI â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub enum AppEvent {
    RecordingStarted,
    RecordingStopped,
    Transcribed { text: String, raw: String, for_long_dictate: bool },
    StatusUpdate(String),
    Error(String),
    UpdateAvailable { version: String, url: String },
}

// ── Commands flowing FROM UI TO pipeline ─────────────────────────────────────

pub enum PipelineCmd {
    StartRecording,
    StopRecording,
}

//â”€â”€ Shared state pipeline needs to read from UI â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

pub struct SharedFlags {
    pub long_dictate_active: bool,
    pub auto_paste: bool,
}

// â”€â”€ App â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
    // Channel to send commands to the pipeline (e.g. Start/Stop from Long Dictate)
    cmd_tx: Sender<PipelineCmd>,

    // System tray (lives on the main thread)
    tray: Option<tray_icon::TrayIcon>,

    // Native always-on-top recording overlay
    overlay: Option<crate::overlay::Overlay>,

    // Tray menu item IDs for matching MenuEvent clicks
    tray_menu_ids: Option<crate::TrayMenuIds>,

    // Track the record key at startup so we can show a restart warning on change
    startup_record_key: String,

    // Request focus on the Long Dictate text area after clicking Start
    focus_long_dictate_text: bool,

    // Update notification from GitHub releases: (version, url)
    update_info: Option<(String, String)>,
    update_dismissed: bool,

    // True when user explicitly clicked Quit in tray menu; bypasses close-to-tray
    force_quit: bool,
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
        cmd_tx: Sender<PipelineCmd>,
        tray: Option<tray_icon::TrayIcon>,
        tray_menu_ids: Option<crate::TrayMenuIds>,
        overlay: Option<crate::overlay::Overlay>,
    ) -> Self {
        Self::setup_theme(&cc.egui_ctx);

        let startup_record_key = config.record_key.clone();
        Self {
            store,
            config,
            flags,
            status: "Ready â€” hold key to dictate".to_string(),
            is_recording: false,
            active_tab: Tab::History,
            long_dictate_text: String::new(),
            copy_flash: None,
            settings_saved_flash: None,
            event_rx,
            cmd_tx,
            tray,
            tray_menu_ids,
            overlay,
            startup_record_key,
            force_quit: false,
            focus_long_dictate_text: false,
            update_info: None,
            update_dismissed: false,
        }
    }

    fn setup_theme(ctx: &egui::Context) {
        let mut visuals = egui::Visuals::dark();

        // â”€â”€ Base fills â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        visuals.panel_fill = Theme::BG_PANEL;
        visuals.window_fill = Theme::BG_PANEL;
        visuals.extreme_bg_color = Theme::BG_BASE;
        visuals.faint_bg_color = Theme::BG_CARD;

        // â”€â”€ Widget styles â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        visuals.widgets.noninteractive.bg_fill = Theme::BG_CARD;
        visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, Theme::TEXT_SECONDARY);
        visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(0.5, Theme::BORDER_SUBTLE);
        visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);

        visuals.widgets.inactive.bg_fill = Color32::from_rgb(42, 42, 55);
        visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, Theme::TEXT_PRIMARY);
        visuals.widgets.inactive.bg_stroke = egui::Stroke::new(0.5, Theme::BORDER_SUBTLE);
        visuals.widgets.inactive.rounding = egui::Rounding::same(6.0);

        visuals.widgets.hovered.bg_fill = Color32::from_rgb(38, 38, 50);
        visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, Theme::TEXT_PRIMARY);
        visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, Theme::ACCENT);
        visuals.widgets.hovered.rounding = egui::Rounding::same(6.0);

        visuals.widgets.active.bg_fill = Theme::ACCENT;
        visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, Color32::WHITE);
        visuals.widgets.active.bg_stroke = egui::Stroke::new(0.0, Color32::TRANSPARENT);
        visuals.widgets.active.rounding = egui::Rounding::same(6.0);

        visuals.widgets.open.bg_fill = Color32::from_rgb(42, 42, 55);
        visuals.widgets.open.fg_stroke = egui::Stroke::new(1.0, Theme::TEXT_PRIMARY);
        visuals.widgets.open.rounding = egui::Rounding::same(6.0);

        visuals.selection.bg_fill = Color32::from_rgba_premultiplied(100, 140, 255, 60);
        visuals.selection.stroke = egui::Stroke::new(1.0, Theme::ACCENT);

        // â”€â”€ Window chrome â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        visuals.window_shadow = egui::epaint::Shadow {
            offset: egui::Vec2::new(0.0, 2.0),
            blur: 8.0,
            spread: 0.0,
            color: Color32::from_black_alpha(60),
        };
        visuals.window_rounding = egui::Rounding::same(10.0);

        ctx.set_visuals(visuals);

        // â”€â”€ Typography + spacing â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        let mut style = (*ctx.style()).clone();

        style.text_styles.insert(
            egui::TextStyle::Heading,
            egui::FontId::new(18.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Small,
            egui::FontId::new(11.5, egui::FontFamily::Proportional),
        );
        style.text_styles.insert(
            egui::TextStyle::Button,
            egui::FontId::new(13.0, egui::FontFamily::Proportional),
        );

        style.spacing.item_spacing = egui::Vec2::new(8.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(16.0);
        style.spacing.button_padding = egui::Vec2::new(12.0, 6.0);

        // Slim scroll bars
        style.spacing.scroll.bar_width = 6.0;
        style.spacing.scroll.bar_inner_margin = 2.0;

        ctx.set_style(style);
    }
}

impl eframe::App for PhonixApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Always repaint periodically so pipeline events (recording state,
        // transcription results) are processed even when there's no user input.
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        // â”€â”€ Poll events from pipeline â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                AppEvent::RecordingStarted => {
                    self.is_recording = true;
                    self.status = "Recordingâ€¦".into();
                    self.set_tray_recording(true);
                    if let Some(ref ov) = self.overlay {
                        ov.set_state(crate::overlay::STATE_RECORDING);
                    }
                    if self.config.sound_enabled {
                        crate::sound::play_start();
                    }
                }
                AppEvent::RecordingStopped => {
                    self.is_recording = false;
                    self.status = "Transcribingâ€¦".into();
                    self.set_tray_recording(false);
                    if let Some(ref ov) = self.overlay {
                        ov.set_state(crate::overlay::STATE_TRANSCRIBING);
                    }
                    if self.config.sound_enabled {
                        crate::sound::play_stop();
                    }
                }
                AppEvent::Transcribed { text, raw, for_long_dictate } => {
                    self.status = "Ready â€” hold key to dictate".into();
                    if let Some(ref ov) = self.overlay {
                        ov.set_state(crate::overlay::STATE_HIDDEN);
                    }
                    if for_long_dictate {
                        if !self.long_dictate_text.is_empty() {
                            self.long_dictate_text.push(' ');
                        }
                        self.long_dictate_text.push_str(&text);
                        self.active_tab = Tab::LongDictate;
                    }
                    self.store.lock().unwrap().push(Entry::new(text, raw));
                }
                AppEvent::StatusUpdate(ref s) => {
                    if let Some(ref ov) = self.overlay {
                        if s.contains("Cleaning") {
                            ov.set_state(crate::overlay::STATE_CLEANING);
                        } else if s.contains("Transcribing") {
                            ov.set_state(crate::overlay::STATE_TRANSCRIBING);
                        } else if s.contains("Ready") {
                            ov.set_state(crate::overlay::STATE_HIDDEN);
                        }
                    }
                    self.status = s.clone();
                }
                AppEvent::Error(e) => {
                    if let Some(ref ov) = self.overlay {
                        ov.set_state(crate::overlay::STATE_HIDDEN);
                    }
                    self.status = format!("Error: {e}");
                }
                AppEvent::UpdateAvailable { version, url } => {
                    self.update_info = Some((version, url));
                }
            }
        }

        // â”€â”€ Poll tray events â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        while let Ok(event) = tray_icon::TrayIconEvent::receiver().try_recv() {
            if matches!(event, tray_icon::TrayIconEvent::Click { .. }) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }

        // â”€â”€ Poll tray menu events (right-click menu) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        while let Ok(event) = tray_icon::menu::MenuEvent::receiver().try_recv() {
            if let Some(ref ids) = self.tray_menu_ids {
                if event.id == ids.open {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
                } else if event.id == ids.quit {
                    self.force_quit = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            }
        }

        // Intercept window close â†’ hide to tray instead (if enabled)
        if ctx.input(|i| i.viewport().close_requested()) && self.config.close_to_tray && !self.force_quit {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
        }

        // Keep repainting while recording so the status dot animates
        if self.is_recording {
            ctx.request_repaint_after(std::time::Duration::from_millis(50));
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

        // â”€â”€ Render â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(Theme::BG_BASE)
                    .inner_margin(egui::Margin::symmetric(16.0, 12.0)),
            )
            .show(ctx, |ui| {
                self.render_update_banner(ui);
                self.render_header(ui);
                ui.add_space(8.0);
                self.render_tabs(ui);
                ui.add_space(8.0);

                match self.active_tab {
                    Tab::History => self.render_history(ui),
                    Tab::LongDictate => self.render_long_dictate(ui),
                    Tab::Settings => self.render_settings(ui),
                }
            });
    }
}

impl PhonixApp {
    // â”€â”€ Header â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn render_update_banner(&mut self, ui: &mut egui::Ui) {
        if self.update_dismissed {
            return;
        }
        let (version, url) = match &self.update_info {
            Some((v, u)) => (v.clone(), u.clone()),
            None => return,
        };

        egui::Frame::none()
            .fill(Color32::from_rgb(25, 35, 55))
            .rounding(egui::Rounding::same(8.0))
            .stroke(egui::Stroke::new(1.0, Theme::ACCENT))
            .inner_margin(egui::Margin::symmetric(12.0, 8.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("Update available: v{}", version))
                            .size(13.0)
                            .color(Theme::ACCENT),
                    );

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Dismiss")
                                        .size(11.5)
                                        .color(Theme::TEXT_SECONDARY),
                                )
                                .fill(Color32::TRANSPARENT)
                                .rounding(egui::Rounding::same(4.0)),
                            )
                            .clicked()
                        {
                            self.update_dismissed = true;
                        }

                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new("Download")
                                        .size(12.0)
                                        .color(Color32::WHITE),
                                )
                                .fill(Theme::ACCENT)
                                .rounding(egui::Rounding::same(4.0)),
                            )
                            .clicked()
                        {
                            crate::update::open_in_browser(&url);
                        }
                    });
                });
            });
        ui.add_space(6.0);
    }

    fn render_header(&mut self, ui: &mut egui::Ui) {
        let header_bg = if self.is_recording {
            Color32::from_rgba_premultiplied(255, 40, 40, 20)
        } else {
            Theme::BG_PANEL
        };

        egui::Frame::none()
            .fill(header_bg)
            .inner_margin(egui::Margin::symmetric(16.0, 12.0))
            .rounding(egui::Rounding::same(10.0))
            .stroke(egui::Stroke::new(0.5, Theme::BORDER))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Animated status indicator
                    let (dot_color, dot_size) = if self.is_recording {
                        let t = ui.input(|i| i.time);
                        let pulse = (t * 3.0).sin() as f32 * 0.3 + 0.7;
                        (
                            Color32::from_rgb(
                                (255.0 * pulse) as u8,
                                (70.0 * pulse) as u8,
                                (70.0 * pulse) as u8,
                            ),
                            10.0,
                        )
                    } else {
                        (Theme::SUCCESS, 8.0)
                    };

                    let (rect, _) = ui.allocate_exact_size(
                        egui::Vec2::splat(dot_size),
                        egui::Sense::hover(),
                    );
                    ui.painter()
                        .circle_filled(rect.center(), dot_size / 2.0, dot_color);

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(&self.status)
                            .size(14.0)
                            .color(Theme::TEXT_PRIMARY)
                            .strong(),
                    );
                });
            });
    }

    // â”€â”€ Tabs â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn render_tabs(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;

            for (tab, label) in [
                (Tab::History, "  History  "),
                (Tab::LongDictate, "  Long Dictate  "),
                (Tab::Settings, "  Settings  "),
            ] {
                let is_active = self.active_tab == tab;
                let text_color = if is_active {
                    Theme::TEXT_PRIMARY
                } else {
                    Theme::TEXT_SECONDARY
                };

                let btn = egui::Button::new(
                    RichText::new(label).color(text_color).size(13.0),
                )
                .fill(if is_active {
                    Color32::from_rgba_premultiplied(100, 140, 255, 35)
                } else {
                    Color32::TRANSPARENT
                })
                .stroke(if is_active {
                    egui::Stroke::new(1.0, Color32::from_rgba_premultiplied(100, 140, 255, 90))
                } else {
                    egui::Stroke::NONE
                })
                .rounding(egui::Rounding::same(6.0));

                if ui.add(btn).clicked() {
                    self.active_tab = tab;
                }
            }
        });
    }

    // â”€â”€ History â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn render_history(&mut self, ui: &mut egui::Ui) {
        let entries = self.store.lock().unwrap().entries.clone();

        if entries.is_empty() {
            ui.add_space(80.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new("ðŸŽ™")
                        .size(40.0)
                        .color(Color32::from_rgb(60, 60, 80)),
                );
                ui.add_space(12.0);
                ui.label(
                    RichText::new("No recordings yet")
                        .size(16.0)
                        .color(Color32::from_rgb(120, 120, 145)),
                );
                ui.add_space(4.0);
                ui.label(
                    RichText::new("Hold your record key and speak")
                        .size(13.0)
                        .color(Theme::TEXT_MUTED),
                );
            });
            return;
        }

        // Toolbar
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} recordings", entries.len()))
                    .size(12.0)
                    .color(Theme::TEXT_SECONDARY),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let clear_btn = egui::Button::new(
                    RichText::new("Clear all")
                        .size(12.0)
                        .color(Theme::TEXT_SECONDARY),
                )
                .fill(Color32::TRANSPARENT)
                .stroke(egui::Stroke::NONE);
                if ui.add(clear_btn).clicked() {
                    self.store.lock().unwrap().clear();
                }
            });
        });
        ui.add_space(6.0);

        let mut to_delete: Option<String> = None;

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for entry in &entries {
                    egui::Frame::none()
                        .fill(Theme::BG_CARD)
                        .rounding(egui::Rounding::same(10.0))
                        .stroke(egui::Stroke::new(0.5, Theme::BORDER))
                        .inner_margin(egui::Margin::symmetric(14.0, 12.0))
                        .show(ui, |ui| {
                            // Header row: timestamp + action buttons
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(
                                        entry
                                            .timestamp
                                            .format("%b %d  %H:%M")
                                            .to_string(),
                                    )
                                    .size(11.5)
                                    .color(Theme::TEXT_SECONDARY),
                                );
                                ui.with_layout(
                                    Layout::right_to_left(Align::Center),
                                    |ui| {
                                        // Delete button
                                        let del_resp = ui.allocate_ui(Vec2::new(24.0, 24.0), |ui| {
                                            let (rect, resp) = ui.allocate_exact_size(
                                                Vec2::splat(20.0),
                                                egui::Sense::click(),
                                            );
                                            let hovered = resp.hovered();
                                            let color = if hovered {
                                                Theme::DANGER
                                            } else {
                                                Theme::TEXT_MUTED
                                            };
                                            if hovered {
                                                ui.painter().rect_filled(
                                                    rect,
                                                    egui::Rounding::same(4.0),
                                                    Color32::from_rgba_premultiplied(255, 80, 80, 20),
                                                );
                                            }
                                            // Trash can icon (bin outline)
                                            let c = rect.center();
                                            let p = ui.painter();
                                            let s = egui::Stroke::new(1.4, color);
                                            // Lid
                                            p.line_segment([c + egui::vec2(-5.0, -4.0), c + egui::vec2(5.0, -4.0)], s);
                                            // Handle
                                            p.line_segment([c + egui::vec2(-2.0, -6.0), c + egui::vec2(2.0, -6.0)], s);
                                            p.line_segment([c + egui::vec2(-2.0, -6.0), c + egui::vec2(-2.0, -4.0)], s);
                                            p.line_segment([c + egui::vec2(2.0, -6.0), c + egui::vec2(2.0, -4.0)], s);
                                            // Body
                                            p.line_segment([c + egui::vec2(-4.0, -3.0), c + egui::vec2(-3.0, 6.0)], s);
                                            p.line_segment([c + egui::vec2(4.0, -3.0), c + egui::vec2(3.0, 6.0)], s);
                                            p.line_segment([c + egui::vec2(-3.0, 6.0), c + egui::vec2(3.0, 6.0)], s);
                                            // Inner lines
                                            let thin = egui::Stroke::new(1.0, color);
                                            p.line_segment([c + egui::vec2(-1.0, -2.0), c + egui::vec2(-1.0, 4.5)], thin);
                                            p.line_segment([c + egui::vec2(1.0, -2.0), c + egui::vec2(1.0, 4.5)], thin);
                                            resp
                                        });
                                        if del_resp.inner
                                            .on_hover_text("Delete")
                                            .clicked()
                                        {
                                            to_delete = Some(entry.id.clone());
                                        }

                                        // Copy button
                                        let flashing = self
                                            .copy_flash
                                            .as_ref()
                                            .map(|(id, _)| id == &entry.id)
                                            .unwrap_or(false);

                                        let (copy_label, copy_color) = if flashing {
                                            ("âœ“ Copied", Theme::SUCCESS)
                                        } else {
                                            ("Copy", Theme::ACCENT)
                                        };

                                        let copy_btn = egui::Button::new(
                                            RichText::new(copy_label)
                                                .size(12.0)
                                                .color(copy_color),
                                        )
                                        .fill(Color32::TRANSPARENT)
                                        .stroke(egui::Stroke::NONE);

                                        if ui.add(copy_btn).clicked() {
                                            if let Ok(mut cb) = arboard::Clipboard::new()
                                            {
                                                let _ =
                                                    cb.set_text(entry.text.clone());
                                                self.copy_flash = Some((
                                                    entry.id.clone(),
                                                    Instant::now(),
                                                ));
                                            }
                                        }
                                    },
                                );
                            });

                            ui.add_space(6.0);

                            // Entry text
                            let preview = truncate(&entry.text, 280);
                            ui.label(
                                RichText::new(preview)
                                    .size(13.5)
                                    .color(Color32::from_rgb(210, 210, 225)),
                            );
                        });
                    ui.add_space(6.0);
                }
            });

        if let Some(id) = to_delete {
            self.store.lock().unwrap().remove(&id);
        }
    }

    // â”€â”€ Long Dictate â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn render_long_dictate(&mut self, ui: &mut egui::Ui) {
        ui.add_space(4.0);

        // Control bar
        egui::Frame::none()
            .fill(Color32::from_rgb(25, 25, 35))
            .rounding(egui::Rounding::same(8.0))
            .inner_margin(egui::Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (btn_text, btn_color) = if self.is_recording {
                        ("â¹  Stop", Theme::DANGER)
                    } else {
                        ("ðŸŽ™  Start", Theme::SUCCESS)
                    };

                    let start_btn = egui::Button::new(
                        RichText::new(btn_text)
                            .color(Color32::WHITE)
                            .size(13.0),
                    )
                    .fill(Color32::from_rgb(
                        (btn_color.r() as u16 * 200 / 255) as u8,
                        (btn_color.g() as u16 * 200 / 255) as u8,
                        (btn_color.b() as u16 * 200 / 255) as u8,
                    ))
                    .rounding(egui::Rounding::same(6.0));

                    if ui.add(start_btn).clicked() {
                        if self.is_recording {
                            self.flags.lock().unwrap().long_dictate_active = false;
                            let _ = self.cmd_tx.try_send(PipelineCmd::StopRecording);
                        } else {
                            self.flags.lock().unwrap().long_dictate_active = true;
                            let _ = self.cmd_tx.try_send(PipelineCmd::StartRecording);
                            self.focus_long_dictate_text = true;
                        }
                    }

                    ui.add_space(8.0);

                    let copy_btn = egui::Button::new(
                        RichText::new("ðŸ“‹  Copy All")
                            .size(12.5)
                            .color(Theme::TEXT_SECONDARY),
                    )
                    .fill(Theme::BG_ELEVATED)
                    .rounding(egui::Rounding::same(6.0));
                    if ui.add(copy_btn).clicked() && !self.long_dictate_text.is_empty() {
                        if let Ok(mut cb) = arboard::Clipboard::new() {
                            let _ = cb.set_text(self.long_dictate_text.clone());
                        }
                    }

                    let clear_btn = egui::Button::new(
                        RichText::new("Clear")
                            .size(12.5)
                            .color(Theme::TEXT_SECONDARY),
                    )
                    .fill(Theme::BG_ELEVATED)
                    .rounding(egui::Rounding::same(6.0));
                    if ui.add(clear_btn).clicked() {
                        self.long_dictate_text.clear();
                    }

                    if self.is_recording {
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let t = ui.input(|i| i.time);
                            let pulse = (t * 3.0).sin() as f32 * 0.3 + 0.7;
                            let r = (255.0 * pulse) as u8;
                            let g = (80.0 * pulse) as u8;
                            let b = (80.0 * pulse) as u8;
                            ui.label(
                                RichText::new("â— Live")
                                    .size(12.0)
                                    .color(Color32::from_rgb(r, g, b)),
                            );
                        });
                    }
                });
            });

        ui.add_space(8.0);

        // Text area
        let available = ui.available_size();
        egui::Frame::none()
            .fill(Theme::BG_INPUT)
            .rounding(egui::Rounding::same(8.0))
            .stroke(egui::Stroke::new(0.5, Theme::BORDER))
            .inner_margin(egui::Margin::same(12.0))
            .show(ui, |ui| {
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        let response = ui.add_sized(
                            Vec2::new(
                                (available.x - 28.0).max(100.0),
                                (available.y - 90.0).max(100.0),
                            ),
                            TextEdit::multiline(&mut self.long_dictate_text)
                                .hint_text(
                                    "Click Start to begin recording. \
                                     Text accumulates here â€” copy when done.",
                                )
                                .font(egui::TextStyle::Body)
                                .text_color(Color32::from_rgb(210, 210, 225)),
                        );
                        if self.focus_long_dictate_text {
                            if response.has_focus() {
                                self.focus_long_dictate_text = false;
                            } else {
                                response.request_focus();
                            }
                        }
                    });
            });
    }

    // â”€â”€ Settings â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

    fn render_settings(&mut self, ui: &mut egui::Ui) {
        ScrollArea::vertical().show(ui, |ui| {
            // â”€â”€ Section: Recording â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            ui.add_space(4.0);
            egui::Frame::none()
                .fill(Theme::BG_CARD)
                .rounding(egui::Rounding::same(10.0))
                .stroke(egui::Stroke::new(0.5, Theme::BORDER))
                .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("âš™  Recording")
                            .size(15.0)
                            .strong()
                            .color(Theme::TEXT_HEADING),
                    );
                    ui.add_space(8.0);
                    egui::Grid::new("g_rec")
                        .num_columns(2)
                        .spacing([16.0, 10.0])
                        .show(ui, |ui| {
                            ui.label(
                                RichText::new("Record key")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.vertical(|ui| {
                                for &(group_label, start, end) in KEY_GROUPS {
                                    ui.horizontal(|ui| {
                                        ui.allocate_ui_with_layout(
                                            Vec2::new(55.0, 18.0),
                                            Layout::left_to_right(Align::Center),
                                            |ui| {
                                                ui.label(
                                                    RichText::new(group_label)
                                                        .size(11.0)
                                                        .color(Theme::TEXT_MUTED),
                                                );
                                            },
                                        );
                                        for &(config_name, display_label) in &SUPPORTED_KEYS[start..end] {
                                            let selected = self.config.record_key == config_name;
                                            let text_color = if selected { Color32::WHITE } else { Theme::TEXT_SECONDARY };
                                            let btn = egui::Button::new(
                                                RichText::new(display_label).color(text_color).size(13.0),
                                            )
                                            .fill(if selected { Theme::ACCENT } else { Color32::TRANSPARENT })
                                            .stroke(if selected {
                                                egui::Stroke::new(1.0, Theme::ACCENT)
                                            } else {
                                                egui::Stroke::new(0.5, Theme::BORDER_SUBTLE)
                                            })
                                            .rounding(egui::Rounding::same(6.0));
                                            if ui.add(btn).clicked() {
                                                self.config.record_key = config_name.to_string();
                                            }
                                        }
                                    });
                                }
                            });
                            ui.end_row();

                            ui.label(
                                RichText::new("Auto-paste")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.checkbox(
                                &mut self.config.auto_paste,
                                "Paste into active window on transcription",
                            );
                            ui.end_row();

                            ui.label(
                                RichText::new("Sound effect")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.checkbox(
                                &mut self.config.sound_enabled,
                                "Beep on record start / stop",
                            );
                            ui.end_row();

                            ui.label(
                                RichText::new("Close to tray")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.checkbox(
                                &mut self.config.close_to_tray,
                                "Hide to system tray instead of quitting",
                            );
                            ui.end_row();
                        });
                });

            // â”€â”€ Section: Whisper â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            ui.add_space(8.0);
            egui::Frame::none()
                .fill(Theme::BG_CARD)
                .rounding(egui::Rounding::same(10.0))
                .stroke(egui::Stroke::new(0.5, Theme::BORDER))
                .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("ðŸŽ¤  Whisper  (speech â†’ text)")
                            .size(15.0)
                            .strong()
                            .color(Theme::TEXT_HEADING),
                    );
                    ui.add_space(8.0);

                    egui::Grid::new("g_wh_provider")
                        .num_columns(2)
                        .spacing([16.0, 10.0])
                        .show(ui, |ui| {
                            use crate::config::WhisperProvider;

                            ui.label(
                                RichText::new("Provider")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.horizontal(|ui| {
                                for provider in [
                                    WhisperProvider::Groq,
                                    WhisperProvider::OpenAI,
                                    WhisperProvider::Local,
                                ] {
                                    let selected =
                                        self.config.whisper_provider == provider;
                                    let label = provider.label();
                                    let text_color = if selected { Color32::WHITE } else { Theme::TEXT_SECONDARY };
                                    let btn = egui::Button::new(
                                        RichText::new(label).color(text_color).size(13.0),
                                    )
                                    .fill(if selected {
                                        Theme::ACCENT
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(if selected {
                                        egui::Stroke::new(1.0, Theme::ACCENT)
                                    } else {
                                        egui::Stroke::new(0.5, Theme::BORDER_SUBTLE)
                                    })
                                    .rounding(egui::Rounding::same(6.0));
                                    if ui.add(btn).clicked() {
                                        self.config.whisper_provider = provider;
                                        self.config.whisper_url_override.clear();
                                        self.config.whisper_model_override.clear();
                                    }
                                }
                            });
                            ui.end_row();

                            if self.config.whisper_provider.needs_key() {
                                ui.label(
                                    RichText::new("API Key")
                                        .color(Theme::TEXT_SECONDARY),
                                );
                                ui.add(
                                    TextEdit::singleline(
                                        &mut self.config.whisper_api_key,
                                    )
                                    .password(true)
                                    .hint_text("paste your API key here"),
                                );
                                ui.end_row();
                            }

                            ui.label(
                                RichText::new("Endpoint")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.label(
                                RichText::new(self.config.whisper_url())
                                    .small()
                                    .color(Theme::TEXT_MUTED),
                            );
                            ui.end_row();

                            ui.label(
                                RichText::new("Model").color(Theme::TEXT_SECONDARY),
                            );
                            ui.label(
                                RichText::new(self.config.whisper_model())
                                    .small()
                                    .color(Theme::TEXT_MUTED),
                            );
                            ui.end_row();
                        });

                    ui.add_space(4.0);

                    egui::CollapsingHeader::new(
                        RichText::new("Advanced â€” override URL / model")
                            .small()
                            .color(Theme::TEXT_SECONDARY),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::Grid::new("g_wh_adv")
                            .num_columns(2)
                            .spacing([16.0, 10.0])
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("URL override")
                                        .color(Theme::TEXT_SECONDARY),
                                );
                                ui.add(
                                    TextEdit::singleline(
                                        &mut self.config.whisper_url_override,
                                    )
                                    .hint_text("leave blank to use provider default"),
                                );
                                ui.end_row();

                                ui.label(
                                    RichText::new("Model override")
                                        .color(Theme::TEXT_SECONDARY),
                                );
                                ui.add(
                                    TextEdit::singleline(
                                        &mut self.config.whisper_model_override,
                                    )
                                    .hint_text("leave blank to use provider default"),
                                );
                                ui.end_row();
                            });
                    });
                });

            // â”€â”€ Section: Cleanup â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            ui.add_space(8.0);
            egui::Frame::none()
                .fill(Theme::BG_CARD)
                .rounding(egui::Rounding::same(10.0))
                .stroke(egui::Stroke::new(0.5, Theme::BORDER))
                .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("âœ¨  Cleanup  (text â†’ polished text)")
                            .size(15.0)
                            .strong()
                            .color(Theme::TEXT_HEADING),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(
                            "Removes filler words, fixes sentences. Uses same API key as Whisper when possible.",
                        )
                        .small()
                        .color(Theme::TEXT_MUTED),
                    );
                    ui.add_space(8.0);

                    egui::Grid::new("g_cl")
                        .num_columns(2)
                        .spacing([16.0, 10.0])
                        .show(ui, |ui| {
                            use crate::config::CleanupProvider;

                            ui.label(
                                RichText::new("Enable cleanup")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.checkbox(&mut self.config.cleanup_enabled, "");
                            ui.end_row();

                            ui.label(
                                RichText::new("Provider")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.horizontal(|ui| {
                                for provider in [
                                    CleanupProvider::Groq,
                                    CleanupProvider::OpenAI,
                                    CleanupProvider::Local,
                                ] {
                                    let selected =
                                        self.config.cleanup_provider == provider;
                                    let label = provider.label();
                                    let text_color = if selected { Color32::WHITE } else { Theme::TEXT_SECONDARY };
                                    let btn = egui::Button::new(
                                        RichText::new(label).color(text_color).size(13.0),
                                    )
                                    .fill(if selected {
                                        Theme::ACCENT
                                    } else {
                                        Color32::TRANSPARENT
                                    })
                                    .stroke(if selected {
                                        egui::Stroke::new(1.0, Theme::ACCENT)
                                    } else {
                                        egui::Stroke::new(0.5, Theme::BORDER_SUBTLE)
                                    })
                                    .rounding(egui::Rounding::same(6.0));
                                    if ui.add(btn).clicked() {
                                        self.config.cleanup_provider = provider;
                                        self.config.cleanup_url_override.clear();
                                        self.config.cleanup_model_override.clear();
                                    }
                                }
                            });
                            ui.end_row();

                            if self.config.cleanup_shares_whisper_key() {
                                ui.label(
                                    RichText::new("API Key")
                                        .color(Theme::TEXT_SECONDARY),
                                );
                                ui.label(
                                    RichText::new("Using Whisper API key above")
                                        .small()
                                        .color(Theme::SUCCESS),
                                );
                                ui.end_row();
                            } else if self.config.cleanup_provider
                                != CleanupProvider::Local
                            {
                                ui.label(
                                    RichText::new("API Key")
                                        .color(Theme::TEXT_SECONDARY),
                                );
                                ui.add(
                                    TextEdit::singleline(
                                        &mut self.config.cleanup_api_key,
                                    )
                                    .password(true)
                                    .hint_text("paste API key for this provider"),
                                );
                                ui.end_row();
                            }

                            ui.label(
                                RichText::new("Endpoint")
                                    .color(Theme::TEXT_SECONDARY),
                            );
                            ui.label(
                                RichText::new(self.config.cleanup_url())
                                    .small()
                                    .color(Theme::TEXT_MUTED),
                            );
                            ui.end_row();

                            ui.label(
                                RichText::new("Model").color(Theme::TEXT_SECONDARY),
                            );
                            ui.label(
                                RichText::new(self.config.cleanup_model())
                                    .small()
                                    .color(Theme::TEXT_MUTED),
                            );
                            ui.end_row();
                        });

                    ui.add_space(4.0);

                    egui::CollapsingHeader::new(
                        RichText::new("Advanced â€” override cleanup URL / model")
                            .small()
                            .color(Theme::TEXT_SECONDARY),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        egui::Grid::new("g_cl_adv")
                            .num_columns(2)
                            .spacing([16.0, 10.0])
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("URL override")
                                        .color(Theme::TEXT_SECONDARY),
                                );
                                ui.add(
                                    TextEdit::singleline(
                                        &mut self.config.cleanup_url_override,
                                    )
                                    .hint_text("leave blank to use provider default"),
                                );
                                ui.end_row();

                                ui.label(
                                    RichText::new("Model override")
                                        .color(Theme::TEXT_SECONDARY),
                                );
                                ui.add(
                                    TextEdit::singleline(
                                        &mut self.config.cleanup_model_override,
                                    )
                                    .hint_text("leave blank to use provider default"),
                                );
                                ui.end_row();
                            });
                    });
                });

            // â”€â”€ Save button â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
            ui.add_space(14.0);
            ui.horizontal(|ui| {
                let save_btn = egui::Button::new(
                    RichText::new("  ðŸ’¾  Save  ")
                        .color(Color32::WHITE)
                        .size(14.0),
                )
                .fill(Theme::ACCENT)
                .rounding(egui::Rounding::same(8.0));

                if ui.add(save_btn).clicked() {
                    match self.config.save() {
                        Ok(_) => self.settings_saved_flash = Some(Instant::now()),
                        Err(e) => self.status = format!("Save failed: {e}"),
                    }
                    self.flags.lock().unwrap().auto_paste = self.config.auto_paste;
                }

                if let Some(t) = self.settings_saved_flash {
                    if t.elapsed().as_secs() < 2 {
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new("âœ“ Saved")
                                .color(Theme::SUCCESS)
                                .strong(),
                        );
                    }
                }
            });

            if self.config.record_key != self.startup_record_key {
                ui.add_space(8.0);
                ui.label(
                    RichText::new("âš  Record key changed. Restart the app for it to take effect.")
                        .small()
                        .color(Color32::from_rgb(255, 190, 60)),
                );
            }
        });
    }

    // â”€â”€ Tray icon â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€

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
        format!("{}â€¦", &s[..cut])
    }
}

