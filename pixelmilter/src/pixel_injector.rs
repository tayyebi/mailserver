use anyhow::{Context, Result};
use mail_parser::{Message, MessageParser, MimeHeaders};
use regex::Regex;
use std::sync::OnceLock;
use tracing::{debug, warn};

#[derive(Clone)]
pub struct PixelInjector {
    pixel_base_url: String,
    html_regex: OnceLock<Regex>,
    body_regex: OnceLock<Regex>,
}

impl PixelInjector {
    pub fn new(pixel_base_url: String) -> Self {
        Self {
            pixel_base_url,
            html_regex: OnceLock::new(),
            body_regex: OnceLock::new(),
        }
    }

    pub fn inject_pixel(&self, message_body: &[u8], message_id: &str) -> Result<Vec<u8>> {
        // Try to parse as MIME message first
        if let Some(message) = MessageParser::default().parse(message_body) {
            debug!(message_id = %message_id, "Parsing as MIME message");
            return self.inject_pixel_mime(&message, message_id);
        }

        // Fallback to simple HTML injection
        debug!(message_id = %message_id, "Parsing as simple HTML");
        self.inject_pixel_simple(message_body, message_id)
    }

    fn inject_pixel_mime(&self, message: &Message, message_id: &str) -> Result<Vec<u8>> {
        let mut modified = false;
        let mut result = Vec::new();

        // Process each part of the message
        if message.is_multipart() {
            result = self.process_multipart_message(message, message_id, &mut modified)?;
        } else {
            // Single part message
            if let Some(content_type) = message.content_type() {
                if content_type.type_() == "text" && content_type.subtype() == Some("html") {
                    if let Some(body) = message.body_text(0) {
                        let injected = self.inject_pixel_html(body, message_id)?;
                        if injected != body {
                            modified = true;
                            // Reconstruct the message with new body
                            result = self.reconstruct_single_part_message(message, &injected)?;
                        }
                    }
                }
            }
        }

        if modified {
            Ok(result)
        } else {
            // Return original if no modifications were made
            Ok(message.raw_message().to_vec())
        }
    }

    fn process_multipart_message(
        &self,
        message: &Message,
        message_id: &str,
        modified: &mut bool,
    ) -> Result<Vec<u8>> {
        // For multipart messages, we need to process each part
        let mut parts = Vec::new();
        
        for attachment in message.attachments() {
            if let Some(content_type) = attachment.content_type() {
                if content_type.type_() == "text" && content_type.subtype() == Some("html") {
                    if let Some(body) = attachment.text() {
                        let injected = self.inject_pixel_html(body, message_id)?;
                        if injected != body {
                            *modified = true;
                            parts.push(injected);
                        } else {
                            parts.push(body.to_string());
                        }
                    }
                } else if let Some(body) = attachment.text() {
                    parts.push(body.to_string());
                }
            }
        }

        if *modified {
            // Reconstruct multipart message
            self.reconstruct_multipart_message(message, &parts)
        } else {
            Ok(message.raw_message().to_vec())
        }
    }

    fn reconstruct_single_part_message(&self, message: &Message, new_body: &str) -> Result<Vec<u8>> {
        let mut result = String::new();
        
        // Add headers
        for header in message.headers() {
            result.push_str(&format!("{}: {}\r\n", header.name(), header.value()));
        }
        
        result.push_str("\r\n");
        result.push_str(new_body);
        
        Ok(result.into_bytes())
    }

