use log::{debug, error, info, warn};
use std::io::{self, Read};

use crate::db::Database;

pub fn run_filter(db_url: &str, sender: &str, recipients: &[String], pixel_base_url: &str) {
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

    // 2. Open database and check if tracking is enabled for sender
    let mut modified = email_data.clone();
    match std::panic::catch_unwind(|| {
        debug!("[filter] opening database at {}", db_url);
        let db = Database::open(db_url);
        let tracking = db.is_tracking_enabled_for_sender(sender);
        let footer_html = db.get_footer_for_sender(sender);
        (db, tracking, footer_html)
    }) {
        Ok((db, tracking, footer_html)) => {
            info!(
                "[filter] tracking enabled for sender={}: {}",
                sender, tracking
            );
            if let Some(footer) = footer_html {
                debug!("[filter] injecting footer for sender={}", sender);
                modified = inject_footer(&modified, &footer);
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
        Err(_) => {
            warn!("[filter] filter database/pixel logic failed, falling back to unmodified email");
        }
    }

    // 4. Reinject via SMTP to 127.0.0.1:10025
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
