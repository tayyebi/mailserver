use log::{debug, error, info, warn};
use std::io::{self, Read};

use crate::db::Database;

pub fn run_filter(db_url: &str, sender: &str, recipients: &[String], pixel_base_url: &str, unsubscribe_base_url: &str) {
    info!(
        "[filter] starting content filter sender={}, recipients={}",
        sender,
        recipients.join(", ")
    );

    // 1. Read entire email from stdin
    debug!("[filter] reading email from stdin");
    let mut email_data = String::new();
    if let Err(e) = io::stdin().read_to_string(&mut email_data) {
        error!("[filter] failed to read email from stdin: {}", e);
        return;
    }
    info!(
        "[filter] read email from stdin ({} bytes)",
        email_data.len()
    );

    // 2. Check if the content filter feature is enabled
    let mut modified = email_data.clone();
    match std::panic::catch_unwind(|| {
        debug!("[filter] opening database at {}", db_url);
        let db = Database::open(db_url);

        // Check feature toggle — if disabled, bypass all filter logic
        let filter_enabled = db
            .get_setting("feature_filter_enabled")
            .map(|v| v != "false")
            .unwrap_or(true);

        if !filter_enabled {
            info!("[filter] content filter feature is disabled, bypassing");
            return (db, false, None, false, false, String::new());
        }

        let tracking = db.is_tracking_enabled_for_sender(sender);
        let footer_html = db.get_footer_for_sender(sender);

        // Check if unsubscribe injection is enabled globally and per-domain
        let unsubscribe_global = db
            .get_setting("feature_unsubscribe_enabled")
            .map(|v| v != "false")
            .unwrap_or(true);
        let sender_domain = sender.split('@').nth(1).unwrap_or("").to_lowercase();
        let unsubscribe_domain = if unsubscribe_global && !sender_domain.is_empty() {
            db.is_unsubscribe_enabled_for_domain(&sender_domain)
        } else {
            false
        };

        (db, tracking, footer_html, true, unsubscribe_domain, sender_domain)
    }) {
        Ok((db, tracking, footer_html, enabled, unsubscribe_domain, sender_domain)) => {
            if !enabled {
                // Feature disabled — pass through unmodified
            } else {
                info!(
                    "[filter] tracking enabled for sender={}: {}",
                    sender, tracking
                );
                if let Some(footer) = footer_html {
                    debug!("[filter] injecting footer for sender={}", sender);
                    modified = inject_footer(&modified, &footer);
                }

                if unsubscribe_domain && !unsubscribe_base_url.is_empty() {
                    // Inject a single List-Unsubscribe header for the primary recipient (RFC 8058).
                    // The content filter reinjects one message, so we use the first recipient's token.
                    if let Some(primary_recipient) = recipients.first() {
                        let token = uuid::Uuid::new_v4().to_string();
                        let unsub_url = format!("{}/unsubscribe?token={}", unsubscribe_base_url.trim_end_matches('/'), token);
                        db.create_unsubscribe_token(&token, primary_recipient, &sender_domain);
                        let headers = format!(
                            "List-Unsubscribe: <{}>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click",
                            unsub_url
                        );
                        modified = inject_headers(&modified, &headers);
                        info!("[filter] injected List-Unsubscribe header for recipient={} token={}", primary_recipient, token);
                    }
                }

                if tracking {
                    let message_id = uuid::Uuid::new_v4().to_string();
                    let pixel_url = format!("{}{}", pixel_base_url, message_id);
                    let pixel_tag = format!(
                        r#"<img src="{}" width="1" height="1" style="display:none" alt="" />"#,
                        pixel_url
                    );
                    debug!(
                        "[filter] generated tracking pixel message_id={}",
                        message_id
                    );

                    // Try to inject before </body>
                    if let Some(pos) = modified.to_lowercase().rfind("</body>") {
                        modified.insert_str(pos, &pixel_tag);
                        info!(
                            "[filter] injected tracking pixel before </body> for message_id={}",
                            message_id
                        );
                    } else if modified.contains("<html") || modified.contains("<HTML") {
                        // Append to end if HTML but no </body>
                        modified.push_str(&pixel_tag);
                        info!(
                            "[filter] appended tracking pixel to HTML email for message_id={}",
                            message_id
                        );
                    } else {
                        debug!(
                            "[filter] email is not HTML — skipping pixel injection for message_id={}",
                            message_id
                        );
                    }

                    // Record tracked message
                    let subject = extract_header(&email_data, "Subject").unwrap_or_default();
                    let recipient = recipients.first().map(|s| s.as_str()).unwrap_or("");
                    debug!(
                        "[filter] recording tracked message: message_id={}, subject={}",
                        message_id, subject
                    );
                    db.create_tracked_message(&message_id, sender, recipient, &subject, None);
                    info!(
                        "[filter] tracked message recorded: message_id={}",
                        message_id
                    );
                } else {
                    debug!("[filter] no tracking — passing email through unmodified");
                }
            }
        }
        Err(_) => {
            warn!("[filter] filter database/pixel logic failed, falling back to unmodified email");
        }
    }

    // 4. Strip invalid DKIM-Signature headers when email was modified, so OpenDKIM
    //    can re-sign the modified content cleanly on the reinject port.
    if modified != email_data {
        debug!("[filter] email was modified, stripping DKIM-Signature headers before reinjection");
        modified = strip_dkim_signatures(&modified);
    }

    // 5. Reinject via SMTP to 127.0.0.1:10025
    info!("[filter] reinjecting email via SMTP to 127.0.0.1:10025");
    if let Err(e) = reinject_smtp(&modified, sender, recipients) {
        warn!(
            "[filter] failed to reinject modified email: {}. attempting unmodified fallback",
            e
        );
        if let Err(e) = reinject_smtp(&email_data, sender, recipients) {
            error!("[filter] failed to reinject unmodified fallback email: {}", e);
            return;
        }
        info!("[filter] unmodified fallback email reinjected successfully");
        return;
    }
    info!("[filter] email reinjected successfully");
}

