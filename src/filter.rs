use std::io::{self, Read};

use crate::db::Database;

pub fn run_filter(db_path: &str, sender: &str, recipients: &[String], pixel_base_url: &str) {
    // 1. Read entire email from stdin
    let mut email_data = String::new();
    io::stdin()
        .read_to_string(&mut email_data)
        .expect("Failed to read email from stdin");

    // 2. Open database and check if tracking is enabled for sender
    let db = Database::open(db_path);
    let tracking = db.is_tracking_enabled_for_sender(sender);

    // 3. If tracking enabled, inject pixel
    let mut modified = email_data.clone();

    if tracking {
        let message_id = uuid::Uuid::new_v4().to_string();
        let pixel_url = format!("{}{}", pixel_base_url, message_id);
        let pixel_tag = format!(
            r#"<img src="{}" width="1" height="1" style="display:none" alt="" />"#,
            pixel_url
        );

        // Try to inject before </body>
        if let Some(pos) = modified.to_lowercase().rfind("</body>") {
            modified.insert_str(pos, &pixel_tag);
        } else if modified.contains("<html") || modified.contains("<HTML") {
            // Append to end if HTML but no </body>
            modified.push_str(&pixel_tag);
        }
        // If not HTML at all, skip pixel injection

        // Record tracked message
        let subject = extract_header(&email_data, "Subject").unwrap_or_default();
        let recipient = recipients.first().map(|s| s.as_str()).unwrap_or("");
        db.create_tracked_message(&message_id, sender, recipient, &subject, None);
    }

    // 4. Reinject via SMTP to 127.0.0.1:10025
    reinject_smtp(&modified, sender, recipients);
}

fn extract_header(email: &str, header_name: &str) -> Option<String> {
    let prefix = format!("{}:", header_name.to_lowercase());
    for line in email.lines() {
        if line.is_empty() {
            break; // End of headers
        }
        if line.to_lowercase().starts_with(&prefix) {
            return Some(line[header_name.len() + 1..].trim().to_string());
        }
    }
    None
}

fn reinject_smtp(email: &str, sender: &str, recipients: &[String]) {
    use std::io::Write;
    use std::net::TcpStream;

    let mut stream =
        TcpStream::connect("127.0.0.1:10025").expect("Failed to connect to reinjection port");

    let mut buf = [0u8; 512];

    // Read greeting
    read_response(&mut stream, &mut buf);

    // EHLO
    write!(stream, "EHLO localhost\r\n").unwrap();
    stream.flush().unwrap();
    read_response(&mut stream, &mut buf);

    // MAIL FROM
    write!(stream, "MAIL FROM:<{}>\r\n", sender).unwrap();
    stream.flush().unwrap();
    read_response(&mut stream, &mut buf);

    // RCPT TO for each recipient
    for rcpt in recipients {
        write!(stream, "RCPT TO:<{}>\r\n", rcpt).unwrap();
        stream.flush().unwrap();
        read_response(&mut stream, &mut buf);
    }

    // DATA
    write!(stream, "DATA\r\n").unwrap();
    stream.flush().unwrap();
    read_response(&mut stream, &mut buf);

    // Send email body (dot-stuff lines starting with .)
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
    read_response(&mut stream, &mut buf);

    // QUIT
    write!(stream, "QUIT\r\n").unwrap();
    stream.flush().unwrap();
    let _ = read_response(&mut stream, &mut buf);
}

fn read_response(stream: &mut std::net::TcpStream, buf: &mut [u8]) -> String {
    use std::io::Read;
    let n = stream.read(buf).unwrap_or(0);
    String::from_utf8_lossy(&buf[..n]).to_string()
}
