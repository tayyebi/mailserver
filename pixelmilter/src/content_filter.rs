use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{BufRead, Write};
use std::path::PathBuf;
use tracing::{debug, info, trace, warn};
use uuid::Uuid;

use crate::pixel_injector::PixelInjector;

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

fn generate_message_id() -> String {
    let now = Utc::now();
    let uuid = Uuid::new_v4();
    format!("{}-{}", now.format("%Y%m%d-%H%M%S"), uuid)
}

pub struct ContentFilter {
    injector: PixelInjector,
    data_dir: PathBuf,
    tracking_requires_opt_in: bool,
    opt_in_header: String,
    disclosure_header: String,
    inject_disclosure: bool,
}

impl ContentFilter {
    pub fn new(
        pixel_base_url: String,
        footer_html: Option<String>,
        data_dir: PathBuf,
        tracking_requires_opt_in: bool,
        opt_in_header: String,
        disclosure_header: String,
        inject_disclosure: bool,
    ) -> Self {
        let injector = if let Some(footer) = footer_html {
            PixelInjector::with_footer(pixel_base_url, footer)
        } else {
            PixelInjector::new(pixel_base_url)
        };

        Self {
            injector,
            data_dir,
            tracking_requires_opt_in,
            opt_in_header,
            disclosure_header,
            inject_disclosure,
        }
    }

