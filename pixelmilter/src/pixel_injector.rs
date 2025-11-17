use anyhow::Result;
use regex::Regex;
use std::sync::OnceLock;
use tracing::{debug, info, warn};

#[derive(Clone)]
pub struct PixelInjector {
    pixel_base_url: String,
    footer_html: Option<String>,
    html_regex: OnceLock<Regex>,
    body_regex: OnceLock<Regex>,
}

impl PixelInjector {
    pub fn new(pixel_base_url: String) -> Self {
        Self {
            pixel_base_url,
            footer_html: None,
            html_regex: OnceLock::new(),
            body_regex: OnceLock::new(),
        }
    }

    pub fn with_footer(pixel_base_url: String, footer_html: String) -> Self {
        Self {
            pixel_base_url,
            footer_html: Some(footer_html),
            html_regex: OnceLock::new(),
            body_regex: OnceLock::new(),
        }
    }

    pub fn inject_pixel(&self, message_body: &[u8], message_id: &str, is_html: bool) -> Result<Vec<u8>> {
        let original_size = message_body.len();
        debug!(
            message_id = %message_id,
            body_size = original_size,
            is_html = is_html,
            pixel_base_url = %self.pixel_base_url,
            "Starting pixel injection"
        );
        
        // Only inject if Content-Type is HTML
        if !is_html {
            debug!(
                message_id = %message_id,
                body_size = original_size,
                "Non-HTML Content-Type, skipping pixel injection"
            );
            return Ok(message_body.to_vec());
        }
        
        // Convert to string for processing
        let body_str = String::from_utf8_lossy(message_body);
        debug!(
            message_id = %message_id,
            body_str_len = body_str.len(),
            "Converted body to string for processing"
        );
        
        info!(
            message_id = %message_id,
            body_size = original_size,
            "HTML Content-Type confirmed, injecting pixel"
        );
        let injected = self.inject_pixel_html(&body_str, message_id)?;
        let new_size = injected.len();
        let size_diff = new_size as i64 - original_size as i64;
        info!(
            message_id = %message_id,
            original_size = original_size,
            new_size = new_size,
            size_increase = size_diff,
            "Pixel injection completed"
        );
        Ok(injected.into_bytes())
    }


