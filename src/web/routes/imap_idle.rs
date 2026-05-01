use askama::Template;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    response::{Html, IntoResponse, Redirect, Response},
};
use log::{info, warn};

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Templates ──

/// A single row in the IMAP IDLE session table, enriched with the computed session duration.
struct SessionRow {
    pub id: String,
    pub account: String,
    pub folder: String,
    pub connected_at: String,
    pub last_ping_at: String,
    pub duration: String,
}

#[derive(Template)]
#[template(path = "imap_idle/list.html")]
struct ImapIdleTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    sessions: Vec<SessionRow>,
    total_sessions: usize,
    unique_accounts: usize,
    unique_domains: usize,
}

// ── Helpers ──

/// Format a duration in seconds as a human-readable string, e.g. "2h 15m" or "45s".
fn format_duration(secs: i64) -> String {
    if secs < 0 {
        warn!(
            "[idle] negative session duration {}s — possible clock skew",
            secs
        );
        return "0s".to_string();
    }
    let secs = secs as u64;
    let hours = secs / 3600;
    let minutes = (secs % 3600) / 60;
    let seconds = secs % 60;
    if hours > 0 {
        format!("{}h {}m", hours, minutes)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

fn same_origin(headers: &HeaderMap) -> bool {
    headers.contains_key("referer") || headers.contains_key("origin")
}

// ── Handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /imap-idle — listing active IDLE sessions");

    let now_secs = chrono::Utc::now().timestamp();

    let sessions: Vec<crate::web::ImapIdleSession> = {
        let reg = state.idle_registry.lock().unwrap();
        let mut list: Vec<crate::web::ImapIdleSession> = reg.values().cloned().collect();
        list.sort_by(|a, b| a.connected_at.cmp(&b.connected_at));
        list
    };

    let unique_accounts: std::collections::HashSet<i64> =
        sessions.iter().map(|s| s.account_id).collect();
    let unique_domains: std::collections::HashSet<&str> =
        sessions.iter().map(|s| s.domain.as_str()).collect();

    let total_sessions = sessions.len();
    let unique_accounts_count = unique_accounts.len();
    let unique_domains_count = unique_domains.len();

    let rows: Vec<SessionRow> = sessions
        .into_iter()
        .map(|s| {
            let elapsed = now_secs - s.connected_at_secs;
            SessionRow {
                id: s.id,
                account: format!("{}@{}", s.username, s.domain),
                folder: s.folder,
                connected_at: s.connected_at,
                last_ping_at: s.last_ping_at,
                duration: format_duration(elapsed),
            }
        })
        .collect();

    let tmpl = ImapIdleTemplate {
        nav_active: "IMAP IDLE",
        flash: None,
        sessions: rows,
        total_sessions,
        unique_accounts: unique_accounts_count,
        unique_domains: unique_domains_count,
    };
    Html(tmpl.render().unwrap())
}

/// Forcibly terminate a single IMAP IDLE session by its ID.
pub async fn disconnect(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<String>,
    headers: HeaderMap,
) -> Response {
    info!(
        "[web] POST /imap-idle/{}/disconnect — by username={}",
        id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] imap-idle disconnect blocked: non same-origin request");
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }

    let found = {
        let mut reg = state.idle_registry.lock().unwrap();
        if let Some(session) = reg.remove(&id) {
            // Signal the polling task to exit on its next tick after removing
            // the session from the registry so it is never visible in a partially
            // removed state.
            session
                .shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
            true
        } else {
            false
        }
    };

    if found {
        info!("[idle] admin disconnected session {}", id);
    } else {
        warn!("[idle] disconnect requested for unknown session {}", id);
    }

    Redirect::to("/imap-idle").into_response()
}

/// Forcibly terminate all active IMAP IDLE sessions.
pub async fn disconnect_all(
    auth: AuthAdmin,
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Response {
    info!(
        "[web] POST /imap-idle/disconnect-all — by username={}",
        auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] imap-idle disconnect-all blocked: non same-origin request");
        return axum::http::StatusCode::FORBIDDEN.into_response();
    }

    let count = {
        let mut reg = state.idle_registry.lock().unwrap();
        let count = reg.len();
        for session in reg.values() {
            session
                .shutdown
                .store(true, std::sync::atomic::Ordering::Relaxed);
        }
        reg.clear();
        count
    };

    info!("[idle] admin disconnected all {} sessions", count);
    Redirect::to("/imap-idle").into_response()
}
