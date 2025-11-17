use anyhow::{Context, Result};
use axum::{
    extract::{Path, Query},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Extension, Json, Router,
};
use axum_server::tls_rustls::RustlsConfig;
use chrono::{DateTime, Utc};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::fs;
use std::net::SocketAddr;
use std::path::{Path as FsPath, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

mod pixel;
mod stats;

use pixel::PIXEL_GIF;
use stats::StatsCollector;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Data directory for storing tracking information
    #[arg(long, env = "DATA_DIR", default_value = "/data/pixel")]
    data_dir: PathBuf,

    /// TLS certificate file path
    #[arg(long, env = "PIXEL_TLS_CERT", default_value = "/etc/ssl/certs/server.pem")]
    tls_cert: PathBuf,

    /// TLS private key file path
    #[arg(long, env = "PIXEL_TLS_KEY", default_value = "/etc/ssl/private/server.key")]
    tls_key: PathBuf,

    /// Server bind address
    #[arg(long, env = "BIND_ADDRESS", default_value = "0.0.0.0:8443")]
    bind_address: SocketAddr,

    /// Log level
    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    log_level: String,

    /// Enable development mode (HTTP instead of HTTPS)
    #[arg(long, env = "DEV_MODE", default_value = "false")]
    dev_mode: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MessageMetadata {
    id: String,
    created: DateTime<Utc>,
    created_str: String,
    sender: String,
    size: usize,
    tracking_enabled: bool,
    opened: bool,
    open_count: u32,
    first_open: Option<DateTime<Utc>>,
    first_open_str: Option<String>,
    last_open: Option<DateTime<Utc>>,
    last_open_str: Option<String>,
    tracking_events: Vec<TrackingEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrackingEvent {
    timestamp: DateTime<Utc>,
    timestamp_str: String,
    client_ip: String,
    user_agent: String,
}

#[derive(Debug, Clone, Serialize)]
struct HealthResponse {
    status: String,
    timestamp: DateTime<Utc>,
    data_dir: String,
    data_dir_exists: bool,
    data_dir_writable: bool,
    version: String,
}

#[derive(Debug, Clone, Serialize)]
struct StatusResponse {
    status: String,
    service: String,
    version: String,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize)]
struct ErrorResponse {
    error: String,
    timestamp: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
struct PixelQuery {
    id: Option<String>,
}

#[derive(Debug, Clone)]
struct AppState {
    data_dir: PathBuf,
    stats: Arc<RwLock<StatsCollector>>,
}

impl AppState {
    fn new(data_dir: PathBuf) -> Self {
        Self {
            data_dir,
            stats: Arc::new(RwLock::new(StatsCollector::new())),
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut args = Args::parse();

    setup_logging(&args.log_level)?;

    info!("Starting Pixel Server");
    info!("Data Directory: {:?}", args.data_dir);
    info!("Bind Address: {}", args.bind_address);
    info!("TLS Cert: {:?}", args.tls_cert);
    info!("TLS Key: {:?}", args.tls_key);

    let mut use_tls = !args.dev_mode;
    if use_tls && !tls_assets_available(&args.tls_cert, &args.tls_key) {
        warn!(
            cert = ?args.tls_cert,
            key = ?args.tls_key,
            "TLS certificate or key not found or not a regular file; falling back to development (HTTP) mode"
        );
        use_tls = false;
        args.dev_mode = true;
    }

    info!("Development Mode: {}", args.dev_mode);

    // Ensure data directory exists
    debug!(data_dir = ?args.data_dir, "Checking data directory");
    fs::create_dir_all(&args.data_dir)
        .with_context(|| format!("Failed to create data directory: {:?}", args.data_dir))?;
    info!(data_dir = ?args.data_dir, "Data directory ready");

    debug!("Creating application state");
    let state = AppState::new(args.data_dir.clone());
    info!("Application state created");

    debug!("Creating application router");
    let app = create_app(state);
    info!("Application router created");

    if !use_tls {
        info!(
            bind_address = %args.bind_address,
            "Starting server in development mode (HTTP)"
        );
        axum::Server::bind(&args.bind_address)
            .serve(app.into_make_service())
            .await?;
    } else {
        info!(
            bind_address = %args.bind_address,
            tls_cert = ?args.tls_cert,
            tls_key = ?args.tls_key,
            "Loading TLS configuration"
        );
        let config = RustlsConfig::from_pem_file(&args.tls_cert, &args.tls_key)
            .await
            .context("Failed to load TLS configuration")?;
        info!("TLS configuration loaded successfully");

        info!(
            bind_address = %args.bind_address,
            "Starting server with TLS"
        );
        axum_server::bind_rustls(args.bind_address, config)
            .serve(app.into_make_service_with_connect_info::<SocketAddr>())
            .await?;
    }

    Ok(())
}

fn create_app(state: AppState) -> Router {
    Router::new()
        .route("/", get(status_handler))
        .route("/health", get(health_handler))
        .route("/pixel", get(pixel_handler))
        .route("/msg/:id/meta", get(message_meta_handler))
        .route("/msg/:id/body", get(message_body_handler))
        .route("/msg/:id/headers", get(message_headers_handler))
        .route("/stats", get(stats_handler))
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(Extension(state))
}

async fn status_handler() -> Json<StatusResponse> {
    Json(StatusResponse {
        status: "ok".to_string(),
        service: "pixelserver".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        timestamp: Utc::now(),
    })
}

async fn health_handler(Extension(state): Extension<AppState>) -> Json<HealthResponse> {
    let data_dir_exists = state.data_dir.exists();
    let data_dir_writable = state.data_dir.metadata()
        .map(|m| !m.permissions().readonly())
        .unwrap_or(false);

    Json(HealthResponse {
        status: "ok".to_string(),
        timestamp: Utc::now(),
        data_dir: state.data_dir.to_string_lossy().to_string(),
        data_dir_exists,
        data_dir_writable,
        version: env!("CARGO_PKG_VERSION").to_string(),
    })
}

async fn pixel_handler(
    Query(params): Query<PixelQuery>,
    headers: HeaderMap,
    Extension(state): Extension<AppState>,
) -> Response {
    debug!("Received pixel request");
    let client_ip = extract_client_ip(&headers);
    let user_agent = extract_user_agent(&headers);
    debug!(
        client_ip = %client_ip,
        user_agent_len = user_agent.len(),
        "Extracted client information"
    );

    let message_id = match params.id {
        Some(id) if is_valid_message_id(&id) => {
            debug!(message_id = %id, "Message ID validated");
            id
        }
        Some(id) => {
            warn!(
                message_id = %id,
                client_ip = %client_ip,
                "Invalid message ID format"
            );
            return serve_pixel_response();
        }
        None => {
            warn!(
                client_ip = %client_ip,
                "Pixel request without ID parameter"
            );
            return serve_pixel_response();
        }
    };

    info!(
        message_id = %message_id,
        client_ip = %client_ip,
        user_agent = %user_agent,
        "Processing pixel request"
    );

    // Update tracking data
    debug!(
        message_id = %message_id,
        "Updating tracking data"
    );
    if let Err(e) = update_tracking(&state.data_dir, &message_id, &client_ip, &user_agent).await {
        error!(
            message_id = %message_id,
            error = %e,
            "Failed to update tracking data"
        );
    } else {
        debug!(
            message_id = %message_id,
            "Tracking data updated successfully"
        );
    }

    // Update stats
    debug!(message_id = %message_id, "Updating statistics");
    {
        let mut stats = state.stats.write().await;
        stats.record_pixel_request(&message_id, &client_ip);
    }
    debug!(message_id = %message_id, "Statistics updated");

    serve_pixel_response()
}

async fn message_meta_handler(
    Path(id): Path<String>,
    Extension(state): Extension<AppState>,
) -> Result<Json<MessageMetadata>, (StatusCode, Json<ErrorResponse>)> {
    debug!(message_id = %id, "Received message metadata request");
    
    if !is_valid_message_id(&id) {
        warn!(
            message_id = %id,
            "Invalid message ID format in metadata request"
        );
        return Err((
            StatusCode::BAD_REQUEST,
            Json(ErrorResponse {
                error: "Invalid message ID format".to_string(),
                timestamp: Utc::now(),
            }),
        ));
    }

    let meta_file = state.data_dir.join(&id).join("meta.json");
    debug!(
        message_id = %id,
        meta_file = ?meta_file,
        "Checking metadata file"
    );
    
    if !meta_file.exists() {
        warn!(
            message_id = %id,
            meta_file = ?meta_file,
            "Metadata file not found"
        );
        return Err((
            StatusCode::NOT_FOUND,
            Json(ErrorResponse {
                error: "Message not found".to_string(),
                timestamp: Utc::now(),
            }),
        ));
    }

    debug!(
        message_id = %id,
        "Reading metadata file"
    );
    match fs::read_to_string(&meta_file) {
        Ok(content) => {
            debug!(
                message_id = %id,
                content_size = content.len(),
                "Parsing metadata JSON"
            );
            match serde_json::from_str::<MessageMetadata>(&content) {
                Ok(metadata) => {
                    info!(
                        message_id = %id,
                        sender = %metadata.sender,
                        tracking_enabled = metadata.tracking_enabled,
                        opened = metadata.opened,
                        open_count = metadata.open_count,
                        "Metadata retrieved successfully"
                    );
                    Ok(Json(metadata))
                }
                Err(e) => {
                    error!(
                        message_id = %id,
                        error = %e,
                        content_size = content.len(),
                        "Failed to parse metadata JSON"
                    );
                    Err((
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: "Failed to parse metadata".to_string(),
                            timestamp: Utc::now(),
                        }),
                    ))
                }
            }
        }
        Err(e) => {
            error!(
                message_id = %id,
                error = %e,
                meta_file = ?meta_file,
                "Failed to read metadata file"
            );
            Err((
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Failed to read metadata".to_string(),
                    timestamp: Utc::now(),
                }),
            ))
        }
    }
}