    fn reconstruct_multipart_message(&self, message: &Message, parts: &[String]) -> Result<Vec<u8>> {
        // This is a simplified reconstruction - in production you'd want more robust MIME handling
        let boundary = message
            .content_type()
            .and_then(|ct| ct.attribute("boundary"))
            .unwrap_or("----=_NextPart_000_0000_01234567.89ABCDEF");

        let mut result = String::new();
        
        // Add headers
        for header in message.headers() {
            result.push_str(&format!("{}: {}\r\n", header.name(), header.value()));
        }
        
        result.push_str("\r\n");
        
        // Add parts
        for (i, part) in parts.iter().enumerate() {
            result.push_str(&format!("--{}\r\n", boundary));
            result.push_str("Content-Type: text/html; charset=utf-8\r\n");
            result.push_str("\r\n");
            result.push_str(part);
            result.push_str("\r\n");
        }
        
        result.push_str(&format!("--{}--\r\n", boundary));
        
        Ok(result.into_bytes())
    }

    fn inject_pixel_simple(&self, message_body: &[u8], message_id: &str) -> Result<Vec<u8>> {
        let body_str = String::from_utf8_lossy(message_body);
        
        // Check if it looks like HTML
        if body_str.to_lowercase().contains("<html") || body_str.to_lowercase().contains("<body") {
            let injected = self.inject_pixel_html(&body_str, message_id)?;
            Ok(injected.into_bytes())
        } else {
            // Not HTML, return unchanged
            Ok(message_body.to_vec())
        }
    }

    fn inject_pixel_html(&self, html: &str, message_id: &str) -> Result<String> {
        let pixel_url = format!("{}{}", self.pixel_base_url, message_id);
        let pixel_img = format!(
            r#"<img src="{}" width="1" height="1" style="display:none;border:0;outline:0;" alt="" />"#,
            pixel_url
        );

        // Try to inject before closing body tag
        let body_regex = self.body_regex.get_or_init(|| {
            Regex::new(r"(?i)(</body\s*>)").expect("Invalid body regex")
        });

        if let Some(captures) = body_regex.captures(html) {
            let result = body_regex.replace(html, format!("{}{}", pixel_img, &captures[1]));
            debug!(message_id = %message_id, "Injected pixel before </body> tag");
            return Ok(result.to_string());
        }

        // Try to inject before closing html tag
        let html_regex = self.html_regex.get_or_init(|| {
            Regex::new(r"(?i)(</html\s*>)").expect("Invalid html regex")
        });

        if let Some(captures) = html_regex.captures(html) {
            let result = html_regex.replace(html, format!("{}{}", pixel_img, &captures[1]));
            debug!(message_id = %message_id, "Injected pixel before </html> tag");
            return Ok(result.to_string());
        }

        // Fallback: append to end if it looks like HTML
        if html.to_lowercase().contains("<html") || html.to_lowercase().contains("<body") {
            debug!(message_id = %message_id, "Injected pixel at end of HTML content");
            Ok(format!("{}{}", html, pixel_img))
        } else {
            // Not HTML content
            debug!(message_id = %message_id, "No HTML content found, skipping pixel injection");
            Ok(html.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_pixel_simple_html() {
        let injector = PixelInjector::new("https://example.com/pixel?id=".to_string());
        let html = r#"<html><body><h1>Hello</h1></body></html>"#;
        let result = injector.inject_pixel_html(html, "test-id").unwrap();
        
        assert!(result.contains(r#"<img src="https://example.com/pixel?id=test-id""#));
        assert!(result.contains(r#"</body></html>"#));
    }

    #[test]
    fn test_inject_pixel_no_body_tag() {
        let injector = PixelInjector::new("https://example.com/pixel?id=".to_string());
        let html = r#"<html><h1>Hello</h1></html>"#;
        let result = injector.inject_pixel_html(html, "test-id").unwrap();
        
        assert!(result.contains(r#"<img src="https://example.com/pixel?id=test-id""#));
        assert!(result.contains(r#"</html>"#));
    }

    #[test]
    fn test_inject_pixel_plain_text() {
        let injector = PixelInjector::new("https://example.com/pixel?id=".to_string());
        let text = "This is plain text";
        let result = injector.inject_pixel_html(text, "test-id").unwrap();
        
        // Should not inject pixel in plain text
        assert_eq!(result, text);
    }
}
