/*
 * Postfix Reinjection Utility
 * 
 * This utility is designed to reinject processed emails back into Postfix.
 * It is typically used in a content filter pipeline where an email is passed via stdin,
 * processed, and then needs to be sent back to the MTA for final delivery.
 * 
 * Key responsibilities:
 * - Reading the raw email content from stdin.
 * - Parsing the email to extract the envelope sender (From) and recipient (To).
 * - Constructing a new SMTP message.
 * - Sending the message to a specific Postfix listener port (default 10025) via SMTP.
 */

use std::env;
use std::fs::File;
use std::io::{self, Read};
use std::process;
use lettre::{Message, SmtpTransport, Transport};
use lettre::transport::smtp::authentication::Credentials;
use mailparse::parse_mail;
use regex::Regex;

fn extract_email_address(header_value: &str) -> String {
    let re = Regex::new(r"<([^>]+)>").unwrap();
    if let Some(caps) = re.captures(header_value) {
        caps.get(1).map_or(header_value.trim(), |m| m.as_str()).to_string()
    } else {
        header_value.trim().to_string()
    }
}

fn main() {
    // Read email from stdin
    let mut buffer = Vec::new();
    if let Err(e) = io::stdin().read_to_end(&mut buffer) {
        eprintln!("ERROR: Failed to read stdin: {}", e);
        process::exit(1);
    }

    // Parse email
    let parsed = match parse_mail(&buffer) {
        Ok(mail) => mail,
        Err(e) => {
            eprintln!("ERROR: Failed to parse email: {}", e);
            process::exit(1);
        }
    };

    // Extract headers
    let mut from_addr = None;
    let mut to_addr = None;
    for header in parsed.get_headers() {
        let key = header.get_key().to_ascii_lowercase();
        let value = header.get_value();
        if key == "from" {
            from_addr = Some(extract_email_address(&value));
        } else if key == "to" {
            to_addr = Some(extract_email_address(&value));
        }
    }

    let to_addr = match to_addr {
        Some(addr) => addr,
        None => {
            eprintln!("ERROR: No To header found");
            process::exit(1);
        }
    };
    let from_addr = from_addr.unwrap_or_else(|| "noreply@localhost".to_string());

    // Build the message
    let message = match Message::builder()
        .from(from_addr.parse().unwrap())
        .to(to_addr.parse().unwrap())
        .subject("") // subject is required, but will be replaced by raw
        .body(String::from_utf8_lossy(&buffer).to_string()) {
        Ok(msg) => msg,
        Err(e) => {
            eprintln!("ERROR: Failed to build message: {}", e);
            process::exit(1);
        }
    };

    // Connect to SMTP (127.0.0.1:10025, no TLS)
    let mailer = SmtpTransport::builder_dangerous("127.0.0.1")
        .port(10025)
        .build();

    match mailer.send(&message) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("ERROR: SMTP error: {}", e);
            process::exit(1);
        }
    }
}
