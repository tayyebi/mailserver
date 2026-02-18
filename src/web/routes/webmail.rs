use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Response},
    Form,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use log::{debug, error, info, warn};
use serde::Deserialize;

use crate::db::Account;
use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Helpers ──

fn is_safe_path_component(s: &str) -> bool {
    !s.is_empty() && !s.contains('/') && !s.contains('\\') && s != "." && s != ".."
}

const MAILDIR_ROOT: &str = "/data/mail";

fn maildir_path(domain: &str, username: &str) -> String {
    format!("{}/{}/{}/Maildir", MAILDIR_ROOT, domain, username)
}

fn sanitize_header_value(s: &str) -> String {
    s.replace(['\r', '\n'], " ")
        .chars()
        .filter(|c| !c.is_control())
        .collect()
fn sanitize_header_value(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

// ── Structures ──

#[allow(dead_code)]
pub struct WebmailEmail {
    pub filename: String,
    pub subject: String,
    pub from: String,
    pub to: String,
    pub date: String,
    pub is_new: bool,
}

#[derive(Deserialize)]
pub struct WebmailQuery {
    pub account_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct ComposeForm {
    pub account_id: i64,
    pub to: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    pub subject: String,
    #[serde(default)]
    pub reply_to: String,
    #[serde(default)]
    pub in_reply_to: String,
    #[serde(default)]
    pub priority: String,
    #[serde(default)]
    pub sender_name: String,
    #[serde(default)]
    pub custom_headers: String,
    pub body: String,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "webmail/inbox.html")]
struct InboxTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    accounts: Vec<Account>,
    selected_account: Option<Account>,
    emails: Vec<WebmailEmail>,
    logs: Vec<String>,
}

#[derive(Template)]
#[template(path = "webmail/view.html")]
struct ViewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    account: Account,
    subject: String,
    from: String,
    to: String,
    date: String,
    body: String,
}

#[derive(Template)]
#[template(path = "webmail/compose.html")]
struct ComposeTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    accounts: Vec<Account>,
    selected_account: Option<Account>,
    send_log: Vec<String>,
}

// ── Handlers ──

pub async fn inbox(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(query): Query<WebmailQuery>,
) -> Html<String> {
    info!("[web] GET /webmail — inbox");
    let mut logs: Vec<String> = Vec::new();

    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;
    logs.push(format!("Loaded {} accounts from database", accounts.len()));

    let mut selected_account: Option<Account> = None;
    let mut emails: Vec<WebmailEmail> = Vec::new();

    if let Some(account_id) = query.account_id {
        logs.push(format!("Account ID {} selected", account_id));
        let acct = state
            .blocking_db(move |db| db.get_account_with_domain(account_id))
            .await;
        if let Some(acct) = acct {
            let domain = acct.domain_name.as_deref().unwrap_or("unknown");
            if !is_safe_path_component(domain) || !is_safe_path_component(&acct.username) {
                logs.push("Invalid domain or username for path construction".to_string());
                warn!("[web] unsafe path component: domain={}, username={}", domain, acct.username);
                selected_account = Some(acct);
            } else {
            let maildir_base = maildir_path(domain, &acct.username);
            logs.push(format!("Maildir path: {}", maildir_base));

            // Create Maildir directories if they don't exist
            for subdir in &["new", "cur", "tmp"] {
                let dir_path = format!("{}/{}", maildir_base, subdir);
                if let Err(e) = std::fs::create_dir_all(&dir_path) {
                    logs.push(format!("Warning: Failed to create directory {}: {} (subsequent operations may fail)", dir_path, e));
                    warn!("[web] failed to create maildir directory {}: {}", dir_path, e);
                }
            }

            for (subdir, is_new) in &[("new", true), ("cur", false)] {
                let dir_path = format!("{}/{}", maildir_base, subdir);
                logs.push(format!("Scanning directory: {}", dir_path));
                match std::fs::read_dir(&dir_path) {
                    Ok(entries) => {
                        for entry in entries.flatten() {
                            let path = entry.path();
                            if !path.is_file() {
                                continue;
                            }
                            let fname = entry.file_name().to_string_lossy().to_string();
                            logs.push(format!("Reading email file: {}", fname));
                            match std::fs::read(&path) {
                                Ok(data) => {
                                    match mailparse::parse_mail(&data) {
                                        Ok(parsed) => {
                                            let subject = parsed
                                                .headers
                                                .iter()
                                                .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
                                                .map(|h| h.get_value())
                                                .unwrap_or_default();
                                            let from = parsed
                                                .headers
                                                .iter()
                                                .find(|h| h.get_key().eq_ignore_ascii_case("From"))
                                                .map(|h| h.get_value())
                                                .unwrap_or_default();
                                            let to = parsed
                                                .headers
                                                .iter()
                                                .find(|h| h.get_key().eq_ignore_ascii_case("To"))
                                                .map(|h| h.get_value())
                                                .unwrap_or_default();
                                            let date = parsed
                                                .headers
                                                .iter()
                                                .find(|h| h.get_key().eq_ignore_ascii_case("Date"))
                                                .map(|h| h.get_value())
                                                .unwrap_or_default();
                                            let encoded =
                                                URL_SAFE_NO_PAD.encode(fname.as_bytes());
                                            logs.push(format!(
                                                "Parsed email: subject={}, from={}, is_new={}",
                                                subject, from, is_new
                                            ));
                                            emails.push(WebmailEmail {
                                                filename: encoded,
                                                subject,
                                                from,
                                                to,
                                                date,
                                                is_new: *is_new,
                                            });
                                        }
                                        Err(e) => {
                                            logs.push(format!(
                                                "Failed to parse email {}: {}",
                                                fname, e
                                            ));
                                            warn!(
                                                "[web] failed to parse email {}: {}",
                                                fname, e
                                            );
                                        }
                                    }
                                }
                                Err(e) => {
                                    logs.push(format!("Failed to read file {}: {}", fname, e));
                                    warn!("[web] failed to read email file {}: {}", fname, e);
                                }
                            }
                        }
                    }
                    Err(e) => {
                        logs.push(format!(
                            "Directory {} not found or not readable: {}",
                            dir_path, e
                        ));
                        debug!(
                            "[web] maildir directory {} not accessible: {}",
                            dir_path, e
                        );
                    }
                }
            }

            logs.push(format!("Total emails found: {}", emails.len()));
            selected_account = Some(acct);
            } // end safe path else
        } else {
            logs.push(format!("Account ID {} not found in database", account_id));
            warn!("[web] account id={} not found for webmail", account_id);
        }
    } else {
        logs.push("No account selected".to_string());
    }

    let tmpl = InboxTemplate {
        nav_active: "Webmail",
        flash: None,
        accounts,
        selected_account,
        emails,
        logs,
    };
    Html(tmpl.render().unwrap())
}

