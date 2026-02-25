//! REST API endpoints for email operations.
//!
//! All endpoints require HTTP Basic Auth (admin credentials).
//!
//! Endpoints:
//!   `GET  /api/emails`           — List emails in an account's inbox or folder
//!   `GET  /api/emails/:filename` — Read a single email
//!   `POST /api/emails`           — Send an email
//!   `DELETE /api/emails/:filename` — Delete an email

use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use log::info;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

use super::webmail::{
    extract_body, folder_root, is_safe_folder, is_safe_path_component, maildir_path, read_emails,
};

// ── Query / body types ────────────────────────────────────────────────────────

#[derive(Deserialize)]
pub struct ListQuery {
    pub account_id: i64,
    #[serde(default)]
    pub folder: String,
    #[serde(default = "default_page")]
    pub page: usize,
}

fn default_page() -> usize {
    1
}

#[derive(Deserialize)]
pub struct EmailQuery {
    pub account_id: i64,
    #[serde(default)]
    pub folder: String,
}

#[derive(Deserialize, Serialize)]
pub struct SendEmailBody {
    pub account_id: i64,
    pub to: String,
    pub subject: String,
    pub body: String,
    #[serde(default)]
    pub cc: String,
    #[serde(default)]
    pub bcc: String,
    #[serde(default)]
    pub reply_to: String,
    #[serde(default)]
    pub sender_name: String,
    /// "plain" or "html"
    #[serde(default = "default_body_format")]
    pub body_format: String,
}

fn default_body_format() -> String {
    "plain".to_string()
}

// ── Helpers ───────────────────────────────────────────────────────────────────

const PAGE_SIZE: usize = 20;

fn json_error(status: StatusCode, message: &str) -> impl IntoResponse {
    (status, Json(json!({"error": message})))
}

// ── GET /api/emails ───────────────────────────────────────────────────────────

pub async fn list_emails(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(q): Query<ListQuery>,
) -> impl IntoResponse {
    info!(
        "[api] GET /api/emails account_id={} folder={:?} page={}",
        q.account_id, q.folder, q.page
    );

    if !is_safe_folder(&q.folder) {
        return json_error(StatusCode::BAD_REQUEST, "Invalid folder name").into_response();
    }

    let account_id = q.account_id;
    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return json_error(StatusCode::NOT_FOUND, "Account not found").into_response(),
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain) || !is_safe_path_component(&acct.username) {
        return json_error(StatusCode::BAD_REQUEST, "Invalid account path").into_response();
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let folder = q.folder.clone();
    let mut logs = Vec::new();
    let emails = read_emails(&maildir_base, &folder, &mut logs);

    let total = emails.len();
    let total_pages = if total == 0 {
        1
    } else {
        (total + PAGE_SIZE - 1) / PAGE_SIZE
    };
    let page = q.page.max(1).min(total_pages);
    let start = (page - 1) * PAGE_SIZE;

    let page_emails: Vec<serde_json::Value> = emails
        .iter()
        .skip(start)
        .take(PAGE_SIZE)
        .map(|e| {
            json!({
                "filename": e.filename,
                "subject": e.subject,
                "from": e.from,
                "to": e.to,
                "date": e.date,
                "is_new": e.is_new,
                "is_spam": e.is_spam
            })
        })
        .collect();

    Json(json!({
        "account_id": q.account_id,
        "folder": if q.folder.is_empty() { "INBOX" } else { q.folder.as_str() },
        "page": page,
        "total_pages": total_pages,
        "total_count": total,
        "emails": page_emails
    }))
    .into_response()
}

// ── GET /api/emails/:filename ─────────────────────────────────────────────────

