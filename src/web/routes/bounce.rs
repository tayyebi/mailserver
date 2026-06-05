use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use log::{debug, error, info, warn};
use serde::Deserialize;

use crate::db::{Account, BounceInbox};
use crate::web::{auth::AuthAdmin, fire_webhook, AppState};

// ── Constants ──

const MAILDIR_ROOT: &str = "/data/mail";
const REPORTS_PER_PAGE: usize = 10;

// ── Helpers ──

fn is_safe_path_component(s: &str) -> bool {
    !s.is_empty() && !s.contains('/') && !s.contains('\\') && s != "." && s != ".."
}

fn maildir_path(domain: &str, username: &str) -> String {
    format!("{}/{}/{}/Maildir", MAILDIR_ROOT, domain, username)
}

// ── RFC 3464 – An Extensible Message Format for Delivery Status Notifications ──
//
// RFC 3464 (January 2003) defines the machine-readable Delivery Status
// Notification (DSN) format.  A DSN is a `multipart/report` message with
// `report-type=delivery-status` (RFC 3462 §2) containing at least two
// parts:
//
//   Part 1 – text/plain (or text/html): Human-readable explanation of the
//            delivery failure addressed to the original sender.
//   Part 2 – message/delivery-status: Machine-readable key/value fields
//            organised into a per-message section and one or more
//            per-recipient sections (parsed by `parse_dsn_part()` below).
//   Part 3 – message/rfc822 (optional): Headers (or full content) of the
//            original undeliverable message.
//
// Per-message fields (RFC 3464 §2.2):
//   Reporting-MTA       – the MTA that generated the DSN
//   Arrival-Date        – when the original message arrived at the Reporting-MTA
//
// Per-recipient fields (RFC 3464 §2.3):
//   Final-Recipient     – the mailbox that could not be reached
//   Action              – what the MTA did: "failed", "delayed", "delivered",
//                         "relayed", "expanded"
//   Status              – enhanced status code (RFC 3463), e.g. "5.1.1"
//   Diagnostic-Code     – SMTP reply text from the remote MTA
//   Remote-MTA          – the MTA that returned the error
//   Last-Attempt-Date   – when the last delivery attempt was made

/// Machine-readable fields extracted from the `message/delivery-status`
/// MIME part of a DSN message (RFC 3464 §2).
#[derive(Default, Clone)]
pub struct DsnFields {
    /// `Reporting-MTA` — the MTA that generated this DSN (§2.2.1).
    pub reporting_mta: String,
    /// `Arrival-Date` — when the original message arrived at the
    /// Reporting-MTA (§2.2.3).
    pub arrival_date: String,
    /// `Final-Recipient` — the mailbox that could not be reached (§2.3.1).
    pub final_recipient: String,
    /// `Action` — what the MTA did: "failed", "delayed", "delivered",
    /// "relayed", "expanded" (§2.3.3).
    pub action: String,
    /// `Status` — enhanced mail system status code per RFC 3463
    /// (e.g. "5.1.1") (§2.3.4).
    pub status: String,
    /// `Diagnostic-Code` — SMTP reply text from the remote MTA (§2.3.6).
    pub diagnostic_code: String,
    /// `Remote-MTA` — the MTA that returned the error (§2.3.5).
    pub remote_mta: String,
    /// `Last-Attempt-Date` — when the last delivery attempt was made (§2.3.7).
    pub last_attempt_date: String,
}

/// A fully parsed bounce report, combining the outer email metadata with
/// the structured DSN fields and (optionally) the original message headers.
pub struct BounceReport {
    /// Subject line of the DSN email itself.
    pub email_subject: String,
    /// Date header of the DSN email.
    pub email_date: String,
    /// Unix timestamp parsed from `email_date`; used for sorting.
    pub email_timestamp: i64,
    /// Base64url-encoded Maildir filename; used as a stable identifier.
    #[allow(dead_code)]
    pub email_filename: String,
    /// Structured fields from the `message/delivery-status` MIME part.
    pub fields: DsnFields,
    /// Subject of the *original* undeliverable message.
    pub original_subject: String,
    /// From header of the *original* undeliverable message.
    pub original_from: String,
}

