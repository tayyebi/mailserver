use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use log::{debug, error, info, warn};
use serde::Deserialize;
use std::io::Read;

use crate::db::{Account, DmarcInbox};
use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Constants ──

const MAILDIR_ROOT: &str = "/data/mail";

// ── Helpers ──

fn is_safe_path_component(s: &str) -> bool {
    !s.is_empty() && !s.contains('/') && !s.contains('\\') && s != "." && s != ".."
}

fn maildir_path(domain: &str, username: &str) -> String {
    format!("{}/{}/{}/Maildir", MAILDIR_ROOT, domain, username)
}

// ── DMARC data structures ──

#[derive(Default)]
pub struct DmarcReportMeta {
    pub org_name: String,
    pub email: String,
    pub report_id: String,
    pub date_begin: String,
    pub date_end: String,
}

#[derive(Default)]
pub struct DmarcPolicy {
    pub domain: String,
    pub adkim: String,
    pub aspf: String,
    pub p: String,
    pub sp: String,
    pub pct: String,
}

#[derive(Default, Clone)]
pub struct DmarcRecord {
    pub source_ip: String,
    pub count: String,
    pub disposition: String,
    pub dkim_result: String,
    pub spf_result: String,
    pub header_from: String,
    pub auth_dkim_domain: String,
    pub auth_dkim_result: String,
    pub auth_spf_domain: String,
    pub auth_spf_result: String,
}

pub struct DmarcReport {
    pub email_subject: String,
    pub email_date: String,
    pub email_filename: String,
    pub meta: DmarcReportMeta,
    pub policy: DmarcPolicy,
    pub records: Vec<DmarcRecord>,
}

// ── DMARC XML parsing ──

/// Extract and decompress a DMARC XML report from email attachment bytes.
/// Returns the raw XML bytes on success.
fn decompress_dmarc_attachment(name: &str, data: &[u8]) -> Option<Vec<u8>> {
    let lower = name.to_lowercase();
    if lower.ends_with(".zip") {
        // ZIP archive
        let cursor = std::io::Cursor::new(data);
        let mut archive = zip::ZipArchive::new(cursor).ok()?;
        for i in 0..archive.len() {
            let mut file = archive.by_index(i).ok()?;
            let fname = file.name().to_lowercase();
            if fname.ends_with(".xml") {
                let mut buf = Vec::new();
                file.read_to_end(&mut buf).ok()?;
                return Some(buf);
            }
        }
        None
    } else if lower.ends_with(".gz") || lower.ends_with(".xml.gz") {
        // Gzip compressed
        let mut decoder = flate2::read::GzDecoder::new(data);
        let mut buf = Vec::new();
        decoder.read_to_end(&mut buf).ok()?;
        Some(buf)
    } else if lower.ends_with(".xml") {
        Some(data.to_vec())
    } else {
        None
    }
}

