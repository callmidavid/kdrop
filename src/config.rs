use anyhow::Result;
use directories::UserDirs;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use uuid::Uuid;

/// Port used for both the HTTP server and multicast UDP discovery.
/// Same as LocalSend — allows cross-app discovery.
pub const KDROP_PORT: u16 = 53317;

/// Multicast group address for peer discovery (same as LocalSend v2).
pub const MULTICAST_ADDR: &str = "224.0.0.167";

/// How often (seconds) to broadcast our presence to the network.
pub const ANNOUNCE_INTERVAL_SECS: u64 = 5;

/// How long (seconds) before a peer is considered gone if no announcement received.
pub const PEER_TTL_SECS: u64 = 15;

/// Protocol version advertised during discovery.
pub const PROTOCOL_VERSION: &str = "2.0";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub enum DeviceType {
    Mobile,
    Desktop,
    Web,
    Headless,
    Server,
}

impl std::fmt::Display for DeviceType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DeviceType::Mobile => write!(f, "mobile"),
            DeviceType::Desktop => write!(f, "desktop"),
            DeviceType::Web => write!(f, "web"),
            DeviceType::Headless => write!(f, "headless"),
            DeviceType::Server => write!(f, "server"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Config {
    /// Unique device fingerprint (UUID), persisted across sessions.
    pub fingerprint: String,
    /// Human-readable device name shown to peers.
    pub alias: String,
    /// Device type for icon hints.
    pub device_type: DeviceType,
    /// OS/model string shown to peers.
    pub device_model: String,
    /// Directory where received files are saved.
    pub download_dir: PathBuf,
    /// HTTP server port (default: KDROP_PORT).
    pub port: u16,
}

impl Config {
    /// Load config from disk or create a new default config.
    pub fn load_or_create() -> Result<Self> {
        let path = config_file_path();
        if path.exists() {
            let raw = std::fs::read_to_string(&path)?;
            let cfg: Config = serde_json::from_str(&raw)?;
            return Ok(cfg);
        }

        let cfg = Config::default();
        cfg.save()?;
        Ok(cfg)
    }

    pub fn save(&self) -> Result<()> {
        let path = config_file_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, serde_json::to_string_pretty(self)?)?;
        Ok(())
    }

    /// Returns the URL of this device's HTTP server on the local network.
    pub fn local_url(&self) -> String {
        let ip = local_ip_address::local_ip()
            .map(|ip| ip.to_string())
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        format!("http://{}:{}", ip, self.port)
    }
}

impl Default for Config {
    fn default() -> Self {
        let alias = hostname::get()
            .ok()
            .and_then(|h| h.into_string().ok())
            .unwrap_or_else(|| "Linux Device".to_string());

        let download_dir = UserDirs::new()
            .and_then(|u| u.download_dir().map(|p| p.to_path_buf()))
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("kdrop");

        Self {
            fingerprint: Uuid::new_v4().to_string(),
            alias,
            device_type: DeviceType::Desktop,
            device_model: "Linux Desktop".to_string(),
            download_dir,
            port: KDROP_PORT,
        }
    }
}

fn config_file_path() -> PathBuf {
    directories::ProjectDirs::from("io", "kdrop", "kdrop")
        .map(|dirs| dirs.config_dir().join("config.json"))
        .unwrap_or_else(|| PathBuf::from("/tmp/kdrop-config.json"))
}