pub async fn view_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(filename_b64): Path<String>,
    Query(query): Query<WebmailQuery>,
) -> Response {
    info!("[web] GET /webmail/view/{} — viewing email", filename_b64);

    let account_id = match query.account_id {
        Some(id) => id,
        None => {
            warn!("[web] no account_id provided for email view");
            return Html("Missing account_id parameter".to_string()).into_response();
        }
    };

    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => {
            warn!("[web] account id={} not found for email view", account_id);
            return Html("Account not found".to_string()).into_response();
        }
    };

    let filename = match URL_SAFE_NO_PAD.decode(filename_b64.as_bytes()) {
        Ok(bytes) => match String::from_utf8(bytes) {
            Ok(s) => s,
            Err(_) => {
                error!("[web] invalid UTF-8 in decoded filename");
                return Html("Invalid filename encoding".to_string()).into_response();
            }
        },
        Err(e) => {
            error!("[web] failed to decode base64 filename: {}", e);
            return Html("Invalid filename encoding".to_string()).into_response();
        }
    };

    debug!("[web] decoded filename: {}", filename);

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    if !is_safe_path_component(domain) || !is_safe_path_component(&acct.username) || !is_safe_path_component(&filename) {
        warn!("[web] unsafe path component in view_email");
        return Html("Invalid path component".to_string()).into_response();
    }
    let maildir_base = maildir_path(domain, &acct.username);

    // Search in both new/ and cur/
    let mut file_path = None;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", maildir_base, subdir, filename);
        debug!("[web] checking path: {}", candidate);
        if std::path::Path::new(&candidate).is_file() {
            file_path = Some(candidate);
            break;
        }
    }

    let file_path = match file_path {
        Some(p) => p,
        None => {
            warn!("[web] email file not found: {}", filename);
            return Html("Email not found".to_string()).into_response();
        }
    };

    debug!("[web] reading email from: {}", file_path);
    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(e) => {
            error!("[web] failed to read email file: {}", e);
            return Html("Failed to read email".to_string()).into_response();
        }
    };

    let parsed = match mailparse::parse_mail(&data) {
        Ok(p) => p,
        Err(e) => {
            error!("[web] failed to parse email: {}", e);
            return Html("Failed to parse email".to_string()).into_response();
        }
    };

    let subject = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let from = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("From"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let to = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("To"))
        .map(|h| h.get_value())
        .unwrap_or_default();
    let date = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Date"))
        .map(|h| h.get_value())
        .unwrap_or_default();

    // Extract body: prefer text/plain, fall back to text/html (escaped)
    let body = extract_body(&parsed);
    debug!(
        "[web] parsed email: subject={}, from={}, body_len={}",
        subject,
        from,
        body.len()
    );

    let tmpl = ViewTemplate {
        nav_active: "Webmail",
        flash: None,
        account: acct,
        subject,
        from,
        to,
        date,
        body,
    };
    Html(tmpl.render().unwrap()).into_response()
}