pub async fn get_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(filename_b64): Path<String>,
    Query(q): Query<EmailQuery>,
) -> impl IntoResponse {
    info!(
        "[api] GET /api/emails/{} account_id={}",
        filename_b64, q.account_id
    );

    if !is_safe_folder(&q.folder) {
        return json_error(StatusCode::BAD_REQUEST, "Invalid folder name").into_response();
    }

    let account_id = q.account_id;
    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return json_error(StatusCode::NOT_FOUND, "Account not found").into_response(),
    };

    let filename = match URL_SAFE_NO_PAD
        .decode(filename_b64.as_bytes())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(f) => f,
        None => {
            return json_error(StatusCode::BAD_REQUEST, "Invalid filename encoding")
                .into_response()
        }
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
    {
        return json_error(StatusCode::BAD_REQUEST, "Invalid path component").into_response();
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let root = folder_root(&maildir_base, &q.folder);

    let mut file_path = None;
    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            file_path = Some(candidate);
            break;
        }
    }

    let file_path = match file_path {
        Some(p) => p,
        None => return json_error(StatusCode::NOT_FOUND, "Email not found").into_response(),
    };

    let data = match std::fs::read(&file_path) {
        Ok(d) => d,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to read email: {}", e),
            )
            .into_response()
        }
    };

    let parsed = match mailparse::parse_mail(&data) {
        Ok(p) => p,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to parse email: {}", e),
            )
            .into_response()
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
    let is_spam = parsed
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("X-Spam-Flag"))
        .map(|h| h.get_value().trim().eq_ignore_ascii_case("YES"))
        .unwrap_or(false);
    let body = extract_body(&parsed);

    Json(json!({
        "filename": filename_b64,
        "subject": subject,
        "from": from,
        "to": to,
        "date": date,
        "body": body,
        "is_spam": is_spam
    }))
    .into_response()
}

// ── POST /api/emails ──────────────────────────────────────────────────────────

pub async fn send_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Json(body): Json<SendEmailBody>,
) -> impl IntoResponse {
    info!(
        "[api] POST /api/emails account_id={} to={}",
        body.account_id, body.to
    );

    let account_id = body.account_id;
    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return json_error(StatusCode::NOT_FOUND, "Account not found").into_response(),
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown");
    let email_addr = format!("{}@{}", acct.username, domain);

    let from_addr = if body.sender_name.is_empty() {
        email_addr.clone()
    } else {
        let safe_name = body.sender_name.replace(['\r', '\n'], " ");
        format!("{} <{}>", safe_name, email_addr)
    };

    use lettre::message::header::ContentType;
    use lettre::message::SinglePart;
    use lettre::{SmtpTransport, Transport};

    let from_mb = match from_addr.parse() {
        Ok(a) => a,
        Err(e) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                &format!("Invalid from address: {}", e),
            )
            .into_response()
        }
    };
    let to_mb = match body.to.parse() {
        Ok(a) => a,
        Err(e) => {
            return json_error(
                StatusCode::BAD_REQUEST,
                &format!("Invalid to address: {}", e),
            )
            .into_response()
        }
    };

    let mut builder = lettre::Message::builder()
        .from(from_mb)
        .to(to_mb)
        .subject(&body.subject);

    for addr in body.cc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(a) = addr.parse() {
            builder = builder.cc(a);
        }
    }
    for addr in body.bcc.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        if let Ok(a) = addr.parse() {
            builder = builder.bcc(a);
        }
    }
    if !body.reply_to.trim().is_empty() {
        if let Ok(a) = body.reply_to.trim().parse() {
            builder = builder.reply_to(a);
        }
    }

    let email = match body.body_format.as_str() {
        "html" => builder.singlepart(
            SinglePart::builder()
                .header(ContentType::TEXT_HTML)
                .body(body.body.clone()),
        ),
        _ => builder.body(body.body.clone()),
    };

    let email = match email {
        Ok(e) => e,
        Err(e) => {
            return json_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                &format!("Failed to build email: {}", e),
            )
            .into_response()
        }
    };

    match SmtpTransport::builder_dangerous("127.0.0.1")
        .port(25)
        .build()
        .send(&email)
    {
        Ok(_) => {
            info!("[api] email sent to {}", body.to);
            (StatusCode::OK, Json(json!({"status": "sent"}))).into_response()
        }
        Err(e) => json_error(
            StatusCode::BAD_GATEWAY,
            &format!("SMTP error: {}", e),
        )
        .into_response(),
    }
}