fn inject_headers(email: &str, headers: &str) -> String {
    // Detect line-ending style
    let eol = if email.contains("\r\n") { "\r\n" } else { "\n" };
    let sep = if eol == "\r\n" { "\r\n\r\n" } else { "\n\n" };
    // Find end of header section
    if let Some(pos) = email.find(sep) {
        let mut result = email[..pos].to_string();
        // Append new headers before the blank line
        for line in headers.lines() {
            result.push_str(line);
            result.push_str(eol);
        }
        result.push_str(eol);
        result.push_str(&email[pos + sep.len()..]);
        result
    } else {
        email.to_string()
    }
}

fn inject_footer(email: &str, footer_html: &str) -> String {
    if footer_html.trim().is_empty() {
        return email.to_string();
    }
    let mut output = email.to_string();
    let lower = output.to_ascii_lowercase();
    let footer_block = format!(
        r#"<div class="domain-footer" style="margin-top:24px;border-top:1px solid #e2e8f0;padding-top:12px;font-size:0.9em;color:#475569;line-height:1.4;">{}</div>"#,
        footer_html
    );
    if let Some(pos) = lower.rfind("</body>") {
        output.insert_str(pos, &footer_block);
        return output;
    }
    if lower.contains("<html") {
        output.push_str(&footer_block);
        return output;
    }
    let plain = strip_html_tags(footer_html);
    if plain.is_empty() {
        return output;
    }
    output.push_str("\n\n-- \n");
    output.push_str(&plain);
    output
}

fn strip_html_tags(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    let mut in_tag = false;
    for c in input.chars() {
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
            }
            _ if !in_tag => output.push(c),
            _ => {}
        }
    }
    output.trim().to_string()
}

fn extract_header(email: &str, header_name: &str) -> Option<String> {
    debug!("[filter] extracting header: {}", header_name);
    let prefix = format!("{}:", header_name.to_lowercase());
    for line in email.lines() {
        if line.is_empty() {
            break; // End of headers
        }
        if line.to_lowercase().starts_with(&prefix) {
            let value = line[header_name.len() + 1..].trim().to_string();
            debug!("[filter] found header {}={}", header_name, value);
            return Some(value);
        }
    }
    debug!("[filter] header {} not found", header_name);
    None
}

