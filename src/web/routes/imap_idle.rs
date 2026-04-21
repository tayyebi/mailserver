use askama::Template;
use axum::{extract::State, response::Html};
use log::info;

use crate::web::auth::AuthAdmin;
use crate::web::AppState;
use crate::web::ImapIdleSession;

// ── Templates ──

#[derive(Template)]
#[template(path = "imap_idle/list.html")]
struct ImapIdleTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    sessions: Vec<ImapIdleSession>,
}

// ── Handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /imap-idle — listing active IDLE sessions");

    let sessions: Vec<ImapIdleSession> = {
        let reg = state.idle_registry.lock().unwrap();
        let mut list: Vec<ImapIdleSession> = reg.values().cloned().collect();
        list.sort_by(|a, b| a.connected_at.cmp(&b.connected_at));
        list
    };

    let tmpl = ImapIdleTemplate {
        nav_active: "IMAP IDLE",
        flash: None,
        sessions,
    };
    Html(tmpl.render().unwrap())
}
