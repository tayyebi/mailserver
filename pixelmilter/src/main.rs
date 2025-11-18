use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

mod milter;
mod pixel_injector;

use milter::{MilterCallbacks, MilterResult, MilterServer, MilterOptions};
use pixel_injector::PixelInjector;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Socket path for milter communication (Unix socket) or TCP address (e.g., "0.0.0.0:8892")
    #[arg(long, env = "PIXEL_MILTER_ADDRESS")]
    address: Option<String>,

    /// Base URL for tracking pixels
    #[arg(long, env = "PIXEL_BASE_URL", default_value = "https://localhost:8443/pixel?id=")]
    pixel_base_url: String,

    /// Require opt-in header to enable tracking
    #[arg(long, env = "REQUIRE_OPT_IN", default_value = "false", value_parser = parse_bool)]
    require_opt_in: bool,

    /// Header name for opt-in
    #[arg(long, env = "OPT_IN_HEADER", default_value = "X-Track-Open")]
    opt_in_header: String,

    /// Header name for privacy disclosure
    #[arg(long, env = "DISCLOSURE_HEADER", default_value = "X-Tracking-Notice")]
    disclosure_header: String,

    /// Add disclosure header to tracked emails
    #[arg(long, env = "INJECT_DISCLOSURE", default_value = "true", value_parser = parse_bool)]
    inject_disclosure: bool,

    /// Data directory for storing tracking information
    #[arg(long, env = "DATA_DIR", default_value = "/data/pixel")]
    data_dir: PathBuf,

    /// Path to domain-wide footer HTML file
    #[arg(long, env = "FOOTER_HTML_FILE", default_value = "/opt/pixelmilter/domain-wide-footer.html")]
    footer_html_file: PathBuf,

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

#[derive(Debug, Clone)]
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
    footer_html_file: PathBuf,
    milter_options: MilterOptions,
}

#[derive(Clone)]
struct PixelMilter {
    config: PixelMilterConfig,
    connections: ConnectionState,
    injector: PixelInjector,
}

