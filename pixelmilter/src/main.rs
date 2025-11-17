use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{error, info};
use uuid::Uuid;

mod milter;
mod pixel_injector;

use milter::{MilterCallbacks, MilterResult, MilterServer};
use pixel_injector::PixelInjector;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Socket path for milter communication
    #[arg(long, env = "PIXEL_MILTER_SOCKET", default_value = "/var/run/pixelmilter/pixel.sock")]
    socket: PathBuf,

    /// Base URL for tracking pixels
    #[arg(long, env = "PIXEL_BASE_URL", default_value = "https://localhost:8443/pixel?id=")]
    pixel_base_url: String,

    /// Require opt-in header to enable tracking
    #[arg(long, env = "REQUIRE_OPT_IN", default_value = "false")]
    require_opt_in: bool,

    /// Header name for opt-in
    #[arg(long, env = "OPT_IN_HEADER", default_value = "X-Track-Open")]
    opt_in_header: String,

    /// Header name for privacy disclosure
    #[arg(long, env = "DISCLOSURE_HEADER", default_value = "X-Tracking-Notice")]
    disclosure_header: String,

    /// Add disclosure header to tracked emails
    #[arg(long, env = "INJECT_DISCLOSURE", default_value = "true")]
    inject_disclosure: bool,

    /// Data directory for storing tracking information
    #[arg(long, env = "DATA_DIR", default_value = "/data/pixel")]
    data_dir: PathBuf,

    /// Log level
    #[arg(long, env = "LOG_LEVEL", default_value = "info")]
    log_level: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct MessageMetadata {
    id: String,
    created: DateTime<Utc>,
    sender: String,
    size: usize,
    tracking_enabled: bool,
    opened: bool,
    open_count: u32,
    first_open: Option<DateTime<Utc>>,
    last_open: Option<DateTime<Utc>>,
    tracking_events: Vec<TrackingEvent>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TrackingEvent {
    timestamp: DateTime<Utc>,
    client_ip: String,
    user_agent: String,
}

#[derive(Debug)]
struct MessageState {
    id: String,
    sender: String,
    headers: HashMap<String, String>,
    raw_headers: String,
    body: Vec<u8>,
    should_track: bool,
}

impl MessageState {
    fn new(sender: String) -> Self {
        Self {
            id: generate_message_id(),
            sender,
            headers: HashMap::new(),
            raw_headers: String::new(),
            body: Vec::new(),
            should_track: false,
        }
    }
}

type ConnectionState = Arc<RwLock<HashMap<String, MessageState>>>;

#[derive(Debug, Clone)]
struct PixelMilterConfig {
    pixel_base_url: String,
    require_opt_in: bool,
    opt_in_header: String,
    disclosure_header: String,
    inject_disclosure: bool,
    data_dir: PathBuf,
}

#[derive(Clone)]
struct PixelMilter {
    config: PixelMilterConfig,
    connections: ConnectionState,
    injector: PixelInjector,
}

impl PixelMilter {
    fn new(config: PixelMilterConfig) -> Self {
        Self {
            injector: PixelInjector::new(config.pixel_base_url.clone()),
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    async fn save_message_metadata(&self, message: &MessageState) -> Result<()> {
        let message_dir = self.config.data_dir.join(&message.id);
        fs::create_dir_all(&message_dir)
            .with_context(|| format!("Failed to create directory: {:?}", message_dir))?;

        // Save headers
        let headers_file = message_dir.join("headers.txt");
        fs::write(&headers_file, &message.raw_headers)
            .with_context(|| format!("Failed to write headers to: {:?}", headers_file))?;

        // Save body
        let body_file = message_dir.join("body.txt");
        fs::write(&body_file, &message.body)
            .with_context(|| format!("Failed to write body to: {:?}", body_file))?;

        // Save metadata
        let metadata = MessageMetadata {
            id: message.id.clone(),
            created: Utc::now(),
            sender: message.sender.clone(),
            size: message.raw_headers.len() + message.body.len(),
            tracking_enabled: message.should_track,
            opened: false,
            open_count: 0,
            first_open: None,
            last_open: None,
            tracking_events: Vec::new(),
        };

        let meta_file = message_dir.join("meta.json");
        let meta_json = serde_json::to_string_pretty(&metadata)
            .context("Failed to serialize metadata")?;
        fs::write(&meta_file, meta_json)
            .with_context(|| format!("Failed to write metadata to: {:?}", meta_file))?;

        info!(
            message_id = %message.id,
            sender = %message.sender,
            tracking_enabled = message.should_track,
            "Message metadata saved"
        );

        Ok(())
    }
}

#[async_trait::async_trait]
impl MilterCallbacks for PixelMilter {
    async fn connect(&self, ctx_id: &str, hostname: &str, _addr: &str) -> MilterResult {
        info!(ctx_id = %ctx_id, hostname = %hostname, "New connection");
        MilterResult::Continue
    }

    async fn mail_from(&self, ctx_id: &str, sender: &str) -> MilterResult {
        let message = MessageState::new(sender.to_string());
        info!(
            ctx_id = %ctx_id,
            message_id = %message.id,
            sender = %sender,
            "New message"
        );

        let mut connections = self.connections.write().await;
        connections.insert(ctx_id.to_string(), message);

        MilterResult::Continue
    }

    async fn header(&self, ctx_id: &str, name: &str, value: &str) -> MilterResult {
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.get_mut(ctx_id) {
            message.raw_headers.push_str(&format!("{}: {}\n", name, value));
            message.headers.insert(name.to_lowercase(), value.to_string());

            // Check for opt-in header
            if self.config.require_opt_in && name.to_lowercase() == self.config.opt_in_header.to_lowercase() {
                message.should_track = matches!(value.to_lowercase().as_str(), "yes" | "true" | "1" | "on");
                info!(
                    ctx_id = %ctx_id,
                    message_id = %message.id,
                    header = %name,
                    value = %value,
                    tracking = message.should_track,
                    "Opt-in header found"
                );
            }
        }

        MilterResult::Continue
    }

    async fn end_of_headers(&self, ctx_id: &str) -> MilterResult {
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.get_mut(ctx_id) {
            message.raw_headers.push('\n');

            // If opt-in is not required, enable tracking by default
            if !self.config.require_opt_in {
                message.should_track = true;
            }

            info!(
                ctx_id = %ctx_id,
                message_id = %message.id,
                tracking_enabled = message.should_track,
                "End of headers"
            );
        }

        MilterResult::Continue
    }

    async fn body(&self, ctx_id: &str, chunk: &[u8]) -> MilterResult {
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.get_mut(ctx_id) {
            message.body.extend_from_slice(chunk);
        }

        MilterResult::Continue
    }

    async fn end_of_message(&self, ctx_id: &str) -> MilterResult {
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.remove(ctx_id) {
            // Save message metadata
            if let Err(e) = self.save_message_metadata(&message).await {
                error!(
                    ctx_id = %ctx_id,
                    message_id = %message.id,
                    error = %e,
                    "Failed to save message metadata"
                );
                return MilterResult::Accept;
            }

            // Inject pixel if tracking is enabled
            if message.should_track {
                match self.injector.inject_pixel(&message.body, &message.id) {
                    Ok(modified_body) => {
                        if modified_body != message.body {
                            info!(
                                ctx_id = %ctx_id,
                                message_id = %message.id,
                                "Pixel injected successfully"
                            );
                            return MilterResult::ReplaceBody(modified_body);
                        } else {
                            info!(
                                ctx_id = %ctx_id,
                                message_id = %message.id,
                                "No HTML content found for pixel injection"
                            );
                        }
                    }
                    Err(e) => {
                        error!(
                            ctx_id = %ctx_id,
                            message_id = %message.id,
                            error = %e,
                            "Failed to inject pixel"
                        );
                    }
                }
            } else {
                info!(
                    ctx_id = %ctx_id,
                    message_id = %message.id,
                    "Tracking disabled for message"
                );
            }
        }

        MilterResult::Accept
    }

    async fn close(&self, ctx_id: &str) -> MilterResult {
        let mut connections = self.connections.write().await;
        connections.remove(ctx_id);
        MilterResult::Continue
    }
}

fn generate_message_id() -> String {
    let now = Utc::now();
    let uuid = Uuid::new_v4();
    format!("{}-{}", now.format("%Y%m%d-%H%M%S"), uuid)
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

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(false)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    setup_logging(&args.log_level)?;

    info!("Starting Pixel Milter");
    info!("Socket: {:?}", args.socket);
    info!("Pixel Base URL: {}", args.pixel_base_url);
    info!("Require Opt-in: {}", args.require_opt_in);
    info!("Data Directory: {:?}", args.data_dir);

    // Ensure data directory exists
    fs::create_dir_all(&args.data_dir)
        .with_context(|| format!("Failed to create data directory: {:?}", args.data_dir))?;

    // Ensure socket directory exists
    if let Some(socket_dir) = args.socket.parent() {
        fs::create_dir_all(socket_dir)
            .with_context(|| format!("Failed to create socket directory: {:?}", socket_dir))?;
    }

    // Remove existing socket
    if args.socket.exists() {
        fs::remove_file(&args.socket)
            .with_context(|| format!("Failed to remove existing socket: {:?}", args.socket))?;
    }

    let config = PixelMilterConfig {
        pixel_base_url: args.pixel_base_url,
        require_opt_in: args.require_opt_in,
        opt_in_header: args.opt_in_header,
        disclosure_header: args.disclosure_header,
        inject_disclosure: args.inject_disclosure,
        data_dir: args.data_dir,
    };

    let milter = PixelMilter::new(config);
    let server = MilterServer::new(milter);

    info!("Pixel Milter started successfully");

    // Set up signal handling for graceful shutdown
    let socket_path = args.socket.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info!("Received shutdown signal, cleaning up...");
        if socket_path.exists() {
            let _ = fs::remove_file(&socket_path);
        }
        std::process::exit(0);
    });

    server.run(&args.socket).await
        .context("Failed to run milter server")?;

    Ok(())
}
