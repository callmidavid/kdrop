use crate::config::Config;
use crate::peer::{FileInfo, Peer, PeerRegistry, PendingSession};
use crate::server::{AppEvent, AppState, SharedFileItem};
use crate::transfer::{TransferManager, TransferProgress};
use eframe::egui;
use image::Luma;
use qrcode::QrCode;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tracing::info;

pub struct KdropApp {
    config: Arc<Config>,
    registry: PeerRegistry,
    app_state: AppState,
    transfer_manager: Arc<TransferManager>,
    current_progress: Arc<Mutex<Option<TransferProgress>>>,
    show_qr_modal: bool,
    qr_texture: Option<egui::TextureHandle>,
    incoming_request: Option<PendingSession>,
}

impl KdropApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        config: Arc<Config>,
        registry: PeerRegistry,
        app_state: AppState,
    ) -> Self {
        // Apply Kafy OS Dark styling
        let mut style = (*cc.egui_ctx.style()).clone();
        style.visuals.dark_mode = true;
        style.visuals.override_text_color = Some(egui::Color32::from_rgb(240, 240, 245));
        style.visuals.window_fill = egui::Color32::from_rgb(20, 22, 32);
        style.visuals.panel_fill = egui::Color32::from_rgb(16, 18, 26);
        style.visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(24, 28, 40);
        style.visuals.widgets.inactive.bg_fill = egui::Color32::from_rgb(32, 36, 52);
        style.visuals.widgets.hovered.bg_fill = egui::Color32::from_rgb(46, 52, 76);
        style.visuals.widgets.active.bg_fill = egui::Color32::from_rgb(93, 63, 211);
        cc.egui_ctx.set_style(style);

        let transfer_manager = Arc::new(TransferManager::new(config.clone()));
        let current_progress = Arc::new(Mutex::new(None));

        Self {
            config,
            registry,
            app_state,
            transfer_manager,
            current_progress,
            show_qr_modal: false,
            qr_texture: None,
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
                .map(|p| {
                    if p == 0 {
                        egui::Color32::BLACK
                    } else {
                        egui::Color32::WHITE
                    }
                })
                .collect();

            let color_image = egui::ColorImage { size, pixels };
            self.qr_texture = Some(ctx.load_texture("qr_code", color_image, Default::default()));
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
}

impl eframe::App for KdropApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.check_incoming_requests();

        // Repaint periodically for live discovery and progress updates
        ctx.request_repaint_after(Duration::from_millis(500));

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);

            // 1. Header Bar
            ui.horizontal(|ui| {
                ui.heading(
                    egui::RichText::new("💧 kdrop")
                        .size(20.0)
                        .strong()
                        .color(egui::Color32::from_rgb(180, 150, 250)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("📱 Show QR / Web Link").clicked() {
                        self.show_qr_modal = true;
                        self.generate_qr_texture(ctx);
                    }

                    ui.label(
                        egui::RichText::new(format!("URL: {}", self.config.local_url()))
                            .size(13.0)
                            .color(egui::Color32::from_rgb(140, 150, 175)),
                    );
                });
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(10.0);

            // 2. Active Transfer Progress
            let progress_opt = self.current_progress.lock().unwrap().clone();
            if let Some(progress) = progress_opt {
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    match progress {
                        TransferProgress::WaitingForAccept(peer_name) => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label(format!("Waiting for {} to accept transfer...", peer_name));
                            });
                        }
                        TransferProgress::Transferring {
                            file_name,
                            current_file,
                            total_files,
                            bytes_sent,
                            total_bytes,
                        } => {
                            let ratio = if total_bytes > 0 {
                                (bytes_sent as f32) / (total_bytes as f32)
                            } else {
                                0.0
                            };
                            ui.label(format!(
                                "Sending {} ({}/{}): {:.1} MB / {:.1} MB",
                                file_name,
                                current_file,
                                total_files,
                                bytes_sent as f64 / 1_000_000.0,
                                total_bytes as f64 / 1_000_000.0,
                            ));
                            ui.add(egui::ProgressBar::new(ratio).show_percentage());
                        }
                        TransferProgress::Completed => {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(100, 220, 150), "✔ Transfer completed!");
                                if ui.button("Dismiss").clicked() {
                                    *self.current_progress.lock().unwrap() = None;
                                }
                            });
                        }
                        TransferProgress::Declined => {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(250, 150, 100), "⚠ Recipient declined the transfer.");
                                if ui.button("Dismiss").clicked() {
                                    *self.current_progress.lock().unwrap() = None;
                                }
                            });
                        }
                        TransferProgress::Failed(err) => {
                            ui.horizontal(|ui| {
                                ui.colored_label(egui::Color32::from_rgb(240, 80, 80), format!("✖ Error: {}", err));
                                if ui.button("Dismiss").clicked() {
                                    *self.current_progress.lock().unwrap() = None;
                                }
                            });
                        }
                    }
                });
                ui.add_space(10.0);
            }

            // 3. Nearby Devices
            ui.label(
                egui::RichText::new("Nearby Devices")
                    .size(16.0)
                    .strong(),
            );
            ui.add_space(6.0);

            let peers = self.registry.all();
            if peers.is_empty() {
                ui.label(
                    egui::RichText::new("Looking for nearby devices... Make sure phones/computers are on the same Wi-Fi.")
                        .size(13.0)
                        .color(egui::Color32::GRAY),
                );
            } else {
                for peer in peers {
                    ui.group(|ui| {
                        ui.set_width(ui.available_width());
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(peer.icon()).size(24.0));
                            ui.vertical(|ui| {
                                ui.label(egui::RichText::new(&peer.alias).strong());
                                ui.label(
                                    egui::RichText::new(format!("{} • {}:{}", peer.device_model, peer.ip, peer.port))
                                        .size(11.0)
                                        .color(egui::Color32::GRAY),
                                );
                            });

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Send Files").clicked() {
                                    if let Some(files) = rfd::FileDialog::new().pick_files() {
                                        let tm = self.transfer_manager.clone();
                                        let progress_clone = self.current_progress.clone();
                                        let peer_clone = peer.clone();

                                        tokio::spawn(async move {
                                            let cb_prog = progress_clone.clone();
                                            let _ = tm
                                                .send_files(&peer_clone, files, move |prog| {
                                                    *cb_prog.lock().unwrap() = Some(prog);
                                                })
                                                .await;
                                        });
                                    }
                                }
                            });
                        });
                    });
                    ui.add_space(4.0);
                }
            }

            ui.add_space(14.0);
            ui.separator();
            ui.add_space(10.0);

            // 4. Download Directory & Files
            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new("Downloads Folder")
                        .size(16.0)
                        .strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Open Folder").clicked() {
                        let _ = open::that(&self.config.download_dir);
                    }
                });
            });

            ui.add_space(6.0);

            let shared_files = self.app_state.shared_files.lock().unwrap().clone();
            if shared_files.is_empty() {
                ui.label(
                    egui::RichText::new("No files received yet. Files sent to this machine will appear here.")
                        .size(13.0)
                        .color(egui::Color32::GRAY),
                );
            } else {
                egui::ScrollArea::vertical().max_height(160.0).show(ui, |ui| {
                    for file in shared_files {
                        ui.horizontal(|ui| {
                            ui.label("📄");
                            ui.label(
                                egui::RichText::new(&file.file_name)
                                    .strong(),
                            );
                            ui.label(
                                egui::RichText::new(format!("({:.1} MB)", file.size as f64 / 1_000_000.0))
                                    .size(12.0)
                                    .color(egui::Color32::GRAY),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.button("Open").clicked() {
                                    let _ = open::that(&file.path);
                                }
                            });
                        });
                        ui.separator();
                    }
                });
            }
        });

        // 5. Incoming Transfer Dialog Modal
        if let Some(session) = self.incoming_request.clone() {
            egui::Window::new("Incoming Transfer Request")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.heading("📥 File Transfer Request");
                    ui.add_space(8.0);
                    ui.label(format!("'{}' wants to send you {} file(s):", session.sender_alias, session.files.len()));
                    ui.add_space(6.0);

                    for file in &session.files {
                        ui.label(format!("• {} ({:.1} MB)", file.file_name, file.size as f64 / 1_000_000.0));
                    }

                    ui.add_space(14.0);
                    ui.horizontal(|ui| {
                        if ui.button("Decline").clicked() {
                            self.app_state.session_decisions.lock().unwrap().insert(session.session_id.clone(), false);
                            self.app_state.pending_sessions.lock().unwrap().remove(&session.session_id);
                            let _ = self.app_state.event_tx.send(AppEvent::TransferDecision {
                                session_id: session.session_id.clone(),
                                accepted: false,
                            });
                        }

                        if ui.button(egui::RichText::new("Accept").color(egui::Color32::from_rgb(100, 220, 150))).clicked() {
                            self.app_state.session_decisions.lock().unwrap().insert(session.session_id.clone(), true);
                            let _ = self.app_state.event_tx.send(AppEvent::TransferDecision {
                                session_id: session.session_id.clone(),
                                accepted: true,
                            });
                        }
                    });
                });
        }

        // 6. QR Code / Web Link Modal
        if self.show_qr_modal {
            egui::Window::new("Share via Web / QR")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new("Scan on your iPhone / Android").strong());
                        ui.add_space(8.0);

                        if let Some(texture) = &self.qr_texture {
                            ui.image(texture);
                        }

                        ui.add_space(8.0);
                        ui.label(format!("Or open in browser:\n{}", self.config.local_url()));
                        ui.add_space(12.0);

                        if ui.button("Close").clicked() {
                            self.show_qr_modal = false;
                        }
                    });
                });
        }
    }
}

