#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
use frame_record::{native, recorder};
mod source;

use eframe::egui::{
    self, pos2, vec2, Align2, Color32, FontId, Id, Rect, RichText, Sense, Stroke, StrokeKind,
};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use recorder::{AudioDevice, Codec, EncodingOptions, Recording};
use std::{
    path::PathBuf,
    sync::mpsc::{self, Receiver},
    thread,
    time::{Duration, Instant},
};

const BG: Color32 = Color32::from_rgb(24, 24, 24);
const CAPTION: Color32 = Color32::from_rgb(17, 17, 17);
const FIELD: Color32 = Color32::from_rgb(31, 31, 31);
const LINE: Color32 = Color32::from_rgb(48, 48, 48);
const TEXT: Color32 = Color32::from_rgb(204, 204, 204);
const MUTED: Color32 = Color32::from_rgb(157, 157, 157);
const ACCENT: Color32 = Color32::from_rgb(0, 120, 212);
const RED: Color32 = Color32::from_rgb(196, 43, 28);
const GREEN: Color32 = Color32::from_rgb(46, 160, 67);
const AMBER: Color32 = Color32::from_rgb(210, 153, 34);

fn application_icon() -> egui::IconData {
    eframe::icon_data::from_png_bytes(include_bytes!("../assets/app-icon.png"))
        .expect("embedded application icon")
}

struct FrameRecord {
    native: native::NativeFrame,
    app_icon: egui::TextureHandle,
    name: String,
    auto_name: String,
    codec: Codec,
    encoding: EncodingOptions,
    microphone: Option<String>,
    devices: Vec<AudioDevice>,
    devices_rx: Option<Receiver<Result<Vec<AudioDevice>, String>>>,
    ffmpeg: Option<PathBuf>,
    videos: PathBuf,
    recording: Option<Recording>,
    settings: bool,
    error: Option<String>,
    saved_at: Option<Instant>,
    source_saved: bool,
    #[cfg(debug_assertions)]
    smoke: Option<smoke::Smoke>,
}