// ── DSN parsing ──

/// Parse the body of a `message/delivery-status` MIME part into
/// [`DsnFields`].
///
/// Per RFC 3464 §2.1 the body consists of a per-message section followed
/// by one or more per-recipient sections, separated by blank lines.  Each
/// section contains `Field-Name: value` lines.  We parse the first
/// per-recipient section we encounter (most DSNs report a single
/// recipient).
///
/// The `dns;` / `smtp;` / `rfc822;` type prefixes on structured values
/// (e.g. `Final-Recipient: rfc822; user@example.com`) are stripped so
/// callers receive a clean value.
fn parse_dsn_part(body: &str) -> DsnFields {
    let mut fields = DsnFields::default();
    for line in body.lines() {
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let raw_value = line[colon + 1..].trim();
            // Strip type prefix (e.g. "dns; …", "rfc822; …", "smtp; …")
            let value = if let Some(semi) = raw_value.find(';') {
                raw_value[semi + 1..].trim().to_string()
            } else {
                raw_value.to_string()
            };
            match key.as_str() {
                "reporting-mta" => fields.reporting_mta = value,
                "arrival-date" => fields.arrival_date = value,
                "final-recipient" => {
                    if fields.final_recipient.is_empty() {
                        fields.final_recipient = value;
                    }
                }
                "action" => {
                    if fields.action.is_empty() {
                        fields.action = value;
                    }
                }
                "status" => {
                    if fields.status.is_empty() {
                        fields.status = value;
                    }
                }
                "diagnostic-code" => {
                    if fields.diagnostic_code.is_empty() {
                        fields.diagnostic_code = value;
                    }
                }
                "remote-mta" => {
                    if fields.remote_mta.is_empty() {
                        fields.remote_mta = value;
                    }
                }
                "last-attempt-date" => {
                    if fields.last_attempt_date.is_empty() {
                        fields.last_attempt_date = value;
                    }
                }
                _ => {}
            }
        }
    }
    fields
}

/// Recursively walk the MIME tree looking for the `message/delivery-status`
/// part defined by RFC 3464 §2.1.
///
/// Returns `None` when no valid delivery-status part is found or when the
/// part does not contain the required `Action` field.
fn find_delivery_status_part(part: &mailparse::ParsedMail) -> Option<DsnFields> {
    let ct = part.ctype.mimetype.to_lowercase();
    if ct == "message/delivery-status" {
        if let Ok(body) = part.get_body() {
            let fields = parse_dsn_part(&body);
            // A valid DSN must contain an Action field (RFC 3464 §2.3.3)
            if !fields.action.is_empty() {
                return Some(fields);
            }
        }
    }
    for subpart in &part.subparts {
        if let Some(f) = find_delivery_status_part(subpart) {
            return Some(f);
        }
    }
    None
}

/// Recursively walk the MIME tree looking for the third body part of a DSN —
/// the `message/rfc822` (or `text/rfc822-headers`) part that contains the
/// *original* undeliverable message (RFC 3464 §2.4).
///
/// Returns `(subject, from)` of the first matching nested message found;
/// both strings are empty when no such part exists.
fn find_original_message_headers(part: &mailparse::ParsedMail) -> (String, String) {
    let ct = part.ctype.mimetype.to_lowercase();
    if ct == "message/rfc822" || ct == "text/rfc822-headers" {
        if let Ok(body_raw) = part.get_body_raw() {
            if let Ok(nested) = mailparse::parse_mail(&body_raw) {
                let subject = nested
                    .headers
                    .iter()
                    .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
                    .map(|h| h.get_value())
                    .unwrap_or_default();
                let from = nested
                    .headers
                    .iter()
                    .find(|h| h.get_key().eq_ignore_ascii_case("From"))
                    .map(|h| h.get_value())
                    .unwrap_or_default();
                return (subject, from);
            }
        }
    }
    for subpart in &part.subparts {
        let result = find_original_message_headers(subpart);
        if !result.0.is_empty() || !result.1.is_empty() {
            return result;
        }
    }
    (String::new(), String::new())
}

