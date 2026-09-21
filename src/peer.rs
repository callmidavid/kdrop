use crate::config::DeviceType;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::Instant;

/// A peer device discovered on the network.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Peer {
    pub alias: String,
    pub device_model: String,
    pub device_type: DeviceType,
    pub fingerprint: String,
    pub port: u16,
    /// IP address (filled in from the UDP packet source, not from JSON body).
    pub ip: String,
    /// Protocol used by this peer (http or https).
    pub protocol: String,
}

impl Peer {
    /// Base URL for sending HTTP requests to this peer.
    pub fn base_url(&self) -> String {
        format!("{}://{}:{}", self.protocol, self.ip, self.port)
    }

    /// Icon character to show next to device name.
    pub fn icon(&self) -> &'static str {
        match self.device_type {
            DeviceType::Mobile => "📱",
            DeviceType::Desktop => "🖥",
            DeviceType::Web => "🌐",
            DeviceType::Headless | DeviceType::Server => "🖧",
        }
    }
}

/// The announcement packet broadcast over multicast UDP (LocalSend-compatible).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Announcement {
    pub alias: String,
    pub device_model: String,
    pub device_type: DeviceType,
    pub fingerprint: String,
    pub port: u16,
    pub protocol: String,
    pub version: String,
    /// Set to true when a device is shutting down (so peers can remove it immediately).
    #[serde(default)]
    pub announce: bool,
}

/// Entry in the peer registry, with a last-seen timestamp for TTL expiry.
struct PeerEntry {
    peer: Peer,
    last_seen: Instant,
}

/// Shared, thread-safe registry of all currently known peers.
#[derive(Clone, Default)]
pub struct PeerRegistry {
    inner: Arc<RwLock<HashMap<String, PeerEntry>>>,
}

impl PeerRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Insert or update a peer (keyed by fingerprint).
    pub fn upsert(&self, peer: Peer) {
        let mut map = self.inner.write().unwrap();
        map.insert(
            peer.fingerprint.clone(),
            PeerEntry {
                peer,
                last_seen: Instant::now(),
            },
        );
    }

    /// Remove a peer by fingerprint (e.g. when they announce shutdown).
    pub fn remove(&self, fingerprint: &str) {
        self.inner.write().unwrap().remove(fingerprint);
    }

    /// Evict peers not seen within the TTL window.
    pub fn evict_stale(&self, ttl_secs: u64) {
        let mut map = self.inner.write().unwrap();
        let threshold = std::time::Duration::from_secs(ttl_secs);
        map.retain(|_, entry| entry.last_seen.elapsed() < threshold);
    }

    /// Get a snapshot of all live peers.
    pub fn all(&self) -> Vec<Peer> {
        self.inner
            .read()
            .unwrap()
            .values()
            .map(|e| e.peer.clone())
            .collect()
    }

    /// Look up a peer by fingerprint.
    pub fn get(&self, fingerprint: &str) -> Option<Peer> {
        self.inner
            .read()
            .unwrap()
            .get(fingerprint)
            .map(|e| e.peer.clone())
    }
}

/// Metadata for a single file in a transfer request.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileInfo {
    pub id: String,
    pub file_name: String,
    pub size: u64,
    pub file_type: String,
}

/// A pending transfer session — kept in memory while waiting for accept/decline.
#[derive(Debug, Clone)]
pub struct PendingSession {
    pub session_id: String,
    pub sender_alias: String,
    pub sender_ip: String,
    pub sender_port: u16,
    pub files: Vec<FileInfo>,
}
