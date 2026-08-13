use crate::config::Config;
use crate::ipc;
use crate::plasma;
use crate::store::{ItemMeta, Kind};
use anyhow::{Context, Result};
use eframe::egui::{self, Color32, CornerRadius, Frame, Margin, RichText, Sense, Stroke};
use std::time::{Duration, Instant};

// High-contrast dark palette (option 3).
const BG: Color32 = Color32::from_rgb(0x12, 0x14, 0x17);
const PANEL: Color32 = Color32::from_rgb(0x1a, 0x1d, 0x21);
const TEXT: Color32 = Color32::from_rgb(0xf2, 0xf4, 0xf8);
const MUTED: Color32 = Color32::from_rgb(0x9a, 0xa3, 0xad);
const SELECTED: Color32 = Color32::from_rgb(0x2f, 0x6f, 0xed);
const HOVER: Color32 = Color32::from_rgb(0x2a, 0x30, 0x38);
const INPUT: Color32 = Color32::from_rgb(0x24, 0x28, 0x2e);
const BADGE_TEXT: Color32 = Color32::from_rgb(0x6b, 0x7c, 0x93);
const BADGE_IMG: Color32 = Color32::from_rgb(0x1a, 0x9e, 0x8f);
const AMBER: Color32 = Color32::from_rgb(0xff, 0xb0, 0x20);
const ERROR: Color32 = Color32::from_rgb(0xff, 0x5c, 0x5c);

