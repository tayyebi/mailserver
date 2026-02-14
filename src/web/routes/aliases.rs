use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};
use std::collections::HashMap;

use crate::web::auth::AuthAdmin;
use crate::web::forms::{AliasEditForm, AliasForm};
use crate::web::regen_configs;
use crate::web::AppState;

fn is_catch_all(source: &str, domain: Option<&str>) -> bool {
    let normalized = source.trim().to_ascii_lowercase();
    if normalized == "*" || normalized.starts_with("*@") {
        return true;
    }
    if let Some(domain) = domain {
        let d = domain.to_ascii_lowercase();
        if normalized == d || normalized == format!("@{}", d) {
            return true;
        }
    }
    false
}

// ── View models ──

struct AliasRow {
    id: i64,
    sort_order: i64,
    domain_name: String,
    source: String,
    destination: String,
    type_label: String,
    tracking_label: String,
    active_label: String,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "aliases/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    alias_rows: Vec<AliasRow>,
    coverage_copy: String,
    coverage_pct: f64,
}

#[derive(Template)]
#[template(path = "aliases/new.html")]
struct NewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domains: Vec<crate::db::Domain>,
}

#[derive(Template)]
#[template(path = "aliases/edit.html")]
struct EditTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    alias: crate::db::Alias,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    title: &'a str,
    message: &'a str,
    back_url: &'a str,
    back_label: &'a str,
}

// ── Handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /aliases — listing aliases");
    let aliases = state.db.list_all_aliases_with_domain();
    debug!("[web] found {} aliases", aliases.len());
    let domains = state.db.list_domains();

    let mut catch_ready: HashMap<i64, bool> = HashMap::new();
    for a in &aliases {
        if is_catch_all(&a.source, a.domain_name.as_deref()) && a.active {
            catch_ready.insert(a.domain_id, true);
        }
    }

    let domain_total = domains.len() as f64;
    let coverage_pct = if domain_total > 0.0 {
        (catch_ready.len() as f64 / domain_total * 100.0).round()
    } else {
        0.0
    };
    let coverage_copy = if domain_total > 0.0 {
        format!(
            "{} of {} domains have an active catch-all",
            catch_ready.len(),
            domains.len()
        )
    } else {
        "Add a domain to calculate catch-all coverage".to_string()
    };

    let alias_rows: Vec<AliasRow> = aliases
        .iter()
        .map(|a| {
            let is_catch = is_catch_all(&a.source, a.domain_name.as_deref());
            AliasRow {
                id: a.id,
                sort_order: a.sort_order,
                domain_name: a.domain_name.as_deref().unwrap_or("-").to_string(),
                source: a.source.clone(),
                destination: a.destination.clone(),
                type_label: if is_catch {
                    "Catch-all".to_string()
                } else {
                    "Targeted".to_string()
                },
                tracking_label: if a.tracking_enabled {
                    "On".to_string()
                } else {
                    "Off".to_string()
                },
                active_label: if a.active {
                    "Active".to_string()
                } else {
                    "Disabled".to_string()
                },
            }
        })
        .collect();

    let tmpl = ListTemplate {
        nav_active: "Aliases",
        flash: None,
        alias_rows,
        coverage_copy,
        coverage_pct,
    };
    Html(tmpl.render().unwrap())
}

pub async fn new_form(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    debug!("[web] GET /aliases/new — new alias form");
    let domains = state.db.list_domains();
    let tmpl = NewTemplate {
        nav_active: "Aliases",
        flash: None,
        domains,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<AliasForm>,
) -> Response {
    let tracking = form.tracking_enabled.is_some();
    let sort_order = form.sort_order.unwrap_or(0);
    info!("[web] POST /aliases — creating alias source={}, destination={}, tracking={}, sort_order={}",
        form.source, form.destination, tracking, sort_order);
    match state.db.create_alias(
        form.domain_id,
        &form.source,
        &form.destination,
        tracking,
        sort_order,
    ) {
        Ok(id) => {
            info!(
                "[web] alias created successfully: {} -> {} (id={})",
                form.source, form.destination, id
            );
            regen_configs(&state);
            Redirect::to("/aliases").into_response()
        }
        Err(e) => {
            error!(
                "[web] failed to create alias {} -> {}: {}",
                form.source, form.destination, e
            );
            let tmpl = ErrorTemplate {
                nav_active: "Aliases",
                flash: None,
                title: "Error",
                message: &e,
                back_url: "/aliases/new",
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
    debug!("[web] GET /aliases/{}/edit — edit alias form", id);
    let alias = match state.db.get_alias(id) {
        Some(a) => a,
        None => {
            warn!("[web] alias id={} not found for edit", id);
            return Redirect::to("/aliases").into_response();
        }
    };
    let tmpl = EditTemplate {
        nav_active: "Aliases",
        flash: None,
        alias,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn update(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<AliasEditForm>,
) -> Response {
    let active = form.active.is_some();
    let tracking = form.tracking_enabled.is_some();
    let sort_order = form.sort_order.unwrap_or(0);
    info!("[web] POST /aliases/{} — updating alias source={}, destination={}, active={}, tracking={}, sort_order={}",
        id, form.source, form.destination, active, tracking, sort_order);
    state.db.update_alias(
        id,
        &form.source,
        &form.destination,
        active,
        tracking,
        sort_order,
    );
    regen_configs(&state);
    Redirect::to("/aliases").into_response()
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /aliases/{}/delete — deleting alias", id);
    state.db.delete_alias(id);
    regen_configs(&state);
    Redirect::to("/aliases").into_response()
}
