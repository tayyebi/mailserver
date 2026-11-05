use askama::Template;
use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use log::{debug, error, warn};
use serde::Deserialize;
use std::fs::File;
use std::io::{ErrorKind, Read, Seek, SeekFrom};

use crate::applog;
use crate::fail2ban::MAIL_LOG_PATH;
use crate::web::auth::AuthAdmin;

/// Bytes read from the end of the mail log for each request. Keeps the
/// viewer fast even when the log has grown large.
const MAIL_TAIL_BYTES: u64 = 1024 * 1024;
const DEFAULT_LINES: usize = 300;
const MAX_LINES: usize = 2000;

// ── Query params ──

#[derive(Deserialize)]
pub struct LogParams {
    #[serde(default)]
    q: String,
    #[serde(default)]
    lines: String,
}

fn parse_lines(raw: &str) -> usize {
    raw.trim()
        .parse::<usize>()
        .unwrap_or(DEFAULT_LINES)
        .clamp(1, MAX_LINES)
}

// ── View models ──

struct LogSource {
    name: &'static str,
    view_url: &'static str,
    records: &'static str,
    storage: &'static str,
    implementation: &'static str,
}

struct AppLogRow {
    timestamp: String,
    level: String,
    target: String,
    message: String,
    severe: bool,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "logs/overview.html")]
struct OverviewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    sources: Vec<LogSource>,
}

#[derive(Template)]
#[template(path = "logs/mail.html")]
struct MailLogTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    file_path: &'static str,
    error: Option<String>,
    query: String,
    lines: usize,
    shown: usize,
    entries: Vec<String>,
}

#[derive(Template)]
#[template(path = "logs/app.html")]
struct AppLogTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    query: String,
    lines: usize,
    capacity: usize,
    entries: Vec<AppLogRow>,
}

// ── Helpers ──

/// Reads the last `max_bytes` of a file as text. If the read starts in the
/// middle of the file, the partial first line is dropped.
fn read_tail(path: &str, max_bytes: u64) -> std::io::Result<String> {
    let mut file = File::open(path)?;
    let len = file.metadata()?.len();
    let start = len.saturating_sub(max_bytes);
    file.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf).into_owned();
    if start == 0 {
        return Ok(text);
    }
    Ok(match text.find('\n') {
        Some(idx) => text[idx + 1..].to_string(),
        None => String::new(),
    })
}

/// Case-insensitive substring match. `needle_lower` must already be lowercase.
fn matches_query(haystack: &str, needle_lower: &str) -> bool {
    needle_lower.is_empty() || haystack.to_lowercase().contains(needle_lower)
}

/// Returns up to `limit` lines matching `query`, newest first.
fn select_lines(text: &str, query: &str, limit: usize) -> Vec<String> {
    let needle = query.trim().to_lowercase();
    text.lines()
        .rev()
        .filter(|line| matches_query(line, &needle))
        .take(limit)
        .map(str::to_string)
        .collect()
}

fn render<T: Template>(tmpl: T) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            error!("[web] failed to render logs template: {}", e);
            crate::web::errors::render_error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Template Error",
                "Failed to render the log page. Please try again.",
                "/logs",
                "Logs",
            )
            .into_response()
        }
    }
}

// ── Handlers ──

pub async fn overview(auth: AuthAdmin) -> Response {
    debug!(
        "[web] GET /logs — log overview for username={}",
        auth.admin.username
    );

    let sources = vec![
        LogSource {
            name: "Mail (Postfix and Dovecot)",
            view_url: "/logs/mail",
            records: "SMTP sessions, queue and delivery events, IMAP and POP logins, and SASL authentication failures.",
            storage: "Container stdout (docker logs), plus /var/log/mail.log inside the container. That file is not on a volume, so it is lost when the container is recreated.",
            implementation: "templates/config/postfix-main.cf.txt and dovecot.conf.txt (maillog_file, log_path); entrypoint.sh (tee to /var/log/mail.log); src/web/routes/logs.rs (viewer)",
        },
        LogSource {
            name: "Application",
            view_url: "/logs/app",
            records: "Records from the admin server and background tasks: config regeneration, fail2ban, database errors.",
            storage: "In-memory buffer of the last 2000 records in the serve process, plus stderr (docker logs). Cleared on restart.",
            implementation: "src/applog.rs (buffer and logger); src/main.rs (logger setup); RUST_LOG sets the level",
        },
        LogSource {
            name: "Fail2ban",
            view_url: "/fail2ban",
            records: "Authentication failures counted per IP, bans, unbans, and whitelist or blacklist changes.",
            storage: "PostgreSQL table fail2ban_log. Failures are read from /var/log/mail.log.",
            implementation: "src/fail2ban.rs (log watcher); src/db.rs (record_fail2ban_attempt, ban_ip); src/web/routes/fail2ban.rs",
        },
        LogSource {
            name: "Webhooks",
            view_url: "/webhooks",
            records: "Outbound webhook requests with response status, duration, and any error. Failed deliveries can be retried.",
            storage: "PostgreSQL table webhook_logs",
            implementation: "src/db.rs (log_webhook), called from src/filter.rs, src/web/mod.rs, src/web/routes/webhook.rs",
        },
        LogSource {
            name: "MCP",
            view_url: "/mcp",
            records: "MCP API calls with method, tool, success, error, and duration.",
            storage: "PostgreSQL table mcp_logs",
            implementation: "src/db.rs (log_mcp_call), called from src/web/routes/mcp.rs",
        },
        LogSource {
            name: "Tracking",
            view_url: "/tracking",
            records: "Tracking pixel opens with client IP and user agent.",
            storage: "PostgreSQL table pixel_opens",
            implementation: "src/web/routes/pixel.rs, src/db.rs (record_pixel_open)",
        },
    ];

    render(OverviewTemplate {
        nav_active: "Logs",
        flash: None,
        sources,
    })
}

