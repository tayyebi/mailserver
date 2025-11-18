use anyhow::{Context, Result};
use std::io::{self, BufRead, BufReader, Write};
use std::path::PathBuf;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::pixel_injector::PixelInjector;

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
        let mut message_id = Uuid::new_v4().to_string();

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
                    
                    // Extract Message-ID if present
                    if line.starts_with("Message-ID:") || line.starts_with("message-id:") {
                        let parts: Vec<&str> = line.splitn(2, ':').collect();
                        if parts.len() == 2 {
                            let msg_id = parts[1].trim();
                            if !msg_id.is_empty() {
                                message_id = msg_id.to_string();
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
            message_id = %message_id,
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
            
            match self.injector.inject_pixel(body_bytes, &message_id, true) {
                Ok(modified_body) => {
                    // Write modified body
                    stdout.write_all(&modified_body)
                        .context("Failed to write modified body to stdout")?;
                    
                    debug!(
                        message_id = %message_id,
                        original_size = body_bytes.len(),
                        modified_size = modified_body.len(),
                        "Pixel injected successfully"
                    );
                }
                Err(e) => {
                    warn!(
                        message_id = %message_id,
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
}