impl FrameRecord {
    fn new(
        cc: &eframe::CreationContext<'_>,
    ) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        let ctx = &cc.egui_ctx;
        ctx.options_mut(|options| options.zoom_with_keyboard = false);
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = BG;
        visuals.window_fill = BG;
        visuals.extreme_bg_color = CAPTION;
        visuals.faint_bg_color = FIELD;
        visuals.override_text_color = Some(TEXT);
        visuals.selection.bg_fill = ACCENT.gamma_multiply(0.25);
        visuals.selection.stroke = Stroke::new(1.0, ACCENT);
        visuals.widgets.inactive.bg_fill = FIELD;
        visuals.widgets.inactive.weak_bg_fill = FIELD;
        visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, LINE);
        visuals.widgets.hovered.bg_fill = Color32::from_gray(42);
        visuals.widgets.hovered.weak_bg_fill = Color32::from_gray(42);
        visuals.widgets.hovered.bg_stroke = Stroke::new(1.0, MUTED.gamma_multiply(0.5));
        visuals.widgets.active.bg_fill = Color32::from_gray(50);
        visuals.widgets.active.weak_bg_fill = Color32::from_gray(50);
        for widget in [
            &mut visuals.widgets.inactive,
            &mut visuals.widgets.hovered,
            &mut visuals.widgets.active,
        ] {
            widget.corner_radius = 4.into();
        }
        ctx.set_visuals(visuals);
        let icon = application_icon();
        let app_icon = ctx.load_texture(
            "app-icon",
            egui::ColorImage::from_rgba_unmultiplied(
                [icon.width as usize, icon.height as usize],
                &icon.rgba,
            ),
            egui::TextureOptions::LINEAR,
        );
        ctx.global_style_mut(|style| {
            style.spacing.item_spacing = vec2(10.0, 10.0);
            style.spacing.button_padding = vec2(12.0, 8.0);
            style
                .text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(13.0));
            style
                .text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
        });
        let RawWindowHandle::Win32(handle) = cc.window_handle()?.as_raw() else {
            return Err("Windows is required".into());
        };
        let native =
            unsafe { native::NativeFrame::new(handle.hwnd.get() as _, ctx.pixels_per_point()) };
        let (codec, microphone, encoding) = recorder::load_preferences();
        let name = native::default_name();
        let mut app = Self {
            native,
            app_icon,
            name: name.clone(),
            auto_name: name,
            codec,
            encoding,
            microphone,
            devices: Vec::new(),
            devices_rx: None,
            ffmpeg: recorder::find_ffmpeg(),
            videos: native::videos_folder()?,
            recording: None,
            settings: false,
            error: None,
            saved_at: None,
            source_saved: false,
            #[cfg(debug_assertions)]
            smoke: std::env::args()
                .any(|arg| {
                    arg == "--smoke-test" || arg == "--preview" || arg == "--readme-screenshot"
                })
                .then(smoke::Smoke::new),
        };
        app.refresh_devices();
        Ok(app)
    }

    fn refresh_devices(&mut self) {
        if self.devices_rx.is_some() {
            return;
        }
        self.ffmpeg = recorder::find_ffmpeg();
        if let Some(ffmpeg) = self.ffmpeg.clone() {
            let (tx, rx) = mpsc::channel();
            self.devices_rx = Some(rx);
            thread::spawn(move || {
                let _ = tx.send(recorder::list_audio_devices(&ffmpeg));
            });
        }
    }

    fn start(&mut self, ctx: &egui::Context) -> Result<(), String> {
        self.settings = false;
        self.error = None;
        self.native.update(ctx.pixels_per_point(), false, None);
        self.ffmpeg = recorder::find_ffmpeg();
        let ffmpeg = self.ffmpeg.as_ref().ok_or("FFmpeg was not found. Run scripts/setup-ffmpeg.ps1, or put ffmpeg.exe beside Frame Record.")?;
        if let Some(mic) = &self.microphone {
            if self.devices_rx.is_some() {
                return Err("Microphones are still being checked. Try again in a moment.".into());
            }
            if !self.devices.iter().any(|device| &device.id == mic) {
                return Err("The selected microphone is disconnected. Choose another microphone in Settings, or select Off.".into());
            }
        }
        if self.name == self.auto_name {
            // An untouched default name follows the time each recording starts.
            self.auto_name = native::default_name();
            self.name = self.auto_name.clone();
        }
        let output = self.videos.join(recorder::filename(&self.name)?);
        self.recording = Some(Recording::start(
            ffmpeg,
            self.native.validate_region()?,
            self.codec,
            self.encoding,
            self.microphone.as_deref(),
            output,
        )?);
        self.saved_at = None;
        self.native.update(ctx.pixels_per_point(), true, None);
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(
            "Frame Record — Recording".into(),
        ));
        Ok(())
    }

    fn stop(&mut self) {
        if let Some(recording) = &mut self.recording {
            if let Err(error) = recording.stop() {
                self.error = Some(error);
            }
        }
    }

    fn close(&mut self, ctx: &egui::Context) {
        if let Some(recording) = &mut self.recording {
            recording.close_after = true;
            self.stop();
        } else {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }

    fn caption(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let p = ui.painter();
        p.rect_filled(
            Rect::from_min_max(rect.min, pos2(rect.right(), 32.0)),
            0,
            CAPTION,
        );
        p.image(
            self.app_icon.id(),
            Rect::from_min_size(pos2(12.0, 6.0), vec2(20.0, 20.0)),
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        p.text(
            pos2(38.0, 16.0),
            Align2::LEFT_CENTER,
            "Frame Record",
            FontId::proportional(12.5),
            TEXT,
        );
        let mic = if self.microphone.is_some() {
            "Mic on"
        } else {
            "Mic off"
        };
        let level = self
            .recording
            .as_ref()
            .filter(|recording| !recording.stopping)
            .and_then(Recording::mic_level);
        p.text(
            pos2(
                rect.right() - if level.is_some() { 162.0 } else { 96.0 },
                16.0,
            ),
            Align2::RIGHT_CENTER,
            format!("{}  ·  {mic}", self.codec.label()),
            FontId::proportional(11.0),
            MUTED,
        );
        if let Some(db) = level {
            // Ten 6 dB segments from -60 dBFS; the last one lights only when the microphone clips.
            let lit = ((db + 60.0) / 6.0).floor().clamp(0.0, 10.0) as usize;
            for segment in 0..10 {
                let color = match segment {
                    _ if segment >= lit => LINE,
                    9 => RED,
                    7 | 8 => AMBER,
                    _ => GREEN,
                };
                p.rect_filled(
                    Rect::from_min_size(
                        pos2(rect.right() - 154.0 + segment as f32 * 6.0, 11.0),
                        vec2(4.0, 10.0),
                    ),
                    1,
                    color,
                );
            }
        }
        let min_rect = Rect::from_min_size(pos2(rect.right() - 80.0, 3.0), vec2(34.0, 26.0));
        let close_rect = Rect::from_min_size(pos2(rect.right() - 42.0, 3.0), vec2(34.0, 26.0));
        if icon_button(ui, min_rect, "Minimize", Icon::Minimize, false).clicked() {
            ui.ctx()
                .send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
        if icon_button(ui, close_rect, "Close", Icon::Close, false).clicked() {
            self.close(ui.ctx());
        }
    }

    fn toolbar(&mut self, ui: &mut egui::Ui, rect: Rect) {
        let active = self.recording.is_some();
        let saving = self.recording.as_ref().is_some_and(|r| r.stopping);
        let p = ui.painter();
        p.rect_filled(
            Rect::from_min_max(pos2(0.0, 32.0), pos2(rect.right(), native::HEADER)),
            0,
            BG,
        );
        p.line_segment(
            [pos2(0.0, 32.0), pos2(rect.right(), 32.0)],
            Stroke::new(1.0, LINE.gamma_multiply(0.65)),
        );
        let button = Rect::from_min_size(pos2(16.0, 45.0), vec2(108.0, 34.0));
        let label = if saving {
            "Saving…"
        } else if active {
            "Stop"
        } else {
            "Record"
        };
        let response = ui
            .add_enabled_ui(!saving, |ui| {
                ui.put(
                    button,
                    egui::Button::new(RichText::new(label).strong().color(Color32::WHITE))
                        .fill(if active { RED } else { ACCENT })
                        .stroke(Stroke::NONE)
                        .corner_radius(4),
                )
            })
            .inner;
        if response.clicked() {
            if active {
                self.stop();
            } else if let Err(error) = self.start(ui.ctx()) {
                self.error = Some(error);
            }
        }
        let field = Rect::from_min_max(pos2(141.0, 49.0), pos2(rect.right() - 180.0, 80.0));
        ui.painter().text(
            pos2(field.left(), 39.0),
            Align2::LEFT_CENTER,
            "FILENAME",
            FontId::proportional(9.0),
            MUTED,
        );
        ui.painter().rect_filled(field, 6, FIELD);
        ui.painter()
            .rect_stroke(field, 6, Stroke::new(1.0, LINE), StrokeKind::Inside);
        let edit = field.shrink2(vec2(10.0, 5.0));
        ui.add_enabled_ui(!active, |ui| {
            ui.put(
                Rect::from_min_max(edit.min, pos2(edit.right() - 37.0, edit.bottom())),
                egui::TextEdit::singleline(&mut self.name)
                    .frame(egui::Frame::NONE)
                    .font(FontId::proportional(13.0))
                    .char_limit(180)
                    .hint_text("Recording name"),
            );
        });
        ui.painter().text(
            pos2(field.right() - 10.0, field.center().y),
            Align2::RIGHT_CENTER,
            ".mp4",
            FontId::proportional(12.0),
            MUTED,
        );
        let region = self.native.region();
        let status = if let Some(recording) = &self.recording {
            let seconds = recording.started.elapsed().as_secs();
            format!("{:02}:{:02}", seconds / 60, seconds % 60)
        } else if self.saved_at.is_some() {
            "Saved".into()
        } else {
            format!("{} × {}", region.width, region.height)
        };
        ui.painter().text(
            pos2(rect.right() - 70.0, 64.0),
            Align2::RIGHT_CENTER,
            status,
            FontId::proportional(12.0),
            if active { RED } else { MUTED },
        );
        let settings = Rect::from_min_size(pos2(rect.right() - 51.0, 47.0), vec2(33.0, 33.0));
        if ui
            .add_enabled_ui(!active, |ui| {
                icon_button(ui, settings, "Settings", Icon::Settings, self.settings)
            })
            .inner
            .clicked()
        {
            self.settings = !self.settings;
            self.error = None;
        }
    }

    fn settings_panel(&mut self, ctx: &egui::Context, frame: &eframe::Frame) -> Option<Rect> {
        if !self.settings {
            return None;
        }
        let old = (self.codec, self.microphone.clone(), self.encoding);
        let response = egui::Area::new(Id::new("settings"))
            .anchor(Align2::RIGHT_TOP, vec2(-12.0, native::HEADER + 8.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(BG)
                    .stroke(Stroke::new(1.0, LINE))
                    .corner_radius(9)
                    .inner_margin(16)
                    .show(ui, |ui| {
                        ui.set_width(260.0);
                        ui.spacing_mut().item_spacing.y = 6.0;
                        ui.spacing_mut().button_padding = vec2(10.0, 6.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("Settings").size(15.0).strong());
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if ui.small_button("×").clicked() {
                                        self.settings = false;
                                    }
                                },
                            );
                        });
                        egui::ScrollArea::vertical()
                            .id_salt("settings_content")
                            .max_height(
                                (ctx.content_rect().height() - native::HEADER - 150.0).max(60.0),
                            )
                            .show(ui, |ui| {
                                ui.set_width(244.0);
                                ui.spacing_mut().slider_width = 189.0;
                                ui.add_space(7.0);
                                ui.label(RichText::new("VIDEO CODEC").size(10.0).color(MUTED));
                                ui.horizontal(|ui| {
                                    ui.selectable_value(&mut self.codec, Codec::H264, "H.264");
                                    ui.selectable_value(&mut self.codec, Codec::H265, "H.265");
                                });
                                ui.add_space(7.0);
                                ui.horizontal(|ui| {
                                    ui.label(RichText::new("FRAME RATE").size(10.0).color(MUTED));
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.add(
                                                egui::DragValue::new(&mut self.encoding.fps)
                                                    .range(1..=240)
                                                    .suffix(" FPS")
                                                    .speed(1),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(7.0);
                                ui.label(RichText::new("COMPRESSION").size(10.0).color(MUTED));
                                ui.add(egui::Slider::new(&mut self.encoding.compression, 0..=51));
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new("Better quality").size(10.0).color(MUTED),
                                    );
                                    ui.with_layout(
                                        egui::Layout::right_to_left(egui::Align::Center),
                                        |ui| {
                                            ui.label(
                                                RichText::new("Smaller files")
                                                    .size(10.0)
                                                    .color(MUTED),
                                            );
                                        },
                                    );
                                });
                                ui.add_space(7.0);
                                ui.label(RichText::new("MICROPHONE").size(10.0).color(MUTED));
                                ui.radio_value(&mut self.microphone, None, "Off");
                                for device in &self.devices {
                                    ui.radio_value(
                                        &mut self.microphone,
                                        Some(device.id.clone()),
                                        &device.name,
                                    );
                                }
                                if self.devices_rx.is_some() {
                                    ui.label(RichText::new("Checking microphones…").color(MUTED));
                                }
                                if self.devices.is_empty() && self.devices_rx.is_none() {
                                    ui.label(RichText::new("No microphones found").color(MUTED));
                                }
                                if ui
                                    .add_enabled(
                                        self.devices_rx.is_none(),
                                        egui::Button::new("Refresh devices"),
                                    )
                                    .clicked()
                                {
                                    self.refresh_devices();
                                }
                            });
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Videos folder").clicked() {
                                let _ = std::fs::create_dir_all(&self.videos);
                                native::open_folder(&self.videos);
                            }
                            if ui.button("Save source code…").clicked() {
                                self.source_saved = false;
                                if let Some(path) = rfd::FileDialog::new()
                                    .set_parent(frame)
                                    .set_title("Save Frame Record source code")
                                    .set_file_name("frame-record-source.zip")
                                    .add_filter("ZIP archive", &["zip"])
                                    .save_file()
                                {
                                    match source::save_to(&path) {
                                        Ok(()) => self.source_saved = true,
                                        Err(error) => {
                                            self.error = Some(format!(
                                                "Could not save the source ZIP: {error}"
                                            ))
                                        }
                                    }
                                }
                            }
                        });
                        ui.label(
                            RichText::new(if self.source_saved {
                                "Source ZIP saved".into()
                            } else {
                                format!(
                                    "Version {} · Embedded source ZIP · {} KB",
                                    env!("CARGO_PKG_VERSION"),
                                    source::ARCHIVE.len().div_ceil(1024)
                                )
                            })
                            .size(10.0)
                            .color(MUTED),
                        );
                    });
            });
        if old != (self.codec, self.microphone.clone(), self.encoding) {
            recorder::save_preferences(self.codec, self.microphone.as_deref(), self.encoding);
        }
        self.settings.then_some(response.response.rect.expand(1.0))
    }
}

