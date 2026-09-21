use crate::config::Config;
use crate::peer::{FileInfo, Peer};
use crate::server::SendRequestPayload;
use anyhow::{anyhow, Context, Result};
use reqwest::Client;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::info;
use uuid::Uuid;

#[derive(Debug, Clone)]
pub enum TransferProgress {
    WaitingForAccept(String), // peer name
    Declined,
    Transferring {
        file_name: String,
        current_file: usize,
        total_files: usize,
        bytes_sent: u64,
        total_bytes: u64,
    },
    Completed,
    Failed(String),
}

pub struct TransferManager {
    client: Client,
    config: Arc<Config>,
}

impl TransferManager {
    pub fn new(config: Arc<Config>) -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(300))
            .build()
            .unwrap_or_default();
        Self { client, config }
    }

    pub async fn send_files<F>(
        &self,
        peer: &Peer,
        paths: Vec<PathBuf>,
        progress_callback: F,
    ) -> Result<()>
    where
        F: Fn(TransferProgress) + Send + Sync + 'static,
    {
        if paths.is_empty() {
            return Ok(());
        }

        progress_callback(TransferProgress::WaitingForAccept(peer.alias.clone()));

        // 1. Gather file metadata
        let mut file_infos = Vec::new();
        let mut total_bytes = 0u64;

        for path in &paths {
            let metadata = tokio::fs::metadata(path)
                .await
                .with_context(|| format!("Failed to read metadata for {:?}", path))?;

            let file_name = path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("file")
                .to_string();

            let size = metadata.len();
            total_bytes += size;

            let mime = mime_guess::from_path(path)
                .first_or_octet_stream()
                .to_string();

            file_infos.push((
                path.clone(),
                FileInfo {
                    id: Uuid::new_v4().to_string(),
                    file_name,
                    size,
                    file_type: mime,
                },
            ));
        }

        // 2. Request transfer
        let base_url = peer.base_url();
        let request_payload = SendRequestPayload {
            sender_alias: self.config.alias.clone(),
            sender_fingerprint: self.config.fingerprint.clone(),
            files: file_infos.iter().map(|(_, info)| info.clone()).collect(),
        };

        let res = self
            .client
            .post(format!("{}/api/send/request", base_url))
            .json(&request_payload)
            .send()
            .await
            .context("Failed to connect to recipient device")?;

        if !res.status().is_success() {
            let err = format!("Device rejected request with status: {}", res.status());
            progress_callback(TransferProgress::Failed(err.clone()));
            return Err(anyhow!(err));
        }

        let resp_json: serde_json::Value = res.json().await?;
        let session_id = resp_json["sessionId"]
            .as_str()
            .ok_or_else(|| anyhow!("Invalid session ID from peer"))?
            .to_string();

        // 3. Poll for decision (up to 60 seconds)
        let mut accepted = false;
        let poll_start = std::time::Instant::now();

        while poll_start.elapsed() < Duration::from_secs(60) {
            tokio::time::sleep(Duration::from_millis(800)).await;

            if let Ok(res) = self
                .client
                .get(format!("{}/api/send/status/{}", base_url, session_id))
                .send()
                .await
            {
                if let Ok(val) = res.json::<serde_json::Value>().await {
                    match val["status"].as_str() {
                        Some("accepted") => {
                            accepted = true;
                            break;
                        }
                        Some("declined") => {
                            progress_callback(TransferProgress::Declined);
                            return Ok(());
                        }
                        _ => {}
                    }
                }
            }
        }

        if !accepted {
            let err = "Transfer timed out waiting for approval".to_string();
            progress_callback(TransferProgress::Failed(err.clone()));
            return Err(anyhow!(err));
        }

        // 4. Stream each file
        let total_files = file_infos.len();
        let mut bytes_accumulated = 0u64;

        for (idx, (path, info)) in file_infos.into_iter().enumerate() {
            let file_bytes = tokio::fs::read(&path)
                .await
                .with_context(|| format!("Failed to read file {:?}", path))?;

            progress_callback(TransferProgress::Transferring {
                file_name: info.file_name.clone(),
                current_file: idx + 1,
                total_files,
                bytes_sent: bytes_accumulated,
                total_bytes,
            });

            let upload_url = format!("{}/api/receive/{}/{}", base_url, session_id, info.id);
            let send_res = self
                .client
                .post(upload_url)
                .body(file_bytes)
                .send()
                .await
                .context("Failed while streaming file to recipient")?;

            if !send_res.status().is_success() {
                let err = format!("Transfer failed for {}: status {}", info.file_name, send_res.status());
                progress_callback(TransferProgress::Failed(err.clone()));
                return Err(anyhow!(err));
            }

            bytes_accumulated += info.size;
        }

        progress_callback(TransferProgress::Completed);
        info!("All files successfully transferred to {}", peer.alias);
        Ok(())
    }
}