/// Scan a Maildir inbox and parse every DSN bounce notification found.
///
/// Only emails that contain a valid `message/delivery-status` MIME part
/// (RFC 3464) are included; routine non-DSN emails delivered to the same
/// address are skipped silently.
///
/// The `on_report` callback is invoked once per successfully parsed report,
/// enabling callers to fire webhooks without collecting all reports first.
fn read_bounce_reports<F>(maildir_base: &str, logs: &mut Vec<String>, mut on_report: F) -> Vec<BounceReport>
where
    F: FnMut(&BounceReport),
{
    let mut reports = Vec::new();
    let mut files: Vec<(String, std::path::PathBuf)> = Vec::new();

    for subdir in &["new", "cur"] {
        let dir_path = format!("{}/{}", maildir_base, subdir);
        match std::fs::read_dir(&dir_path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if path.is_file() {
                        let fname = entry.file_name().to_string_lossy().to_string();
                        files.push((fname, path));
                    }
                }
            }
            Err(e) => {
                debug!("[bounce] maildir '{}' not accessible: {}", dir_path, e);
            }
        }
    }

    // Process newest-looking filenames first
    files.sort_by(|a, b| b.0.cmp(&a.0));

    for (fname, path) in files {
        match std::fs::read(&path) {
            Ok(data) => match mailparse::parse_mail(&data) {
                Ok(parsed) => {
                    let subject = parsed
                        .headers
                        .iter()
                        .find(|h| h.get_key().eq_ignore_ascii_case("Subject"))
                        .map(|h| h.get_value())
                        .unwrap_or_default();
                    let date = parsed
                        .headers
                        .iter()
                        .find(|h| h.get_key().eq_ignore_ascii_case("Date"))
                        .map(|h| h.get_value())
                        .unwrap_or_default();
                    let email_timestamp = mailparse::dateparse(&date).unwrap_or(0);
                    let encoded = URL_SAFE_NO_PAD.encode(fname.as_bytes());

                    // Check if this is a DSN delivery-status report
                    let ct = parsed.ctype.mimetype.to_lowercase();
                    let is_report = ct.contains("multipart/report")
                        || parsed
                            .headers
                            .iter()
                            .find(|h| h.get_key().eq_ignore_ascii_case("Content-Type"))
                            .map(|h| h.get_value().to_lowercase().contains("delivery-status"))
                            .unwrap_or(false);

                    if is_report || !parsed.subparts.is_empty() {
                        if let Some(fields) = find_delivery_status_part(&parsed) {
                            let (original_subject, original_from) =
                                find_original_message_headers(&parsed);
                            let report = BounceReport {
                                email_subject: subject,
                                email_date: date,
                                email_timestamp,
                                email_filename: encoded,
                                fields,
                                original_subject,
                                original_from,
                            };
                            on_report(&report);
                            reports.push(report);
                        }
                    }
                }
                Err(e) => {
                    logs.push(format!("Failed to parse email {}: {}", fname, e));
                }
            },
            Err(e) => {
                logs.push(format!("Failed to read file {}: {}", fname, e));
            }
        }
    }

    reports
}

struct PaginatedReports {
    reports: Vec<BounceReport>,
    page: usize,
    total_pages: usize,
    total_count: usize,
}

