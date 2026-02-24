use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
};
use log::{debug, error, warn};
use std::path::Path as FsPath;
use std::process::Command;

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

const POSTQUEUE_PATHS: [&str; 2] = ["/usr/sbin/postqueue", "/usr/bin/postqueue"];
const POSTSUPER_PATHS: [&str; 2] = ["/usr/sbin/postsuper", "/usr/bin/postsuper"];

fn find_postqueue_bin() -> Option<&'static str> {
    POSTQUEUE_PATHS
        .into_iter()
        .find(|path| FsPath::new(path).exists())
}

fn find_postsuper_bin() -> Option<&'static str> {
    POSTSUPER_PATHS
        .into_iter()
        .find(|path| FsPath::new(path).exists())
}

/// A single entry parsed from the `postqueue -p` output.
pub struct QueueEntry {
    pub id: String,
    pub size: u64,
    pub arrival_time: String,
    pub sender: String,
    pub recipients: Vec<String>,
}

/// Parse the text output of `postqueue -p` into a list of [`QueueEntry`] values.
///
/// The expected format per entry is:
/// ```text
/// <QueueID>[*!]  <Size>  <DayOfWeek> <Month> <Day> <HH:MM:SS>  <Sender>
///                                          <Recipient1>
///                                          <Recipient2>
/// ```
/// Entries are separated by blank lines. The header line starts with `-` and
/// the summary line starts with `--`.
pub fn parse_queue_output(output: &str) -> Vec<QueueEntry> {
    use regex::Regex;
    // Queue ID: alphanumeric, optionally followed by * or !
    // Arrival time: "DayAbbr MonAbbr D HH:MM:SS" (day may be 1 or 2 digits)
    let entry_re = Regex::new(
        r"^([A-F0-9a-f]+)[*!]?\s+(\d+)\s+(\w{3}\s+\w{3}\s+\d{1,2}\s+\d{2}:\d{2}:\d{2})\s+(\S+)$",
    )
    .expect("queue entry regex is valid");

    let mut entries: Vec<QueueEntry> = Vec::new();
    let mut current: Option<QueueEntry> = None;

    for line in output.lines() {
        // Header line or summary line
        if line.starts_with('-') {
            continue;
        }

        if line.trim().is_empty() {
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            continue;
        }

        if line.starts_with(|c: char| c.is_ascii_whitespace()) {
            // Recipient continuation line
            if let Some(ref mut entry) = current {
                entry.recipients.push(line.trim().to_string());
            }
        } else if let Some(caps) = entry_re.captures(line) {
            // Start of a new queue entry
            if let Some(entry) = current.take() {
                entries.push(entry);
            }
            current = Some(QueueEntry {
                id: caps[1].to_string(),
                size: caps[2].parse().unwrap_or_else(|_| {
                    warn!(
                        "[queue] failed to parse size for queue entry '{}'; defaulting to 0",
                        &caps[1]
                    );
                    0
                }),
                arrival_time: caps[3].to_string(),
                sender: caps[4].to_string(),
                recipients: Vec::new(),
            });
        }
    }

    if let Some(entry) = current.take() {
        entries.push(entry);
    }

    entries
}

/// Returns `true` only when the queue ID is a valid Postfix hex queue ID
/// (alphanumeric, max 20 chars) to prevent command injection.
fn is_valid_queue_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 20 && id.chars().all(|c| c.is_ascii_alphanumeric())
}

fn same_origin(headers: &HeaderMap) -> bool {
    let host = match headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        Some(v) => v,
        None => return false,
    };
    let matches_host = |value: &str| {
        let rest = match value.split_once("://") {
            Some((_, rest)) => rest,
            None => return false,
        };
        let authority = rest.split('/').next().unwrap_or(rest);
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        authority.eq_ignore_ascii_case(host)
    };

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        return matches_host(origin);
    }
    if let Some(referer) = headers.get(header::REFERER).and_then(|v| v.to_str().ok()) {
        return matches_host(referer);
    }
    false
}

#[derive(Template)]
#[template(path = "queue/list.html")]
struct QueueTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    entries: Vec<QueueEntry>,
    queue_summary: String,
    error: Option<String>,
}