pub async fn mail(_auth: AuthAdmin, Query(params): Query<LogParams>) -> Response {
    let query = params.q.trim().to_string();
    let lines = parse_lines(&params.lines);
    debug!(
        "[web] GET /logs/mail — lines={}, filter_set={}",
        lines,
        !query.is_empty()
    );

    let read = tokio::task::spawn_blocking(|| read_tail(MAIL_LOG_PATH, MAIL_TAIL_BYTES)).await;
    let (text, error) = match read {
        Ok(Ok(text)) => (text, None),
        Ok(Err(e)) if e.kind() == ErrorKind::NotFound => (
            String::new(),
            Some(format!(
                "{} does not exist. In Docker, entrypoint.sh creates it and writes Postfix and Dovecot output to it. Outside Docker, mail logs go to syslog or the journal instead.",
                MAIL_LOG_PATH
            )),
        ),
        Ok(Err(e)) => {
            warn!("[web] could not read {}: {}", MAIL_LOG_PATH, e);
            (
                String::new(),
                Some(format!("Could not read {}: {}", MAIL_LOG_PATH, e)),
            )
        }
        Err(e) => {
            error!("[web] mail log read task failed: {}", e);
            (
                String::new(),
                Some(format!("Mail log read task failed: {}", e)),
            )
        }
    };

    let entries = select_lines(&text, &query, lines);
    render(MailLogTemplate {
        nav_active: "Mail Log",
        flash: None,
        file_path: MAIL_LOG_PATH,
        error,
        query,
        lines,
        shown: entries.len(),
        entries,
    })
}

pub async fn app(_auth: AuthAdmin, Query(params): Query<LogParams>) -> Response {
    let query = params.q.trim().to_string();
    let needle = query.to_lowercase();
    let lines = parse_lines(&params.lines);
    debug!(
        "[web] GET /logs/app — lines={}, filter_set={}",
        lines,
        !query.is_empty()
    );

    let entries: Vec<AppLogRow> = applog::recent(applog::CAPACITY)
        .into_iter()
        .filter(|l| {
            matches_query(
                &format!("{} {} {}", l.level, l.target, l.message),
                &needle,
            )
        })
        .take(lines)
        .map(|l| {
            let severe = l.level == "ERROR" || l.level == "WARN";
            AppLogRow {
                timestamp: l.timestamp,
                level: l.level,
                target: l.target,
                message: l.message,
                severe,
            }
        })
        .collect();

    render(AppLogTemplate {
        nav_active: "App Log",
        flash: None,
        query,
        lines,
        capacity: applog::CAPACITY,
        entries,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_lines_defaults_and_clamps() {
        assert_eq!(parse_lines(""), DEFAULT_LINES);
        assert_eq!(parse_lines("abc"), DEFAULT_LINES);
        assert_eq!(parse_lines("0"), 1);
        assert_eq!(parse_lines("50"), 50);
        assert_eq!(parse_lines("999999"), MAX_LINES);
    }

    #[test]
    fn select_lines_is_newest_first_and_filters() {
        let text = "a 1\nb 2\na 3\n";
        assert_eq!(select_lines(text, "", 10), vec!["a 3", "b 2", "a 1"]);
        assert_eq!(select_lines(text, "A", 10), vec!["a 3", "a 1"]);
        assert_eq!(select_lines(text, "", 2), vec!["a 3", "b 2"]);
    }

    #[test]
    fn read_tail_drops_partial_first_line() {
        let path = std::env::temp_dir().join(format!(
            "mailserver-logs-test-{}.log",
            std::process::id()
        ));
        std::fs::write(&path, "aaaaaaaaaa\nbbb\nccc\n").unwrap();
        // The last 8 bytes are "bbb\nccc\n". The read starts mid-file, so the
        // first line ("bbb") is dropped because it may be partial.
        let text = read_tail(path.to_str().unwrap(), 8).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(text, "ccc\n");
    }

    #[test]
    fn read_tail_returns_whole_small_file() {
        let path = std::env::temp_dir().join(format!(
            "mailserver-logs-small-{}.log",
            std::process::id()
        ));
        std::fs::write(&path, "one\ntwo\n").unwrap();
        let text = read_tail(path.to_str().unwrap(), 1024).unwrap();
        let _ = std::fs::remove_file(&path);
        assert_eq!(text, "one\ntwo\n");
    }
}