// ── DELETE /api/emails/:filename ──────────────────────────────────────────────

pub async fn delete_email(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(filename_b64): Path<String>,
    Query(q): Query<EmailQuery>,
) -> impl IntoResponse {
    info!(
        "[api] DELETE /api/emails/{} account_id={}",
        filename_b64, q.account_id
    );

    if !is_safe_folder(&q.folder) {
        return json_error(StatusCode::BAD_REQUEST, "Invalid folder name").into_response();
    }

    let account_id = q.account_id;
    let acct = match state
        .blocking_db(move |db| db.get_account_with_domain(account_id))
        .await
    {
        Some(a) => a,
        None => return json_error(StatusCode::NOT_FOUND, "Account not found").into_response(),
    };

    let filename = match URL_SAFE_NO_PAD
        .decode(filename_b64.as_bytes())
        .ok()
        .and_then(|b| String::from_utf8(b).ok())
    {
        Some(f) => f,
        None => {
            return json_error(StatusCode::BAD_REQUEST, "Invalid filename encoding")
                .into_response()
        }
    };

    let domain = acct.domain_name.as_deref().unwrap_or("unknown").to_string();
    if !is_safe_path_component(&domain)
        || !is_safe_path_component(&acct.username)
        || !is_safe_path_component(&filename)
    {
        return json_error(StatusCode::BAD_REQUEST, "Invalid path component").into_response();
    }

    let maildir_base = maildir_path(&domain, &acct.username);
    let root = folder_root(&maildir_base, &q.folder);

    for subdir in &["new", "cur"] {
        let candidate = format!("{}/{}/{}", root, subdir, filename);
        if std::path::Path::new(&candidate).is_file() {
            if let Err(e) = std::fs::remove_file(&candidate) {
                return json_error(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    &format!("Failed to delete email: {}", e),
                )
                .into_response();
            }
            info!("[api] deleted email: {}", candidate);
            return (StatusCode::OK, Json(json!({"status": "deleted"}))).into_response();
        }
    }

    json_error(StatusCode::NOT_FOUND, "Email not found").into_response()
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_page_is_one() {
        assert_eq!(default_page(), 1);
    }

    #[test]
    fn default_body_format_is_plain() {
        assert_eq!(default_body_format(), "plain");
    }

    #[test]
    fn send_email_body_deserializes_minimal() {
        let json = r#"{"account_id":1,"to":"a@b.com","subject":"Hi","body":"Hello"}"#;
        let b: SendEmailBody = serde_json::from_str(json).unwrap();
        assert_eq!(b.account_id, 1);
        assert_eq!(b.to, "a@b.com");
        assert_eq!(b.body_format, "plain");
        assert!(b.cc.is_empty());
    }

    #[test]
    fn send_email_body_deserializes_html_format() {
        let json = r#"{"account_id":2,"to":"x@y.com","subject":"S","body":"<b>B</b>","body_format":"html"}"#;
        let b: SendEmailBody = serde_json::from_str(json).unwrap();
        assert_eq!(b.body_format, "html");
    }

    #[test]
    fn list_query_defaults_page_to_one() {
        // Default page is 1 when using default_page()
        assert_eq!(default_page(), 1);
        // Default folder is empty
        let q = ListQuery {
            account_id: 5,
            folder: String::new(),
            page: default_page(),
        };
        assert_eq!(q.page, 1);
        assert!(q.folder.is_empty());
    }

    #[test]
    fn page_size_constant_is_twenty() {
        assert_eq!(PAGE_SIZE, 20);
    }
}
