use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;
use tracing::debug;

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
        // Convert to string for processing
        let body_str = String::from_utf8_lossy(message_body);
        
        // Check if it looks like HTML content
        if self.contains_html(&body_str) {
            debug!(message_id = %message_id, "Injecting pixel into HTML content");
            let injected = self.inject_pixel_html(&body_str, message_id)?;
            Ok(injected.into_bytes())
        } else {
            // Not HTML, return unchanged
            debug!(message_id = %message_id, "No HTML content found, skipping pixel injection");
            Ok(message_body.to_vec())
        }
    }

    fn contains_html(&self, content: &str) -> bool {
        let lower = content.to_lowercase();
        lower.contains("<html") || 
        lower.contains("<body") || 
        lower.contains("content-type: text/html") ||
        lower.contains("content-type:text/html")
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
        if self.contains_html(html) {
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

    #[test]
    fn test_contains_html() {
        let injector = PixelInjector::new("https://example.com/pixel?id=".to_string());
        
        assert!(injector.contains_html("<html><body>test</body></html>"));
        assert!(injector.contains_html("Content-Type: text/html\r\n\r\n<p>test</p>"));
        assert!(!injector.contains_html("This is plain text"));
    }
}