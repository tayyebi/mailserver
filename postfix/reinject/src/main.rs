/*
 * Postfix Reinjection Utility
 * 
 * This utility is designed to reinject processed emails back into Postfix.
 * It is typically used in a content filter pipeline where an email is passed via stdin,
 * processed, and then needs to be sent back to the MTA for final delivery.
 * 
 * Key responsibilities:
 * - Reading the raw email content from stdin.
 * - Parsing the email to extract the envelope sender (From) and recipient (To) if not provided via args.
 * - Constructing a new SMTP message.
 * - Sending the message to a specific Postfix listener port (default 10025) via SMTP.
 */

use std::io::{self, Read};
use std::process;
use lettre::{SmtpTransport, Transport};
use lettre::address::Envelope;
use mailparse::parse_mail;
use regex::Regex;
use clap::Parser;

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Envelope sender address
    #[arg(long)]
    sender: Option<String>,

    /// Envelope recipient address
    #[arg(long)]
    recipient: Option<String>,

    /// SMTP server host
    #[arg(long, default_value = "127.0.0.1")]
    host: String,

    /// SMTP server port
    #[arg(long, default_value_t = 10025)]
    port: u16,
}

fn extract_email_address(header_value: &str) -> String {
    let re = Regex::new(r"<([^>]+)>").unwrap();
    if let Some(caps) = re.captures(header_value) {
        caps.get(1).map_or(header_value.trim(), |m| m.as_str()).to_string()
    } else {
        header_value.trim().to_string()
    }
}

fn main() {
    let args = Args::parse();

    // Read email from stdin
    let mut buffer = Vec::new();
    if let Err(e) = io::stdin().read_to_end(&mut buffer) {
        eprintln!("ERROR: Failed to read stdin: {}", e);
        process::exit(1);
    }

    let (from_addr, to_addr) = if args.sender.is_some() && args.recipient.is_some() {
        let s = args.sender.unwrap();
        let r = args.recipient.unwrap();
        // Handle empty sender (bounce)
        let sender = if s.is_empty() {
            "<>".to_string()
        } else {
            s
        };
        (sender, r)
    } else {
        // Parse email to extract headers if args are missing
        let parsed = match parse_mail(&buffer) {
            Ok(mail) => mail,
            Err(e) => {
                eprintln!("ERROR: Failed to parse email: {}", e);
                process::exit(1);
            }
        };

        let mut extracted_from = None;
        let mut extracted_to = None;

        for header in parsed.get_headers() {
            let key = header.get_key().to_ascii_lowercase();
            let value = header.get_value();
            if key == "from" {
                extracted_from = Some(extract_email_address(&value));
            } else if key == "to" {
                extracted_to = Some(extract_email_address(&value));
            }
        }

        let final_from = args.sender.unwrap_or_else(|| extracted_from.unwrap_or_else(|| "noreply@localhost".to_string()));
        let final_to = args.recipient.unwrap_or_else(|| match extracted_to {
            Some(addr) => addr,
            None => {
                eprintln!("ERROR: No To header found and no recipient argument provided");
                process::exit(1);
            }
        });

        (final_from, final_to)
    };

    // Parse sender address (handle <> or empty as null sender)
    let sender_opt = if from_addr == "<>" || from_addr.is_empty() {
        None
    } else {
        Some(from_addr.parse().expect("Failed to parse sender address"))
    };

    // Parse recipient address
    let recipient_addr = to_addr.parse().expect("Failed to parse recipient address");

    // Construct Envelope
    let envelope = match Envelope::new(
        sender_opt,
        vec![recipient_addr],
    ) {
        Ok(env) => env,
        Err(e) => {
            eprintln!("ERROR: Failed to create envelope: {}", e);
            process::exit(1);
        }
    };

    // Connect to SMTP
    let mailer = SmtpTransport::builder_dangerous(&args.host)
        .port(args.port)
        .build();

    // Send raw email with explicit envelope
    match mailer.send_raw(&envelope, &buffer) {
        Ok(_) => {},
        Err(e) => {
            eprintln!("ERROR: SMTP error: {}", e);
            process::exit(1);
        }
    }
}