fn extract_body(parsed: &mailparse::ParsedMail) -> String {
    // Try to find text/plain part first
    if let Some(text) = find_body_part(parsed, "text/plain") {
        return text;
    }
    // Fall back to text/html (escape HTML for safe display)
    if let Some(html) = find_body_part(parsed, "text/html") {
        return html
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;");
    }
    // Last resort: try top-level body
    parsed.get_body().unwrap_or_default()
}

fn find_body_part(parsed: &mailparse::ParsedMail, mime_type: &str) -> Option<String> {
    if parsed.subparts.is_empty() {
        let ctype = parsed.ctype.mimetype.to_lowercase();
        if ctype == mime_type {
            return parsed.get_body().ok();
        }
        return None;
    }
    for part in &parsed.subparts {
        if let Some(body) = find_body_part(part, mime_type) {
            return Some(body);
        }
    }
    None
}

pub async fn compose(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(query): Query<WebmailQuery>,
) -> Html<String> {
    info!("[web] GET /webmail/compose — compose email form");

    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;

    let selected_account = if let Some(account_id) = query.account_id {
        state
            .blocking_db(move |db| db.get_account_with_domain(account_id))
            .await
    } else {
        None
    };

    let tmpl = ComposeTemplate {
        nav_active: "Webmail",
        flash: None,
        accounts,
        selected_account,
        send_log: Vec::new(),
    };
    Html(tmpl.render().unwrap())
}

