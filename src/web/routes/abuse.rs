use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use log::{debug, error, info, warn};
use serde::Deserialize;

use crate::db::{Account, AbuseInbox};
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

// ── ARF (RFC 5965) data structures ──

/// Parsed fields from a `message/feedback-report` MIME part (RFC 5965).
#[derive(Default, Clone)]
pub struct AbuseReportFields {
    /// Feedback-Type: abuse | spam | fraud | virus | not-spam | other (required)
    pub feedback_type: String,
    /// Version: 1 (required per RFC 5965)
    pub version: String,
    /// User-Agent: the reporting MUA / FBL agent
    pub user_agent: String,
    /// Reported-Domain: the domain being reported
    pub reported_domain: String,
    /// Source-IP: source IP of the reported message
    pub source_ip: String,
    /// Arrival-Date: when the message arrived at the reporter
    pub arrival_date: String,
    /// Original-Rcpt-To: the original recipient address
    pub original_rcpt_to: String,
    /// Original-Mail-From: the original envelope sender
    pub original_mail_from: String,
}

/// An abuse report extracted from a single email.
pub struct AbuseReport {
    pub email_subject: String,
    pub email_date: String,
    pub email_timestamp: i64,
    #[allow(dead_code)]
    pub email_filename: String,
    pub fields: AbuseReportFields,
    /// Subject of the original reported message (from the embedded message/rfc822 part)
    pub original_subject: String,
    /// From of the original reported message
    pub original_from: String,
}

// ── ARF parsing ──

/// Parse the `message/feedback-report` MIME part content as RFC 5965 fields.
/// The part body contains header-like `Key: Value` lines.
fn parse_feedback_report_part(body: &str) -> AbuseReportFields {
    let mut fields = AbuseReportFields::default();
    for line in body.lines() {
        if let Some(colon) = line.find(':') {
            let key = line[..colon].trim().to_lowercase();
            let value = line[colon + 1..].trim().to_string();
            match key.as_str() {
                "feedback-type" => fields.feedback_type = value,
                "version" => fields.version = value,
                "user-agent" => fields.user_agent = value,
                "reported-domain" => fields.reported_domain = value,
                "source-ip" => fields.source_ip = value,
                "arrival-date" => fields.arrival_date = value,
                "original-rcpt-to" => fields.original_rcpt_to = value,
                "original-mail-from" => fields.original_mail_from = value,
                _ => {}
            }
        }
    }
    fields
}

/// Recursively search a parsed email tree for a `message/feedback-report` part.
fn find_feedback_report_part(part: &mailparse::ParsedMail) -> Option<AbuseReportFields> {
    let ct = part.ctype.mimetype.to_lowercase();
    if ct == "message/feedback-report" {
        if let Ok(body) = part.get_body() {
            let fields = parse_feedback_report_part(&body);
            // Only consider it valid if Feedback-Type is present (per RFC 5965)
            if !fields.feedback_type.is_empty() {
                return Some(fields);
            }
        }
    }
    for subpart in &part.subparts {
        if let Some(f) = find_feedback_report_part(subpart) {
            return Some(f);
        }
    }
    None
}