fn paginate_reports(
    mut reports: Vec<BounceReport>,
    requested_page: usize,
    per_page: usize,
) -> PaginatedReports {
    let per_page = per_page.max(1);
    reports.sort_by(|a, b| {
        b.email_timestamp
            .cmp(&a.email_timestamp)
            .then_with(|| b.email_filename.cmp(&a.email_filename))
    });
    let total_count = reports.len();
    let total_pages = ((total_count + per_page - 1) / per_page).max(1);
    let page = requested_page.max(1).min(total_pages);
    let start = (page - 1) * per_page;
    let reports = reports.into_iter().skip(start).take(per_page).collect();

    PaginatedReports {
        reports,
        page,
        total_pages,
        total_count,
    }
}

// ── Forms ──

#[derive(Deserialize)]
pub struct AddBounceInboxForm {
    pub account_id: i64,
    pub label: String,
}

#[derive(Deserialize)]
pub struct ReportsQuery {
    #[serde(default = "default_page")]
    pub page: usize,
}

fn default_page() -> usize {
    1
}

// ── Templates ──

#[derive(Template)]
#[template(path = "bounce/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<String>,
    inboxes: Vec<BounceInbox>,
    accounts: Vec<Account>,
}

#[derive(Template)]
#[template(path = "bounce/reports.html")]
struct ReportsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    inbox: BounceInbox,
    reports: Vec<BounceReport>,
    logs: Vec<String>,
    page: usize,
    total_pages: usize,
    total_count: usize,
    page_size: usize,
}

// ── Handlers ──