    fn inject_pixel_html(&self, html: &str, message_id: &str) -> Result<String> {
        let html_size = html.len();
        let pixel_url = format!("{}{}", self.pixel_base_url, message_id);
        debug!(
            message_id = %message_id,
            pixel_url = %pixel_url,
            html_size = html_size,
            "Creating pixel image tag"
        );
        
        let pixel_img = format!(
            r#"<img src="{}" width="1" height="1" style="display:none;border:0;outline:0;" alt="" />"#,
            pixel_url
        );
        let pixel_img_size = pixel_img.len();
        debug!(
            message_id = %message_id,
            pixel_img_size = pixel_img_size,
            "Pixel image tag created"
        );

        let lower_html = html.to_lowercase();
        let has_body_tag = lower_html.contains("<body");
        let has_closing_body_tag = lower_html.contains("</body");

        debug!(
            message_id = %message_id,
            has_body_tag = has_body_tag,
            has_closing_body_tag = has_closing_body_tag,
            "Checking for body tags"
        );

        // Prepare content to inject (pixel + footer if available)
        let content_to_inject = if let Some(ref footer) = self.footer_html {
            format!("{}{}", footer, pixel_img)
        } else {
            pixel_img.clone()
        };

        // Try to inject before closing body tag (preferred method)
        if has_closing_body_tag {
            debug!(message_id = %message_id, "Attempting to inject before </body> tag");
            let body_regex = self.body_regex.get_or_init(|| {
                debug!("Initializing body regex");
                Regex::new(r"(?i)(</body\s*>)").expect("Invalid body regex")
            });

            if let Some(captures) = body_regex.captures(html) {
                let result = body_regex.replace(html, format!("{}{}", content_to_inject, &captures[1]));
                let result_size = result.len();
                info!(
                    message_id = %message_id,
                    injection_method = "before_body_tag",
                    original_size = html_size,
                    result_size = result_size,
                    has_footer = self.footer_html.is_some(),
                    "Injected pixel and footer before </body> tag"
                );
                return Ok(result.to_string());
            }
        }

        // If we have <body> but no </body>, add </body> with pixel before it
        if has_body_tag && !has_closing_body_tag {
            debug!(
                message_id = %message_id,
                "Body tag found but no closing tag, adding </body> with pixel"
            );
            
            // Try to find </html> tag to insert before it
            let html_regex = self.html_regex.get_or_init(|| {
                debug!("Initializing html regex");
                Regex::new(r"(?i)(</html\s*>)").expect("Invalid html regex")
            });

            if let Some(captures) = html_regex.captures(html) {
                let closing_body_with_content = format!("{}{}", content_to_inject, "</body>");
                let result = html_regex.replace(html, format!("{}{}", closing_body_with_content, &captures[1]));
                let result_size = result.len();
                info!(
                    message_id = %message_id,
                    injection_method = "add_closing_body_tag",
                    original_size = html_size,
                    result_size = result_size,
                    has_footer = self.footer_html.is_some(),
                    "Added </body> tag with pixel and footer before </html> tag"
                );
                return Ok(result.to_string());
            }

            // No </html> tag either, append </body> with content at the end
            let result = format!("{}{}</body>", html, content_to_inject);
            let result_size = result.len();
            info!(
                message_id = %message_id,
                injection_method = "add_closing_body_tag_end",
                original_size = html_size,
                result_size = result_size,
                has_footer = self.footer_html.is_some(),
                "Added </body> tag with pixel and footer at end"
            );
            return Ok(result);
        }

        // If no body tag at all, wrap content in body tags
        if !has_body_tag {
            debug!(
                message_id = %message_id,
                "No body tag found, wrapping content in body tags"
            );
            
            // Check if we have <html> tags
            let has_html_tag = lower_html.contains("<html");
            let has_closing_html_tag = lower_html.contains("</html");

            if has_html_tag && has_closing_html_tag {
                // Find </html> and insert body tags before it
                let html_regex = self.html_regex.get_or_init(|| {
                    debug!("Initializing html regex");
                    Regex::new(r"(?i)(</html\s*>)").expect("Invalid html regex")
                });

                if let Some(captures) = html_regex.captures(html) {
                    let body_wrapper = format!("<body>{}{}</body>", content_to_inject, &captures[1]);
                    let result = html_regex.replace(html, body_wrapper);
                    let result_size = result.len();
                    info!(
                        message_id = %message_id,
                        injection_method = "wrap_in_body_tags",
                        original_size = html_size,
                        result_size = result_size,
                        has_footer = self.footer_html.is_some(),
                        "Wrapped content in body tags with pixel and footer"
                    );
                    return Ok(result.to_string());
                }
            }

            // Fallback: wrap entire content in body tags
            let result = format!("<body>{}{}</body>", html, content_to_inject);
            let result_size = result.len();
            info!(
                message_id = %message_id,
                injection_method = "wrap_in_body_tags_fallback",
                original_size = html_size,
                result_size = result_size,
                has_footer = self.footer_html.is_some(),
                "Wrapped entire content in body tags with pixel and footer"
            );
            return Ok(result);
        }

        // Fallback: try to inject before </html> tag
        debug!(message_id = %message_id, "Trying to inject before </html> tag as fallback");
        let html_regex = self.html_regex.get_or_init(|| {
            debug!("Initializing html regex");
            Regex::new(r"(?i)(</html\s*>)").expect("Invalid html regex")
        });

        if let Some(captures) = html_regex.captures(html) {
            let result = html_regex.replace(html, format!("{}{}", content_to_inject, &captures[1]));
            let result_size = result.len();
            warn!(
                message_id = %message_id,
                injection_method = "before_html_tag_fallback",
                original_size = html_size,
                result_size = result_size,
                has_footer = self.footer_html.is_some(),
                "Injected pixel and footer before </html> tag (fallback)"
            );
            return Ok(result.to_string());
        }

        // Last resort: append to end (shouldn't happen for valid HTML)
        warn!(
            message_id = %message_id,
            has_footer = self.footer_html.is_some(),
            "No suitable injection point found, appending to end"
        );
        let result = format!("{}{}", html, content_to_inject);
        Ok(result)
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