async fn message_body_handler(
    Path(id): Path<String>,
    Extension(state): Extension<AppState>,
) -> Result<String, (StatusCode, String)> {
    if !is_valid_message_id(&id) {
        return Err((StatusCode::BAD_REQUEST, "Invalid message ID format".to_string()));
    }

    let body_file = state.data_dir.join(&id).join("body.txt");
    
    if !body_file.exists() {
        return Err((StatusCode::NOT_FOUND, "Message not found".to_string()));
    }

    match fs::read_to_string(&body_file) {
        Ok(content) => Ok(content),
        Err(e) => {
            error!(message_id = %id, error = %e, "Failed to read body file");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Failed to read message body".to_string()))
        }
    }
}

async fn message_headers_handler(
    Path(id): Path<String>,
    Extension(state): Extension<AppState>,
) -> Result<String, (StatusCode, String)> {
    if !is_valid_message_id(&id) {
        return Err((StatusCode::BAD_REQUEST, "Invalid message ID format".to_string()));
    }

    let headers_file = state.data_dir.join(&id).join("headers.txt");
    
    if !headers_file.exists() {
        return Err((StatusCode::NOT_FOUND, "Message not found".to_string()));
    }

    match fs::read_to_string(&headers_file) {
        Ok(content) => Ok(content),
        Err(e) => {
            error!(message_id = %id, error = %e, "Failed to read headers file");
            Err((StatusCode::INTERNAL_SERVER_ERROR, "Failed to read message headers".to_string()))
        }
    }
}

