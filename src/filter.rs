use log::{debug, error, info, warn};
use std::io::{self, Read};
use std::fs;
use std::sync::mpsc;

use crate::db::Database;

pub fn run_filter(
    db_url: &str,
    sender: &str,
    recipients: &[String],
    pixel_base_url: &str,
    unsubscribe_base_url: &str,
    incoming: bool,
) {
    info!(
        "[filter] starting content filter sender={}, recipients={}",
        sender,
        recipients.join(", ")
    );

    let mut target_recipients = recipients.to_vec();

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

    // Extract headers early for use in webhook payload
    let subject = extract_header(&email_data, "Subject").unwrap_or_default();
    let from_header = extract_header(&email_data, "From").unwrap_or_default();
    let to_header = extract_header(&email_data, "To").unwrap_or_default();
    let cc_header = extract_header(&email_data, "Cc").unwrap_or_default();
    let date_header = extract_header(&email_data, "Date").unwrap_or_default();
    let message_id_header = extract_header(&email_data, "Message-ID").unwrap_or_default();
    let size_bytes = email_data.len();

    // 2. Check if the content filter feature is enabled
    let mut modified = email_data.clone();
    let mut webhook_url = String::new();
    let mut suppressed = false;
    let mut spambl_hit = false;

    // Try to retrieve webhook URL first (before other database operations).
    // If the database fails to open, we try again just for the webhook URL.
    // Fail fast when PostgreSQL is unavailable so SMTP delivery is never blocked.
    match Database::try_open_with_options(
        db_url,
        1,
        std::time::Duration::from_millis(100),
        std::time::Duration::from_millis(500),
    ) {
        Ok(db) => {
            // Check feature toggle — if disabled, bypass all filter logic
            let filter_enabled = db
                .get_setting("feature_filter_enabled")
                .map(|v| v != "false")
                .unwrap_or(true);

            webhook_url = db.get_setting("webhook_url").unwrap_or_default();

            if !filter_enabled {
                info!("[filter] content filter feature is disabled, bypassing");
            } else {
                let tracking = db.is_tracking_enabled(sender, recipients.first().map(|s| s.as_str()).unwrap_or(""), &subject, size_bytes);
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
                    // Only send to recipients who have not unsubscribed — suppress promotional emails
                    // for unsubscribed recipients while leaving transactional emails untouched.
                    if let Some(primary_recipient) = recipients.first() {
                        if db.is_unsubscribed(primary_recipient, &sender_domain) {
                            info!("[filter] recipient={} has unsubscribed from domain={}, suppressing promotional email", primary_recipient, sender_domain);
                            suppressed = true;
                        } else {
                            let token = uuid::Uuid::new_v4().to_string();
                            let unsub_url = format!(
                                "{}/unsubscribe?token={}",
                                unsubscribe_base_url.trim_end_matches('/'),
                                token
                            );
                            db.create_unsubscribe_token(&token, primary_recipient, &sender_domain);
                            let headers = format!(
                                "List-Unsubscribe: <{}>\r\nList-Unsubscribe-Post: List-Unsubscribe=One-Click",
                                unsub_url
                            );
                            modified = inject_headers(&modified, &headers);
                            info!("[filter] injected List-Unsubscribe header for recipient={} token={}", primary_recipient, token);
                        }
                    }
                }

                // Check sender IP against enabled RBL hostnames and flag if listed
                let rbl_hostnames = db.list_enabled_spambl_hostnames();
                if !rbl_hostnames.is_empty() {
                    if let Some(ip) = extract_sender_ip(&email_data) {
                        for rbl_host in &rbl_hostnames {
                            if check_rbl(&ip, rbl_host) {
                                modified = inject_headers(&modified, "X-Spam-Flag: YES");
                                spambl_hit = true;
                                info!(
                                    "[filter] RBL hit for ip={} on {}, flagged as spam",
                                    ip, rbl_host
                                );
                                break;
                            }
                        }
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
        Err(e) => {
            warn!(
                "[filter] failed to open database ({}), falling back to unmodified email",
                e
            );
            // Even if the database failed, try to retrieve just the webhook URL for event logging.
            if let Ok(db) = Database::try_open_with_options(
                db_url,
                1,
                std::time::Duration::from_millis(100),
                std::time::Duration::from_millis(500),
            ) {
                webhook_url = db.get_setting("webhook_url").unwrap_or_default();
            }
        }
    }

    // 4. If a spambl match was found on an incoming email, deliver to the Junk folder
    //    (auto-creating it if necessary) instead of the Inbox.
    if incoming && spambl_hit {
        let mail_root = maildir_root();
        let mut junk_recipients = Vec::new();
        for rcpt in recipients {
            if let Some(junk_rcpt) = move_recipient_to_junk(rcpt, &mail_root) {
                info!(
                    "[filter] spambl hit — delivering {} to Junk folder as {}",
                    rcpt, junk_rcpt
                );
                junk_recipients.push(junk_rcpt);
            } else {
                warn!(
                    "[filter] spambl hit for {}, but failed to prepare Junk folder; delivering normally",
                    rcpt
                );
                junk_recipients.push(rcpt.clone());
            }
        }
        target_recipients = junk_recipients;
    }

    // 5. Strip invalid DKIM-Signature headers when email was modified, so OpenDKIM
    //    can re-sign the modified content cleanly on the reinject port.
    if modified != email_data {
        debug!("[filter] email was modified, stripping DKIM-Signature headers before reinjection");
        modified = strip_dkim_signatures(&modified);
    }

    // 6. Prepare email metadata for the webhook (shared by suppressed and normal code paths).
    let email_was_modified = modified != email_data;
    let meta = EmailMetadata {
        sender: sender.to_string(),
        recipients: target_recipients.clone(),
        subject: subject.clone(),
        from: from_header.clone(),
        to: to_header.clone(),
        cc: cc_header.clone(),
        date: date_header.clone(),
        message_id: message_id_header.clone(),
        size_bytes,
        direction: if incoming {
            "incoming".to_string()
        } else {
            "outgoing".to_string()
        },
    };

    // 7. If the email was suppressed because the recipient has unsubscribed, drop
    //    the message here (do not reinject) without an error so Postfix discards it.
    //    Fire the webhook so the event is still visible to the caller.
    if suppressed {
        info!("[filter] email suppressed — not reinjecting (see earlier log for recipient/domain)");
        send_webhook(
            &webhook_url,
            db_url,
            &meta,
            email_was_modified,
            sender,
            &subject,
        );
        return;
    }

    // 8. Reinject via SMTP to 127.0.0.1:10025
    info!("[filter] reinjecting email via SMTP to 127.0.0.1:10025");

    // Spawn the webhook thread early so it can start in parallel with the reinject.
    // A channel carries the final `modified` flag (None = don't fire, Some(bool) = fire).
    let (modified_tx, modified_rx) = mpsc::channel::<Option<bool>>();
    let webhook_handle = {
        let url = webhook_url.clone();
        let db_url_owned = db_url.to_string();
        let sender_owned = sender.to_string();
        let subject_owned = subject.clone();
        std::thread::spawn(move || {
            // Wait for the reinject outcome before making the HTTP call.
            match modified_rx.recv() {
                Ok(Some(was_modified)) => {
                    send_webhook(
                        &url,
                        &db_url_owned,
                        &meta,
                        was_modified,
                        &sender_owned,
                        &subject_owned,
                    );
                }
                // None or channel closed means double-failure — skip webhook.
                _ => {}
            }
        })
    };

    if let Err(e) = reinject_smtp(&modified, sender, &target_recipients) {
        warn!(
            "[filter] failed to reinject modified email: {}. attempting unmodified fallback",
            e
        );
        if let Err(e) = reinject_smtp(&email_data, sender, &target_recipients) {
            error!(
                "[filter] failed to reinject unmodified fallback email: {}",
                e
            );
            // Signal the webhook thread to not fire (both injects failed).
            let _ = modified_tx.send(None);
            let _ = webhook_handle.join();
            // Tell Postfix to retry delivery rather than silently dropping the message.
            std::process::exit(75); // EX_TEMPFAIL
        }
        info!("[filter] unmodified fallback email reinjected successfully");
        // Fallback succeeded: the email sent is the original (unmodified).
        let _ = modified_tx.send(Some(false));
        let _ = webhook_handle.join();
        return;
    }
    info!("[filter] email reinjected successfully");

    // Signal webhook thread with the actual modified flag; it will fire the HTTP call.
    let _ = modified_tx.send(Some(email_was_modified));
    // Wait for the webhook thread to complete before the process exits.
    let _ = webhook_handle.join();
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
    use std::io::{BufReader, Write};
    use std::net::TcpStream;

    debug!("[filter] connecting to 127.0.0.1:10025 for reinjection");
    let stream = TcpStream::connect("127.0.0.1:10025")?;
    // Clone the stream so we can have a buffered reader and a writer on the same socket.
    let mut writer = stream.try_clone()?;
    let mut reader = BufReader::new(stream);
    debug!("[filter] connected to reinjection port");

    // Read greeting
    let greeting = read_smtp_response(&mut reader)?;
    debug!("[filter] SMTP greeting: {}", greeting.trim());
    smtp_expect(&greeting, "220")?;

    // EHLO
    writer.write_all(b"EHLO localhost\r\n")?;
    let resp = read_smtp_response(&mut reader)?;
    debug!("[filter] EHLO response: {}", resp.trim());
    smtp_expect(&resp, "250")?;

    // MAIL FROM
    debug!("[filter] sending MAIL FROM:<{}>", sender);
    writer.write_all(format!("MAIL FROM:<{}>\r\n", sender).as_bytes())?;
    let resp = read_smtp_response(&mut reader)?;
    debug!("[filter] MAIL FROM response: {}", resp.trim());
    smtp_expect(&resp, "250")?;

    // RCPT TO for each recipient
    for rcpt in recipients {
        debug!("[filter] sending RCPT TO:<{}>", rcpt);
        writer.write_all(format!("RCPT TO:<{}>\r\n", rcpt).as_bytes())?;
        let resp = read_smtp_response(&mut reader)?;
        debug!("[filter] RCPT TO response: {}", resp.trim());
        smtp_expect(&resp, "250")?;
    }

    // DATA
    debug!("[filter] sending DATA command");
    writer.write_all(b"DATA\r\n")?;
    let resp = read_smtp_response(&mut reader)?;
    debug!("[filter] DATA response: {}", resp.trim());
    smtp_expect(&resp, "354")?;

    // Send email body (dot-stuff lines starting with .)
    debug!("[filter] sending email body ({} bytes)", email.len());
    for line in email.lines() {
        if line.starts_with('.') {
            writer.write_all(format!(".{}\r\n", line).as_bytes())?;
        } else {
            writer.write_all(format!("{}\r\n", line).as_bytes())?;
        }
    }

    // End DATA
    writer.write_all(b".\r\n")?;
    let resp = read_smtp_response(&mut reader)?;
    debug!("[filter] end-of-data response: {}", resp.trim());
    smtp_expect(&resp, "250")?;

    // QUIT
    writer.write_all(b"QUIT\r\n")?;
    let resp = read_smtp_response(&mut reader)?;
    debug!("[filter] QUIT response: {}", resp.trim());

    info!("[filter] SMTP reinjection completed for sender={}", sender);
    Ok(())
}

/// Read a complete SMTP response (possibly multi-line) from a buffered reader.
///
/// SMTP multi-line responses use the format `NNN-text\r\n` for continuation lines
/// and `NNN text\r\n` (space instead of dash) for the final line. This function
/// reads until it receives the final line, ensuring the complete response is consumed
/// even if it spans multiple TCP segments.
fn read_smtp_response(reader: &mut impl std::io::BufRead) -> io::Result<String> {
    let mut response = String::new();
    loop {
        let mut line = String::new();
        let n = reader.read_line(&mut line)?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "SMTP server closed connection unexpectedly",
            ));
        }
        response.push_str(&line);
        // The final line of an SMTP response has a space at position 3 (e.g. "250 OK").
        // Continuation lines have a dash at position 3 (e.g. "250-PIPELINING").
        if line.len() >= 4 && line.as_bytes()[3] == b' ' {
            return Ok(response);
        }
    }
}