pub async fn list(
    _auth: AuthAdmin,
    State(state): State<AppState>,
) -> Html<String> {
    info!("[web] GET /bounce — list bounce inboxes");
    let inboxes = state.blocking_db(|db| db.list_bounce_inboxes()).await;
    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;
    let tmpl = ListTemplate {
        nav_active: "Bounces",
        flash: None,
        inboxes,
        accounts,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<AddBounceInboxForm>,
) -> Response {
    info!(
        "[web] POST /bounce — creating bounce inbox account_id={}",
        form.account_id
    );
    let account_id = form.account_id;
    let label = form.label.clone();
    let result = state
        .blocking_db(move |db| db.create_bounce_inbox(account_id, &label))
        .await;
    match result {
        Ok(_) => Redirect::to("/bounce").into_response(),
        Err(e) => {
            error!("[web] failed to create bounce inbox: {}", e);
            let inboxes = state.blocking_db(|db| db.list_bounce_inboxes()).await;
            let accounts = state
                .blocking_db(|db| db.list_all_accounts_with_domain())
                .await;
            let tmpl = ListTemplate {
                nav_active: "Bounces",
                flash: Some(format!("Error: {}", e)),
                inboxes,
                accounts,
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /bounce/{}/delete — deleting bounce inbox", id);
    state.blocking_db(move |db| db.delete_bounce_inbox(id)).await;
    Redirect::to("/bounce").into_response()
}

pub async fn reports(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<ReportsQuery>,
) -> Response {
    info!("[web] GET /bounce/{}/reports", id);
    let page = params.page.max(1);

    let inbox = match state.blocking_db(move |db| db.get_bounce_inbox(id)).await {
        Some(i) => i,
        None => {
            warn!("[web] bounce inbox id={} not found", id);
            return Redirect::to("/bounce").into_response();
        }
    };

    let username = inbox.account_username.clone().unwrap_or_default();
    let domain = inbox.account_domain.clone().unwrap_or_default();
    let mut logs: Vec<String> = Vec::new();
    logs.push(format!("Reading mailbox: {}", maildir_path(&domain, &username)));

    let webhook_state = state.clone();
    let inbox_for_webhook = inbox.clone();

    let reports = if is_safe_path_component(&domain) && is_safe_path_component(&username) {
        let maildir_base = maildir_path(&domain, &username);
        read_bounce_reports(&maildir_base, &mut logs, |report| {
            fire_webhook(
                &webhook_state,
                "bounce.report.parsed",
                serde_json::json!({
                    "inbox_id": inbox_for_webhook.id,
                    "label": inbox_for_webhook.label,
                    "account": format!(
                        "{}@{}",
                        inbox_for_webhook
                            .account_username
                            .as_deref()
                            .unwrap_or_default(),
                        inbox_for_webhook
                            .account_domain
                            .as_deref()
                            .unwrap_or_default()
                    ),
                    "action": report.fields.action,
                    "status": report.fields.status,
                    "final_recipient": report.fields.final_recipient,
                    "diagnostic_code": report.fields.diagnostic_code,
                    "remote_mta": report.fields.remote_mta,
                }),
            );
        })
    } else {
        warn!(
            "[web] unsafe path component: domain={}, username={}",
            domain, username
        );
        Vec::new()
    };

    let pagination = paginate_reports(reports, page, REPORTS_PER_PAGE);

    let tmpl = ReportsTemplate {
        nav_active: "Bounces",
        flash: None,
        inbox,
        reports: pagination.reports,
        logs,
        page: pagination.page,
        total_pages: pagination.total_pages,
        total_count: pagination.total_count,
        page_size: REPORTS_PER_PAGE,
    };
    Html(tmpl.render().unwrap()).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn build_report(ts: i64, subject: &str) -> BounceReport {
        BounceReport {
            email_subject: subject.to_string(),
            email_date: "2024-02-20".to_string(),
            email_timestamp: ts,
            email_filename: format!("file-{}", subject),
            fields: DsnFields::default(),
            original_subject: String::new(),
            original_from: String::new(),
        }
    }

    #[test]
    fn paginate_bounce_reports_sorts_and_limits() {
        let reports = vec![
            build_report(2, "second"),
            build_report(3, "third"),
            build_report(1, "first"),
        ];

        let page_one = paginate_reports(reports, 1, 2);
        assert_eq!(page_one.total_count, 3);
        assert_eq!(page_one.total_pages, 2);
        assert_eq!(page_one.page, 1);
        assert_eq!(page_one.reports.len(), 2);
        assert_eq!(page_one.reports[0].email_subject, "third");
        assert_eq!(page_one.reports[1].email_subject, "second");
    }

    #[test]
    fn paginate_bounce_reports_page_two() {
        let reports = vec![
            build_report(2, "second"),
            build_report(3, "third"),
            build_report(1, "first"),
        ];

        let page_two = paginate_reports(reports, 2, 2);
        assert_eq!(page_two.page, 2);
        assert_eq!(page_two.reports.len(), 1);
        assert_eq!(page_two.reports[0].email_subject, "first");
    }

    #[test]
    fn paginate_bounce_reports_empty() {
        let reports: Vec<BounceReport> = Vec::new();
        let result = paginate_reports(reports, 1, 10);
        assert_eq!(result.total_count, 0);
        assert_eq!(result.total_pages, 1);
        assert_eq!(result.page, 1);
        assert!(result.reports.is_empty());
    }

    #[test]
    fn parse_dsn_part_extracts_fields() {
        let dsn_body = "\
Reporting-MTA: dns; mail.example.com\r
Arrival-Date: Mon, 20 Feb 2024 10:00:00 +0000\r
\r
Final-Recipient: rfc822; user@example.org\r
Action: failed\r
Status: 5.1.1\r
Diagnostic-Code: smtp; 550 5.1.1 User unknown\r
Remote-MTA: dns; mx.example.org\r
Last-Attempt-Date: Mon, 20 Feb 2024 10:05:00 +0000\r
";
        let fields = parse_dsn_part(dsn_body);
        assert_eq!(fields.reporting_mta, "mail.example.com");
        assert_eq!(fields.final_recipient, "user@example.org");
        assert_eq!(fields.action, "failed");
        assert_eq!(fields.status, "5.1.1");
        assert!(fields.diagnostic_code.contains("550 5.1.1 User unknown"));
        assert_eq!(fields.remote_mta, "mx.example.org");
    }

    #[test]
    fn parse_dsn_part_handles_empty() {
        let fields = parse_dsn_part("");
        assert!(fields.action.is_empty());
        assert!(fields.status.is_empty());
        assert!(fields.final_recipient.is_empty());
    }
}
