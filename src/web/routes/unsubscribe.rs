use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use log::{debug, info, warn};

use crate::web::forms::UnsubscribeQuery;
use crate::web::AppState;
use crate::web::auth::AuthAdmin;

// ── Templates ──

#[derive(Template)]
#[template(path = "unsubscribe/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    entries: Vec<crate::db::UnsubscribeEntry>,
}

#[derive(Template)]
#[template(path = "unsubscribe/confirm.html")]
struct ConfirmTemplate<'a> {
    token: &'a str,
    success: bool,
    message: &'a str,
}

// ── Public routes (no auth) ──

pub fn public_routes() -> Router<AppState> {
    Router::new()
        .route("/unsubscribe", get(unsubscribe_confirm_page).post(unsubscribe_one_click))
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
                success: true,
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
            info!("[web] unsubscribe recorded: email={} domain={}", email, domain);
            let tmpl = ConfirmTemplate {
                token: &params.token,
                success: true,
                message: "You have been successfully unsubscribed.",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        None => {
            warn!("[web] unsubscribe token not found: {}", params.token);
            let tmpl = ConfirmTemplate {
                token: &params.token,
                success: false,
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
    let tmpl = ListTemplate {
        nav_active: "Unsubscribe",
        flash: None,
        entries,
    };
    Html(tmpl.render().unwrap())
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!("[web] POST /unsubscribe/{}/delete — deleting unsubscribe entry", id);
    state.blocking_db(move |db| db.delete_unsubscribe(id)).await;
    axum::response::Redirect::to("/unsubscribe/list").into_response()
}
