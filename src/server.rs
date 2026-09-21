use crate::config::Config;
use crate::peer::{FileInfo, PendingSession, PeerRegistry};
use anyhow::Result;
use axum::{
    body::Bytes,
    extract::{Multipart, Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{sse::Event, IntoResponse, Response, Sse},
    routing::{get, post},
    Json, Router,
};
use futures::stream::Stream;
use image::Luma;
use qrcode::QrCode;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::convert::Infallible;
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;
use tracing::{error, info, warn};
use uuid::Uuid;

// Embedded static web UI files
const INDEX_HTML: &str = include_str!("web/index.html");
const STYLE_CSS: &str = include_str!("web/style.css");
const APP_JS: &str = include_str!("web/app.js");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    pub alias: String,
    pub device_model: String,
    pub device_type: String,
    pub fingerprint: String,
    pub port: u16,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SharedFileItem {
    pub id: String,
    pub file_name: String,
    pub size: u64,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendRequestPayload {
    pub sender_alias: String,
    pub sender_fingerprint: String,
    pub files: Vec<FileInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmPayload {
    pub accepted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum AppEvent {
    #[serde(rename = "transfer_request")]
    TransferRequest {
        session_id: String,
        sender_alias: String,
        files: Vec<FileInfo>,
    },
    #[serde(rename = "transfer_decision")]
    TransferDecision {
        session_id: String,
        accepted: bool,
    },
    #[serde(rename = "files_updated")]
    FilesUpdated,
}

#[derive(Clone)]
pub struct AppState {
    pub config: Arc<Config>,
    pub registry: PeerRegistry,
    pub pending_sessions: Arc<Mutex<HashMap<String, PendingSession>>>,
    pub session_decisions: Arc<Mutex<HashMap<String, bool>>>,
    pub shared_files: Arc<Mutex<Vec<SharedFileItem>>>,
    pub event_tx: broadcast::Sender<AppEvent>,
}

impl AppState {
    pub fn new(config: Arc<Config>, registry: PeerRegistry) -> Self {
        let (event_tx, _) = broadcast::channel(100);
        Self {
            config,
            registry,
            pending_sessions: Arc::new(Mutex::new(HashMap::new())),
            session_decisions: Arc::new(Mutex::new(HashMap::new())),
            shared_files: Arc::new(Mutex::new(Vec::new())),
            event_tx,
        }
    }

    pub fn add_shared_file(&self, path: PathBuf) -> Result<SharedFileItem> {
        let metadata = std::fs::metadata(&path)?;
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("file")
            .to_string();

        let item = SharedFileItem {
            id: Uuid::new_v4().to_string(),
            file_name,
            size: metadata.len(),
            path,
        };

        self.shared_files.lock().unwrap().push(item.clone());
        let _ = self.event_tx.send(AppEvent::FilesUpdated);
        Ok(item)
    }
}

pub fn create_router(state: AppState) -> Router {
    Router::new()
        // Web frontend
        .route("/", get(serve_index))
        .route("/style.css", get(serve_css))
        .route("/app.js", get(serve_js))
        .route("/qr", get(serve_qr))
        // API endpoints
        .route("/api/info", get(get_info))
        .route("/api/files", get(list_files))
        .route("/api/files/:id", get(download_file))
        .route("/api/upload", post(handle_upload))
        .route("/api/send/request", post(handle_send_request))
        .route("/api/send/status/:session_id", get(handle_check_status))
        .route("/api/send/confirm/:session_id", post(handle_confirm))
        .route("/api/receive/:session_id/:file_id", post(handle_receive_file))
        .route("/api/events", get(sse_handler))
        .layer(CorsLayer::permissive())
        .with_state(state)
}

// Handler implementations
async fn serve_index() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/html; charset=utf-8")], INDEX_HTML)
}

async fn serve_css() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "text/css; charset=utf-8")], STYLE_CSS)
}

async fn serve_js() -> impl IntoResponse {
    ([(header::CONTENT_TYPE, "application/javascript; charset=utf-8")], APP_JS)
}

async fn serve_qr(State(state): State<AppState>) -> impl IntoResponse {
    let url = state.config.local_url();
    let qr = QrCode::new(url.as_bytes()).unwrap();
    let img = qr.render::<Luma<u8>>().max_dimensions(256, 256).build();

    let mut buffer = Cursor::new(Vec::new());
    if img.write_to(&mut buffer, image::ImageFormat::Png).is_ok() {
        (
            [(header::CONTENT_TYPE, "image/png")],
            buffer.into_inner(),
        ).into_response()
    } else {
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to render QR").into_response()
    }
}

async fn get_info(State(state): State<AppState>) -> Json<ServerInfo> {
    Json(ServerInfo {
        alias: state.config.alias.clone(),
        device_model: state.config.device_model.clone(),
        device_type: state.config.device_type.to_string(),
        fingerprint: state.config.fingerprint.clone(),
        port: state.config.port,
        version: "2.0".to_string(),
    })
}

