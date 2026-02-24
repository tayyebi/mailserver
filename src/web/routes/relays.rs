use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::web::auth::AuthAdmin;
use crate::web::forms::{RelayAssignmentForm, RelayEditForm, RelayForm};
use crate::web::regen_configs;
use crate::web::AppState;

// ── Templates ──

#[derive(Template)]
#[template(path = "relays/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    relays: Vec<crate::db::OutboundRelay>,
}

#[derive(Template)]
#[template(path = "relays/new.html")]
struct NewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "relays/edit.html")]
struct EditTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    relay: crate::db::OutboundRelay,
    assignments: Vec<crate::db::OutboundRelayAssignment>,
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
    info!("[web] GET /relays — listing outbound relays");
    let relays = state.blocking_db(|db| db.list_outbound_relays()).await;
    debug!("[web] found {} relays", relays.len());
    let tmpl = ListTemplate {
        nav_active: "Relays",
        flash: None,
        relays,
    };
    Html(tmpl.render().unwrap())
}

pub async fn new_form(_auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    debug!("[web] GET /relays/new — new relay form");
    let tmpl = NewTemplate {
        nav_active: "Relays",
        flash: None,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<RelayForm>,
) -> Response {
    let port = form.port.unwrap_or(587);
    let auth_type = if form.auth_type.is_empty() {
        "none".to_string()
    } else {
        form.auth_type.clone()
    };
    info!(
        "[web] POST /relays — creating relay name={} host={}:{} auth={}",
        form.name, form.host, port, auth_type
    );

    let name = form.name.clone();
    let host = form.host.clone();
    let username = form
        .username
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let password = form
        .password
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    let result = state
        .blocking_db(move |db| {
            db.create_outbound_relay(
                &name,
                &host,
                port,
                &auth_type,
                username.as_deref(),
                password.as_deref(),
            )
        })
        .await;

    match result {
        Ok(id) => {
            info!("[web] relay created id={}", id);
            regen_configs(&state).await;
            Redirect::to("/relays").into_response()
        }
        Err(e) => {
            error!("[web] failed to create relay: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "Relays",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: &e,
                back_url: "/relays/new",
                back_label: "Back",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn edit_form(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    debug!("[web] GET /relays/{}/edit — edit relay form", id);
    let relay = match state.blocking_db(move |db| db.get_outbound_relay(id)).await {
        Some(r) => r,
        None => {
            warn!("[web] relay id={} not found for edit", id);
            return Redirect::to("/relays").into_response();
        }
    };
    let assignments = state
        .blocking_db(move |db| db.list_relay_assignments(id))
        .await;
    let tmpl = EditTemplate {
        nav_active: "Relays",
        flash: None,
        relay,
        assignments,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn update(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<RelayEditForm>,
) -> Response {
    let port = form.port.unwrap_or(587);
    let active = form.active.is_some();
    let auth_type = if form.auth_type.is_empty() {
        "none".to_string()
    } else {
        form.auth_type.clone()
    };
    info!(
        "[web] POST /relays/{} — updating relay name={} host={}:{} auth={} active={}",
        id, form.name, form.host, port, auth_type, active
    );

    let name = form.name.clone();
    let host = form.host.clone();
    let username = form
        .username
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string);
    let new_password = form
        .password
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(str::to_string);

    state
        .blocking_db(move |db| {
            // Preserve the existing password when the form field is left blank
            let final_password: Option<String> = if new_password.is_some() {
                new_password
            } else {
                db.get_outbound_relay(id).and_then(|r| r.password)
            };
            db.update_outbound_relay(
                id,
                &name,
                &host,
                port,
                &auth_type,
                username.as_deref(),
                final_password.as_deref(),
                active,
            )
        })
        .await;

    regen_configs(&state).await;
    Redirect::to(&format!("/relays/{}/edit", id)).into_response()
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /relays/{}/delete — deleting relay", id);
    state
        .blocking_db(move |db| db.delete_outbound_relay(id))
        .await;
    regen_configs(&state).await;
    Redirect::to("/relays").into_response()
}

pub async fn add_assignment(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<RelayAssignmentForm>,
) -> Response {
    info!(
        "[web] POST /relays/{}/assignments — adding assignment type={} pattern={}",
        id, form.assignment_type, form.pattern
    );

    let assignment_type = form.assignment_type.clone();
    let pattern = form.pattern.trim().to_string();

    let result = state
        .blocking_db(move |db| db.create_relay_assignment(id, &assignment_type, &pattern))
        .await;

    match result {
        Ok(_) => {
            regen_configs(&state).await;
        }
        Err(e) => {
            error!("[web] failed to create relay assignment: {}", e);
        }
    }
    Redirect::to(&format!("/relays/{}/edit", id)).into_response()
}

pub async fn remove_assignment(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path((relay_id, assignment_id)): Path<(i64, i64)>,
) -> Response {
    warn!(
        "[web] POST /relays/{}/assignments/{}/delete — removing assignment",
        relay_id, assignment_id
    );
    state
        .blocking_db(move |db| db.delete_relay_assignment(assignment_id))
        .await;
    regen_configs(&state).await;
    Redirect::to(&format!("/relays/{}/edit", relay_id)).into_response()
}