impl PixelMilter {
    fn new(config: PixelMilterConfig) -> Result<Self> {
        // Load footer HTML if file exists
        let footer_html = if config.footer_html_file.exists() {
            debug!(footer_file = ?config.footer_html_file, "Loading footer HTML file");
            match fs::read_to_string(&config.footer_html_file) {
                Ok(content) => {
                    info!(
                        footer_file = ?config.footer_html_file,
                        footer_size = content.len(),
                        "Footer HTML loaded successfully"
                    );
                    Some(content)
                }
                Err(e) => {
                    warn!(
                        footer_file = ?config.footer_html_file,
                        error = %e,
                        "Failed to load footer HTML file, continuing without footer"
                    );
                    None
                }
            }
        } else {
            debug!(
                footer_file = ?config.footer_html_file,
                "Footer HTML file does not exist, continuing without footer"
            );
            None
        };

        let injector = if let Some(footer) = footer_html {
            PixelInjector::with_footer(config.pixel_base_url.clone(), footer)
        } else {
            PixelInjector::new(config.pixel_base_url.clone())
        };

        Ok(Self {
            injector,
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    async fn save_message_metadata(&self, message: &MessageState) -> Result<()> {
        debug!(
            message_id = %message.id,
            data_dir = ?self.config.data_dir,
            "Starting to save message metadata"
        );

        let message_dir = self.config.data_dir.join(&message.id);
        debug!(message_dir = ?message_dir, "Creating message directory");
        fs::create_dir_all(&message_dir)
            .with_context(|| format!("Failed to create directory: {:?}", message_dir))?;
        info!(message_dir = ?message_dir, "Message directory created");

        // Save headers
        let headers_file = message_dir.join("headers.txt");
        let headers_size = message.raw_headers.len();
        debug!(
            message_id = %message.id,
            headers_file = ?headers_file,
            headers_size = headers_size,
            "Writing headers file"
        );
        fs::write(&headers_file, &message.raw_headers)
            .with_context(|| format!("Failed to write headers to: {:?}", headers_file))?;
        debug!(message_id = %message.id, "Headers file written successfully");

        // Save body
        let body_file = message_dir.join("body.txt");
        let body_size = message.body.len();
        debug!(
            message_id = %message.id,
            body_file = ?body_file,
            body_size = body_size,
            "Writing body file"
        );
        fs::write(&body_file, &message.body)
            .with_context(|| format!("Failed to write body to: {:?}", body_file))?;
        debug!(message_id = %message.id, "Body file written successfully");

        // Save metadata
        let total_size = message.raw_headers.len() + message.body.len();
        let metadata = MessageMetadata {
            id: message.id.clone(),
            created: Utc::now(),
            sender: message.sender.clone(),
            size: total_size,
            tracking_enabled: message.should_track,
            opened: false,
            open_count: 0,
            first_open: None,
            last_open: None,
            tracking_events: Vec::new(),
        };

        let meta_file = message_dir.join("meta.json");
        debug!(
            message_id = %message.id,
            meta_file = ?meta_file,
            "Serializing metadata"
        );
        let meta_json = serde_json::to_string_pretty(&metadata)
            .context("Failed to serialize metadata")?;
        debug!(
            message_id = %message.id,
            meta_json_size = meta_json.len(),
            "Writing metadata file"
        );
        fs::write(&meta_file, meta_json)
            .with_context(|| format!("Failed to write metadata to: {:?}", meta_file))?;

        info!(
            message_id = %message.id,
            sender = %message.sender,
            tracking_enabled = message.should_track,
            total_size = total_size,
            headers_size = headers_size,
            body_size = body_size,
            "Message metadata saved successfully"
        );

        Ok(())
    }
}

#[async_trait::async_trait]
impl MilterCallbacks for PixelMilter {
    fn get_milter_options(&self) -> &MilterOptions {
        &self.config.milter_options
    }

    async fn connect(&self, ctx_id: &str, hostname: &str, addr: &str) -> MilterResult {
        debug!(
            ctx_id = %ctx_id,
            hostname = %hostname,
            addr = %addr,
            "Processing connect callback"
        );
        
        // Create an initial connection state to handle early protocol events
        let message = MessageState::new(String::new());
        let mut connections = self.connections.write().await;
        connections.insert(ctx_id.to_string(), message);
        
        info!(
            ctx_id = %ctx_id,
            hostname = %hostname,
            addr = %addr,
            connection_count = connections.len(),
            "New milter connection established and state created"
        );
        MilterResult::Continue
    }

    async fn mail_from(&self, ctx_id: &str, sender: &str) -> MilterResult {
        debug!(
            ctx_id = %ctx_id,
            sender = %sender,
            "Processing mail_from callback"
        );
        
        let mut connections = self.connections.write().await;
        let connection_count = connections.len();
        
        // Update existing connection or create new one if it doesn't exist
        let message = if let Some(existing_msg) = connections.get_mut(ctx_id) {
            // Update the existing message with sender information
            existing_msg.sender = sender.to_string();
            existing_msg.id = generate_message_id();
            // Clear any previous data for a new message
            existing_msg.headers.clear();
            existing_msg.raw_headers.clear();
            existing_msg.body.clear();
            existing_msg.should_track = false;
            debug!(
                ctx_id = %ctx_id,
                message_id = %existing_msg.id,
                "Updated existing message state"
            );
            existing_msg.clone()
        } else {
            // Create new message state if connection doesn't exist
            let new_message = MessageState::new(sender.to_string());
            debug!(
                ctx_id = %ctx_id,
                message_id = %new_message.id,
                "Created new message state"
            );
            connections.insert(ctx_id.to_string(), new_message.clone());
            new_message
        };

        info!(
            ctx_id = %ctx_id,
            message_id = %message.id,
            sender = %sender,
            active_connections = connections.len(),
            previous_connections = connection_count,
            "Mail from address received and stored"
        );

        MilterResult::Continue
    }

    async fn header(&self, ctx_id: &str, name: &str, value: &str) -> MilterResult {
        debug!(
            ctx_id = %ctx_id,
            header_name = %name,
            header_value_len = value.len(),
            "Processing header callback"
        );
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.get_mut(ctx_id) {
            message.raw_headers.push_str(&format!("{}: {}\n", name, value));
            message.headers.insert(name.to_lowercase(), value.to_string());
            debug!(
                ctx_id = %ctx_id,
                message_id = %message.id,
                header_name = %name,
                total_headers = message.headers.len(),
                "Header added to message"
            );

            // Check for opt-in header
            if self.config.require_opt_in && name.to_lowercase() == self.config.opt_in_header.to_lowercase() {
                let previous_tracking = message.should_track;
                message.should_track = matches!(value.to_lowercase().as_str(), "yes" | "true" | "1" | "on");
                info!(
                    ctx_id = %ctx_id,
                    message_id = %message.id,
                    header = %name,
                    value = %value,
                    tracking_enabled = message.should_track,
                    previous_tracking = previous_tracking,
                    "Opt-in header found and processed"
                );
            }
        } else {
            warn!(
                ctx_id = %ctx_id,
                "Received header for unknown connection context - creating new state"
            );
            // Create a new connection state to handle orphaned events
            let mut connections = self.connections.write().await;
            let message = MessageState::new(String::new());
            connections.insert(ctx_id.to_string(), message);
        }

        MilterResult::Continue
    }

    async fn end_of_headers(&self, ctx_id: &str) -> MilterResult {
        debug!(ctx_id = %ctx_id, "Processing end_of_headers callback");
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.get_mut(ctx_id) {
            message.raw_headers.push('\n');
            let header_count = message.headers.len();
            let headers_size = message.raw_headers.len();

            // If opt-in is not required, enable tracking by default
            let previous_tracking = message.should_track;
            if !self.config.require_opt_in {
                message.should_track = true;
            }

            info!(
                ctx_id = %ctx_id,
                message_id = %message.id,
                tracking_enabled = message.should_track,
                previous_tracking = previous_tracking,
                require_opt_in = self.config.require_opt_in,
                header_count = header_count,
                headers_size = headers_size,
                "End of headers reached"
            );
        } else {
            warn!(
                ctx_id = %ctx_id,
                "Received end_of_headers for unknown connection context - creating new state"
            );
            // Create a new connection state to handle orphaned events
            let mut connections = self.connections.write().await;
            let mut message = MessageState::new(String::new());
            if !self.config.require_opt_in {
                message.should_track = true;
            }
            connections.insert(ctx_id.to_string(), message);
        }

        MilterResult::Continue
    }

    async fn body(&self, ctx_id: &str, chunk: &[u8]) -> MilterResult {
        let chunk_size = chunk.len();
        debug!(
            ctx_id = %ctx_id,
            chunk_size = chunk_size,
            "Processing body chunk"
        );
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.get_mut(ctx_id) {
            let previous_size = message.body.len();
            message.body.extend_from_slice(chunk);
            let new_size = message.body.len();
            debug!(
                ctx_id = %ctx_id,
                message_id = %message.id,
                chunk_size = chunk_size,
                previous_body_size = previous_size,
                new_body_size = new_size,
                "Body chunk appended"
            );
        } else {
            warn!(
                ctx_id = %ctx_id,
                chunk_size = chunk_size,
                "Received body chunk for unknown connection context - creating new state"
            );
            // Create a new connection state and store the body chunk
            let mut message = MessageState::new(String::new());
            message.body.extend_from_slice(chunk);
            connections.insert(ctx_id.to_string(), message);
        }

        MilterResult::Continue
    }

    async fn end_of_message(&self, ctx_id: &str) -> MilterResult {
        debug!(ctx_id = %ctx_id, "Processing end_of_message callback");
        let mut connections = self.connections.write().await;
        if let Some(message) = connections.remove(ctx_id) {
            let message_size = message.raw_headers.len() + message.body.len();
            info!(
                ctx_id = %ctx_id,
                message_id = %message.id,
                sender = %message.sender,
                message_size = message_size,
                headers_size = message.raw_headers.len(),
                body_size = message.body.len(),
                tracking_enabled = message.should_track,
                "End of message reached"
            );

            // Save message metadata
            debug!(
                ctx_id = %ctx_id,
                message_id = %message.id,
                "Saving message metadata"
            );
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
                // Check Content-Type header to determine if it's HTML
                let content_type = message.headers
                    .get("content-type")
                    .map(|s| s.to_lowercase())
                    .unwrap_or_default();
                
                let is_html = content_type.contains("text/html");
                
                debug!(
                    ctx_id = %ctx_id,
                    message_id = %message.id,
                    content_type = %content_type,
                    is_html = is_html,
                    body_size = message.body.len(),
                    "Checking Content-Type for pixel injection"
                );

                if is_html {
                    debug!(
                        ctx_id = %ctx_id,
                        message_id = %message.id,
                        body_size = message.body.len(),
                        "HTML Content-Type detected, attempting to inject tracking pixel"
                    );
                    match self.injector.inject_pixel(&message.body, &message.id, true) {
                        Ok(modified_body) => {
                            let original_size = message.body.len();
                            let modified_size = modified_body.len();
                            if modified_body != message.body {
                                let size_diff = modified_size as i64 - original_size as i64;
                                info!(
                                    ctx_id = %ctx_id,
                                    message_id = %message.id,
                                    original_size = original_size,
                                    modified_size = modified_size,
                                    size_increase = size_diff,
                                    "Pixel injected successfully"
                                );
                                return MilterResult::ReplaceBody(modified_body);
                            } else {
                                info!(
                                    ctx_id = %ctx_id,
                                    message_id = %message.id,
                                    body_size = original_size,
                                    "Pixel injection completed but body unchanged"
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
                        content_type = %content_type,
                        "Non-HTML Content-Type, skipping pixel injection"
                    );
                }
            } else {
                info!(
                    ctx_id = %ctx_id,
                    message_id = %message.id,
                    "Tracking disabled for message, skipping pixel injection"
                );
            }
        } else {
            warn!(
                ctx_id = %ctx_id,
                "Received end_of_message for unknown connection context - no message to process"
            );
            // Create a minimal connection state just to prevent further errors
            let mut connections = self.connections.write().await;
            let message = MessageState::new(String::new());
            connections.insert(ctx_id.to_string(), message);
        }

        debug!(ctx_id = %ctx_id, "Accepting message");
        MilterResult::Accept
    }

    async fn close(&self, ctx_id: &str) -> MilterResult {
        debug!(ctx_id = %ctx_id, "Processing close callback");
        let mut connections = self.connections.write().await;
        let removed = connections.remove(ctx_id);
        if removed.is_some() {
            info!(
                ctx_id = %ctx_id,
                remaining_connections = connections.len(),
                "Connection closed and cleaned up"
            );
        } else {
            debug!(
                ctx_id = %ctx_id,
                "Close called for non-existent connection"
            );
        }
        MilterResult::Continue
    }
}

fn parse_bool(s: &str) -> Result<bool, String> {
    match s.to_lowercase().as_str() {
        "true" | "1" | "yes" | "on" | "t" | "y" => Ok(true),
        "false" | "0" | "no" | "off" | "f" | "n" => Ok(false),
        _ => Err(format!(
            "Invalid boolean value: '{}'. Expected one of: true, false, 1, 0, yes, no, on, off",
            s
        )),
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

#[tokio::main]
async fn main() -> Result<()> {
    // Parse arguments first - this may exit with code 2 on parse errors
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(e) => {
            eprintln!("Error parsing arguments: {}", e);
            std::process::exit(2);
        }
    };

    // Setup logging - if this fails, we can't log, so print to stderr
    if let Err(e) = setup_logging(&args.log_level) {
        eprintln!("Failed to setup logging: {}", e);
        std::process::exit(1);
    }

    info!("Starting Pixel Milter");
    info!("Pixel Base URL: {}", args.pixel_base_url);
    info!("Require Opt-in: {}", args.require_opt_in);
    info!("Data Directory: {:?}", args.data_dir);

    // Determine connection type
    let use_inet = args.address.as_ref()
        .map(|a| a.contains(':') && !a.starts_with('/'))
        .unwrap_or(false);
    
    let address = args.address.clone().unwrap_or_else(|| {
        "/var/run/pixelmilter/pixel.sock".to_string()
    });

    if use_inet {
        info!("Using TCP/inet connection: {}", address);
    } else {
        info!("Using Unix socket: {}", address);
        let socket_path = PathBuf::from(&address);
        
        // Ensure socket directory exists
        if let Some(socket_dir) = socket_path.parent() {
            debug!(socket_dir = ?socket_dir, "Checking socket directory");
            if let Err(e) = fs::create_dir_all(socket_dir) {
                error!(
                    socket_dir = ?socket_dir,
                    error = %e,
                    "Failed to create socket directory"
                );
                eprintln!("Failed to create socket directory {:?}: {}", socket_dir, e);
                std::process::exit(1);
            }
            info!(socket_dir = ?socket_dir, "Socket directory ready");
        }

        // Remove existing socket
        if socket_path.exists() {
            warn!(socket = ?socket_path, "Removing existing socket file");
            if let Err(e) = fs::remove_file(&socket_path) {
                error!(
                    socket = ?socket_path,
                    error = %e,
                    "Failed to remove existing socket"
                );
                eprintln!("Failed to remove existing socket {:?}: {}", socket_path, e);
                std::process::exit(1);
            }
            info!(socket = ?socket_path, "Existing socket file removed");
        } else {
            debug!(socket = ?socket_path, "Socket file does not exist, will create new one");
        }
    }

    // Ensure data directory exists
    debug!(data_dir = ?args.data_dir, "Checking data directory");
    if let Err(e) = fs::create_dir_all(&args.data_dir) {
        error!(
            data_dir = ?args.data_dir,
            error = %e,
            "Failed to create data directory"
        );
        eprintln!("Failed to create data directory {:?}: {}", args.data_dir, e);
        std::process::exit(1);
    }
    info!(data_dir = ?args.data_dir, "Data directory ready");

    let config = PixelMilterConfig {
        pixel_base_url: args.pixel_base_url,
        require_opt_in: args.require_opt_in,
        opt_in_header: args.opt_in_header,
        disclosure_header: args.disclosure_header,
        inject_disclosure: args.inject_disclosure,
        data_dir: args.data_dir,
        footer_html_file: args.footer_html_file,
        milter_options: MilterOptions::default(),
    };

    debug!("Creating PixelMilter instance");
    let milter = PixelMilter::new(config.clone())
        .context("Failed to create PixelMilter instance")?;
    
    let milter_options = MilterOptions::default();
    info!(
        milter_protocol_version = milter_options.protocol_version,
        milter_action_flags = milter_options.action_flags,
        milter_step_flags = milter_options.step_flags,
        "Milter options initialized"
    );

    debug!("Creating MilterServer instance");
    let server = MilterServer::new(milter);

    info!(
        pixel_base_url = %config.pixel_base_url,
        require_opt_in = config.require_opt_in,
        opt_in_header = %config.opt_in_header,
        disclosure_header = %config.disclosure_header,
        inject_disclosure = config.inject_disclosure,
        "Pixel Milter initialized successfully"
    );

    // Set up signal handling for graceful shutdown
    let socket_path = if !use_inet {
        Some(PathBuf::from(&address))
    } else {
        None
    };
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.expect("Failed to listen for Ctrl+C");
        info!("Received shutdown signal, cleaning up...");
        if let Some(ref path) = socket_path {
            if path.exists() {
                let _ = fs::remove_file(path);
            }
        }
        std::process::exit(0);
    });

    // Run the server - this should never return unless there's an error
    if use_inet {
        if let Err(e) = server.run_inet(&address).await {
            error!(error = %e, "Fatal error running milter server");
            eprintln!("Fatal error: {}", e);
            std::process::exit(1);
        }
    } else {
        let socket_path = PathBuf::from(&address);
        if let Err(e) = server.run_unix(&socket_path).await {
            error!(error = %e, "Fatal error running milter server");
            eprintln!("Fatal error: {}", e);
            std::process::exit(1);
        }
    }

    // This should never be reached, but if it is, log it
    error!("Milter server exited unexpectedly");
    std::process::exit(1);
}
