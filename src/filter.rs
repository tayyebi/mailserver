use log::{info, debug};
use std::io::{self, Read};

use crate::db::Database;

pub fn run_filter(db_path: &str, sender: &str, recipients: &[String], pixel_base_url: &str) {
    info!("[filter] starting content filter sender={}, recipients={}", sender, recipients.join(", "));

    // 1. Read entire email from stdin
    debug!("[filter] reading email from stdin");
    let mut email_data = String::new();
    io::stdin()
        .read_to_string(&mut email_data)
        .expect("Failed to read email from stdin");
    info!("[filter] read email from stdin ({} bytes)", email_data.len());

    // 2. Open database and check if tracking is enabled for sender
    debug!("[filter] opening database at {}", db_path);
    let db = Database::open(db_path);
    let tracking = db.is_tracking_enabled_for_sender(sender);
    info!("[filter] tracking enabled for sender={}: {}", sender, tracking);

    // 3. If tracking enabled, inject pixel
    let mut modified = email_data.clone();

    if tracking {
        let message_id = uuid::Uuid::new_v4().to_string();
        let pixel_url = format!("{}{}", pixel_base_url, message_id);
        let pixel_tag = format!(
            r#"<img src="{}" width="1" height="1" style="display:none" alt="" />"#,
            pixel_url
        );
        debug!("[filter] generated tracking pixel message_id={}", message_id);

        // Try to inject before </body>
        if let Some(pos) = modified.to_lowercase().rfind("</body>") {
            modified.insert_str(pos, &pixel_tag);
            info!("[filter] injected tracking pixel before </body> for message_id={}", message_id);
        } else if modified.contains("<html") || modified.contains("<HTML") {
            // Append to end if HTML but no </body>
            modified.push_str(&pixel_tag);
            info!("[filter] appended tracking pixel to HTML email for message_id={}", message_id);
        } else {
            debug!("[filter] email is not HTML — skipping pixel injection for message_id={}", message_id);
        }

        // Record tracked message
        let subject = extract_header(&email_data, "Subject").unwrap_or_default();
        let recipient = recipients.first().map(|s| s.as_str()).unwrap_or("");
        debug!("[filter] recording tracked message: message_id={}, subject={}", message_id, subject);
        db.create_tracked_message(&message_id, sender, recipient, &subject, None);
        info!("[filter] tracked message recorded: message_id={}", message_id);
    } else {
        debug!("[filter] no tracking — passing email through unmodified");
    }

    // 4. Reinject via SMTP to 127.0.0.1:10025
    info!("[filter] reinjecting email via SMTP to 127.0.0.1:10025");
    reinject_smtp(&modified, sender, recipients);
    info!("[filter] email reinjected successfully");
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

fn reinject_smtp(email: &str, sender: &str, recipients: &[String]) {
    use std::io::Write;
    use std::net::TcpStream;

    debug!("[filter] connecting to 127.0.0.1:10025 for reinjection");
    let mut stream =
        TcpStream::connect("127.0.0.1:10025").expect("Failed to connect to reinjection port");
    debug!("[filter] connected to reinjection port");

    let mut buf = [0u8; 512];

    // Read greeting
    let greeting = read_response(&mut stream, &mut buf);
    debug!("[filter] SMTP greeting: {}", greeting.trim());

    // EHLO
    write!(stream, "EHLO localhost\r\n").unwrap();
    stream.flush().unwrap();
    let resp = read_response(&mut stream, &mut buf);
    debug!("[filter] EHLO response: {}", resp.trim());

    // MAIL FROM
    debug!("[filter] sending MAIL FROM:<{}>", sender);
    write!(stream, "MAIL FROM:<{}>\r\n", sender).unwrap();
    stream.flush().unwrap();
    let resp = read_response(&mut stream, &mut buf);
    debug!("[filter] MAIL FROM response: {}", resp.trim());

    // RCPT TO for each recipient
    for rcpt in recipients {
        debug!("[filter] sending RCPT TO:<{}>", rcpt);
        write!(stream, "RCPT TO:<{}>\r\n", rcpt).unwrap();
        stream.flush().unwrap();
        let resp = read_response(&mut stream, &mut buf);
        debug!("[filter] RCPT TO response: {}", resp.trim());
    }

    // DATA
    debug!("[filter] sending DATA command");
    write!(stream, "DATA\r\n").unwrap();
    stream.flush().unwrap();
    let resp = read_response(&mut stream, &mut buf);
    debug!("[filter] DATA response: {}", resp.trim());

    // Send email body (dot-stuff lines starting with .)
    debug!("[filter] sending email body ({} bytes)", email.len());
    for line in email.lines() {
        if line.starts_with('.') {
            write!(stream, ".{}\r\n", line).unwrap();
        } else {
            write!(stream, "{}\r\n", line).unwrap();
        }
    }

    // End DATA
    write!(stream, ".\r\n").unwrap();
    stream.flush().unwrap();
    let resp = read_response(&mut stream, &mut buf);
    debug!("[filter] end-of-data response: {}", resp.trim());

    // QUIT
    write!(stream, "QUIT\r\n").unwrap();
    stream.flush().unwrap();
    let resp = read_response(&mut stream, &mut buf);
    debug!("[filter] QUIT response: {}", resp.trim());

    info!("[filter] SMTP reinjection completed for sender={}", sender);
}

fn read_response(stream: &mut std::net::TcpStream, buf: &mut [u8]) -> String {
    use std::io::Read;
    let n = stream.read(buf).unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).to_string()
}
