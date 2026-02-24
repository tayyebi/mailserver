use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::web::auth::AuthAdmin;
use crate::web::fire_webhook;
use crate::web::forms::{ForwardingEditForm, ForwardingForm};
use crate::web::regen_configs;
use crate::web::AppState;

// ── Templates ──

#[derive(Template)]
#[template(path = "forwarding/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    forwardings: Vec<crate::db::Forwarding>,
}

#[derive(Template)]
#[template(path = "forwarding/new.html")]
struct NewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "forwarding/edit.html")]
struct EditTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    forwarding: crate::db::Forwarding,
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
    info!("[web] GET /forwarding — listing forwardings");
    let forwardings = state
        .blocking_db(|db| db.list_all_forwardings_with_domain())
        .await;
    debug!("[web] found {} forwardings", forwardings.len());
    let tmpl = ListTemplate {
        nav_active: "Forwarding",
        flash: None,
        forwardings,
    };
    Html(tmpl.render().unwrap())
}

pub async fn new_form(_auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    debug!("[web] GET /forwarding/new — new forwarding form");
    let tmpl = NewTemplate {
        nav_active: "Forwarding",
        flash: None,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<ForwardingForm>,
) -> Response {
    let keep_copy = form.keep_copy.is_some();
    info!(
        "[web] POST /forwarding — creating forwarding source={}, destination={}, keep_copy={}",
        form.source, form.destination, keep_copy
    );

    // Extract domain from source email
    let source_parts: Vec<&str> = form.source.split('@').collect();
    if source_parts.len() != 2 {
        warn!("[web] invalid source email format: {}", form.source);
        let tmpl = ErrorTemplate {
            nav_active: "Forwarding",
            flash: None,
            status_code: 400,
            status_text: "Invalid Source Email",
            title: "Invalid Source Email",
            message: &format!(
                "The source email '{}' is not valid. Use the format 'user@domain.com'.",
                form.source
            ),
            back_url: "/forwarding/new",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }

    let source_domain = source_parts[1].to_ascii_lowercase();
    let domain_check = source_domain.clone();
    let domain_opt = state
        .blocking_db(move |db| db.get_domain_by_name(&domain_check))
        .await;

    let domain = match domain_opt {
        Some(d) => d,
        None => {
            warn!(
                "[web] attempted to create forwarding with unregistered domain: {}",
                source_domain
            );
            let tmpl = ErrorTemplate {
                nav_active: "Forwarding",
                flash: None,
                status_code: 400,
                status_text: "Unregistered Domain",
                title: "Unregistered Domain",
                message: &format!(
                    "The domain '{}' is not registered. Please add it in the Domains section first.",
                    source_domain
                ),
                back_url: "/forwarding/new",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };

    let domain_id = domain.id;
    let source = form.source.clone();
    let destination = form.destination.clone();
    let create_result = state
        .blocking_db(move |db| db.create_forwarding(domain_id, &source, &destination, keep_copy))
        .await;

    match create_result {
        Ok(id) => {
            info!(
                "[web] forwarding created: {} -> {} (id={}, keep_copy={})",
                form.source, form.destination, id, keep_copy
            );
            regen_configs(&state).await;
            fire_webhook(
                &state,
                "forwarding.created",
                serde_json::json!({"source": form.source, "destination": form.destination}),
            );
            Redirect::to("/forwarding").into_response()
        }
        Err(e) => {
            error!(
                "[web] failed to create forwarding {} -> {}: {}",
                form.source, form.destination, e
            );
            let tmpl = ErrorTemplate {
                nav_active: "Forwarding",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: &e,
                back_url: "/forwarding/new",
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
    debug!("[web] GET /forwarding/{}/edit — edit forwarding form", id);
    let forwarding = match state.blocking_db(move |db| db.get_forwarding(id)).await {
        Some(f) => f,
        None => {
            warn!("[web] forwarding id={} not found for edit", id);
            return Redirect::to("/forwarding").into_response();
        }
    };
    let tmpl = EditTemplate {
        nav_active: "Forwarding",
        flash: None,
        forwarding,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn update(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<ForwardingEditForm>,
) -> Response {
    let active = form.active.is_some();
    let keep_copy = form.keep_copy.is_some();
    info!(
        "[web] POST /forwarding/{} — updating forwarding source={}, destination={}, active={}, keep_copy={}",
        id, form.source, form.destination, active, keep_copy
    );
    let source = form.source.clone();
    let destination = form.destination.clone();
    state
        .blocking_db(move |db| db.update_forwarding(id, &source, &destination, active, keep_copy))
        .await;
    regen_configs(&state).await;
    fire_webhook(&state, "forwarding.updated", serde_json::json!({"id": id}));
    Redirect::to("/forwarding").into_response()
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /forwarding/{}/delete — deleting forwarding", id);
    state.blocking_db(move |db| db.delete_forwarding(id)).await;
    regen_configs(&state).await;
    fire_webhook(&state, "forwarding.deleted", serde_json::json!({"id": id}));
    Redirect::to("/forwarding").into_response()
}
