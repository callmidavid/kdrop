use crate::config::Config;
use crate::peer::{PeerRegistry, PendingSession};
use crate::proximity::ProximityTracker;
use crate::server::{AppEvent, AppState};
use crate::transfer::{TransferManager, TransferProgress};
use eframe::egui;
use image::Luma;
use qrcode::QrCode;
use std::sync::{Arc, Mutex};
use std::time::Duration;

// ── macOS HIG design tokens ────────────────────────────────────────────────────
const BG: egui::Color32 = egui::Color32::from_rgb(28, 28, 30);
const PANEL: egui::Color32 = egui::Color32::from_rgb(36, 36, 38);
const CARD: egui::Color32 = egui::Color32::from_rgb(44, 44, 46);
const CARD_BORDER: egui::Color32 = egui::Color32::from_rgb(58, 58, 60);
const ACCENT: egui::Color32 = egui::Color32::from_rgb(10, 132, 255);
const SUCCESS: egui::Color32 = egui::Color32::from_rgb(48, 209, 88);
const DANGER: egui::Color32 = egui::Color32::from_rgb(255, 69, 58);
const WARNING: egui::Color32 = egui::Color32::from_rgb(255, 214, 10);
const TEXT_PRIMARY: egui::Color32 = egui::Color32::from_rgb(242, 242, 247);
const TEXT_MUTED: egui::Color32 = egui::Color32::from_rgb(142, 142, 147);
const SEPARATOR: egui::Color32 = egui::Color32::from_rgb(56, 56, 58);

const LOGO_PNG: &[u8] = include_bytes!("../assets/logo.png");

pub struct KdropApp {
    config: Arc<Config>,
    registry: PeerRegistry,
    app_state: AppState,
    proximity_tracker: Arc<ProximityTracker>,
    transfer_manager: Arc<TransferManager>,
    current_progress: Arc<Mutex<Option<TransferProgress>>>,
    show_qr_modal: bool,
    qr_texture: Option<egui::TextureHandle>,
    logo_texture: Option<egui::TextureHandle>,
    incoming_request: Option<PendingSession>,
}