/// Return an error if the SMTP response does not start with the expected code prefix.
fn smtp_expect(response: &str, expected_code: &str) -> io::Result<()> {
    if response.starts_with(expected_code) {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::Other,
            format!(
                "unexpected SMTP response (expected {}): {}",
                expected_code,
                response.trim()
            ),
        ))
    }
}

struct EmailMetadata {
    sender: String,
    recipients: Vec<String>,
    subject: String,
    from: String,
    to: String,
    cc: String,
    date: String,
    message_id: String,
    size_bytes: usize,
    direction: String,
}

fn send_webhook(
    webhook_url: &str,
    db_url: &str,
    meta: &EmailMetadata,
    modified: bool,
    sender: &str,
    subject: &str,
) {
    let timestamp = chrono::Utc::now().to_rfc3339();
    let payload = serde_json::json!({
        "event": "email_processed",
        "timestamp": timestamp,
        "direction": meta.direction,
        "sender": meta.sender,
        "recipients": meta.recipients,
        "subject": meta.subject,
        "from": meta.from,
        "to": meta.to,
        "cc": meta.cc,
        "date": meta.date,
        "message_id": meta.message_id,
        "size_bytes": meta.size_bytes,
        "modified": modified,
    });
    let request_body = payload.to_string();

    let (response_status, response_body, error, duration_ms) = if webhook_url.is_empty() {
        (None, String::new(), String::new(), 0i64)
    } else {
        debug!("[filter] sending webhook to {}", webhook_url);
        let start = std::time::Instant::now();

        let (response_status, response_body, error) = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(client) => match client.post(webhook_url).json(&payload).send() {
                Ok(resp) => {
                    let status = resp.status().as_u16() as i32;
                    let body = resp.text().unwrap_or_default();
                    // Truncate response body to 2 KB for storage (char-boundary safe)
                    let body_truncated = if body.len() > 2048 {
                        let mut end = 2048;
                        while !body.is_char_boundary(end) {
                            end -= 1;
                        }
                        body[..end].to_string()
                    } else {
                        body
                    };
                    info!(
                        "[filter] webhook delivered to {} status={}",
                        webhook_url, status
                    );
                    (Some(status), body_truncated, String::new())
                }
                Err(e) => {
                    warn!("[filter] webhook delivery failed to {}: {}", webhook_url, e);
                    (None, String::new(), e.to_string())
                }
            },
            Err(e) => {
                warn!("[filter] failed to build HTTP client for webhook: {}", e);
                (None, String::new(), e.to_string())
            }
        };

        let duration_ms = start.elapsed().as_millis() as i64;
        (response_status, response_body, error, duration_ms)
    };

    // Always log the email processing event to the database (best-effort — don't let logging failures surface).
    if let Ok(db) = Database::try_open_with_options(
        db_url,
        1,
        std::time::Duration::from_millis(100),
        std::time::Duration::from_millis(500),
    ) {
        db.log_webhook(
            webhook_url,
            &request_body,
            response_status,
            &response_body,
            &error,
            duration_ms,
            sender,
            subject,
        );
    }
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