fn reinject_smtp(email: &str, sender: &str, recipients: &[String]) -> io::Result<()> {
    use std::io::Write;
    use std::net::TcpStream;

    debug!("[filter] connecting to 127.0.0.1:10025 for reinjection");
    let mut stream = TcpStream::connect("127.0.0.1:10025")?;
    debug!("[filter] connected to reinjection port");

    let mut buf = [0u8; 512];

    // Read greeting
    let greeting = read_response(&mut stream, &mut buf)?;
    debug!("[filter] SMTP greeting: {}", greeting.trim());

    // EHLO
    write!(stream, "EHLO localhost\r\n")?;
    stream.flush()?;
    let resp = read_response(&mut stream, &mut buf)?;
    debug!("[filter] EHLO response: {}", resp.trim());

    // MAIL FROM
    debug!("[filter] sending MAIL FROM:<{}>", sender);
    write!(stream, "MAIL FROM:<{}>\r\n", sender)?;
    stream.flush()?;
    let resp = read_response(&mut stream, &mut buf)?;
    debug!("[filter] MAIL FROM response: {}", resp.trim());

    // RCPT TO for each recipient
    for rcpt in recipients {
        debug!("[filter] sending RCPT TO:<{}>", rcpt);
        write!(stream, "RCPT TO:<{}>\r\n", rcpt)?;
        stream.flush()?;
        let resp = read_response(&mut stream, &mut buf)?;
        debug!("[filter] RCPT TO response: {}", resp.trim());
    }

    // DATA
    debug!("[filter] sending DATA command");
    write!(stream, "DATA\r\n")?;
    stream.flush()?;
    let resp = read_response(&mut stream, &mut buf)?;
    debug!("[filter] DATA response: {}", resp.trim());

    // Send email body (dot-stuff lines starting with .)
    debug!("[filter] sending email body ({} bytes)", email.len());
    for line in email.lines() {
        if line.starts_with('.') {
            write!(stream, ".{}\r\n", line)?;
        } else {
            write!(stream, "{}\r\n", line)?;
        }
    }

    // End DATA
    write!(stream, ".\r\n")?;
    stream.flush()?;
    let resp = read_response(&mut stream, &mut buf)?;
    debug!("[filter] end-of-data response: {}", resp.trim());

    // QUIT
    write!(stream, "QUIT\r\n")?;
    stream.flush()?;
    let resp = read_response(&mut stream, &mut buf)?;
    debug!("[filter] QUIT response: {}", resp.trim());

    info!("[filter] SMTP reinjection completed for sender={}", sender);
    Ok(())
}

fn read_response(stream: &mut std::net::TcpStream, buf: &mut [u8]) -> io::Result<String> {
    use std::io::Read;
    let n = stream.read(buf)?;
    Ok(String::from_utf8_lossy(&buf[..n]).to_string())
}