pub fn run_show() -> Result<()> {
    let _ = ipc::status().context("daemon not reachable — start `clipd daemon` (or enable autostart)")?;
    let status = ipc::status_msg()?;
    let items = ipc::list(status.max_items.max(1)).context("list history")?;
    let cfg = Config::load().unwrap_or_default();
    let size = cfg.clamped_window_size();

    // Wayland ignores client-side with_position — we move via KWin after map.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("clipd")
            .with_inner_size(size)
            .with_min_inner_size([320.0, 240.0])
            .with_always_on_top()
            .with_decorations(true),
        ..Default::default()
    };

    eframe::run_native(
        "clipd",
        options,
        Box::new(move |cc| {
            apply_high_contrast(&cc.egui_ctx);
            Ok(Box::new(PopupApp::new(items, status, size)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    Ok(())
}

fn apply_high_contrast(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    for font_id in style.text_styles.values_mut() {
        font_id.size = (font_id.size * 1.25).round().clamp(13.0, 28.0);
    }
    style.spacing.item_spacing = egui::vec2(8.0, 6.0);
    style.spacing.button_padding = egui::vec2(10.0, 6.0);
    ctx.set_style(style);

    let mut v = egui::Visuals::dark();
    v.window_fill = BG;
    v.panel_fill = PANEL;
    v.extreme_bg_color = Color32::from_rgb(0x0c, 0x0e, 0x10);
    v.faint_bg_color = HOVER;
    v.override_text_color = Some(TEXT);
    v.hyperlink_color = SELECTED;
    v.warn_fg_color = AMBER;
    v.error_fg_color = ERROR;
    v.selection.bg_fill = SELECTED;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.noninteractive.bg_fill = PANEL;
    v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, MUTED);
    v.widgets.inactive.bg_fill = INPUT;
    v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.hovered.bg_fill = HOVER;
    v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
    v.widgets.active.bg_fill = SELECTED;
    v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
    v.widgets.open.bg_fill = HOVER;
    v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
    v.window_stroke = Stroke::new(1.0, Color32::from_rgb(0x3a, 0x40, 0x48));
    ctx.set_visuals(v);
}

struct PopupApp {
    items: Vec<ItemMeta>,
    filter: String,
    selected: usize,
    textures: Vec<Option<egui::TextureHandle>>,
    error: Option<String>,
    close: bool,
    show_settings: bool,
    max_items: usize,
    shortcut: String,
    /// Last values successfully written to the daemon.
    saved_max_items: usize,
    saved_shortcut: String,
    /// When set, show a brief "Saved" toast until this instant.
    saved_toast_until: Option<Instant>,
    pause_remaining_secs: Option<u64>,
    last_status_refresh: Instant,
    started: Instant,
    had_focus: bool,
    place_attempts: u8,
    last_size: [f32; 2],
    size_dirty: bool,
    last_size_save: Instant,
}

impl PopupApp {
    fn new(items: Vec<ItemMeta>, status: ipc::StatusMsg, size: [f32; 2]) -> Self {
        let textures = vec![None; items.len()];
        let max_items = status.max_items.max(1);
        let pause_remaining_secs = status.pause_remaining_secs;
        let shortcut = if status.shortcut.is_empty() {
            Config::load()
                .map(|c| c.shortcut)
                .unwrap_or_else(|_| "Meta+Shift+V".into())
        } else {
            status.shortcut
        };
        Self {
            items,
            filter: String::new(),
            selected: 0,
            textures,
            error: None,
            close: false,
            show_settings: false,
            max_items,
            shortcut: shortcut.clone(),
            saved_max_items: max_items,
            saved_shortcut: shortcut,
            saved_toast_until: None,
            pause_remaining_secs,
            last_status_refresh: Instant::now(),
            started: Instant::now(),
            had_focus: false,
            place_attempts: 0,
            last_size: size,
            size_dirty: false,
            last_size_save: Instant::now(),
        }
    }

    fn persist_window_size(&mut self, force: bool) {
        if !self.size_dirty && !force {
            return;
        }
        if !force && self.last_size_save.elapsed() < Duration::from_millis(400) {
            return;
        }
        if let Err(e) = ipc::set_window_size(self.last_size[0], self.last_size[1]) {
            eprintln!("clipd: save window size: {e:#}");
            return;
        }
        self.size_dirty = false;
        self.last_size_save = Instant::now();
    }

    fn close_settings(&mut self) {
        self.save_settings_if_dirty();
        self.show_settings = false;
    }

    fn settings_dirty(&self) -> bool {
        self.max_items != self.saved_max_items || self.shortcut != self.saved_shortcut
    }

    fn save_settings_if_dirty(&mut self) {
        if !self.settings_dirty() {
            return;
        }
        self.apply_settings();
    }

    /// True when the widget interaction that may have edited a value is finished.
    fn interaction_finished(resp: &egui::Response) -> bool {
        resp.drag_stopped()
            || resp.lost_focus()
            // Wheel / button step without an active drag or text focus.
            || (resp.changed() && !resp.dragged() && !resp.has_focus())
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let q = self.filter.to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                if q.is_empty() {
                    return true;
                }
                it.preview.to_lowercase().contains(&q)
                    || it.mime.to_lowercase().contains(&q)
                    || it.kind.as_str().contains(&q)
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn activate(&mut self, idx: usize) {
        let Some(item) = self.items.get(idx) else {
            return;
        };
        match ipc::select(item.id) {
            Ok(()) => self.close = true,
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn refresh_status(&mut self) {
        if let Ok(s) = ipc::status_msg() {
            self.pause_remaining_secs = s.pause_remaining_secs;
            // Do not overwrite max_items / shortcut while the panel is open —
            // live edits would be stomped every repaint.
            if !self.show_settings {
                self.max_items = s.max_items.max(1);
                if !s.shortcut.is_empty() {
                    self.shortcut = s.shortcut;
                }
            }
        }
        self.last_status_refresh = Instant::now();
    }

    fn apply_settings(&mut self) {
        match ipc::set_config(Some(self.max_items), Some(self.shortcut.clone())) {
            Ok(()) => {
                self.error = None;
                self.saved_max_items = self.max_items;
                self.saved_shortcut = self.shortcut.clone();
                self.saved_toast_until = Some(Instant::now() + Duration::from_millis(1400));
                self.refresh_status();
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }

    fn pause(&mut self, minutes: u32) {
        match ipc::pause(minutes) {
            Ok(()) => {
                self.error = None;
                self.refresh_status();
            }
            Err(e) => self.error = Some(format!("{e:#}")),
        }
    }
}

impl eframe::App for PopupApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.close {
            self.persist_window_size(true);
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Remember resized window size.
        // Wayland: viewport.inner_rect is often None (no global position), so use screen_rect.
        if self.started.elapsed() > Duration::from_millis(350) {
            let size = ctx.input(|i| {
                i.viewport()
                    .inner_rect
                    .map(|r| r.size())
                    .unwrap_or_else(|| i.screen_rect().size())
            });
            let w = size.x.round();
            let h = size.y.round();
            if w >= 320.0
                && h >= 240.0
                && ((w - self.last_size[0]).abs() > 1.0 || (h - self.last_size[1]).abs() > 1.0)
            {
                self.last_size = [w, h];
                self.size_dirty = true;
            }
        }
        self.persist_window_size(false);

        // Close when the popup loses focus (click elsewhere / Alt-Tab).
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused {
            self.had_focus = true;
        } else if self.had_focus || self.started.elapsed() > Duration::from_millis(400) {
            self.persist_window_size(true);
            self.close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Plasma Wayland: ask KWin to move us to the cursor (and apply saved size).
        if self.place_attempts < 3 {
            let due = match self.place_attempts {
                0 => Duration::from_millis(40),
                1 => Duration::from_millis(120),
                _ => Duration::from_millis(250),
            };
            if self.started.elapsed() >= due {
                self.place_attempts += 1;
                // Reinforce size via egui too (helps when compositor accepted the map).
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                    self.last_size[0],
                    self.last_size[1],
                )));
                if let Err(e) = plasma::move_show_to_cursor(Some(self.last_size)) {
                    eprintln!("clipd: place near cursor: {e:#}");
                }
            }
        }

        ctx.request_repaint_after(Duration::from_millis(50));
        if self.last_status_refresh.elapsed() >= Duration::from_secs(1) {
            self.refresh_status();
        }

        // Defer toggle until after the settings panel so DragValue/TextEdit commit first.
        let mut toggle_settings = false;

        egui::TopBottomPanel::top("search")
            .frame(
                Frame::new()
                    .fill(PANEL)
                    .inner_margin(Margin::symmetric(12, 10))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(0x2e, 0x34, 0x3c))),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let edit = egui::TextEdit::singleline(&mut self.filter)
                        .hint_text(RichText::new("Filter…").color(MUTED))
                        .desired_width(ui.available_width() - 100.0)
                        .text_color(TEXT)
                        .background_color(INPUT)
                        .margin(Margin::symmetric(10, 8));
                    let resp = ui.add(edit);
                    if !self.show_settings {
                        resp.request_focus();
                    }
                    if ui
                        .add_sized(
                            [88.0, 32.0],
                            egui::Button::new(RichText::new("Settings").color(TEXT))
                                .fill(if self.show_settings { SELECTED } else { INPUT }),
                        )
                        .clicked()
                    {
                        toggle_settings = true;
                    }
                });
                if let Some(err) = &self.error {
                    ui.add_space(2.0);
                    ui.label(RichText::new(err).color(ERROR).strong());
                }
                if let Some(secs) = self.pause_remaining_secs {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(format!(
                            "Shortcut paused — apps own it for {m}m {s:02}s",
                            m = secs / 60,
                            s = secs % 60
                        ))
                        .color(AMBER)
                        .strong(),
                    );
                }
            });

        if self.show_settings {
            egui::SidePanel::right("settings")
                .resizable(true)
                .default_width(230.0)
                .frame(
                    Frame::new()
                        .fill(PANEL)
                        .inner_margin(Margin::symmetric(12, 10))
                        .stroke(Stroke::new(1.0, Color32::from_rgb(0x2e, 0x34, 0x3c))),
                )
                .show(ctx, |ui| {
                    ui.heading(RichText::new("Settings").color(TEXT));
                    ui.separator();
                    ui.label(RichText::new("Max history items").color(MUTED));
                    let max_resp = ui.add(
                        egui::DragValue::new(&mut self.max_items)
                            .range(1..=50_000)
                            .speed(1.0)
                            .prefix(""),
                    );
                    if Self::interaction_finished(&max_resp) {
                        self.save_settings_if_dirty();
                    }
                    ui.add_space(8.0);
                    ui.label(RichText::new("Plasma shortcut").color(MUTED));
                    ui.label(
                        RichText::new(
                            "Type the chord as text (e.g. Meta+D). Do not press the keys — Plasma steals Meta/global chords before this window sees them.",
                        )
                        .color(MUTED)
                        .small(),
                    );
                    let shortcut_resp = ui.add(
                        egui::TextEdit::singleline(&mut self.shortcut)
                            .hint_text("Meta+Shift+V")
                            .text_color(TEXT)
                            .background_color(INPUT),
                    );
                    if Self::interaction_finished(&shortcut_resp) {
                        self.save_settings_if_dirty();
                    }
                    ui.horizontal_wrapped(|ui| {
                        for chord in ["Meta+Shift+V", "Ctrl+Shift+V", "Ctrl+Alt+V"] {
                            let selected = self.shortcut == chord;
                            if ui
                                .add(
                                    egui::Button::new(RichText::new(chord).color(TEXT))
                                        .fill(if selected { SELECTED } else { INPUT }),
                                )
                                .clicked()
                            {
                                self.shortcut = chord.to_string();
                                self.save_settings_if_dirty();
                            }
                        }
                    });
                    ui.label(
                        RichText::new("Applied via Plasma global accel (not owned by clipd).")
                            .color(MUTED)
                            .small(),
                    );
                    ui.separator();
                    ui.label(RichText::new("Pause shortcut for apps").color(MUTED));
                    ui.horizontal_wrapped(|ui| {
                        for minutes in [5_u32, 15, 30, 60] {
                            if ui.button(format!("{minutes}m")).clicked() {
                                self.pause(minutes);
                            }
                        }
                    });
                    if ui.button("Resume shortcut").clicked() {
                        self.pause(0);
                    }
                    ui.label(
                        RichText::new("Clipboard capture continues while the shortcut is paused.")
                            .color(MUTED)
                            .small(),
                    );
                    ui.separator();
                    ui.label(
                        RichText::new(
                            "If Plasma ignores the new chord: System Settings → Keyboard → Shortcuts → clipd History → set the key → Apply.",
                        )
                        .color(MUTED)
                        .small(),
                    );
                });
        }

        if toggle_settings {
            if self.show_settings {
                self.close_settings();
            } else {
                self.show_settings = true;
                if let Ok(s) = ipc::status_msg() {
                    self.pause_remaining_secs = s.pause_remaining_secs;
                    self.max_items = s.max_items.max(1);
                    if !s.shortcut.is_empty() {
                        self.shortcut = s.shortcut;
                    }
                }
                self.saved_max_items = self.max_items;
                self.saved_shortcut = self.shortcut.clone();
            }
        }

        if let Some(until) = self.saved_toast_until {
            if Instant::now() >= until {
                self.saved_toast_until = None;
            } else {
                ctx.request_repaint_after(until.saturating_duration_since(Instant::now()));
                egui::Area::new(egui::Id::new("saved_toast"))
                    .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-14.0, -14.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        Frame::new()
                            .fill(Color32::from_rgb(0x1e, 0x3a, 0x2f))
                            .stroke(Stroke::new(1.0, Color32::from_rgb(0x3d, 0x9e, 0x6f)))
                            .corner_radius(CornerRadius::same(6))
                            .inner_margin(Margin::symmetric(12, 8))
                            .show(ui, |ui| {
                                ui.label(
                                    RichText::new("Saved")
                                        .color(Color32::from_rgb(0xb6, 0xf0, 0xce))
                                        .strong(),
                                );
                            });
                    });
            }
        }

        let indices = self.filtered_indices();
        if self.selected >= indices.len() && !indices.is_empty() {
            self.selected = indices.len() - 1;
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(BG).inner_margin(Margin::symmetric(8, 8)))
            .show(ctx, |ui| {
                if indices.is_empty() {
                    ui.vertical_centered(|ui| {
                        ui.add_space(ui.available_height() * 0.35);
                        let msg = if self.items.is_empty() {
                            "No clipboard history yet"
                        } else {
                            "No matches"
                        };
                        ui.label(RichText::new(msg).color(MUTED).size(18.0));
                    });
                    return;
                }

                egui::ScrollArea::vertical().show(ui, |ui| {
                    let mut clicked: Option<(usize, usize)> = None;
                    for (row, &idx) in indices.iter().enumerate() {
                        let kind = self.items[idx].kind;
                        let selected = row == self.selected;

                        if kind == Kind::Image && self.textures[idx].is_none() {
                            if let Some(bytes) = self.items[idx].thumb.clone() {
                                if let Ok(img) = load_egui_image(&bytes) {
                                    self.textures[idx] = Some(ctx.load_texture(
                                        format!("thumb-{idx}"),
                                        img,
                                        Default::default(),
                                    ));
                                }
                            }
                        }

                        let preview = match kind {
                            Kind::Text => {
                                let p = &self.items[idx].preview;
                                if p.is_empty() {
                                    "(empty text)".to_string()
                                } else {
                                    p.clone()
                                }
                            }
                            Kind::Image => {
                                let it = &self.items[idx];
                                format!("{} · {} KB", it.mime, it.size / 1024)
                            }
                        };

                        let id = ui.id().with("row").with(idx);
                        let full_w = ui.available_width();

                        // Reserve height for padding + content.
                        let row_h = if kind == Kind::Image { 64.0 } else { 44.0 };
                        let (rect, resp) =
                            ui.allocate_exact_size(egui::vec2(full_w, row_h), Sense::click());
                        let hovered = resp.hovered();
                        let fill = if selected {
                            SELECTED
                        } else if hovered {
                            HOVER
                        } else {
                            Color32::TRANSPARENT
                        };
                        ui.painter().rect(
                            rect,
                            CornerRadius::same(6),
                            fill,
                            Stroke::NONE,
                            egui::StrokeKind::Inside,
                        );

                        let text_color = if selected { Color32::WHITE } else { TEXT };
                        let muted_color = if selected {
                            Color32::from_rgb(0xd0, 0xe0, 0xff)
                        } else {
                            MUTED
                        };

                        ui.scope_builder(egui::UiBuilder::new().max_rect(rect.shrink2(egui::vec2(10.0, 6.0))), |ui| {
                            ui.horizontal_centered(|ui| {
                                // Kind badge chip.
                                let (badge, badge_bg) = match kind {
                                    Kind::Text => ("T", BADGE_TEXT),
                                    Kind::Image => ("IMG", BADGE_IMG),
                                };
                                let badge_galley = ui.painter().layout_no_wrap(
                                    badge.to_string(),
                                    egui::FontId::proportional(11.0),
                                    Color32::WHITE,
                                );
                                let badge_size = egui::vec2(
                                    badge_galley.size().x + 12.0,
                                    badge_galley.size().y + 6.0,
                                );
                                let (badge_rect, _) =
                                    ui.allocate_exact_size(badge_size, Sense::hover());
                                ui.painter().rect(
                                    badge_rect,
                                    CornerRadius::same(4),
                                    badge_bg,
                                    Stroke::NONE,
                                    egui::StrokeKind::Inside,
                                );
                                ui.painter().galley(
                                    badge_rect.center() - badge_galley.size() * 0.5,
                                    badge_galley,
                                    Color32::WHITE,
                                );

                                if let Some(tex) = &self.textures[idx] {
                                    ui.add_space(6.0);
                                    ui.image((tex.id(), egui::vec2(48.0, 48.0)));
                                }

                                ui.add_space(8.0);
                                ui.vertical(|ui| {
                                    ui.label(RichText::new(preview).color(text_color));
                                    if kind == Kind::Image {
                                        ui.label(
                                            RichText::new("image")
                                                .color(muted_color)
                                                .small(),
                                        );
                                    }
                                });
                            });
                        });

                        // Silence unused id warning by touching it for focus rect uniqueness.
                        let _ = id;

                        if resp.clicked() || resp.double_clicked() {
                            clicked = Some((row, idx));
                        }
                        if selected {
                            ui.scroll_to_rect(rect, None);
                        }

                        ui.add_space(4.0);
                    }
                    if let Some((row, idx)) = clicked {
                        self.selected = row;
                        self.activate(idx);
                    }
                });
            });

        let escape = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if escape {
            if self.show_settings {
                self.close_settings();
            } else {
                self.persist_window_size(true);
                self.close = true;
            }
        }
        ctx.input(|i| {
            if i.key_pressed(egui::Key::ArrowDown) && !indices.is_empty() {
                self.selected = (self.selected + 1).min(indices.len() - 1);
            }
            if i.key_pressed(egui::Key::ArrowUp) && !indices.is_empty() {
                self.selected = self.selected.saturating_sub(1);
            }
        });

        if ctx.input(|i| i.key_pressed(egui::Key::Enter)) {
            if let Some(&idx) = indices.get(self.selected) {
                self.activate(idx);
            }
        }
    }
}

fn load_egui_image(bytes: &[u8]) -> Result<egui::ColorImage> {
    let img = image::load_from_memory(bytes).context("decode thumb")?;
    let rgba = img.to_rgba8();
    let size = [rgba.width() as usize, rgba.height() as usize];
    Ok(egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw()))
}