async fn stats_handler(Extension(state): Extension<AppState>) -> Json<serde_json::Value> {
    let stats = state.stats.read().await;
    let computed_stats = stats.compute_stats(&state.data_dir).await;
    Json(computed_stats)
}

fn serve_pixel_response() -> Response {
    let mut headers = HeaderMap::new();
    headers.insert(header::CONTENT_TYPE, "image/gif".parse().unwrap());
    headers.insert(header::CACHE_CONTROL, "no-cache, no-store, must-revalidate".parse().unwrap());
    headers.insert(header::PRAGMA, "no-cache".parse().unwrap());
    headers.insert(header::EXPIRES, "0".parse().unwrap());

    (StatusCode::OK, headers, PIXEL_GIF).into_response()
}

async fn update_tracking(
    data_dir: &PathBuf,
    message_id: &str,
    client_ip: &str,
    user_agent: &str,
) -> Result<()> {
    let meta_file = data_dir.join(message_id).join("meta.json");
    debug!(
        message_id = %message_id,
        meta_file = ?meta_file,
        client_ip = %client_ip,
        "Starting tracking update"
    );
    
    if !meta_file.exists() {
        debug!(
            message_id = %message_id,
            meta_file = ?meta_file,
            "Metadata file does not exist, skipping update"
        );
        return Ok(()); // Message doesn't exist, ignore
    }

    debug!(
        message_id = %message_id,
        "Reading existing metadata"
    );
    let content = fs::read_to_string(&meta_file)
        .context("Failed to read metadata file")?;
    
    let mut metadata: MessageMetadata = serde_json::from_str(&content)
        .context("Failed to parse metadata")?;

    let previous_open_count = metadata.open_count;
    let previous_opened = metadata.opened;
    let now = Utc::now();
    
    metadata.opened = true;
    metadata.open_count += 1;
    metadata.last_open = Some(now);
    metadata.last_open_str = Some(now.format("%Y-%m-%d %H:%M:%S UTC").to_string());
    
    let is_first_open = metadata.first_open.is_none();
    if is_first_open {
        debug!(
            message_id = %message_id,
            "First open detected"
        );
        metadata.first_open = Some(now);
        metadata.first_open_str = Some(now.format("%Y-%m-%d %H:%M:%S UTC").to_string());
    }
    
    let event_count_before = metadata.tracking_events.len();
    metadata.tracking_events.push(TrackingEvent {
        timestamp: now,
        timestamp_str: now.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
        client_ip: client_ip.to_string(),
        user_agent: user_agent.to_string(),
    });
    debug!(
        message_id = %message_id,
        previous_open_count = previous_open_count,
        new_open_count = metadata.open_count,
        event_count = metadata.tracking_events.len(),
        is_first_open = is_first_open,
        "Tracking event added"
    );
    
    debug!(
        message_id = %message_id,
        "Serializing updated metadata"
    );
    let updated_content = serde_json::to_string_pretty(&metadata)
        .context("Failed to serialize metadata")?;
    
    debug!(
        message_id = %message_id,
        content_size = updated_content.len(),
        "Writing updated metadata to file"
    );
    fs::write(&meta_file, updated_content)
        .context("Failed to write updated metadata")?;

    info!(
        message_id = %message_id,
        client_ip = %client_ip,
        open_count = metadata.open_count,
        previous_open_count = previous_open_count,
        previous_opened = previous_opened,
        is_first_open = is_first_open,
        total_events = metadata.tracking_events.len(),
        event_count_before = event_count_before,
        "Tracking data updated successfully"
    );

    Ok(())
}