/// Parse DMARC aggregate report XML into structured data.
fn parse_dmarc_xml(xml: &[u8]) -> Option<(DmarcReportMeta, DmarcPolicy, Vec<DmarcRecord>)> {
    use quick_xml::events::Event;
    use quick_xml::Reader;

    let mut reader = Reader::from_reader(xml);
    reader.config_mut().trim_text(true);

    let mut meta = DmarcReportMeta::default();
    let mut policy = DmarcPolicy::default();
    let mut records: Vec<DmarcRecord> = Vec::new();
    let mut current_record: Option<DmarcRecord> = None;

    // Track element path as a stack of tag names
    let mut path: Vec<String> = Vec::new();
    let mut buf = Vec::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Start(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                path.push(tag.clone());
                if tag == "record" {
                    current_record = Some(DmarcRecord::default());
                }
            }
            Ok(Event::End(e)) => {
                let tag = String::from_utf8_lossy(e.name().as_ref()).to_string();
                if tag == "record" {
                    if let Some(rec) = current_record.take() {
                        records.push(rec);
                    }
                }
                path.pop();
            }
            Ok(Event::Text(e)) => {
                let text = String::from_utf8_lossy(e.as_ref()).into_owned();
                let path_str = path.join("/");
                // report_metadata fields
                if path_str == "feedback/report_metadata/org_name" {
                    meta.org_name = text;
                } else if path_str == "feedback/report_metadata/email" {
                    meta.email = text;
                } else if path_str == "feedback/report_metadata/report_id" {
                    meta.report_id = text;
                } else if path_str == "feedback/report_metadata/date_range/begin" {
                    // Convert Unix timestamp to human-readable date
                    meta.date_begin = unix_ts_to_date(&text);
                } else if path_str == "feedback/report_metadata/date_range/end" {
                    meta.date_end = unix_ts_to_date(&text);
                }
                // policy_published fields
                else if path_str == "feedback/policy_published/domain" {
                    policy.domain = text;
                } else if path_str == "feedback/policy_published/adkim" {
                    policy.adkim = text;
                } else if path_str == "feedback/policy_published/aspf" {
                    policy.aspf = text;
                } else if path_str == "feedback/policy_published/p" {
                    policy.p = text;
                } else if path_str == "feedback/policy_published/sp" {
                    policy.sp = text;
                } else if path_str == "feedback/policy_published/pct" {
                    policy.pct = text;
                }
                // record fields
                else if path_str == "feedback/record/row/source_ip" {
                    if let Some(ref mut rec) = current_record {
                        rec.source_ip = text;
                    }
                } else if path_str == "feedback/record/row/count" {
                    if let Some(ref mut rec) = current_record {
                        rec.count = text;
                    }
                } else if path_str == "feedback/record/row/policy_evaluated/disposition" {
                    if let Some(ref mut rec) = current_record {
                        rec.disposition = text;
                    }
                } else if path_str == "feedback/record/row/policy_evaluated/dkim" {
                    if let Some(ref mut rec) = current_record {
                        rec.dkim_result = text;
                    }
                } else if path_str == "feedback/record/row/policy_evaluated/spf" {
                    if let Some(ref mut rec) = current_record {
                        rec.spf_result = text;
                    }
                } else if path_str == "feedback/record/identifiers/header_from" {
                    if let Some(ref mut rec) = current_record {
                        rec.header_from = text;
                    }
                } else if path_str == "feedback/record/auth_results/dkim/domain" {
                    if let Some(ref mut rec) = current_record {
                        rec.auth_dkim_domain = text;
                    }
                } else if path_str == "feedback/record/auth_results/dkim/result" {
                    if let Some(ref mut rec) = current_record {
                        rec.auth_dkim_result = text;
                    }
                } else if path_str == "feedback/record/auth_results/spf/domain" {
                    if let Some(ref mut rec) = current_record {
                        rec.auth_spf_domain = text;
                    }
                } else if path_str == "feedback/record/auth_results/spf/result" {
                    if let Some(ref mut rec) = current_record {
                        rec.auth_spf_result = text;
                    }
                }
            }
            Ok(Event::Eof) => break,
            Err(e) => {
                warn!("[dmarc] XML parse error: {}", e);
                break;
            }
            _ => {}
        }
        buf.clear();
    }

    Some((meta, policy, records))
}

fn unix_ts_to_date(ts_str: &str) -> String {
    if let Ok(ts) = ts_str.parse::<i64>() {
        use chrono::TimeZone;
        match chrono::Utc.timestamp_opt(ts, 0) {
            chrono::LocalResult::Single(dt) => return dt.format("%Y-%m-%d").to_string(),
            _ => {}
        }
    }
    ts_str.to_string()
}

/// Find a DMARC report attachment in a parsed email part tree.
fn find_dmarc_attachment(part: &mailparse::ParsedMail) -> Option<(String, Vec<u8>)> {
    // Check if this part is an attachment with a relevant content-type or name
    let ct = part.ctype.mimetype.to_lowercase();
    let is_zip = ct.contains("zip")
        || ct.contains("application/octet-stream")
        || ct.contains("application/gzip")
        || ct.contains("application/x-gzip");
    let is_xml = ct.contains("xml") || ct.contains("text/plain");

    // Try to get the attachment filename
    let name = part
        .headers
        .iter()
        .find(|h| h.get_key().eq_ignore_ascii_case("Content-Disposition"))
        .and_then(|h| {
            let v = h.get_value();
            // Look for filename= in the header
            v.split(';')
                .find(|s| s.trim().to_lowercase().starts_with("filename"))
                .and_then(|s| {
                    s.find('=')
                        .map(|i| s[i + 1..].trim().trim_matches('"').to_string())
                })
        })
        .or_else(|| {
            // Try Content-Type name parameter
            part.headers
                .iter()
                .find(|h| h.get_key().eq_ignore_ascii_case("Content-Type"))
                .and_then(|h| {
                    let v = h.get_value();
                    v.split(';')
                        .find(|s| s.trim().to_lowercase().starts_with("name"))
                        .and_then(|s| {
                            s.find('=')
                                .map(|i| s[i + 1..].trim().trim_matches('"').to_string())
                        })
                })
        });

    if let Some(ref n) = name {
        let nl = n.to_lowercase();
        if nl.ends_with(".xml") || nl.ends_with(".zip") || nl.ends_with(".gz") {
            if let Ok(data) = part.get_body_raw() {
                return Some((n.clone(), data));
            }
        }
    }

    // Also handle when content-type directly matches
    if (is_zip || is_xml) && part.subparts.is_empty() {
        if let Some(n) = name {
            if let Ok(data) = part.get_body_raw() {
                return Some((n, data));
            }
        }
    }

    // Recurse into subparts
    for subpart in &part.subparts {
        if let Some(result) = find_dmarc_attachment(subpart) {
            return Some(result);
        }
    }

    None
}

