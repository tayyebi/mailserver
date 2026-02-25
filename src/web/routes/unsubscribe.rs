use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect, Response},
    routing::get,
    Form,
    Router,
};
use log::{debug, info, warn};

use serde_json;

use crate::web::auth::AuthAdmin;
use crate::web::forms::{TrackingPatternForm, TrackingRuleForm, UnsubscribeQuery};
use crate::web::AppState;

// ── Templates ──

#[derive(Template)]
#[template(path = "unsubscribe/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    entries: Vec<crate::db::UnsubscribeEntry>,
    patterns: Vec<crate::db::UnsubscribePattern>,
    rules: Vec<crate::db::UnsubscribeRule>,
}

#[derive(Template)]
#[template(path = "unsubscribe/confirm.html")]
struct ConfirmTemplate<'a> {
    token: &'a str,
    success: bool,
    requires_confirmation: bool,
    message: &'a str,
}

// ── Public routes (no auth) ──

pub fn public_routes() -> Router<AppState> {
    Router::new().route(
        "/unsubscribe",
        get(unsubscribe_confirm_page).post(unsubscribe_one_click),
    )
}

/// GET /unsubscribe?token=xxx — show a confirmation/status page
async fn unsubscribe_confirm_page(
    State(state): State<AppState>,
    Query(params): Query<UnsubscribeQuery>,
) -> Response {
    debug!("[web] GET /unsubscribe token={}", params.token);
    if params.token.is_empty() {
        let tmpl = ConfirmTemplate {
            token: "",
            success: false,
            requires_confirmation: false,
            message: "No unsubscribe token provided.",
        };
        return (StatusCode::BAD_REQUEST, Html(tmpl.render().unwrap())).into_response();
    }
    let token = params.token.clone();
    let entry = state
        .blocking_db(move |db| db.get_unsubscribe_by_token(&token))
        .await;
    match entry {
        Some((email, domain)) => {
            let already = state
                .blocking_db(move |db| db.is_unsubscribed(&email, &domain))
                .await;
            let tmpl = ConfirmTemplate {
                token: &params.token,
                success: !already,
                requires_confirmation: !already,
                message: if already {
                    "You are already unsubscribed."
                } else {
                    "Click the button below to confirm your unsubscribe request."
                },
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        None => {
            warn!("[web] unsubscribe token not found: {}", params.token);
            let tmpl = ConfirmTemplate {
                token: &params.token,
                success: false,
                requires_confirmation: false,
                message: "Invalid or expired unsubscribe token.",
            };
            (StatusCode::NOT_FOUND, Html(tmpl.render().unwrap())).into_response()
        }
    }
}

/// POST /unsubscribe?token=xxx — RFC 8058 one-click unsubscribe
/// Accepts both mail-client automated POST (body: List-Unsubscribe=One-Click)
/// and manual browser form submission.
async fn unsubscribe_one_click(
    State(state): State<AppState>,
    Query(params): Query<UnsubscribeQuery>,
) -> Response {
    info!("[web] POST /unsubscribe token={}", params.token);
    if params.token.is_empty() {
        let tmpl = ConfirmTemplate {
            token: "",
            success: false,
            requires_confirmation: false,
            message: "No unsubscribe token provided.",
        };
        return (StatusCode::BAD_REQUEST, Html(tmpl.render().unwrap())).into_response();
    }
    let token = params.token.clone();
    let entry = state
        .blocking_db(move |db| db.get_unsubscribe_by_token(&token))
        .await;
    match entry {
        Some((email, domain)) => {
            let email_for_db = email.clone();
            let domain_for_db = domain.clone();
            state
                .blocking_db(move |db| db.record_unsubscribe(&email_for_db, &domain_for_db))
                .await;
            info!(
                "[web] unsubscribe recorded: email={} domain={}",
                email, domain
            );
            let tmpl = ConfirmTemplate {
                token: &params.token,
                success: true,
                requires_confirmation: false,
                message: "You have been successfully unsubscribed.",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        None => {
            warn!("[web] unsubscribe token not found: {}", params.token);
            let tmpl = ConfirmTemplate {
                token: &params.token,
                success: false,
                requires_confirmation: false,
                message: "Invalid or expired unsubscribe token.",
            };
            (StatusCode::NOT_FOUND, Html(tmpl.render().unwrap())).into_response()
        }
    }
}

// ── Admin routes (auth required) ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /unsubscribe/list — listing unsubscribe entries");
    let entries = state.blocking_db(|db| db.list_unsubscribes()).await;
    debug!("[web] found {} unsubscribe entries", entries.len());
    let patterns = state.blocking_db(|db| db.list_unsubscribe_patterns()).await;
    let rules = state.blocking_db(|db| db.list_unsubscribe_rules()).await;
    let tmpl = ListTemplate {
        nav_active: "Unsubscribe",
        flash: None,
        entries,
        patterns,
        rules,
    };
    Html(tmpl.render().unwrap())
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!(
        "[web] POST /unsubscribe/{}/delete — deleting unsubscribe entry",
        id
    );
    state.blocking_db(move |db| db.delete_unsubscribe(id)).await;
    axum::response::Redirect::to("/unsubscribe/list").into_response()
}

pub async fn create_pattern(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TrackingPatternForm>,
) -> Response {
    info!("[web] POST /unsubscribe/patterns — creating pattern={}", form.pattern);
    let pattern = form.pattern.trim().to_string();
    state
        .blocking_db(move |db| db.create_unsubscribe_pattern(&pattern))
        .await
        .ok();
    Redirect::to("/unsubscribe/list").into_response()
}

pub async fn delete_pattern(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /unsubscribe/patterns/{}/delete — deleting pattern", id);
    state
        .blocking_db(move |db| db.delete_unsubscribe_pattern(id))
        .await;
    Redirect::to("/unsubscribe/list").into_response()
}

pub async fn create_rule(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TrackingRuleForm>,
) -> Response {
    info!("[web] POST /unsubscribe/rules — creating rule name={}", form.name);
    let name = form.name.trim().to_string();
    let match_mode = if form.match_mode == "OR" { "OR" } else { "AND" }.to_string();
    let conditions: Vec<crate::db::TrackingCondition> =
        serde_json::from_str(&form.conditions_json).unwrap_or_default();
    state
        .blocking_db(move |db| db.create_unsubscribe_rule(&name, &match_mode, &conditions))
        .await
        .ok();
    Redirect::to("/unsubscribe/list").into_response()
}

pub async fn delete_rule(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /unsubscribe/rules/{}/delete — deleting rule", id);
    state
        .blocking_db(move |db| db.delete_unsubscribe_rule(id))
        .await;
    Redirect::to("/unsubscribe/list").into_response()
}
