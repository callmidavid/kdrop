use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tracing::{info, warn};

/// Proximity classifications based on calibrated BLE RSSI values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProximityZone {
    /// Very close (< 20 cm, typically touching or right next to laptop)
    Touch,
    /// Nearby (within arm's reach, ~0.5m – 1.5m)
    Near,
    /// Far / ambient (> 1.5m)
    Far,
}

impl ProximityZone {
    pub fn from_rssi(rssi: f32) -> Self {
        if rssi >= -44.0 {
            ProximityZone::Touch
        } else if rssi >= -65.0 {
            ProximityZone::Near
        } else {
            ProximityZone::Far
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ProximityZone::Touch => "⚡ Beside Laptop (< 20 cm)",
            ProximityZone::Near => "📶 Nearby (< 1.5 m)",
            ProximityZone::Far => "📡 In Room (> 1.5 m)",
        }
    }
}

/// An exponential moving average (EMA) filter to eliminate radio noise and jitter.
#[derive(Debug, Clone)]
pub struct RssiFilter {
    pub smoothed: f32,
    alpha: f32,
}

impl RssiFilter {
    pub fn new(initial_rssi: i16) -> Self {
        Self {
            smoothed: initial_rssi as f32,
            alpha: 0.35, // Responsive yet smooth
        }
    }

    pub fn update(&mut self, raw_rssi: i16) -> f32 {
        self.smoothed = self.alpha * (raw_rssi as f32) + (1.0 - self.alpha) * self.smoothed;
        self.smoothed
    }
}

/// A physical BLE device detected nearby.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NearbyBleDevice {
    pub address: String,
    pub name: Option<String>,
    pub smoothed_rssi: f32,
    pub zone: ProximityZone,
    #[serde(skip, default = "Instant::now")]
    pub last_seen: Instant,
}

/// Thread-safe registry tracking real-time Bluetooth LE proximity.
#[derive(Clone, Default)]
pub struct ProximityTracker {
    devices: Arc<Mutex<HashMap<String, (NearbyBleDevice, RssiFilter)>>>,
}

impl ProximityTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record or update an RSSI observation for a device.
    pub fn record_observation(&self, address: String, name: Option<String>, raw_rssi: i16) {
        let mut map = self.devices.lock().unwrap();
        let entry = map.entry(address.clone()).or_insert_with(|| {
            let filter = RssiFilter::new(raw_rssi);
            let zone = ProximityZone::from_rssi(filter.smoothed);
            (
                NearbyBleDevice {
                    address,
                    name: name.clone(),
                    smoothed_rssi: filter.smoothed,
                    zone,
                    last_seen: Instant::now(),
                },
                filter,
            )
        });

        // Update smoothed value
        let smoothed = entry.1.update(raw_rssi);
        entry.0.smoothed_rssi = smoothed;
        entry.0.zone = ProximityZone::from_rssi(smoothed);
        entry.0.last_seen = Instant::now();
        if name.is_some() {
            entry.0.name = name;
        }
    }

    /// Evicts devices not heard from in the last 12 seconds.
    pub fn evict_stale(&self) {
        let mut map = self.devices.lock().unwrap();
        map.retain(|_, (dev, _)| dev.last_seen.elapsed() < Duration::from_secs(12));
    }

    /// Returns the closest device currently in the "Touch / Bump" zone (< 20 cm).
    pub fn get_bumped_device(&self) -> Option<NearbyBleDevice> {
        let map = self.devices.lock().unwrap();
        map.values()
            .filter(|(dev, _)| dev.zone == ProximityZone::Touch)
            .max_by(|a, b| a.0.smoothed_rssi.partial_cmp(&b.0.smoothed_rssi).unwrap())
            .map(|(dev, _)| dev.clone())
    }

    /// Returns all active nearby BLE devices sorted from closest to farthest.
    pub fn get_all_devices(&self) -> Vec<NearbyBleDevice> {
        let map = self.devices.lock().unwrap();
        let mut list: Vec<NearbyBleDevice> = map.values().map(|(dev, _)| dev.clone()).collect();
        list.sort_by(|a, b| b.smoothed_rssi.partial_cmp(&a.smoothed_rssi).unwrap());
        list
    }
}

/// Spawns the Linux BLE proximity scanner using BlueZ.
pub fn start_proximity_scanner(tracker: Arc<ProximityTracker>) {
    #[cfg(target_os = "linux")]
    {
        let scan_tracker = tracker.clone();
        tokio::spawn(async move {
            info!("Initializing Bluetooth LE proximity scanner...");

            let session = match bluer::Session::new().await {
                Ok(s) => s,
                Err(e) => {
                    warn!("Bluetooth D-Bus session unavailable: {}. Proximity sensing disabled.", e);
                    return;
                }
            };

            let adapter = match session.default_adapter().await {
                Ok(a) => a,
                Err(e) => {
                    warn!("No Bluetooth adapter found (hci0): {}. Proximity sensing disabled.", e);
                    return;
                }
            };

            if let Err(e) = adapter.set_powered(true).await {
                warn!("Could not power on Bluetooth adapter: {}", e);
            }

            info!("Bluetooth adapter '{}' active. Starting passive BLE discovery...", adapter.name());

            let mut discover_events = match adapter.discover_devices().await {
                Ok(stream) => stream,
                Err(e) => {
                    warn!("Failed to start BLE discovery stream: {}. (Run with Bluetooth permissions)", e);
                    return;
                }
            };

            use futures::StreamExt;
            while let Some(event) = discover_events.next().await {
                match event {
                    bluer::AdapterEvent::DeviceAdded(addr) => {
                        if let Ok(dev) = adapter.device(addr) {
                            if let Ok(Some(rssi)) = dev.rssi().await {
                                let name = dev.name().await.ok().flatten();
                                scan_tracker.record_observation(addr.to_string(), name, rssi);
                            }
                        }
                    }
                    _ => {}
                }
            }
        });

        // Periodic stale device eviction loop
        let evict_tracker = tracker;
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(4));
            loop {
                interval.tick().await;
                evict_tracker.evict_stale();
            }
        });
    }

    #[cfg(not(target_os = "linux"))]
    {
        let _ = tracker;
        debug!("BLE proximity scanning not supported on this platform.");
    }
}