pub async fn list(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    debug!(
        "[web] GET /queue — queue page for username={}",
        auth.admin.username
    );

    let postqueue_bin = find_postqueue_bin();

    let (entries, queue_summary, error) = match postqueue_bin {
        Some(postqueue_bin) => match Command::new(postqueue_bin).arg("-p").output() {
            Ok(output) if output.status.success() => {
                let raw = String::from_utf8_lossy(&output.stdout).to_string();
                let summary = raw
                    .lines()
                    .find(|l| l.starts_with("--"))
                    .unwrap_or("")
                    .trim_start_matches("--")
                    .trim()
                    .to_string();
                (parse_queue_output(&raw), summary, None)
            }
            Ok(output) => {
                error!(
                    "[web] postqueue failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
                (
                    Vec::new(),
                    String::new(),
                    Some("Failed to read queue output from postqueue.".to_string()),
                )
            }
            Err(e) => {
                error!("[web] failed to run postqueue: {}", e);
                (
                    Vec::new(),
                    String::new(),
                    Some("Failed to run postqueue command.".to_string()),
                )
            }
        },
        None => (
            Vec::new(),
            String::new(),
            Some("postqueue binary not found in /usr/sbin or /usr/bin.".to_string()),
        ),
    };

    let tmpl = QueueTemplate {
        nav_active: "Queue",
        flash: None,
        entries,
        queue_summary,
        error,
    };

    match tmpl.render() {
        Ok(html) => Html(html),
        Err(e) => {
            error!("[web] failed to render queue template: {}", e);
            crate::web::errors::render_error_page(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Template Error",
                "Failed to render queue page. Please try again.",
                "/",
                "Dashboard",
            )
        }
    }
}

pub async fn flush(auth: AuthAdmin, headers: HeaderMap) -> Response {
    debug!(
        "[web] POST /queue/flush — flush queue for username={}",
        auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] queue flush blocked due to non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    match find_postqueue_bin() {
        Some(postqueue_bin) => match Command::new(postqueue_bin).arg("-f").output() {
            Ok(output) if output.status.success() => {
                debug!("[web] queue flush command completed successfully");
            }
            Ok(output) => {
                error!(
                    "[web] queue flush failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                error!("[web] failed to run queue flush command: {}", e);
            }
        },
        None => error!("[web] postqueue binary not found; queue flush unavailable"),
    }

    Redirect::to("/queue").into_response()
}

pub async fn purge(auth: AuthAdmin, headers: HeaderMap) -> Response {
    debug!(
        "[web] POST /queue/purge — purge entire queue for username={}",
        auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] queue purge blocked due to non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    match find_postsuper_bin() {
        Some(postsuper_bin) => match Command::new(postsuper_bin).args(["-d", "ALL"]).output() {
            Ok(output) if output.status.success() => {
                debug!("[web] queue purge command completed successfully");
            }
            Ok(output) => {
                error!(
                    "[web] queue purge failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                error!("[web] failed to run queue purge command: {}", e);
            }
        },
        None => error!("[web] postsuper binary not found; queue purge unavailable"),
    }

    Redirect::to("/queue").into_response()
}

pub async fn delete_message(
    auth: AuthAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    debug!(
        "[web] POST /queue/{}/delete — delete message for username={}",
        id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] queue delete blocked due to non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    if !is_valid_queue_id(&id) {
        warn!("[web] queue delete rejected invalid queue id: {:?}", id);
        return StatusCode::BAD_REQUEST.into_response();
    }

    match find_postsuper_bin() {
        Some(postsuper_bin) => match Command::new(postsuper_bin).args(["-d", &id]).output() {
            Ok(output) if output.status.success() => {
                debug!("[web] deleted queue message {}", id);
            }
            Ok(output) => {
                error!(
                    "[web] queue delete {} failed with status {}: {}",
                    id,
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                error!("[web] failed to run postsuper for message {}: {}", id, e);
            }
        },
        None => error!("[web] postsuper binary not found; message delete unavailable"),
    }

    Redirect::to("/queue").into_response()
}

pub async fn flush_message(
    auth: AuthAdmin,
    headers: HeaderMap,
    Path(id): Path<String>,
) -> Response {
    debug!(
        "[web] POST /queue/{}/flush — flush message for username={}",
        id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] queue flush-message blocked due to non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    if !is_valid_queue_id(&id) {
        warn!(
            "[web] queue flush-message rejected invalid queue id: {:?}",
            id
        );
        return StatusCode::BAD_REQUEST.into_response();
    }

    match find_postqueue_bin() {
        Some(postqueue_bin) => match Command::new(postqueue_bin).args(["-i", &id]).output() {
            Ok(output) if output.status.success() => {
                debug!("[web] flushed queue message {}", id);
            }
            Ok(output) => {
                error!(
                    "[web] queue flush-message {} failed with status {}: {}",
                    id,
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
            }
            Err(e) => {
                error!("[web] failed to run postqueue -i for message {}: {}", id, e);
            }
        },
        None => error!("[web] postqueue binary not found; message flush unavailable"),
    }

    Redirect::to("/queue").into_response()
}

#[cfg(test)]
mod tests {
    use super::{is_valid_queue_id, parse_queue_output, same_origin};
    use axum::http::{header, HeaderMap, HeaderValue};

    #[test]
    fn same_origin_allows_matching_origin_and_host() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("mail.example.com"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://mail.example.com"),
        );

        assert!(same_origin(&headers));
    }

    #[test]
    fn same_origin_rejects_mismatched_origin() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("mail.example.com"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );

        assert!(!same_origin(&headers));
    }

    #[test]
    fn same_origin_allows_matching_referer_when_origin_missing() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("mail.example.com"));
        headers.insert(
            header::REFERER,
            HeaderValue::from_static("https://mail.example.com/queue"),
        );

        assert!(same_origin(&headers));
    }

    #[test]
    fn is_valid_queue_id_accepts_hex_alphanumeric() {
        assert!(is_valid_queue_id("EF7F57AAAD"));
        assert!(is_valid_queue_id("74C8A7AC47"));
        assert!(is_valid_queue_id("8389B9CA3B"));
    }

    #[test]
    fn is_valid_queue_id_rejects_invalid() {
        assert!(!is_valid_queue_id(""));
        assert!(!is_valid_queue_id("../etc/passwd"));
        assert!(!is_valid_queue_id("ID WITH SPACE"));
        assert!(!is_valid_queue_id("toolongidthatexceedstwentycharactersXX"));
    }

    const SAMPLE_QUEUE: &str = "\
-Queue ID-  --Size-- ----Arrival Time---- -Sender/Recipient-------
EF7F57AAAD*    1172 Sat Feb 21 17:03:33  m@tyyi.net
                                         tayyebimohammadreza@gmail.com

74C8A7AC47*    1143 Sat Feb 21 17:06:26  m@tyyi.net
                                         mohammadreza.tayyebi@abrnoc.com

8389B9CA3B*  121656 Sun Feb 22 19:18:15  m@tyyi.net
                                         lucindasmith7291@gmail.com
                                         marcusrodriguez5042@gmail.com

-- 121 Kbytes in 3 Requests.
";

    #[test]
    fn parse_queue_output_returns_all_entries() {
        let entries = parse_queue_output(SAMPLE_QUEUE);
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn parse_queue_output_entry_fields() {
        let entries = parse_queue_output(SAMPLE_QUEUE);

        assert_eq!(entries[0].id, "EF7F57AAAD");
        assert_eq!(entries[0].size, 1172);
        assert_eq!(entries[0].arrival_time, "Sat Feb 21 17:03:33");
        assert_eq!(entries[0].sender, "m@tyyi.net");
        assert_eq!(entries[0].recipients, vec!["tayyebimohammadreza@gmail.com"]);

        assert_eq!(entries[1].id, "74C8A7AC47");
        assert_eq!(
            entries[1].recipients,
            vec!["mohammadreza.tayyebi@abrnoc.com"]
        );

        assert_eq!(entries[2].id, "8389B9CA3B");
        assert_eq!(entries[2].size, 121656);
        assert_eq!(
            entries[2].recipients,
            vec![
                "lucindasmith7291@gmail.com",
                "marcusrodriguez5042@gmail.com"
            ]
        );
    }

    #[test]
    fn parse_queue_output_empty_returns_empty() {
        let entries = parse_queue_output("Mail queue is empty\n");
        assert!(entries.is_empty());
    }
}