fn is_valid_message_id(id: &str) -> bool {
    // Validate message ID format: YYYYMMDD-HHMMSS-UUID
    let re = regex::Regex::new(r"^[0-9]{8}-[0-9]{6}-[a-fA-F0-9-]+$").unwrap();
    re.is_match(id)
}

fn extract_client_ip(headers: &HeaderMap) -> String {
    headers
        .get("x-forwarded-for")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.split(',').next())
        .map(|s| s.trim().to_string())
        .or_else(|| {
            headers
                .get("x-real-ip")
                .and_then(|v| v.to_str().ok())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "unknown".to_string())
}

fn extract_user_agent(headers: &HeaderMap) -> String {
    headers
        .get(header::USER_AGENT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
        .unwrap_or_else(|| "unknown".to_string())
}

fn setup_logging(level: &str) -> Result<()> {
    let filter = match level.to_lowercase().as_str() {
        "trace" => "trace",
        "debug" => "debug",
        "info" => "info",
        "warn" => "warn",
        "error" => "error",
        _ => "info",
    };

    // Configure tracing to write to stderr (which Docker captures via entrypoint 2>&1)
    // The default writer is stderr, which the entrypoint script redirects to stdout
    // Use with_ansi(false) for better Docker log compatibility
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .with_ansi(false) // Disable ANSI colors for Docker logs
        // Default writer is stderr - entrypoint redirects stderr to stdout for docker logs
        .init();

    Ok(())
}

fn tls_assets_available(cert: &PathBuf, key: &PathBuf) -> bool {
    fn is_regular_file(path: &FsPath) -> bool {
        path.is_file()
    }

    is_regular_file(cert.as_path()) && is_regular_file(key.as_path())
}
