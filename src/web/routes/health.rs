use askama::Template;
use axum::{extract::State, response::Html};
use log::info;

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

#[derive(Template)]
#[template(path = "health.html")]
struct HealthTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    hostname: &'a str,
    total: i64,
    successes: i64,
    failures: i64,
    success_rate: String,
    has_failures: bool,
    recent_failures: Vec<HealthRow>,
}

#[derive(Clone)]
struct HealthRow {
    created_at: String,
    status: Option<i32>,
    status_label: String,
    cause: String,
    sender: String,
    subject: String,
    failed: bool,
}

pub async fn page(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /health — delivery observability requested");
    let (total, successes, raw_failures) = state.blocking_db(|db| db.get_delivery_health()).await;
    let failures = total.saturating_sub(successes);
    let success_rate_value = if total == 0 {
        100.0
    } else {
        (successes as f64 / total as f64) * 100.0
    };
    let success_rate = format!("{:.1}", success_rate_value);

    let recent_failures: Vec<HealthRow> = raw_failures
        .into_iter()
        .map(|log| {
            let status_label = log
                .response_status
                .map(|s| s.to_string())
                .unwrap_or_else(|| "error".to_string());
            let has_error = !log.error.is_empty();
            let cause_raw = if has_error {
                log.error
            } else {
                log.response_body
            };
            let cause = if cause_raw.len() > 80 {
                format!("{}…", &cause_raw[..80])
            } else {
                cause_raw
            };
            let failed = log
                .response_status
                .map(|s| s < 200 || s >= 400)
                .unwrap_or(true)
                || has_error;
            HealthRow {
                created_at: log.created_at,
                status: log.response_status,
                status_label,
                cause,
                sender: log.sender,
                subject: log.subject,
                failed,
            }
        })
        .collect();
    let has_failures = !recent_failures.is_empty();

    let tmpl = HealthTemplate {
        nav_active: "Health",
        flash: None,
        hostname: &state.hostname,
        total,
        successes,
        failures,
        success_rate,
        has_failures,
        recent_failures,
    };
    Html(tmpl.render().expect("failed to render health template"))
}