/// Read all emails in an account's INBOX and try to parse DMARC reports from them.
fn read_dmarc_reports(
    maildir_base: &str,
    logs: &mut Vec<String>,
) -> Vec<DmarcReport> {
    let mut reports = Vec::new();

    for subdir in &["new", "cur"] {
        let dir_path = format!("{}/{}", maildir_base, subdir);
        match std::fs::read_dir(&dir_path) {
            Ok(entries) => {
                for entry in entries.flatten() {
                    let path = entry.path();
                    if !path.is_file() {
                        continue;
                    }
                    let fname = entry.file_name().to_string_lossy().to_string();
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
                                let encoded = URL_SAFE_NO_PAD.encode(fname.as_bytes());

                                // Try to find a DMARC attachment
                                if let Some((att_name, att_data)) =
                                    find_dmarc_attachment(&parsed)
                                {
                                    debug!(
                                        "[dmarc] found attachment '{}' in email '{}'",
                                        att_name, subject
                                    );
                                    if let Some(xml_data) =
                                        decompress_dmarc_attachment(&att_name, &att_data)
                                    {
                                        match parse_dmarc_xml(&xml_data) {
                                            Some((meta, policy, records)) => {
                                                reports.push(DmarcReport {
                                                    email_subject: subject,
                                                    email_date: date,
                                                    email_filename: encoded,
                                                    meta,
                                                    policy,
                                                    records,
                                                });
                                            }
                                            None => {
                                                logs.push(format!(
                                                    "Could not parse DMARC XML in: {}",
                                                    fname
                                                ));
                                            }
                                        }
                                    } else {
                                        logs.push(format!(
                                            "Could not decompress attachment '{}' in: {}",
                                            att_name, fname
                                        ));
                                    }
                                }
                                // else: not a DMARC email, skip silently
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
            }
            Err(e) => {
                debug!("[dmarc] maildir '{}' not accessible: {}", dir_path, e);
            }
        }
    }

    reports
}

// ── Forms ──

#[derive(Deserialize)]
pub struct AddDmarcInboxForm {
    pub account_id: i64,
    pub label: String,
}

#[derive(Deserialize)]
pub struct InboxQuery {
    pub inbox_id: Option<i64>,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "dmarc/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<String>,
    inboxes: Vec<DmarcInbox>,
    accounts: Vec<Account>,
}

#[derive(Template)]
#[template(path = "dmarc/reports.html")]
struct ReportsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    inbox: DmarcInbox,
    reports: Vec<DmarcReport>,
    logs: Vec<String>,
}

// ── Handlers ──

pub async fn list(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(_q): Query<InboxQuery>,
) -> Html<String> {
    info!("[web] GET /dmarc — list DMARC inboxes");
    let inboxes = state.blocking_db(|db| db.list_dmarc_inboxes()).await;
    let accounts = state
        .blocking_db(|db| db.list_all_accounts_with_domain())
        .await;
    let tmpl = ListTemplate {
        nav_active: "DMARC",
        flash: None,
        inboxes,
        accounts,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<AddDmarcInboxForm>,
) -> Response {
    info!(
        "[web] POST /dmarc — creating DMARC inbox account_id={}",
        form.account_id
    );
    let account_id = form.account_id;
    let label = form.label.clone();
    let result = state
        .blocking_db(move |db| db.create_dmarc_inbox(account_id, &label))
        .await;
    match result {
        Ok(_) => Redirect::to("/dmarc").into_response(),
        Err(e) => {
            error!("[web] failed to create DMARC inbox: {}", e);
            let inboxes = state.blocking_db(|db| db.list_dmarc_inboxes()).await;
            let accounts = state
                .blocking_db(|db| db.list_all_accounts_with_domain())
                .await;
            let tmpl = ListTemplate {
                nav_active: "DMARC",
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
    warn!("[web] POST /dmarc/{}/delete — deleting DMARC inbox", id);
    state.blocking_db(move |db| db.delete_dmarc_inbox(id)).await;
    Redirect::to("/dmarc").into_response()
}

pub async fn reports(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!("[web] GET /dmarc/{}/reports", id);

    let inbox = match state.blocking_db(move |db| db.get_dmarc_inbox(id)).await {
        Some(i) => i,
        None => {
            warn!("[web] DMARC inbox id={} not found", id);
            return Redirect::to("/dmarc").into_response();
        }
    };

    let username = inbox.account_username.clone().unwrap_or_default();
    let domain = inbox.account_domain.clone().unwrap_or_default();
    let mut logs: Vec<String> = Vec::new();

    let reports = if is_safe_path_component(&domain) && is_safe_path_component(&username) {
        let maildir_base = maildir_path(&domain, &username);
        logs.push(format!("Reading mailbox: {}", maildir_base));
        read_dmarc_reports(&maildir_base, &mut logs)
    } else {
        warn!(
            "[web] unsafe path component: domain={}, username={}",
            domain, username
        );
        Vec::new()
    };

    let tmpl = ReportsTemplate {
        nav_active: "DMARC",
        flash: None,
        inbox,
        reports,
        logs,
    };
    Html(tmpl.render().unwrap()).into_response()
}