/// Remove all DKIM-Signature headers (including folded continuations) from an email.
///
/// Called when the content filter modifies an email body so that the existing
/// DKIM signatures — which were computed over the original content — are stripped
/// before reinjection.  OpenDKIM will then produce a fresh, valid signature for
/// the modified content on the reinject port (127.0.0.1:10025).
fn strip_dkim_signatures(email: &str) -> String {
    // Detect line-ending style and the corresponding header/body separator.
    let eol: &str = if email.contains("\r\n") { "\r\n" } else { "\n" };
    let sep: &str = if eol == "\r\n" { "\r\n\r\n" } else { "\n\n" };

    // Split the email into headers and body at the first blank line.
    let (header_section, body_section) = match email.find(sep) {
        Some(pos) => (&email[..pos], &email[pos + sep.len()..]),
        None => return email.to_string(),
    };

    let mut result = String::with_capacity(email.len());
    let mut skip = false;

    for line in header_section.split(eol) {
        if line.is_empty() {
            continue;
        }
        // A folded header continuation starts with whitespace.
        if line.starts_with(' ') || line.starts_with('\t') {
            if !skip {
                result.push_str(line);
                result.push_str(eol);
            }
            continue;
        }
        // New header field — "DKIM-Signature:" is exactly 15 characters.
        skip = line.len() >= 15 && line[..15].eq_ignore_ascii_case("dkim-signature:");
        if !skip {
            result.push_str(line);
            result.push_str(eol);
        }
    }

    // Re-attach the blank-line separator and the body verbatim.
    result.push_str(eol);
    result.push_str(body_section);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strip_dkim_signatures_removes_single_signature() {
        let email = concat!(
            "From: sender@example.com\r\n",
            "DKIM-Signature: v=1; a=rsa-sha256; d=example.com; s=default;\r\n",
            "\tb=abc123;\r\n",
            "Subject: Hello\r\n",
            "\r\n",
            "Body text.\r\n"
        );
        let result = strip_dkim_signatures(email);
        assert!(!result.contains("DKIM-Signature"));
        assert!(!result.contains("b=abc123"));
        assert!(result.contains("From: sender@example.com"));
        assert!(result.contains("Subject: Hello"));
        assert!(result.contains("Body text."));
    }

    #[test]
    fn strip_dkim_signatures_removes_multiple_signatures() {
        let email = concat!(
            "From: sender@example.com\n",
            "DKIM-Signature: v=1; a=rsa-sha256; d=example.com; s=s1;\n",
            "\tb=sig1;\n",
            "DKIM-Signature: v=1; a=rsa-sha256; d=example.com; s=s2;\n",
            "\tb=sig2;\n",
            "Subject: Test\n",
            "\n",
            "Body.\n"
        );
        let result = strip_dkim_signatures(email);
        assert!(!result.contains("DKIM-Signature"));
        assert!(!result.contains("sig1"));
        assert!(!result.contains("sig2"));
        assert!(result.contains("From: sender@example.com"));
        assert!(result.contains("Subject: Test"));
        assert!(result.contains("Body."));
    }

    #[test]
    fn strip_dkim_signatures_preserves_email_without_dkim() {
        let email = concat!(
            "From: sender@example.com\n",
            "Subject: No DKIM here\n",
            "\n",
            "Body text.\n"
        );
        let result = strip_dkim_signatures(email);
        assert!(result.contains("From: sender@example.com"));
        assert!(result.contains("Subject: No DKIM here"));
        assert!(result.contains("Body text."));
    }

    #[test]
    fn strip_dkim_signatures_keeps_other_headers_intact() {
        let email = concat!(
            "From: a@b.com\n",
            "DKIM-Signature: v=1; b=xyz;\n",
            "To: c@d.com\n",
            "\n",
            "Hello.\n"
        );
        let result = strip_dkim_signatures(email);
        assert!(result.contains("From: a@b.com"));
        assert!(result.contains("To: c@d.com"));
        assert!(!result.contains("DKIM-Signature"));
        assert!(!result.contains("b=xyz"));
    }

    #[test]
    fn strip_dkim_signatures_preserves_crlf_line_endings() {
        let email = concat!(
            "From: a@b.com\r\n",
            "DKIM-Signature: v=1; b=xyz;\r\n",
            "To: c@d.com\r\n",
            "\r\n",
            "Hello.\r\n"
        );
        let result = strip_dkim_signatures(email);
        assert_eq!(result, "From: a@b.com\r\nTo: c@d.com\r\n\r\nHello.\r\n");
    }

    #[test]
    fn inject_headers_inserts_before_body() {
        let email = concat!(
            "From: sender@example.com\r\n",
            "Subject: Test\r\n",
            "\r\n",
            "Body text.\r\n"
        );
        let headers = "List-Unsubscribe: <https://example.com/unsubscribe?token=abc>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n";
        let result = inject_headers(email, headers);
        assert!(result.contains("List-Unsubscribe: <https://example.com/unsubscribe?token=abc>"));
        assert!(result.contains("List-Unsubscribe-Post: List-Unsubscribe=One-Click"));
        assert!(result.contains("From: sender@example.com"));
        assert!(result.contains("Subject: Test"));
        assert!(result.contains("Body text."));
        // Body should come after headers
        let header_pos = result.find("List-Unsubscribe").unwrap();
        let body_pos = result.find("Body text.").unwrap();
        assert!(header_pos < body_pos);
    }

    #[test]
    fn inject_headers_works_with_lf_line_endings() {
        let email = concat!(
            "From: sender@example.com\n",
            "Subject: Test\n",
            "\n",
            "Body.\n"
        );
        let headers = "List-Unsubscribe: <https://example.com/unsubscribe?token=xyz>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click\r\n";
        let result = inject_headers(email, headers);
        assert!(result.contains("List-Unsubscribe: <https://example.com/unsubscribe?token=xyz>"));
        assert!(result.contains("Body."));
    }

    #[test]
    fn inject_headers_returns_original_if_no_header_body_separator() {
        let email = "This is not a valid email";
        let headers = "List-Unsubscribe: <https://example.com/unsubscribe?token=abc>\r\n";
        let result = inject_headers(email, headers);
        assert_eq!(result, email);
    }
}
