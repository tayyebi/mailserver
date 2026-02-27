use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, info, warn};

use crate::db::PixelOpen;
use crate::web::auth::AuthAdmin;
use crate::web::forms::{TrackingPatternForm, TrackingRuleForm};
use crate::web::AppState;

// serde_json used for parsing conditions_json from the rule form
use serde_json;

// ── View models ──

struct TrackingRow {
    message_id: String,
    message_id_short: String,
    sender: String,
    recipient: String,
    subject: String,
    created_at: String,
    open_count: usize,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "tracking/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    messages: Vec<TrackingRow>,
    patterns: Vec<crate::db::TrackingPattern>,
    rules: Vec<crate::db::TrackingRule>,
    pixel_host: String,
    pixel_port: String,
    pixel_scheme: String,
}

#[derive(Template)]
#[template(path = "tracking/detail.html")]
struct DetailTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    message: crate::db::TrackedMessage,
    opens: Vec<PixelOpen>,
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

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /tracking — listing tracked messages");
    let raw_messages = state.blocking_db(|db| db.list_tracked_messages(100)).await;
    debug!("[web] found {} tracked messages", raw_messages.len());

    let mut messages: Vec<TrackingRow> = Vec::with_capacity(raw_messages.len());
    for m in raw_messages {
        let message_id_for_db = m.message_id.clone();
        let open_count = state
            .blocking_db(move |db| db.get_opens_for_message(&message_id_for_db).len())
            .await;
        let message_id_short = if m.message_id.len() > 20 {
            m.message_id[..20].to_string()
        } else {
            m.message_id.clone()
        };
        messages.push(TrackingRow {
            message_id: m.message_id,
            message_id_short,
            sender: m.sender,
            recipient: m.recipient,
            subject: m.subject,
            created_at: m.created_at,
            open_count,
        });
    }

    let patterns = state.blocking_db(|db| db.list_tracking_patterns()).await;
    let rules = state.blocking_db(|db| db.list_tracking_rules()).await;
    let (pixel_host, pixel_port, pixel_scheme) = load_pixel_settings(&state).await;

    let tmpl = ListTemplate {
        nav_active: "Tracking",
        flash: None,
        messages,
        patterns,
        rules,
        pixel_host,
        pixel_port,
        pixel_scheme,
    };
    Html(tmpl.render().unwrap())
}

async fn load_pixel_settings(state: &AppState) -> (String, String, String) {
    let default_host = state.hostname.clone();
    let mut pixel_host = default_host;
    let mut pixel_port = String::new();
    let mut pixel_scheme = "http".to_string();

    if let Some(base) = state.blocking_db(|db| db.get_setting("pixel_base_url")).await {
        if base.starts_with("https://") {
            pixel_scheme = "https".to_string();
        }
        let trimmed = base
            .trim_end_matches("/pixel?id=")
            .trim_end_matches("/pixel");
        let no_scheme = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .unwrap_or(trimmed);
        let host_port = no_scheme.split('/').next().unwrap_or(no_scheme);
        if let Some((h, p)) = host_port.split_once(':') {
            pixel_host = h.to_string();
            pixel_port = p.to_string();
        } else {
            pixel_host = host_port.to_string();
        }
    } else if let Ok(env_val) = std::env::var("PIXEL_BASE_URL") {
        if env_val.starts_with("https://") {
            pixel_scheme = "https".to_string();
        }
        let trimmed = env_val
            .trim_end_matches("/pixel?id=")
            .trim_end_matches("/pixel");
        let no_scheme = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .unwrap_or(trimmed);
        let host_port = no_scheme.split('/').next().unwrap_or(no_scheme);
        if let Some((h, p)) = host_port.split_once(':') {
            pixel_host = h.to_string();
            pixel_port = p.to_string();
        } else {
            pixel_host = host_port.to_string();
        }
    }
    (pixel_host, pixel_port, pixel_scheme)
}

pub async fn update_pixel_settings(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<crate::web::forms::PixelSettingsForm>,
) -> Response {
    info!(
        "[web] POST /tracking/pixel — update pixel host/port for username={}",
        auth.admin.username
    );
    let host = form.pixel_host.trim().to_string();
    if host.is_empty() {
        return Redirect::to("/tracking").into_response();
    }
    let scheme = match form.pixel_scheme.as_deref() {
        Some("https") => "https",
        _ => "http",
    };
    let base = match form.pixel_port {
        Some(p) if p > 0 && !((scheme == "http" && p == 80) || (scheme == "https" && p == 443)) => {
            format!("{}://{}:{}/pixel?id=", scheme, host, p)
        }
        _ => format!("{}://{}/pixel?id=", scheme, host),
    };
    let base_for_db = base.clone();
    state
        .blocking_db(move |db| db.set_setting("pixel_base_url", &base_for_db))
        .await;
    info!(
        "[web] pixel_base_url updated to {} by user={}",
        base, auth.admin.username
    );
    Redirect::to("/tracking").into_response()
}

pub async fn detail(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(msg_id): Path<String>,
) -> Response {
    debug!("[web] GET /tracking/{} — tracking detail requested", msg_id);
    let msg_id_for_db = msg_id.clone();
    let message = match state
        .blocking_db(move |db| db.get_tracked_message(&msg_id_for_db))
        .await
    {
        Some(m) => m,
        None => {
            warn!("[web] tracked message not found: {}", msg_id);
            let tmpl = ErrorTemplate {
                nav_active: "Tracking",
                flash: None,
                status_code: 404,
                status_text: "Not Found",
                title: "Not Found",
                message: "Message not found.",
                back_url: "/tracking",
                back_label: "Back to Tracking",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };
    let msg_id_for_db = msg_id.clone();
    let opens = state
        .blocking_db(move |db| db.get_opens_for_message(&msg_id_for_db))
        .await;
    debug!("[web] tracked message {} has {} opens", msg_id, opens.len());

    let tmpl = DetailTemplate {
        nav_active: "Tracking",
        flash: None,
        message,
        opens,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn create_pattern(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TrackingPatternForm>,
) -> Response {
    info!("[web] POST /tracking/patterns — creating pattern={}", form.pattern);
    let pattern = form.pattern.trim().to_string();
    state
        .blocking_db(move |db| db.create_tracking_pattern(&pattern))
        .await
        .ok();
    Redirect::to("/tracking").into_response()
}

pub async fn delete_pattern(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /tracking/patterns/{}/delete — deleting pattern", id);
    state
        .blocking_db(move |db| db.delete_tracking_pattern(id))
        .await;
    Redirect::to("/tracking").into_response()
}

pub async fn create_rule(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TrackingRuleForm>,
) -> Response {
    info!("[web] POST /tracking/rules — creating rule name={}", form.name);
    let name = form.name.trim().to_string();
    let match_mode = if form.match_mode == "OR" { "OR" } else { "AND" }.to_string();
    let conditions: Vec<crate::db::TrackingCondition> =
        serde_json::from_str(&form.conditions_json).unwrap_or_default();
    state
        .blocking_db(move |db| db.create_tracking_rule(&name, &match_mode, &conditions))
        .await
        .ok();
    Redirect::to("/tracking").into_response()
}

pub async fn delete_rule(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /tracking/rules/{}/delete — deleting rule", id);
    state
        .blocking_db(move |db| db.delete_tracking_rule(id))
        .await;
    Redirect::to("/tracking").into_response()
}
