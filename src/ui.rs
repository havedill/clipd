use crate::config::Config;
use crate::ipc;
use crate::plasma;
use crate::store::{ItemMeta, Kind};
use anyhow::{Context, Result};
use eframe::egui;
use std::time::{Duration, Instant};

pub fn run_show() -> Result<()> {
    let _ = ipc::status().context("daemon not reachable — start `clipd daemon` (or enable autostart)")?;
    let status = ipc::status_msg()?;
    let items = ipc::list(status.max_items.max(1)).context("list history")?;

    // Wayland ignores client-side with_position — we move via KWin after map.
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("clipd")
            .with_inner_size([560.0, 480.0])
            .with_always_on_top()
            .with_decorations(true),
        ..Default::default()
    };

    eframe::run_native(
        "clipd",
        options,
        Box::new(move |cc| {
            bump_fonts(&cc.egui_ctx, 1.2);
            Ok(Box::new(PopupApp::new(items, status)))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe: {e}"))?;
    Ok(())
}

fn bump_fonts(ctx: &egui::Context, scale: f32) {
    let mut style = (*ctx.style()).clone();
    for font_id in style.text_styles.values_mut() {
        font_id.size = (font_id.size * scale).round().clamp(12.0, 28.0);
    }
    ctx.set_style(style);
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
    pause_remaining_secs: Option<u64>,
    started: Instant,
    had_focus: bool,
    place_attempts: u8,
}

impl PopupApp {
    fn new(items: Vec<ItemMeta>, status: ipc::StatusMsg) -> Self {
        let textures = vec![None; items.len()];
        Self {
            items,
            filter: String::new(),
            selected: 0,
            textures,
            error: None,
            close: false,
            show_settings: false,
            max_items: status.max_items.max(1),
            shortcut: if status.shortcut.is_empty() {
                Config::load()
                    .map(|c| c.shortcut)
                    .unwrap_or_else(|_| "Ctrl+D".into())
            } else {
                status.shortcut
            },
            pause_remaining_secs: status.pause_remaining_secs,
            started: Instant::now(),
            had_focus: false,
            place_attempts: 0,
        }
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
            self.max_items = s.max_items.max(1);
            if !s.shortcut.is_empty() {
                self.shortcut = s.shortcut;
            }
        }
    }

    fn apply_settings(&mut self) {
        match ipc::set_config(Some(self.max_items), Some(self.shortcut.clone())) {
            Ok(()) => {
                self.error = None;
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
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Close when the popup loses focus (click elsewhere / Alt-Tab).
        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if focused {
            self.had_focus = true;
        } else if self.had_focus || self.started.elapsed() > Duration::from_millis(400) {
            self.close = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }

        // Plasma Wayland: ask KWin to move us to the cursor (client position is ignored).
        if self.place_attempts < 3 {
            let due = match self.place_attempts {
                0 => Duration::from_millis(40),
                1 => Duration::from_millis(120),
                _ => Duration::from_millis(250),
            };
            if self.started.elapsed() >= due {
                self.place_attempts += 1;
                if let Err(e) = plasma::move_show_to_cursor() {
                    eprintln!("clipd: place near cursor: {e:#}");
                }
            }
        }

        // Poll focus even when idle.
        ctx.request_repaint_after(Duration::from_millis(50));

        // Keep pause countdown fresh while settings are open.
        if self.show_settings {
            self.refresh_status();
        }

        egui::TopBottomPanel::top("search").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search");
                let resp = ui.text_edit_singleline(&mut self.filter);
                if !self.show_settings {
                    resp.request_focus();
                }
                if ui
                    .selectable_label(self.show_settings, "Settings")
                    .clicked()
                {
                    self.show_settings = !self.show_settings;
                }
            });
            if let Some(secs) = self.pause_remaining_secs {
                ui.colored_label(
                    egui::Color32::from_rgb(200, 140, 40),
                    format!(
                        "Shortcut paused — apps own it for {m}m {s:02}s",
                        m = secs / 60,
                        s = secs % 60
                    ),
                );
            }
            if let Some(err) = &self.error {
                ui.colored_label(egui::Color32::RED, err);
            }
        });

        if self.show_settings {
            egui::SidePanel::right("settings")
                .resizable(true)
                .default_width(220.0)
                .show(ctx, |ui| {
                    ui.heading("Settings");
                    ui.separator();
                    ui.label("Max history items");
                    ui.add(egui::DragValue::new(&mut self.max_items).range(1..=5000));
                    ui.add_space(8.0);
                    ui.label("Plasma shortcut");
                    ui.text_edit_singleline(&mut self.shortcut);
                    ui.small("Applied via Plasma global accel (not owned by clipd).");
                    if ui.button("Save settings").clicked() {
                        self.apply_settings();
                    }
                    ui.separator();
                    ui.label("Pause Ctrl+D for apps");
                    ui.horizontal_wrapped(|ui| {
                        for m in [5_u32, 15, 30, 60] {
                            if ui.button(format!("{m}m")).clicked() {
                                self.pause(m);
                            }
                        }
                    });
                    if ui.button("Resume shortcut").clicked() {
                        self.pause(0);
                    }
                    ui.separator();
                    ui.small("If apps still steal the hotkey: System Settings → Keyboard → Shortcuts → clipd History → re-bind, then Apply (or log out/in once).");
                });
        }

        let indices = self.filtered_indices();
        if self.selected >= indices.len() && !indices.is_empty() {
            self.selected = indices.len() - 1;
        }

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut clicked: Option<(usize, usize)> = None;
                for (row, &idx) in indices.iter().enumerate() {
                    let kind = self.items[idx].kind;
                    let selected = row == self.selected;
                    let label = match kind {
                        Kind::Text => {
                            let p = &self.items[idx].preview;
                            if p.is_empty() {
                                "(empty text)".into()
                            } else {
                                p.clone()
                            }
                        }
                        Kind::Image => {
                            let it = &self.items[idx];
                            format!("[image] {} ({} KB)", it.mime, it.size / 1024)
                        }
                    };

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

                    ui.horizontal(|ui| {
                        if let Some(tex) = &self.textures[idx] {
                            ui.image((tex.id(), egui::vec2(48.0, 48.0)));
                        }
                        let resp = ui.selectable_label(selected, label);
                        if resp.clicked() || resp.double_clicked() {
                            clicked = Some((row, idx));
                        }
                    });
                }
                if let Some((row, idx)) = clicked {
                    self.selected = row;
                    self.activate(idx);
                }
            });
        });

        ctx.input(|i| {
            if i.key_pressed(egui::Key::Escape) {
                if self.show_settings {
                    self.show_settings = false;
                } else {
                    self.close = true;
                }
            }
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