impl KdropApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Arc<Config>,
        registry: PeerRegistry,
        app_state: AppState,
        proximity_tracker: Arc<ProximityTracker>,
    ) -> Self {
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.override_text_color = Some(TEXT_PRIMARY);
        style.visuals.window_fill = BG;
        style.visuals.panel_fill = BG;
        style.visuals.faint_bg_color = PANEL;

        style.visuals.widgets.noninteractive.bg_fill = CARD;
        style.visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(1.0, CARD_BORDER);
        style.visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, TEXT_MUTED);
        style.visuals.widgets.inactive.bg_fill = CARD;
        style.visuals.widgets.inactive.bg_stroke = egui::Stroke::new(1.0, CARD_BORDER);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(72, 72, 74);
        style.visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, ACCENT);
        style.visuals.widgets.active.bg_fill = ACCENT;
        style.visuals.widgets.active.bg_stroke = egui::Stroke::new(0.0, ACCENT);

        style.visuals.widgets.noninteractive.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.inactive.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.hovered.rounding = egui::Rounding::same(8.0);
        style.visuals.widgets.active.rounding = egui::Rounding::same(8.0);
        style.visuals.window_rounding = egui::Rounding::same(12.0);

        style.visuals.selection.bg_fill = ACCENT.gamma_multiply(0.4);
        style.visuals.hyperlink_color = ACCENT;

        style.spacing.button_padding = egui::vec2(12.0, 6.0);
        style.spacing.item_spacing = egui::vec2(8.0, 6.0);
        style.spacing.window_margin = egui::Margin::same(0.0);

        cc.egui_ctx.set_style(style);

        // Load official logo PNG into GPU texture
        let logo_texture = if let Ok(img) = image::load_from_memory(LOGO_PNG) {
            let rgba = img.to_rgba8();
            let size = [rgba.width() as usize, rgba.height() as usize];
            let pixels: Vec<egui::Color32> = rgba
                .into_raw()
                .chunks_exact(4)
                .map(|c| egui::Color32::from_rgba_premultiplied(c[0], c[1], c[2], c[3]))
                .collect();
            Some(cc.egui_ctx.load_texture(
                "kdrop_logo",
                egui::ColorImage { size, pixels },
                Default::default(),
            ))
        } else {
            None
        };

        Self {
            config: config.clone(),
            registry,
            app_state,
            proximity_tracker,
            transfer_manager: Arc::new(TransferManager::new(config)),
            current_progress: Arc::new(Mutex::new(None)),
            show_qr_modal: false,
            qr_texture: None,
            logo_texture,
            incoming_request: None,
        }
    }

    fn generate_qr_texture(&mut self, ctx: &egui::Context) {
        if self.qr_texture.is_some() {
            return;
        }
        let url = self.config.local_url();
        if let Ok(qr) = QrCode::new(url.as_bytes()) {
            let img = qr.render::<Luma<u8>>().max_dimensions(256, 256).build();
            let size = [img.width() as usize, img.height() as usize];
            let pixels: Vec<egui::Color32> = img
                .into_raw()
                .into_iter()
                .map(|p| if p == 0 { egui::Color32::BLACK } else { egui::Color32::WHITE })
                .collect();
            self.qr_texture = Some(ctx.load_texture(
                "qr_code",
                egui::ColorImage { size, pixels },
                Default::default(),
            ));
        }
    }

    fn check_incoming_requests(&mut self) {
        let sessions = self.app_state.pending_sessions.lock().unwrap();
        if let Some((_, session)) = sessions.iter().next() {
            self.incoming_request = Some(session.clone());
        } else {
            self.incoming_request = None;
        }
    }

    fn accent_button(ui: &mut egui::Ui, label: &str) -> bool {
        ui.add(
            egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE).strong())
                .fill(ACCENT)
                .rounding(egui::Rounding::same(8.0)),
        )
        .clicked()
    }

    fn danger_button(ui: &mut egui::Ui, label: &str) -> bool {
        ui.add(
            egui::Button::new(egui::RichText::new(label).color(egui::Color32::WHITE).strong())
                .fill(DANGER)
                .rounding(egui::Rounding::same(8.0)),
        )
        .clicked()
    }

    fn ghost_button(ui: &mut egui::Ui, label: &str) -> bool {
        ui.add(
            egui::Button::new(egui::RichText::new(label).color(TEXT_PRIMARY))
                .fill(CARD)
                .stroke(egui::Stroke::new(1.0, CARD_BORDER))
                .rounding(egui::Rounding::same(8.0)),
        )
        .clicked()
    }

    fn section_label(ui: &mut egui::Ui, text: &str) {
        ui.label(
            egui::RichText::new(text)
                .size(11.0)
                .strong()
                .color(TEXT_MUTED),
        );
    }
}