/// Recursively search for the original reported message (message/rfc822 or text/rfc822-headers)
/// and extract Subject and From headers.
fn find_original_message_headers(part: &mailparse::ParsedMail) -> (String, String) {
    let ct = part.ctype.mimetype.to_lowercase();
    if ct == "message/rfc822" || ct == "text/rfc822-headers" {
        // Try to parse as a nested email to get headers
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

/// Read all emails in an account's INBOX and try to parse ARF abuse reports from them.
fn read_abuse_reports<F>(maildir_base: &str, logs: &mut Vec<String>, mut on_report: F) -> Vec<AbuseReport>
where
    F: FnMut(&AbuseReport),
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
                debug!("[abuse] maildir '{}' not accessible: {}", dir_path, e);
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

                    // Check if this is an ARF feedback report email
                    let ct = parsed.ctype.mimetype.to_lowercase();
                    let is_report = ct.contains("multipart/report")
                        || parsed
                            .headers
                            .iter()
                            .find(|h| h.get_key().eq_ignore_ascii_case("Content-Type"))
                            .map(|h| h.get_value().to_lowercase().contains("feedback-report"))
                            .unwrap_or(false);

                    if is_report || !parsed.subparts.is_empty() {
                        if let Some(fields) = find_feedback_report_part(&parsed) {
                            let (original_subject, original_from) =
                                find_original_message_headers(&parsed);
                            let report = AbuseReport {
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
                    // else: not an ARF email, skip silently
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
    reports: Vec<AbuseReport>,
    page: usize,
    total_pages: usize,
    total_count: usize,
}

fn paginate_reports(
    mut reports: Vec<AbuseReport>,
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
pub struct AddAbuseInboxForm {
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
#[template(path = "abuse/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<String>,
    inboxes: Vec<AbuseInbox>,
    accounts: Vec<Account>,
}

#[derive(Template)]
#[template(path = "abuse/reports.html")]
struct ReportsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    inbox: AbuseInbox,
    reports: Vec<AbuseReport>,
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
    info!("[web] GET /abuse — list abuse inboxes");
    let inboxes = state.blocking_db(|db| db.list_abuse_inboxes()).await;
    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;
    let tmpl = ListTemplate {
        nav_active: "Abuse",
        flash: None,
        inboxes,
        accounts,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<AddAbuseInboxForm>,
) -> Response {
    info!(
        "[web] POST /abuse — creating abuse inbox account_id={}",
        form.account_id
    );
    let account_id = form.account_id;
    let label = form.label.clone();
    let result = state
        .blocking_db(move |db| db.create_abuse_inbox(account_id, &label))
        .await;
    match result {
        Ok(_) => Redirect::to("/abuse").into_response(),
        Err(e) => {
            error!("[web] failed to create abuse inbox: {}", e);
            let inboxes = state.blocking_db(|db| db.list_abuse_inboxes()).await;
            let accounts = state
                .blocking_db(|db| db.list_all_accounts_with_domain())
                .await;
            let tmpl = ListTemplate {
                nav_active: "Abuse",
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
    warn!("[web] POST /abuse/{}/delete — deleting abuse inbox", id);
    state.blocking_db(move |db| db.delete_abuse_inbox(id)).await;
    Redirect::to("/abuse").into_response()
}

pub async fn reports(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(params): Query<ReportsQuery>,
) -> Response {
    info!("[web] GET /abuse/{}/reports", id);
    let page = params.page.max(1);

    let inbox = match state.blocking_db(move |db| db.get_abuse_inbox(id)).await {
        Some(i) => i,
        None => {
            warn!("[web] abuse inbox id={} not found", id);
            return Redirect::to("/abuse").into_response();
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
        read_abuse_reports(&maildir_base, &mut logs, |report| {
            fire_webhook(
                &webhook_state,
                "abuse.report.parsed",
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
                    "feedback_type": report.fields.feedback_type,
                    "reported_domain": report.fields.reported_domain,
                    "source_ip": report.fields.source_ip,
                    "original_rcpt_to": report.fields.original_rcpt_to,
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
        nav_active: "Abuse",
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

    fn build_report(ts: i64, subject: &str) -> AbuseReport {
        AbuseReport {
            email_subject: subject.to_string(),
            email_date: "2024-02-20".to_string(),
            email_timestamp: ts,
            email_filename: format!("file-{}", subject),
            fields: AbuseReportFields::default(),
            original_subject: String::new(),
            original_from: String::new(),
        }
    }

    #[test]
    fn paginate_abuse_reports_sorts_and_limits() {
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

        let page_two = paginate_reports(
            vec![
                build_report(2, "second"),
                build_report(3, "third"),
                build_report(1, "first"),
            ],
            2,
            2,
        );
        assert_eq!(page_two.page, 2);
        assert_eq!(page_two.reports.len(), 1);
        assert_eq!(page_two.reports[0].email_subject, "first");
    }

    #[test]
    fn parse_feedback_report_part_extracts_fields() {
        let body = "Feedback-Type: abuse\r\nVersion: 1\r\nUser-Agent: TestFBL/1.0\r\nSource-IP: 203.0.113.42\r\nReported-Domain: spammer.example\r\nOriginal-Rcpt-To: victim@example.com\r\nOriginal-Mail-From: sender@spammer.example\r\nArrival-Date: Mon, 12 Feb 2024 10:00:00 +0000\r\n";
        let fields = parse_feedback_report_part(body);
        assert_eq!(fields.feedback_type, "abuse");
        assert_eq!(fields.version, "1");
        assert_eq!(fields.user_agent, "TestFBL/1.0");
        assert_eq!(fields.source_ip, "203.0.113.42");
        assert_eq!(fields.reported_domain, "spammer.example");
        assert_eq!(fields.original_rcpt_to, "victim@example.com");
        assert_eq!(fields.original_mail_from, "sender@spammer.example");
    }

    #[test]
    fn read_abuse_reports_triggers_callback() {
        use std::fs;
        use std::time::{SystemTime, UNIX_EPOCH};

        let base = std::env::temp_dir().join(format!(
            "abuse-test-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let new_dir = base.join("new");
        fs::create_dir_all(&new_dir).unwrap();
        fs::create_dir_all(base.join("cur")).unwrap();

        // Minimal ARF feedback report email (RFC 5965)
        let email = concat!(
            "Subject: Abuse Report\r\n",
            "Date: Mon, 12 Feb 2024 10:00:00 +0000\r\n",
            "Content-Type: multipart/report; report-type=feedback-report; boundary=\"ABOUND\"\r\n",
            "\r\n",
            "--ABOUND\r\n",
            "Content-Type: text/plain\r\n",
            "\r\n",
            "This is a spam report.\r\n",
            "--ABOUND\r\n",
            "Content-Type: message/feedback-report\r\n",
            "\r\n",
            "Feedback-Type: abuse\r\n",
            "Version: 1\r\n",
            "User-Agent: TestFBL/1.0\r\n",
            "Source-IP: 203.0.113.42\r\n",
            "Reported-Domain: spammer.example\r\n",
            "Original-Rcpt-To: victim@example.com\r\n",
            "--ABOUND--\r\n",
        );

        let file_path = new_dir.join("1707732000.M12345P123.host");
        fs::write(&file_path, email).unwrap();

        let mut logs = Vec::new();
        let mut seen = Vec::new();
        let reports = read_abuse_reports(base.to_str().unwrap(), &mut logs, |report| {
            seen.push(report.fields.feedback_type.clone());
        });

        assert_eq!(reports.len(), 1);
        assert_eq!(seen, vec!["abuse".to_string()]);
        assert_eq!(reports[0].fields.source_ip, "203.0.113.42");
        assert_eq!(reports[0].fields.reported_domain, "spammer.example");
        assert_eq!(reports[0].fields.original_rcpt_to, "victim@example.com");

        fs::remove_dir_all(&base).unwrap();
    }
}