    pub fn process_email(&self, stdin: &mut dyn BufRead, stdout: &mut dyn Write) -> Result<()> {
        let mut headers = Vec::new();
        let mut body = Vec::new();
        let mut in_body = false;
        let mut content_type = None;
        let mut has_opt_in = false;
        let mut sender = String::from("unknown@localhost");
        // Generate proper tracking ID (YYYYMMDD-HHMMSS-UUID format)
        let tracking_id = generate_message_id();

        // Read email line by line
        for line_result in stdin.lines() {
            let line = line_result.context("Failed to read line from stdin")?;
            
            if !in_body {
                // Still reading headers
                if line.is_empty() {
                    // Blank line separates headers from body
                    in_body = true;
                    headers.push(line.clone());
                } else {
                    headers.push(line.clone());
                    
                    // Extract Content-Type
                    if line.starts_with("Content-Type:") || line.starts_with("content-type:") {
                        let parts: Vec<&str> = line.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            content_type = Some(parts[1].trim().to_lowercase());
                        }
                    }
                    
                    // Check for opt-in header
                    if line.starts_with(&format!("{}:", self.opt_in_header)) 
                        || line.starts_with(&format!("{}:", self.opt_in_header.to_lowercase())) {
                        has_opt_in = true;
                    }
                    
                    // Extract From header for sender
                    if line.starts_with("From:") || line.starts_with("from:") {
                        let parts: Vec<&str> = line.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let from_value = parts[1].trim();
                            // Extract email from "Name <email@domain.com>" format
                            if let Some(start) = from_value.find('<') {
                                if let Some(end) = from_value.find('>') {
                                    sender = from_value[start+1..end].to_string();
                                } else {
                                    sender = from_value[start+1..].to_string();
                                }
                            } else {
                                sender = from_value.to_string();
                            }
                        }
                    }
                }
            } else {
                // Reading body
                body.push(line);
            }
        }

        // Determine if we should track
        let should_track = if self.tracking_requires_opt_in {
            has_opt_in
        } else {
            // Check if Content-Type is HTML
            content_type.as_ref()
                .map(|ct| ct.contains("text/html") || ct.contains("html"))
                .unwrap_or(false)
        };

        // Check if Content-Type is HTML for injection
        let is_html = content_type.as_ref()
            .map(|ct| ct.contains("text/html") || ct.contains("html"))
            .unwrap_or(false);

        trace!(
            tracking_id = %tracking_id,
            sender = %sender,
            is_html = is_html,
            should_track = should_track,
            has_opt_in = has_opt_in,
            body_size = body.join("\n").len(),
            "Processing email"
        );

        // Prepare headers (add disclosure header if needed)
        let mut header_lines = headers.clone();
        if should_track && is_html && self.inject_disclosure {
            let disclosure_value = format!("This email contains tracking pixels for analytics purposes.");
            // Find the blank line separator and insert disclosure header before it
            if let Some(blank_idx) = header_lines.iter().position(|h| h.is_empty()) {
                header_lines.insert(blank_idx, format!("{}: {}", self.disclosure_header, disclosure_value));
            } else {
                // No blank line found, add at the end before body
                header_lines.push(format!("{}: {}", self.disclosure_header, disclosure_value));
                header_lines.push(String::new()); // Add blank line separator
            }
        }

        // Write headers to stdout (including blank line separator)
        for header in &header_lines {
            stdout.write_all(header.as_bytes())
                .context("Failed to write header to stdout")?;
            stdout.write_all(b"\n")
                .context("Failed to write newline to stdout")?;
        }

        // Process body if needed
        if should_track && is_html {
            let body_str = body.join("\n");
            let body_bytes = body_str.as_bytes();
            
            match self.injector.inject_pixel(body_bytes, &tracking_id, true) {
                Ok(modified_body) => {
                    // Write modified body
                    stdout.write_all(&modified_body)
                        .context("Failed to write modified body to stdout")?;
                    
                    debug!(
                        tracking_id = %tracking_id,
                        original_size = body_bytes.len(),
                        modified_size = modified_body.len(),
                        "Pixel injected successfully"
                    );
                    
                    // Save metadata for tracking
                    if let Err(e) = self.save_message_metadata(&tracking_id, &sender, &headers, &body_str, true) {
                        warn!(
                            tracking_id = %tracking_id,
                            error = %e,
                            "Failed to save message metadata"
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        tracking_id = %tracking_id,
                        error = %e,
                        "Failed to inject pixel, using original body"
                    );
                    // Write original body on error
                    stdout.write_all(body_str.as_bytes())
                        .context("Failed to write original body to stdout")?;
                }
            }
        } else {
            // Write original body
            for line in &body {
                stdout.write_all(line.as_bytes())
                    .context("Failed to write body line to stdout")?;
                stdout.write_all(b"\n")
                    .context("Failed to write newline to stdout")?;
            }
        }

        stdout.flush().context("Failed to flush stdout")?;
        Ok(())
    }
    
    fn save_message_metadata(
        &self,
        tracking_id: &str,
        sender: &str,
        headers: &[String],
        body: &str,
        tracking_enabled: bool,
    ) -> Result<()> {
        debug!(
            tracking_id = %tracking_id,
            data_dir = ?self.data_dir,
            "Starting to save message metadata"
        );

        let message_dir = self.data_dir.join(tracking_id);
        debug!(message_dir = ?message_dir, "Creating message directory");
        fs::create_dir_all(&message_dir)
            .with_context(|| format!("Failed to create directory: {:?}", message_dir))?;
        info!(message_dir = ?message_dir, "Message directory created");

        // Save headers
        let headers_file = message_dir.join("headers.txt");
        let headers_content = headers.join("\n");
        let headers_size = headers_content.len();
        debug!(
            tracking_id = %tracking_id,
            headers_file = ?headers_file,
            headers_size = headers_size,
            "Writing headers file"
        );
        fs::write(&headers_file, &headers_content)
            .with_context(|| format!("Failed to write headers to: {:?}", headers_file))?;
        debug!(tracking_id = %tracking_id, "Headers file written successfully");

        // Save body
        let body_file = message_dir.join("body.txt");
        let body_size = body.len();
        debug!(
            tracking_id = %tracking_id,
            body_file = ?body_file,
            body_size = body_size,
            "Writing body file"
        );
        fs::write(&body_file, body)
            .with_context(|| format!("Failed to write body to: {:?}", body_file))?;
        debug!(tracking_id = %tracking_id, "Body file written successfully");

        // Save metadata
        let total_size = headers_size + body_size;
        let now = Utc::now();
        let metadata = MessageMetadata {
            id: tracking_id.to_string(),
            created: now,
            created_str: now.format("%Y-%m-%d %H:%M:%S UTC").to_string(),
            sender: sender.to_string(),
            size: total_size,
            tracking_enabled,
            opened: false,
            open_count: 0,
            first_open: None,
            first_open_str: None,
            last_open: None,
            last_open_str: None,
            tracking_events: Vec::new(),
        };

        let meta_file = message_dir.join("meta.json");
        debug!(
            tracking_id = %tracking_id,
            meta_file = ?meta_file,
            "Serializing metadata"
        );
        let meta_json = serde_json::to_string_pretty(&metadata)
            .context("Failed to serialize metadata")?;
        debug!(
            tracking_id = %tracking_id,
            meta_json_size = meta_json.len(),
            "Writing metadata file"
        );
        fs::write(&meta_file, meta_json)
            .with_context(|| format!("Failed to write metadata to: {:?}", meta_file))?;

        info!(
            tracking_id = %tracking_id,
            sender = %sender,
            tracking_enabled = tracking_enabled,
            total_size = total_size,
            headers_size = headers_size,
            body_size = body_size,
            "Message metadata saved successfully"
        );

        Ok(())
    }
}

