use askama::Template;
use axum::{
    extract::{Query, State},
    response::{Html, IntoResponse, Response},
    Form,
};
use log::{debug, info, warn};
use serde::Deserialize;

use crate::db::WebhookLog;
use crate::web::auth::AuthAdmin;
use crate::web::forms::WebhookSettingsForm;
use crate::web::AppState;

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
    webhook_url: String,
    logs: Vec<WebhookLogRow>,
    page: i64,
    total_pages: i64,
    total_count: i64,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    status_code: u16,
    status_text: &'a str,
    title: &'a str,
    message: &'a str,
    back_url: &'a str,
    back_label: &'a str,
}

// ── Handlers ──

pub async fn list(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(params): Query<PageParams>,
) -> Html<String> {
    let page = params.page.max(1);
    info!("[web] GET /webhooks — page={}", page);

    let webhook_url = state
        .blocking_db(|db| db.get_setting("webhook_url"))
        .await
        .unwrap_or_default();

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
                && r.response_status
                    .map(|s| s >= 200 && s < 300)
                    .unwrap_or(false);
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
        webhook_url,
        logs,
        page,
        total_pages,
        total_count,
    };
    Html(tmpl.render().unwrap())
}

pub async fn update_webhook(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<WebhookSettingsForm>,
) -> Response {
    info!(
        "[web] POST /webhooks/settings — update webhook URL by username={}",
        auth.admin.username
    );
    let url = form.webhook_url.trim().to_string();
    // Validate: must be empty or start with http:// or https://
    if !url.is_empty() && !url.starts_with("http://") && !url.starts_with("https://") {
        let tmpl = ErrorTemplate {
            nav_active: "Webhooks",
            flash: None,
            status_code: 400,
            status_text: "Bad Request",
            title: "Error",
            message: "Webhook URL must start with http:// or https://",
            back_url: "/webhooks",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let url_for_db = url.clone();
    state
        .blocking_db(move |db| db.set_setting("webhook_url", &url_for_db))
        .await;
    info!("[web] webhook_url updated by user={}", auth.admin.username);
    let tmpl = ErrorTemplate {
        nav_active: "Webhooks",
        flash: None,
        status_code: 200,
        status_text: "OK",
        title: "Success",
        message: "Webhook URL updated successfully.",
        back_url: "/webhooks",
        back_label: "Back to Webhooks",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn test_webhook(auth: AuthAdmin, State(state): State<AppState>) -> Response {
    info!(
        "[web] POST /webhooks/test — webhook test by username={}",
        auth.admin.username
    );

    let webhook_url = state
        .blocking_db(|db| db.get_setting("webhook_url"))
        .await
        .unwrap_or_default();

    if webhook_url.is_empty() {
        let tmpl = ErrorTemplate {
            nav_active: "Webhooks",
            flash: None,
            status_code: 400,
            status_text: "Bad Request",
            title: "No Webhook URL",
            message: "No webhook URL is configured. Save a webhook URL first, then test it.",
            back_url: "/webhooks",
            back_label: "Back to Webhooks",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }

    let timestamp = chrono::Utc::now().to_rfc3339();
    let payload = serde_json::json!({
        "event": "test",
        "timestamp": timestamp,
        "sender": "test@example.com",
        "recipients": ["recipient@example.com"],
        "subject": "Webhook Test",
        "from": "Test <test@example.com>",
        "to": "recipient@example.com",
        "cc": "",
        "date": timestamp,
        "message_id": "<test@mailserver>",
        "size_bytes": 0,
        "modified": false,
    });
    let request_body = payload.to_string();

    let start = std::time::Instant::now();
    let result = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()
        .map_err(|e| e.to_string())
        .and_then(|client| {
            client
                .post(&webhook_url)
                .json(&payload)
                .send()
                .map_err(|e| e.to_string())
        });
    let duration_ms = start.elapsed().as_millis() as i64;

    let (response_status, response_body, error_msg) = match result {
        Ok(resp) => {
            let status = resp.status().as_u16() as i32;
            let body = resp.text().unwrap_or_default();
            let body_truncated = if body.len() > 2048 {
                let mut end = 2048;
                while !body.is_char_boundary(end) {
                    end -= 1;
                }
                body[..end].to_string()
            } else {
                body
            };
            info!(
                "[web] test webhook delivered to {} status={}",
                webhook_url, status
            );
            (Some(status), body_truncated, String::new())
        }
        Err(e) => {
            warn!("[web] test webhook failed to {}: {}", webhook_url, e);
            (None, String::new(), e.clone())
        }
    };

    // Log the test execution to the database
    let url_clone = webhook_url.clone();
    let rb_clone = request_body.clone();
    let rb2_clone = response_body.clone();
    let err_clone = error_msg.clone();
    state
        .blocking_db(move |db| {
            db.log_webhook(
                &url_clone,
                &rb_clone,
                response_status,
                &rb2_clone,
                &err_clone,
                duration_ms,
                "test@example.com",
                "Webhook Test",
            )
        })
        .await;

    if error_msg.is_empty() {
        let msg = format!(
            "Test webhook delivered to {} — HTTP {} in {} ms.",
            webhook_url,
            response_status.unwrap_or(0),
            duration_ms
        );
        let tmpl = ErrorTemplate {
            nav_active: "Webhooks",
            flash: None,
            status_code: 200,
            status_text: "OK",
            title: "Webhook Test Successful",
            message: &msg,
            back_url: "/webhooks",
            back_label: "Back to Webhooks",
        };
        Html(tmpl.render().unwrap()).into_response()
    } else {
        let msg = format!(
            "Webhook test to {} failed after {} ms: {}",
            webhook_url, duration_ms, error_msg
        );
        let tmpl = ErrorTemplate {
            nav_active: "Webhooks",
            flash: None,
            status_code: 502,
            status_text: "Bad Gateway",
            title: "Webhook Test Failed",
            message: &msg,
            back_url: "/webhooks",
            back_label: "Back to Webhooks",
        };
        Html(tmpl.render().unwrap()).into_response()
    }
}