impl eframe::App for FrameRecord {
    fn clear_color(&self, _: &egui::Visuals) -> [f32; 4] {
        [0.0; 4]
    }

    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.devices_rx {
            match rx.try_recv() {
                Ok(result) => {
                    if let Ok(devices) = result {
                        self.devices = devices;
                    }
                    self.devices_rx = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => self.devices_rx = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if ctx.input(|input| input.viewport().close_requested()) && self.recording.is_some() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.close(ctx);
        }
        if let Some(result) = self.recording.as_mut().and_then(Recording::poll) {
            let recording = self.recording.take().unwrap();
            self.native.update(ctx.pixels_per_point(), false, None);
            match result {
                Ok(()) => self.saved_at = Some(Instant::now()),
                Err(error) => self.error = Some(error),
            }
            ctx.send_viewport_cmd(egui::ViewportCommand::Title("Frame Record".into()));
            if recording.close_after {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
        if self.saved_at.is_some_and(|at| at.elapsed().as_secs() >= 4) {
            self.saved_at = None;
        }
        if self.recording.is_some() || self.devices_rx.is_some() || self.saved_at.is_some() {
            // The microphone meter gets a new level about every 50 ms.
            let metering = self
                .recording
                .as_ref()
                .is_some_and(|r| r.mic_level().is_some());
            ctx.request_repaint_after(Duration::from_millis(if metering { 50 } else { 150 }));
        }
        #[cfg(debug_assertions)]
        if let Some(mut smoke) = self.smoke.take() {
            smoke.tick(self, ctx);
            self.smoke = Some(smoke);
        }
    }

    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let rect = ui.max_rect();
        self.caption(ui, rect);
        self.toolbar(ui, rect);
        let color = if self.recording.is_some() {
            RED
        } else {
            ACCENT.gamma_multiply(0.7)
        };
        let p = ui.painter();
        p.line_segment(
            [
                pos2(native::BORDER, native::HEADER - 1.0),
                pos2(rect.right() - native::BORDER, native::HEADER - 1.0),
            ],
            Stroke::new(1.5, color),
        );
        p.rect_stroke(
            Rect::from_min_max(pos2(1.5, native::HEADER - 1.0), rect.max - vec2(1.5, 1.5)),
            0,
            Stroke::new(3.0, color),
            StrokeKind::Inside,
        );
        let ctx = ui.ctx().clone();
        let mut overlay = self.settings_panel(&ctx, frame);
        if let Some(error) = self.error.clone() {
            self.settings = false;
            let response = egui::Area::new(Id::new("error"))
                .anchor(Align2::RIGHT_TOP, vec2(-12.0, native::HEADER + 8.0))
                .order(egui::Order::Foreground)
                .show(&ctx, |ui| {
                    egui::Frame::new()
                        .fill(BG)
                        .stroke(Stroke::new(1.0, RED.gamma_multiply(0.5)))
                        .corner_radius(9)
                        .inner_margin(16)
                        .show(ui, |ui| {
                            ui.set_width(300.0);
                            ui.label(RichText::new("Frame Record").strong().color(RED));
                            ui.label(error);
                            if ui.button("OK").clicked() {
                                self.error = None;
                            }
                        });
                });
            overlay = self
                .error
                .as_ref()
                .map(|_| response.response.rect.expand(1.0));
        }
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.settings = false;
            self.error = None;
            overlay = None;
        }
        self.native
            .update(ctx.pixels_per_point(), self.recording.is_some(), overlay);
    }
}

enum Icon {
    Minimize,
    Close,
    Settings,
}
fn icon_button(
    ui: &mut egui::Ui,
    rect: Rect,
    label: &str,
    icon: Icon,
    selected: bool,
) -> egui::Response {
    let response = ui.interact(rect, Id::new(label), Sense::click());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(egui::WidgetType::Button, ui.is_enabled(), label)
    });
    let p = ui.painter();
    if response.hovered() || selected {
        p.rect_filled(rect, 6, FIELD);
    }
    let c = rect.center();
    let stroke = Stroke::new(
        1.4,
        if response.hovered() || selected {
            TEXT
        } else {
            MUTED
        },
    );
    match icon {
        Icon::Minimize => {
            p.line_segment([c - vec2(5.0, 0.0), c + vec2(5.0, 0.0)], stroke);
        }
        Icon::Close => {
            p.line_segment([c - vec2(4.0, 4.0), c + vec2(4.0, 4.0)], stroke);
            p.line_segment([c + vec2(-4.0, 4.0), c + vec2(4.0, -4.0)], stroke);
        }
        Icon::Settings => {
            p.circle_stroke(c, 5.0, stroke);
            p.circle_stroke(c, 1.6, stroke);
            for n in 0..8 {
                let a = n as f32 * std::f32::consts::TAU / 8.0;
                let v = vec2(a.cos(), a.sin());
                p.line_segment([c + v * 5.0, c + v * 7.5], stroke);
            }
        }
    }
    response.on_hover_text(label)
}

fn main() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Frame Record")
            .with_icon(application_icon())
            .with_inner_size([968.0, 636.0])
            .with_min_inner_size([640.0, 220.0])
            .with_decorations(false)
            .with_transparent(true)
            .with_resizable(true)
            .with_always_on_top()
            .with_has_shadow(false),
        renderer: eframe::Renderer::Glow,
        centered: true,
        ..Default::default()
    };
    eframe::run_native(
        "Frame Record",
        options,
        Box::new(|cc| Ok(Box::new(FrameRecord::new(cc)?))),
    )
}

#[cfg(debug_assertions)]
mod screenshot;
#[cfg(debug_assertions)]
mod smoke;
