use askama::Template;
use axum::{extract::State, response::Html};
use log::{debug, info};

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Templates ──

#[derive(Template)]
#[template(path = "dashboard.html")]
struct DashboardTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    hostname: &'a str,
    stats: crate::db::Stats,
    idle_session_count: usize,
}

pub async fn page(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET / — dashboard requested");
    let stats = state.blocking_db(|db| db.get_stats()).await;

    let idle_session_count = {
        let reg = state.idle_registry.lock().unwrap();
        reg.len()
    };

    debug!(
        "[web] dashboard stats: domains={}, accounts={}, aliases={}, forwarding={}, tracked={}, opens={}, banned={}, webhooks={}, unsubs={}, dkim_ready={}, idle_sessions={}",
        stats.domain_count,
        stats.account_count,
        stats.alias_count,
        stats.forwarding_count,
        stats.tracked_count,
        stats.open_count,
        stats.banned_count,
        stats.webhook_count,
        stats.unsubscribe_count,
        stats.dkim_ready_count,
        idle_session_count,
    );

    let tmpl = DashboardTemplate {
        nav_active: "Dashboard",
        flash: None,
        hostname: &state.hostname,
        stats,
        idle_session_count,
    };
    Html(tmpl.render().unwrap())
}
