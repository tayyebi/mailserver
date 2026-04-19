use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
    Form,
};
use log::{info, warn};

use crate::db::{RateLimitRule, TrackingCondition};
use crate::web::auth::AuthAdmin;
use crate::web::forms::RateLimitRuleForm;
use crate::web::AppState;

// ── Templates ──

#[derive(Template)]
#[template(path = "rate_limits/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    rules: Vec<RateLimitRule>,
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
    info!("[web] GET /rate-limits — listing rate limit rules");
    let rules = state
        .blocking_db(|db| db.list_rate_limit_rules())
        .await;
    let tmpl = ListTemplate {
        nav_active: "Rate Limits",
        flash: None,
        rules,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create_rule(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<RateLimitRuleForm>,
) -> Response {
    info!(
        "[web] POST /rate-limits/rules — create rate limit rule by username={}",
        auth.admin.username
    );

    let conditions: Vec<TrackingCondition> = match serde_json::from_str(&form.conditions_json) {
        Ok(c) => c,
        Err(e) => {
            warn!("[web] invalid conditions JSON: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "Rate Limits",
                flash: None,
                status_code: 400,
                status_text: "Bad Request",
                title: "Error",
                message: "Invalid conditions JSON.",
                back_url: "/rate-limits",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };

    let name = form.name.clone();
    let match_mode = form.match_mode.clone();
    let max_messages = form.max_messages.max(1);
    let window_seconds = form.window_seconds.max(1);

    match state
        .blocking_db(move |db| {
            db.create_rate_limit_rule(&name, &match_mode, &conditions, max_messages, window_seconds)
        })
        .await
    {
        Ok(_) => {
            info!(
                "[web] rate limit rule created by user={}",
                auth.admin.username
            );
        }
        Err(e) => {
            warn!("[web] failed to create rate limit rule: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "Rate Limits",
                flash: None,
                status_code: 500,
                status_text: "Internal Server Error",
                title: "Error",
                message: &format!("Failed to create rule: {}", e),
                back_url: "/rate-limits",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    }

    let rules = state
        .blocking_db(|db| db.list_rate_limit_rules())
        .await;
    let tmpl = ListTemplate {
        nav_active: "Rate Limits",
        flash: Some("Rate limit rule created."),
        rules,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn delete_rule(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!(
        "[web] POST /rate-limits/rules/{}/delete — delete rule by username={}",
        id, auth.admin.username
    );
    state
        .blocking_db(move |db| db.delete_rate_limit_rule(id))
        .await;
    let rules = state
        .blocking_db(|db| db.list_rate_limit_rules())
        .await;
    let tmpl = ListTemplate {
        nav_active: "Rate Limits",
        flash: Some("Rate limit rule deleted."),
        rules,
    };
    Html(tmpl.render().unwrap()).into_response()
}