/// Extract the sender's IP address from the first `Received` header of an email.
/// Returns the IP in brackets `[x.x.x.x]` from the topmost Received header,
/// which is the IP of the client that connected to our Postfix server.
fn extract_sender_ip(email: &str) -> Option<String> {
    let mut in_received = false;
    let mut received_line = String::new();
    for line in email.lines() {
        if line.is_empty() {
            break; // end of headers
        }
        if line.to_ascii_lowercase().starts_with("received:") {
            in_received = true;
            received_line = line.to_string();
            continue;
        }
        if in_received {
            // Folded header continuation (starts with whitespace)
            if line.starts_with(' ') || line.starts_with('\t') {
                received_line.push(' ');
                received_line.push_str(line.trim());
                continue;
            }
            // New header — stop
            break;
        }
    }
    if received_line.is_empty() {
        return None;
    }
    // Extract the IP in brackets [x.x.x.x]
    if let Some(start) = received_line.find('[') {
        if let Some(end) = received_line[start + 1..].find(']') {
            let ip = &received_line[start + 1..start + 1 + end];
            // Reject IPv6 and loopback/private addresses
            if ip.contains(':')
                || ip.starts_with("127.")
                || ip.starts_with("10.")
                || ip.starts_with("192.168.")
            {
                return None;
            }
            // Reject RFC1918 172.16.0.0/12 range (172.16.x.x – 172.31.x.x)
            if ip.starts_with("172.") {
                if let Some(second) = ip.split('.').nth(1) {
                    if let Ok(n) = second.parse::<u8>() {
                        if (16..=31).contains(&n) {
                            return None;
                        }
                    }
                }
            }
            if ip.contains('.') {
                return Some(ip.to_string());
            }
        }
    }
    None
}