impl eframe::App for KdropApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_incoming_requests();
        ctx.request_repaint_after(Duration::from_millis(500));

        // ── Top Toolbar: spans full window width with hairline bottom border ─
        egui::TopBottomPanel::top("top_toolbar")
            .frame(
                egui::Frame::none()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::symmetric(20.0, 12.0))
                    .stroke(egui::Stroke::new(1.0, SEPARATOR)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(tex) = &self.logo_texture {
                        ui.image(egui::load::SizedTexture::new(tex.id(), egui::vec2(20.0, 20.0)));
                        ui.add_space(4.0);
                    }
                    ui.label(
                        egui::RichText::new("kdrop").size(16.0).strong().color(TEXT_PRIMARY),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if Self::ghost_button(ui, "Share via QR") {
                            self.show_qr_modal = true;
                            self.generate_qr_texture(ctx);
                        }
                        ui.add_space(6.0);
                        ui.label(
                            egui::RichText::new(self.config.local_url())
                                .size(11.0)
                                .monospace()
                                .color(TEXT_MUTED),
                        );
                    });
                });
            });

        // ── Main Body: CentralPanel with 20px padding on left & right ────────
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(BG)
                    .inner_margin(egui::Margin::symmetric(20.0, 14.0)),
            )
            .show(ctx, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        ui.add_space(8.0);

                        // ── BLE Proximity Bump ────────────────────────
                        if let Some(bumped) = self.proximity_tracker.get_bumped_device() {
                            egui::Frame::none()
                                .fill(ACCENT.gamma_multiply(0.12))
                                .stroke(egui::Stroke::new(1.0, ACCENT))
                                .rounding(egui::Rounding::same(12.0))
                                .inner_margin(egui::Margin::same(14.0))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.set_width(ui.available_width());
                                        let (r, _) = ui.allocate_exact_size(
                                            egui::vec2(10.0, 10.0),
                                            egui::Sense::hover(),
                                        );
                                        ui.painter().circle_filled(r.center(), 5.0, SUCCESS);
                                        ui.add_space(6.0);
                                        ui.vertical(|ui| {
                                            let name = bumped.name.as_deref().unwrap_or("Nearby Device");
                                            ui.label(egui::RichText::new(format!("{} is right here", name)).size(13.0).strong().color(TEXT_PRIMARY));
                                            ui.label(egui::RichText::new(format!("Signal: {:.0} dBm  •  Touch range", bumped.smoothed_rssi)).size(11.0).color(TEXT_MUTED));
                                        });
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            if Self::accent_button(ui, "Drop Files") {
                                                if let Some(files) = rfd::FileDialog::new().pick_files() {
                                                    let peers = self.registry.all();
                                                    if let Some(first) = peers.first() {
                                                        let tm = self.transfer_manager.clone();
                                                        let prog = self.current_progress.clone();
                                                        let pc = first.clone();
                                                        tokio::spawn(async move {
                                                            let _ = tm.send_files(&pc, files, move |p| { *prog.lock().unwrap() = Some(p); }).await;
                                                        });
                                                    }
                                                }
                                            }
                                        });
                                    });
                                });
                            ui.add_space(10.0);
                        }

                        // ── Active Transfer Progress ──────────────────
                        let progress_opt = self.current_progress.lock().unwrap().clone();
                        if let Some(progress) = progress_opt {
                            egui::Frame::none()
                                .fill(CARD)
                                .stroke(egui::Stroke::new(1.0, CARD_BORDER))
                                .rounding(egui::Rounding::same(12.0))
                                .inner_margin(egui::Margin::same(14.0))
                                .show(ui, |ui| {
                                    match progress {
                                        TransferProgress::WaitingForAccept(ref peer_name) => {
                                            ui.horizontal(|ui| {
                                                ui.spinner();
                                                ui.label(egui::RichText::new(format!("Waiting for {} to accept…", peer_name)).color(TEXT_MUTED));
                                            });
                                        }
                                        TransferProgress::Transferring { ref file_name, current_file, total_files, bytes_sent, total_bytes } => {
                                            let ratio = if total_bytes > 0 { bytes_sent as f32 / total_bytes as f32 } else { 0.0 };
                                            ui.label(egui::RichText::new(format!("Sending  {}  ({}/{})", file_name, current_file, total_files)).strong().color(TEXT_PRIMARY));
                                            ui.label(egui::RichText::new(format!("{:.1} MB  /  {:.1} MB", bytes_sent as f64 / 1e6, total_bytes as f64 / 1e6)).size(11.0).color(TEXT_MUTED));
                                            ui.add_space(4.0);
                                            ui.add(egui::ProgressBar::new(ratio).show_percentage().fill(ACCENT));
                                        }
                                        TransferProgress::Completed => {
                                            ui.horizontal(|ui| {
                                                ui.colored_label(SUCCESS, "Transfer complete");
                                                if Self::ghost_button(ui, "Dismiss") { *self.current_progress.lock().unwrap() = None; }
                                            });
                                        }
                                        TransferProgress::Declined => {
                                            ui.horizontal(|ui| {
                                                ui.colored_label(WARNING, "Recipient declined");
                                                if Self::ghost_button(ui, "Dismiss") { *self.current_progress.lock().unwrap() = None; }
                                            });
                                        }
                                        TransferProgress::Failed(ref err) => {
                                            ui.horizontal(|ui| {
                                                ui.colored_label(DANGER, format!("Error: {}", err));
                                                if Self::ghost_button(ui, "Dismiss") { *self.current_progress.lock().unwrap() = None; }
                                            });
                                        }
                                    }
                                });
                            ui.add_space(10.0);
                        }

                        // ── Nearby Devices ────────────────────────────
                        Self::section_label(ui, "NEARBY DEVICES");
                        ui.add_space(6.0);

                        let peers = self.registry.all();
                        if peers.is_empty() {
                            egui::Frame::none()
                                .fill(CARD)
                                .stroke(egui::Stroke::new(1.0, CARD_BORDER))
                                .rounding(egui::Rounding::same(12.0))
                                .inner_margin(egui::Margin::same(24.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.vertical_centered(|ui| {
                                        let (r, _) = ui.allocate_exact_size(egui::vec2(14.0, 14.0), egui::Sense::hover());
                                        ui.painter().circle_filled(r.center(), 6.0, TEXT_MUTED);
                                        ui.add_space(8.0);
                                        ui.label(egui::RichText::new("Looking for nearby devices…").size(14.0).strong().color(TEXT_PRIMARY));
                                        ui.label(egui::RichText::new("Make sure other devices are on the same Wi-Fi").size(12.0).color(TEXT_MUTED));
                                    });
                                });
                        } else {
                            for peer in &peers {
                                egui::Frame::none()
                                    .fill(CARD)
                                    .stroke(egui::Stroke::new(1.0, CARD_BORDER))
                                    .rounding(egui::Rounding::same(12.0))
                                    .inner_margin(egui::Margin::same(14.0))
                                    .show(ui, |ui| {
                                        ui.horizontal(|ui| {
                                            ui.set_width(ui.available_width());
                                            egui::Frame::none()
                                                .fill(ACCENT.gamma_multiply(0.18))
                                                .rounding(egui::Rounding::same(6.0))
                                                .inner_margin(egui::Margin::symmetric(8.0, 3.0))
                                                .show(ui, |ui| {
                                                    ui.label(egui::RichText::new(peer.device_type_label()).size(11.0).color(ACCENT));
                                                });
                                            ui.add_space(8.0);
                                            ui.vertical(|ui| {
                                                ui.label(egui::RichText::new(&peer.alias).size(14.0).strong().color(TEXT_PRIMARY));
                                                ui.label(egui::RichText::new(format!("{}  •  {}:{}", peer.device_model, peer.ip, peer.port)).size(11.0).color(TEXT_MUTED));
                                            });
                                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                if Self::accent_button(ui, "Send Files") {
                                                    if let Some(files) = rfd::FileDialog::new().pick_files() {
                                                        let tm = self.transfer_manager.clone();
                                                        let prog = self.current_progress.clone();
                                                        let pc = peer.clone();
                                                        tokio::spawn(async move {
                                                            let _ = tm.send_files(&pc, files, move |p| { *prog.lock().unwrap() = Some(p); }).await;
                                                        });
                                                    }
                                                }
                                            });
                                        });
                                    });
                                ui.add_space(6.0);
                            }
                        }

                        ui.add_space(14.0);
                        ui.add(egui::Separator::default().spacing(12.0));
                        ui.add_space(14.0);

                        // ── Received Files ────────────────────────────
                        ui.horizontal(|ui| {
                            ui.set_width(ui.available_width());
                            Self::section_label(ui, "RECEIVED FILES");
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if Self::ghost_button(ui, "Open Folder") {
                                    let _ = open::that(&self.config.download_dir);
                                }
                            });
                        });
                        ui.add_space(6.0);

                        let shared_files = self.app_state.shared_files.lock().unwrap().clone();
                        if shared_files.is_empty() {
                            egui::Frame::none()
                                .fill(CARD)
                                .stroke(egui::Stroke::new(1.0, CARD_BORDER))
                                .rounding(egui::Rounding::same(12.0))
                                .inner_margin(egui::Margin::same(14.0))
                                .show(ui, |ui| {
                                    ui.set_width(ui.available_width());
                                    ui.label(egui::RichText::new("No files received yet — files sent to this device appear here").size(13.0).color(TEXT_MUTED));
                                });
                        } else {
                            egui::ScrollArea::vertical()
                                .max_height(200.0)
                                .id_source("files_scroll")
                                .show(ui, |ui| {
                                    for (i, file) in shared_files.iter().enumerate() {
                                        egui::Frame::none()
                                            .fill(CARD)
                                            .stroke(egui::Stroke::new(1.0, CARD_BORDER))
                                            .rounding(egui::Rounding::same(10.0))
                                            .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                                            .show(ui, |ui| {
                                                ui.horizontal(|ui| {
                                                    ui.set_width(ui.available_width());
                                                    ui.vertical(|ui| {
                                                        ui.label(egui::RichText::new(&file.file_name).strong().color(TEXT_PRIMARY));
                                                        ui.label(egui::RichText::new(format!("{:.1} MB", file.size as f64 / 1e6)).size(11.0).color(TEXT_MUTED));
                                                    });
                                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                                        if Self::ghost_button(ui, "Open") {
                                                            let _ = open::that(&file.path);
                                                        }
                                                    });
                                                });
                                            });
                                        if i + 1 < shared_files.len() { ui.add_space(4.0); }
                                    }
                                });
                        }
                        ui.add_space(24.0);
                    });
            }); // end CentralPanel

        // ── Modal: Incoming Transfer ──────────────────────────────────────────
        if let Some(session) = self.incoming_request.clone() {
            egui::Window::new("Incoming Transfer")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(
                    egui::Frame::window(&ctx.style())
                        .fill(egui::Color32::from_rgb(36, 36, 38))
                        .stroke(egui::Stroke::new(1.0, SEPARATOR))
                        .rounding(egui::Rounding::same(16.0))
                        .inner_margin(egui::Margin::same(24.0)),
                )
                .show(ctx, |ui| {
                    ui.set_min_width(340.0);
                    ui.label(egui::RichText::new("Incoming Transfer").size(18.0).strong().color(TEXT_PRIMARY));
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(format!("'{}' wants to send you {} file(s)", session.sender_alias, session.files.len())).size(13.0).color(TEXT_MUTED));
                    ui.add_space(10.0);
                    egui::Frame::none()
                        .fill(CARD)
                        .rounding(egui::Rounding::same(8.0))
                        .inner_margin(egui::Margin::same(10.0))
                        .show(ui, |ui| {
                            ui.set_min_width(292.0);
                            for file in &session.files {
                                ui.label(egui::RichText::new(format!("{}   ({:.1} MB)", file.file_name, file.size as f64 / 1e6)).size(12.0).color(TEXT_PRIMARY));
                            }
                        });
                    ui.add_space(16.0);
                    ui.horizontal(|ui| {
                        if Self::danger_button(ui, "Decline") {
                            self.app_state.session_decisions.lock().unwrap().insert(session.session_id.clone(), false);
                            self.app_state.pending_sessions.lock().unwrap().remove(&session.session_id);
                            let _ = self.app_state.event_tx.send(AppEvent::TransferDecision { session_id: session.session_id.clone(), accepted: false });
                        }
                        ui.add_space(8.0);
                        if Self::accent_button(ui, "Accept") {
                            self.app_state.session_decisions.lock().unwrap().insert(session.session_id.clone(), true);
                            let _ = self.app_state.event_tx.send(AppEvent::TransferDecision { session_id: session.session_id.clone(), accepted: true });
                        }
                    });
                });
        }

        // ── Modal: QR code ────────────────────────────────────────────────────
        if self.show_qr_modal {
            egui::Window::new("Share via QR")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .frame(
                    egui::Frame::window(&ctx.style())
                        .fill(egui::Color32::from_rgb(36, 36, 38))
                        .stroke(egui::Stroke::new(1.0, SEPARATOR))
                        .rounding(egui::Rounding::same(16.0))
                        .inner_margin(egui::Margin::same(24.0)),
                )
                .show(ctx, |ui| {
                    ui.set_min_width(300.0);
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Scan with iPhone or Android").size(15.0).strong().color(TEXT_PRIMARY));
                        ui.add_space(12.0);
                        if let Some(texture) = &self.qr_texture { ui.image(texture); }
                        ui.add_space(10.0);
                        ui.label(egui::RichText::new(format!("Or open in browser:\n{}", self.config.local_url())).size(12.0).monospace().color(TEXT_MUTED));
                        ui.add_space(16.0);
                        if Self::ghost_button(ui, "Close") { self.show_qr_modal = false; }
                    });
                });
        }
    }
}
