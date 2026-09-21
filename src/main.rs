mod config;
mod discovery;
mod peer;
mod proximity;
mod server;
mod transfer;
mod ui;

use anyhow::Result;
use config::Config;
use discovery::start_discovery;
use peer::PeerRegistry;
use proximity::{start_proximity_scanner, ProximityTracker};
use qrcode::render::unicode;
use qrcode::QrCode;
use server::{create_router, AppState};
use std::net::SocketAddr;
use std::sync::Arc;
use tracing::{error, info};
use ui::KdropApp;

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args: Vec<String> = std::env::args().collect();
    let headless = args.iter().any(|arg| arg == "--headless" || arg == "-h");

    // 1. Load configuration & initialize state
    let config = Arc::new(Config::load_or_create()?);
    let registry = PeerRegistry::new();
    let app_state = AppState::new(config.clone(), registry.clone());
    let proximity_tracker = Arc::new(ProximityTracker::new());

    info!(
        "Starting kdrop for '{}' ({})",
        config.alias, config.fingerprint
    );

    // 2. Start mDNS / Multicast UDP discovery service
    if let Err(e) = start_discovery(config.clone(), registry.clone()) {
        error!("Failed to initialize discovery service: {}", e);
    }

    // 2.5 Start Bluetooth LE RSSI Proximity scanner
    start_proximity_scanner(proximity_tracker.clone());

    // 3. Start axum HTTP server
    let router = create_router(app_state.clone());
    let bind_addr: SocketAddr = format!("0.0.0.0:{}", config.port).parse()?;
    let listener = tokio::net::TcpListener::bind(bind_addr).await?;
    info!("kdrop server listening on http://{}", bind_addr);

    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, router).await {
            error!("HTTP server encountered an error: {}", e);
        }
    });

    let local_url = config.local_url();
    println!("--------------------------------------------------");
    println!("  kdrop  --  fast local file transfer");
    println!("  Device  : {}", config.alias);
    println!("  Web URL : {}", local_url);
    println!("  Save to : {:?}", config.download_dir);
    println!("--------------------------------------------------");

    // Print ASCII QR code to terminal for fast phone scanning
    if let Ok(code) = QrCode::new(local_url.as_bytes()) {
        let image = code
            .render::<unicode::Dense1x2>()
            .dark_color(unicode::Dense1x2::Light)
            .light_color(unicode::Dense1x2::Dark)
            .build();
        println!("\nScan this QR code with iPhone or Android camera:\n");
        println!("{}", image);
    }

    // 4. GUI or Headless mode
    if headless {
        println!("Running in headless mode. Press Ctrl+C to stop.");
        tokio::signal::ctrl_c().await?;
        println!("\nShutting down kdrop.");
    } else {
        let native_options = eframe::NativeOptions {
            viewport: egui::ViewportBuilder::default()
                .with_inner_size([720.0, 560.0])
                .with_min_inner_size([480.0, 400.0])
                .with_title(format!("kdrop — {}", config.alias)),
            ..Default::default()
        };

        let app_config = config.clone();
        let app_reg = registry.clone();
        let app_st = app_state.clone();
        let app_prox = proximity_tracker.clone();

        // Run GUI on main thread
        let _ = eframe::run_native(
            "kdrop",
            native_options,
            Box::new(move |cc| Box::new(KdropApp::new(cc, app_config, app_reg, app_st, app_prox))),
        );
    }

    Ok(())
}
