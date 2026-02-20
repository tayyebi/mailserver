use askama::Template;
use axum::{
    extract::State,
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::web::AppState;
use crate::web::auth::AuthAdmin;
use crate::web::forms::SpamblToggleForm;

fn same_origin(headers: &HeaderMap) -> bool {
    let host = match headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        Some(v) => v,
        None => return false,
    };
    let matches_host = |value: &str| {
        let rest = match value.split_once("://") {
            Some((_, rest)) => rest,
            None => return false,
        };
        let authority = rest.split('/').next().unwrap_or(rest);
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        authority.eq_ignore_ascii_case(host)
    };

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        return matches_host(origin);
    }
    if let Some(referer) = headers.get(header::REFERER).and_then(|v| v.to_str().ok()) {
        return matches_host(referer);
    }
    false
}

// ── Template ──

#[derive(Template)]
#[template(path = "spambl/list.html")]
struct SpamblListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    lists: Vec<crate::db::SpamblList>,
}

// ── Handlers ──

pub async fn list(auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    debug!(
        "[web] GET /spambl — spambl list for username={}",
        auth.admin.username
    );

    let lists = state.blocking_db(|db| db.list_spambl_lists()).await;

    let tmpl = SpamblListTemplate {
        nav_active: "Spambl",
        flash: None,
        lists,
    };
    match tmpl.render() {
        Ok(html) => Html(html),
        Err(e) => {
            error!("[web] failed to render spambl template: {}", e);
            crate::web::errors::render_error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Template Error",
                "Failed to render spam blocklist page.",
                "/",
                "Dashboard",
            )
        }
    }
}

pub async fn toggle(
    auth: AuthAdmin,
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<SpamblToggleForm>,
) -> Response {
    info!(
        "[web] POST /spambl/toggle — toggle spambl id={} for username={}",
        form.id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] spambl toggle blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    let enabled = form.enabled.as_deref() == Some("on");
    let id = form.id;
    state
        .blocking_db(move |db| db.set_spambl_enabled(id, enabled))
        .await;

    crate::web::regen_configs(&state).await;

    Redirect::to("/spambl").into_response()
}