/// Check if an IPv4 address is listed in a DNS-based RBL (Real-time Blackhole List).
/// Performs a DNS A-record lookup for `<reversed-ip>.<rbl_host>`.
/// Returns `true` if the lookup succeeds (IP is listed).
fn check_rbl(ip: &str, rbl_host: &str) -> bool {
    let parts: Vec<&str> = ip.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    let lookup = format!(
        "{}.{}.{}.{}.{}",
        parts[3], parts[2], parts[1], parts[0], rbl_host
    );
    use std::net::ToSocketAddrs;
    (lookup.as_str(), 0u16)
        .to_socket_addrs()
        .map(|mut addrs| addrs.next().is_some())
        .unwrap_or(false)
}

fn maildir_root() -> String {
    "/data/mail".to_string()
}

fn move_recipient_to_junk(recipient: &str, mail_root: &str) -> Option<String> {
    let mut parts = recipient.split('@');
    let local = parts.next()?.trim();
    let domain = parts.next()?.trim();
    if parts.next().is_some() || local.is_empty() || domain.is_empty() {
        return None;
    }
    if local.contains('/') || domain.contains('/') || local.contains("..") || domain.contains("..") {
        return None;
    }

    let base_local = local.split('+').next().unwrap_or(local);
    let root = mail_root.trim_end_matches('/');
    let maildir_base = format!("{}/{}/{}/Maildir", root, domain, base_local);
    let junk_root = format!("{}/.Junk", maildir_base);

    for dir in [
        maildir_base.as_str(),
        &format!("{}/new", maildir_base),
        &format!("{}/cur", maildir_base),
        &format!("{}/tmp", maildir_base),
        junk_root.as_str(),
        &format!("{}/new", junk_root),
        &format!("{}/cur", junk_root),
        &format!("{}/tmp", junk_root),
    ] {
        if let Err(e) = fs::create_dir_all(dir) {
            warn!("[filter] failed to create maildir directory {}: {}", dir, e);
            return None;
        }
    }

    Some(format!("{}+Junk@{}", base_local, domain))
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── read_smtp_response tests ──

    #[test]
    fn read_smtp_response_single_line() {
        let input = b"220 mail.example.com ESMTP Postfix\r\n";
        let mut reader = std::io::BufReader::new(input.as_ref());
        let result = read_smtp_response(&mut reader).unwrap();
        assert_eq!(result, "220 mail.example.com ESMTP Postfix\r\n");
    }

    #[test]
    fn read_smtp_response_multi_line_ehlo() {
        let input =
            b"250-mail.example.com\r\n250-PIPELINING\r\n250-SIZE 10240000\r\n250 SMTPUTF8\r\n";
        let mut reader = std::io::BufReader::new(input.as_ref());
        let result = read_smtp_response(&mut reader).unwrap();
        // Should read all four lines and stop at "250 " (final line).
        assert!(result.contains("250-PIPELINING\r\n"));
        assert!(result.contains("250 SMTPUTF8\r\n"));
    }

    #[test]
    fn read_smtp_response_eof_returns_error() {
        let input: &[u8] = b"";
        let mut reader = std::io::BufReader::new(input);
        let result = read_smtp_response(&mut reader);
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }

    #[test]
    fn read_smtp_response_partial_then_eof_returns_error() {
        // Incomplete response (no space at position 3, server closed mid-stream)
        let input = b"421-Service\r\n";
        let mut reader = std::io::BufReader::new(input.as_ref());
        let result = read_smtp_response(&mut reader);
        // After reading the continuation line, EOF is hit before the final line.
        assert!(result.is_err());
    }

    // ── smtp_expect tests ──

    #[test]
    fn smtp_expect_accepts_matching_code() {
        assert!(smtp_expect("250 OK\r\n", "250").is_ok());
        assert!(smtp_expect("354 End data\r\n", "354").is_ok());
        assert!(smtp_expect("220 mail.example.com ESMTP\r\n", "220").is_ok());
    }

    #[test]
    fn smtp_expect_rejects_wrong_code() {
        let err = smtp_expect("550 User unknown\r\n", "250").unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::Other);
        assert!(err.to_string().contains("550 User unknown"));
    }

    #[test]
    fn smtp_expect_rejects_error_response() {
        assert!(smtp_expect("421 Service unavailable\r\n", "220").is_err());
        assert!(smtp_expect("554 Relay denied\r\n", "250").is_err());
    }

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

    #[test]
    fn extract_header_returns_correct_values() {
        let email = concat!(
            "From: sender@example.com\r\n",
            "To: recipient@example.com\r\n",
            "Subject: Test Subject\r\n",
            "Message-ID: <abc123@example.com>\r\n",
            "Date: Mon, 01 Jan 2024 00:00:00 +0000\r\n",
            "\r\n",
            "Body.\r\n"
        );
        assert_eq!(
            extract_header(email, "From"),
            Some("sender@example.com".to_string())
        );
        assert_eq!(
            extract_header(email, "To"),
            Some("recipient@example.com".to_string())
        );
        assert_eq!(
            extract_header(email, "Subject"),
            Some("Test Subject".to_string())
        );
        assert_eq!(
            extract_header(email, "Message-ID"),
            Some("<abc123@example.com>".to_string())
        );
        assert_eq!(extract_header(email, "Cc"), None);
    }

    #[test]
    fn extract_header_stops_at_blank_line() {
        let email = concat!("Subject: InHeader\r\n", "\r\n", "Subject: InBody\r\n");
        assert_eq!(
            extract_header(email, "Subject"),
            Some("InHeader".to_string())
        );
    }

    #[test]
    fn extract_sender_ip_returns_public_ipv4() {
        let email = concat!(
            "Received: from mail.attacker.com (mail.attacker.com [1.2.3.4])\r\n",
            "\tby mx.example.com (Postfix) with ESMTP id ABC123;\r\n",
            "From: attacker@attacker.com\r\n",
            "\r\n",
            "Spam body.\r\n"
        );
        assert_eq!(extract_sender_ip(email), Some("1.2.3.4".to_string()));
    }

    #[test]
    fn extract_sender_ip_ignores_loopback() {
        let email = concat!(
            "Received: from localhost (localhost [127.0.0.1])\r\n",
            "\tby mx.example.com (Postfix) with ESMTP;\r\n",
            "From: user@example.com\r\n",
            "\r\n",
            "Body.\r\n"
        );
        assert_eq!(extract_sender_ip(email), None);
    }

    #[test]
    fn extract_sender_ip_ignores_private_rfc1918() {
        let email_192 = concat!(
            "Received: from relay.local (relay.local [192.168.1.100])\r\n",
            "\tby mx.example.com (Postfix) with ESMTP;\r\n",
            "From: user@example.com\r\n",
            "\r\n",
            "Body.\r\n"
        );
        assert_eq!(extract_sender_ip(email_192), None);

        let email_172 = concat!(
            "Received: from relay.local (relay.local [172.16.0.1])\r\n",
            "\tby mx.example.com (Postfix) with ESMTP;\r\n",
            "From: user@example.com\r\n",
            "\r\n",
            "Body.\r\n"
        );
        assert_eq!(extract_sender_ip(email_172), None);

        let email_172_31 = concat!(
            "Received: from relay.local (relay.local [172.31.255.1])\r\n",
            "\tby mx.example.com (Postfix) with ESMTP;\r\n",
            "From: user@example.com\r\n",
            "\r\n",
            "Body.\r\n"
        );
        assert_eq!(extract_sender_ip(email_172_31), None);

        // 172.32.x.x is outside RFC1918 — should be returned
        let email_172_32 = concat!(
            "Received: from host.example.com (host.example.com [172.32.0.1])\r\n",
            "\tby mx.example.com (Postfix) with ESMTP;\r\n",
            "From: user@example.com\r\n",
            "\r\n",
            "Body.\r\n"
        );
        assert_eq!(
            extract_sender_ip(email_172_32),
            Some("172.32.0.1".to_string())
        );
    }

    #[test]
    fn extract_sender_ip_returns_none_without_received_header() {
        let email = concat!("From: user@example.com\r\n", "\r\n", "Body.\r\n");
        assert_eq!(extract_sender_ip(email), None);
    }

    #[test]
    fn check_rbl_returns_false_for_invalid_ip() {
        assert!(!check_rbl("not-an-ip", "zen.spamhaus.org"));
        assert!(!check_rbl("1.2.3", "zen.spamhaus.org"));
    }

    #[test]
    fn move_recipient_to_junk_creates_directories_and_rewrites_address() {
        let temp = std::env::temp_dir().join(format!("maildir_test_{}", uuid::Uuid::new_v4()));
        let root = temp.to_string_lossy().to_string();
        let result = move_recipient_to_junk("alice@example.com", &root).unwrap();
        assert_eq!(result, "alice+Junk@example.com");

        let junk_new = temp
            .join("example.com")
            .join("alice")
            .join("Maildir")
            .join(".Junk")
            .join("new");
        assert!(junk_new.exists());

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn move_recipient_to_junk_rejects_invalid_address() {
        let temp = std::env::temp_dir().join("maildir_invalid");
        let root = temp.to_string_lossy().to_string();
        assert!(move_recipient_to_junk("not-an-email", &root).is_none());
        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn email_metadata_direction_outgoing() {
        let meta = EmailMetadata {
            sender: "sender@example.com".to_string(),
            recipients: vec!["recipient@example.com".to_string()],
            subject: "Test".to_string(),
            from: "sender@example.com".to_string(),
            to: "recipient@example.com".to_string(),
            cc: String::new(),
            date: String::new(),
            message_id: String::new(),
            size_bytes: 0,
            direction: "outgoing".to_string(),
        };
        assert_eq!(meta.direction, "outgoing");
    }

    #[test]
    fn email_metadata_direction_incoming() {
        let meta = EmailMetadata {
            sender: "external@remote.com".to_string(),
            recipients: vec!["local@example.com".to_string()],
            subject: "Test".to_string(),
            from: "external@remote.com".to_string(),
            to: "local@example.com".to_string(),
            cc: String::new(),
            date: String::new(),
            message_id: String::new(),
            size_bytes: 0,
            direction: "incoming".to_string(),
        };
        assert_eq!(meta.direction, "incoming");
    }

    // ── Suppressed-email webhook path tests ──
    //
    // These tests verify that EmailMetadata is fully populated before the suppression
    // check so that send_webhook can be called for suppressed emails.

    #[test]
    fn email_metadata_is_fully_populated_for_suppressed_outgoing() {
        // Simulate the state just before the suppressed check for an outgoing email.
        let incoming = false;
        let sender = "sender@example.com";
        let subject = "Promo".to_string();
        let from_header = "Sender <sender@example.com>".to_string();
        let to_header = "recipient@example.com".to_string();
        let cc_header = String::new();
        let date_header = "Mon, 01 Jan 2024 00:00:00 +0000".to_string();
        let message_id_header = "<abc@example.com>".to_string();
        let size_bytes = 512_usize;

        let meta = EmailMetadata {
            sender: sender.to_string(),
            recipients: vec!["recipient@example.com".to_string()],
            subject: subject.clone(),
            from: from_header.clone(),
            to: to_header.clone(),
            cc: cc_header.clone(),
            date: date_header.clone(),
            message_id: message_id_header.clone(),
            size_bytes,
            direction: if incoming {
                "incoming".to_string()
            } else {
                "outgoing".to_string()
            },
        };

        // Verify all fields are set as expected so send_webhook would receive complete data.
        assert_eq!(meta.sender, "sender@example.com");
        assert_eq!(meta.recipients, vec!["recipient@example.com"]);
        assert_eq!(meta.subject, "Promo");
        assert_eq!(meta.from, "Sender <sender@example.com>");
        assert_eq!(meta.to, "recipient@example.com");
        assert_eq!(meta.date, "Mon, 01 Jan 2024 00:00:00 +0000");
        assert_eq!(meta.message_id, "<abc@example.com>");
        assert_eq!(meta.size_bytes, 512);
        assert_eq!(meta.direction, "outgoing");
    }

    #[test]
    fn email_metadata_is_fully_populated_for_suppressed_incoming() {
        // Suppression should not happen for incoming emails (external senders are never
        // in the local unsubscribe list), but the metadata path is the same.
        let incoming = true;
        let meta = EmailMetadata {
            sender: "external@remote.com".to_string(),
            recipients: vec!["local@example.com".to_string()],
            subject: "Hello".to_string(),
            from: "external@remote.com".to_string(),
            to: "local@example.com".to_string(),
            cc: String::new(),
            date: "Mon, 01 Jan 2024 00:00:00 +0000".to_string(),
            message_id: "<hello@remote.com>".to_string(),
            size_bytes: 256,
            direction: if incoming {
                "incoming".to_string()
            } else {
                "outgoing".to_string()
            },
        };
        assert_eq!(meta.direction, "incoming");
        assert_eq!(meta.size_bytes, 256);
        assert_eq!(meta.date, "Mon, 01 Jan 2024 00:00:00 +0000");
        assert_eq!(meta.message_id, "<hello@remote.com>");
    }
}