async fn list_files(State(state): State<AppState>) -> Json<Vec<SharedFileItem>> {
    let files = state.shared_files.lock().unwrap().clone();
    Json(files)
}

async fn download_file(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Response {
    let file_opt = {
        let files = state.shared_files.lock().unwrap();
        files.iter().find(|f| f.id == id).cloned()
    };

    if let Some(item) = file_opt {
        if let Ok(bytes) = tokio::fs::read(&item.path).await {
            let mime = mime_guess::from_path(&item.path)
                .first_or_octet_stream()
                .to_string();

            return (
                [
                    (header::CONTENT_TYPE, mime),
                    (
                        header::CONTENT_DISPOSITION,
                        format!("attachment; filename=\"{}\"", item.file_name),
                    ),
                ],
                bytes,
            )
                .into_response();
        }
    }

    (StatusCode::NOT_FOUND, "File not found").into_response()
}

async fn handle_upload(
    State(state): State<AppState>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let download_dir = state.config.download_dir.clone();
    let _ = tokio::fs::create_dir_all(&download_dir).await;

    let mut saved_any = false;

    while let Ok(Some(field)) = multipart.next_field().await {
        let file_name = field
            .file_name()
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("upload-{}", Uuid::new_v4()));

        if let Ok(data) = field.bytes().await {
            let target_path = download_dir.join(&file_name);
            if let Ok(_) = tokio::fs::write(&target_path, &data).await {
                info!("Saved uploaded file: {:?}", target_path);
                let _ = state.add_shared_file(target_path);
                saved_any = true;
            }
        }
    }

    if saved_any {
        StatusCode::OK
    } else {
        StatusCode::BAD_REQUEST
    }
}

async fn handle_send_request(
    State(state): State<AppState>,
    Json(payload): Json<SendRequestPayload>,
) -> impl IntoResponse {
    let session_id = Uuid::new_v4().to_string();

    let session = PendingSession {
        session_id: session_id.clone(),
        sender_alias: payload.sender_alias.clone(),
        sender_ip: String::new(),
        sender_port: 0,
        files: payload.files.clone(),
    };

    state
        .pending_sessions
        .lock()
        .unwrap()
        .insert(session_id.clone(), session);

    let _ = state.event_tx.send(AppEvent::TransferRequest {
        session_id: session_id.clone(),
        sender_alias: payload.sender_alias,
        files: payload.files,
    });

    Json(serde_json::json!({
        "sessionId": session_id,
        "status": "pending"
    }))
}

async fn handle_check_status(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let decisions = state.session_decisions.lock().unwrap();
    match decisions.get(&session_id) {
        Some(true) => Json(serde_json::json!({ "status": "accepted" })),
        Some(false) => Json(serde_json::json!({ "status": "declined" })),
        None => Json(serde_json::json!({ "status": "pending" })),
    }
}

async fn handle_confirm(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(payload): Json<ConfirmPayload>,
) -> impl IntoResponse {
    state
        .session_decisions
        .lock()
        .unwrap()
        .insert(session_id.clone(), payload.accepted);

    let _ = state.event_tx.send(AppEvent::TransferDecision {
        session_id,
        accepted: payload.accepted,
    });

    StatusCode::OK
}

async fn handle_receive_file(
    State(state): State<AppState>,
    Path((session_id, file_id)): Path<(String, String)>,
    body: Bytes,
) -> impl IntoResponse {
    let file_info = {
        let sessions = state.pending_sessions.lock().unwrap();
        sessions
            .get(&session_id)
            .and_then(|s| s.files.iter().find(|f| f.id == file_id).cloned())
    };

    if let Some(info) = file_info {
        let download_dir = state.config.download_dir.clone();
        let _ = tokio::fs::create_dir_all(&download_dir).await;

        let target_path = download_dir.join(&info.file_name);
        if let Ok(_) = tokio::fs::write(&target_path, body).await {
            info!("Received and saved file: {:?}", target_path);
            let _ = state.add_shared_file(target_path);
            return StatusCode::OK;
        }
    }

    StatusCode::INTERNAL_SERVER_ERROR
}

async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let rx = state.event_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| match msg {
        Ok(event) => match event {
            AppEvent::TransferRequest {
                session_id,
                sender_alias,
                files,
            } => {
                let data = serde_json::json!({
                    "sessionId": session_id,
                    "senderAlias": sender_alias,
                    "files": files
                });
                Some(Ok(Event::default()
                    .event("transfer_request")
                    .data(data.to_string())))
            }
            AppEvent::TransferDecision {
                session_id,
                accepted,
            } => {
                let data = serde_json::json!({
                    "sessionId": session_id,
                    "accepted": accepted
                });
                Some(Ok(Event::default()
                    .event("transfer_decision")
                    .data(data.to_string())))
            }
            AppEvent::FilesUpdated => Some(Ok(Event::default().event("files_updated").data("{}"))),
        },
        Err(_) => None,
    });

    Sse::new(stream).keep_alive(axum::response::sse::KeepAlive::new().interval(Duration::from_secs(15)))
}
