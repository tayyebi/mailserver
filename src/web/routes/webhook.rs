use askama::Template;
use axum::{
    extract::{Query, State},
    response::Html,
};
use log::{debug, info};
use serde::Deserialize;

use crate::db::WebhookLog;
use crate::web::AppState;
use crate::web::auth::AuthAdmin;

const PAGE_SIZE: i64 = 50;

// ── Query params ──

#[derive(Deserialize)]
pub struct PageParams {
    #[serde(default = "default_page")]
    page: i64,
}

fn default_page() -> i64 {
    1
}

// ── View model ──

struct WebhookLogRow {
    id: i64,
    url: String,
    sender: String,
    subject: String,
    response_status: String,
    duration_ms: String,
    error: String,
    created_at: String,
    success: bool,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "webhook/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    logs: Vec<WebhookLogRow>,
    page: i64,
    total_pages: i64,
    total_count: i64,
}

// ── Handler ──

pub async fn list(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(params): Query<PageParams>,
) -> Html<String> {
    let page = params.page.max(1);
    info!("[web] GET /webhooks — page={}", page);

    let total_count = state.blocking_db(|db| db.count_webhook_logs()).await;
    let total_pages = ((total_count as f64) / (PAGE_SIZE as f64)).ceil() as i64;
    let total_pages = total_pages.max(1);
    let page = page.min(total_pages);
    let offset = (page - 1) * PAGE_SIZE;

    let raw: Vec<WebhookLog> = state
        .blocking_db(move |db| db.list_webhook_logs(PAGE_SIZE, offset))
        .await;

    debug!("[web] /webhooks page={} returned {} rows", page, raw.len());

    let logs: Vec<WebhookLogRow> = raw
        .into_iter()
        .map(|r| {
            let success = r.error.is_empty()
                && r.response_status.map(|s| s >= 200 && s < 300).unwrap_or(false);
            WebhookLogRow {
                id: r.id,
                url: r.url,
                sender: r.sender,
                subject: r.subject,
                response_status: r
                    .response_status
                    .map(|s| s.to_string())
                    .unwrap_or_else(|| "—".to_string()),
                duration_ms: r
                    .duration_ms
                    .map(|d| format!("{} ms", d))
                    .unwrap_or_else(|| "—".to_string()),
                error: r.error,
                created_at: r.created_at,
                success,
            }
        })
        .collect();

    let tmpl = ListTemplate {
        nav_active: "Webhooks",
        flash: None,
        logs,
        page,
        total_pages,
        total_count,
    };
    Html(tmpl.render().unwrap())
}