pub async fn send_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<ComposeForm>,
) -> Html<String> {
    info!("[web] POST /webmail/send — sending email");
    let mut send_log: Vec<String> = Vec::new();
    let flash: Option<String>;

    send_log.push(format!("Looking up account ID {}", form.account_id));
    let account_id = form.account_id;
    let acct = state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await;

    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;

    match acct {
        Some(ref acct) => {
            let domain = acct.domain_name.as_deref().unwrap_or("unknown");
            let email_addr = format!("{}@{}", acct.username, domain);
            let sender_name = sanitize_header_value(form.sender_name.trim());
            let from_addr = if sender_name.is_empty() {
                email_addr.clone()
            } else {
                format!("{} <{}>", sender_name, email_addr)
            };
            send_log.push(format!("From address: {}", from_addr));
            send_log.push(format!("To: {}", form.to));
            if !form.cc.trim().is_empty() {
                send_log.push(format!("CC: {}", form.cc));
            }
            if !form.bcc.trim().is_empty() {
                send_log.push(format!("BCC: {}", form.bcc));
            }
            if !form.reply_to.trim().is_empty() {
                send_log.push(format!("Reply-To: {}", form.reply_to));
            }
            if !form.in_reply_to.trim().is_empty() {
                send_log.push(format!("In-Reply-To: {}", form.in_reply_to));
            }
            if !form.priority.is_empty() && form.priority != "normal" {
                send_log.push(format!("Priority: {}", form.priority));
            }
            send_log.push(format!("Subject: {}", form.subject));
            send_log.push(format!("Body length: {} chars", form.body.len()));

            send_log.push("Building email message...".to_string());
            let mut builder = lettre::Message::builder()
                .from(from_addr.parse().unwrap_or_else(|e| {
                    send_log.push(format!("Warning: could not parse from address '{}': {}, using fallback", from_addr, e));
                    "noreply@localhost".parse().unwrap()
                }))
                .to(match form.to.parse() {
                    Ok(addr) => addr,
                    Err(e) => {
                        send_log.push(format!("Invalid To address: {}", e));
                        error!("[web] invalid To address {}: {}", form.to, e);
                        flash = Some(format!("Invalid To address: {}", e));
                        let tmpl = ComposeTemplate {
                            nav_active: "Webmail",
                            flash: flash.as_deref(),
                            accounts,
                            selected_account: Some(acct.clone()),
                            send_log,
                        };
                        return Html(tmpl.render().unwrap());
                    }
                })
                .subject(&form.subject);

            // Add CC recipients
            for addr in form.cc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                match addr.parse() {
                    Ok(a) => builder = builder.cc(a),
                    Err(e) => send_log.push(format!("Warning: skipping invalid CC address '{}': {}", addr, e)),
                }
            }

            // Add BCC recipients
            for addr in form.bcc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                match addr.parse() {
                    Ok(a) => builder = builder.bcc(a),
                    Err(e) => send_log.push(format!("Warning: skipping invalid BCC address '{}': {}", addr, e)),
                }
            }

            // Set Reply-To
            if !form.reply_to.trim().is_empty() {
                match form.reply_to.trim().parse() {
                    Ok(a) => builder = builder.reply_to(a),
                    Err(e) => send_log.push(format!("Warning: invalid Reply-To address '{}': {}", form.reply_to, e)),
                }
            }

            // Set In-Reply-To
            if !form.in_reply_to.trim().is_empty() {
                let in_reply_to = sanitize_header_value(form.in_reply_to.trim());
                builder = builder.in_reply_to(in_reply_to);
            }

            // Set priority via X-Priority header
            {
                use lettre::message::header::{HeaderName, HeaderValue};
                let priority_value = match form.priority.as_str() {
                    "lowest" => Some("5 (Lowest)"),
                    "low" => Some("4 (Low)"),
                    "high" => Some("2 (High)"),
                    "highest" => Some("1 (Highest)"),
                    _ => None, // "normal" or empty — no header needed
                };
                if let Some(val) = priority_value {
                    if let Ok(header_name) = HeaderName::new_from_ascii("X-Priority".to_string()) {
                        builder = builder.raw_header(HeaderValue::new(
                            header_name,
                            val.to_string(),
                        ));
                    }
                }
            }

            // Add custom headers (one per line, format: "Header-Name: value")
            {
                use lettre::message::header::{HeaderName, HeaderValue};
                for line in form.custom_headers.lines().map(str::trim).filter(|l| !l.is_empty()) {
                    if let Some((name, value)) = line.split_once(':') {
                        let name = name.trim();
                        let value = sanitize_header_value(value.trim());
                        if !name.is_empty() && !value.is_empty() {
                            match HeaderName::new_from_ascii(name.to_string()) {
                                Ok(header_name) => {
                                    builder = builder.raw_header(HeaderValue::new(
                                        header_name,
                                        value.to_string(),
                                    ));
                                    send_log.push(format!("Custom header: {}: {}", name, value));
                                }
                                Err(e) => {
                                    send_log.push(format!("Warning: invalid header name '{}': {}", name, e));
                                }
                            }
                        }
                    }
                }
            }

            let email = match builder.body(form.body.clone())
            {
                Ok(email) => {
                    send_log.push("Email message built successfully".to_string());
                    email
                }
                Err(e) => {
                    send_log.push(format!("Failed to build email: {}", e));
                    error!("[web] failed to build email: {}", e);
                    flash = Some(format!("Failed to build email: {}", e));
                    let tmpl = ComposeTemplate {
                        nav_active: "Webmail",
                        flash: flash.as_deref(),
                        accounts,
                        selected_account: Some(acct.clone()),
                        send_log,
                    };
                    return Html(tmpl.render().unwrap());
                }
            };

            send_log.push("Connecting to SMTP server at 127.0.0.1:25...".to_string());
            use lettre::{SmtpTransport, Transport};
            // builder_dangerous disables TLS — safe here because we connect to the
            // local Postfix instance on the loopback interface (same as filter.rs).
            match SmtpTransport::builder_dangerous("127.0.0.1")
                .port(25)
                .build()
                .send(&email)
            {
                Ok(response) => {
                    send_log.push(format!("SMTP response: {:?}", response));
                    send_log.push("Email sent successfully!".to_string());
                    info!("[web] email sent successfully to {}", form.to);
                    flash = Some("Email sent successfully!".to_string());
                }
                Err(e) => {
                    send_log.push(format!("SMTP error: {}", e));
                    error!("[web] failed to send email: {}", e);
                    flash = Some(format!("Failed to send email: {}", e));
                }
            }

            let tmpl = ComposeTemplate {
                nav_active: "Webmail",
                flash: flash.as_deref(),
                accounts,
                selected_account: Some(acct.clone()),
                send_log,
            };
            Html(tmpl.render().unwrap())
        }
        None => {
            send_log.push(format!("Account ID {} not found!", form.account_id));
            error!(
                "[web] account id={} not found for sending email",
                form.account_id
            );
            flash = Some("Account not found".to_string());
            let tmpl = ComposeTemplate {
                nav_active: "Webmail",
                flash: flash.as_deref(),
                accounts,
                selected_account: None,
                send_log,
            };
            Html(tmpl.render().unwrap())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::maildir_path;

    #[test]
    fn maildir_path_uses_data_mail_root() {
        let path = maildir_path("example.com", "alice");
        assert_eq!(path, "/data/mail/example.com/alice/Maildir");
    }
}
