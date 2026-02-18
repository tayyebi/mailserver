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
    status_code: u16,
    status_text: &'a str,
    title: &'a str,
    message: &'a str,
    back_url: &'a str,
    back_label: &'a str,
}

// ── Handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /aliases — listing aliases");
    let aliases = state
        .blocking_db(|db| db.list_all_aliases_with_domain())
        .await;
    debug!("[web] found {} aliases", aliases.len());
    let domains = state.blocking_db(|db| db.list_domains()).await;

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

pub async fn new_form(_auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    debug!("[web] GET /aliases/new — new alias form");
    let tmpl = NewTemplate {
        nav_active: "Aliases",
        flash: None,
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
    
    // Extract domain from source email
    let source_parts: Vec<&str> = form.source.split('@').collect();
    if source_parts.len() != 2 {
        warn!(
            "[web] invalid source email format (no @ or multiple @): {}",
            form.source
        );
        let tmpl = ErrorTemplate {
            nav_active: "Aliases",
            flash: None,
            status_code: 400,
            status_text: "Invalid Source Email",
            title: "Invalid Source Email",
            message: &format!(
                "The source email '{}' is not in a valid format. It must be in the format 'user@domain.com' or '*@domain.com'.",
                form.source
            ),
            back_url: "/aliases/new",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    
    let source_domain = source_parts[1].to_ascii_lowercase();
    
    // Validate that the domain exists in registered domains
    let domain_name_check = source_domain.clone();
    let domain_opt = state
        .blocking_db(move |db| db.get_domain_by_name(&domain_name_check))
        .await;
    
    let domain = match domain_opt {
        Some(d) => d,
        None => {
            warn!(
                "[web] attempted to create alias with unregistered domain: {}",
                source_domain
            );
            let tmpl = ErrorTemplate {
                nav_active: "Aliases",
                flash: None,
                status_code: 400,
                status_text: "Unregistered Domain",
                title: "Unregistered Domain",
                message: &format!(
                    "The domain '{}' extracted from the source email '{}' is not registered. Please add the domain first in the Domains section.",
                    source_domain, form.source
                ),
                back_url: "/aliases/new",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };
    
    let domain_id = domain.id;
    
    // Validate that destination account exists
    let destination_check = form.destination.clone();
    let destination_exists = state
        .blocking_db(move |db| db.email_exists(&destination_check))
        .await;
    
    if !destination_exists {
        warn!(
            "[web] attempted to create alias to non-existent destination: {}",
            form.destination
        );
        let tmpl = ErrorTemplate {
            nav_active: "Aliases",
            flash: None,
            status_code: 400,
            status_text: "Invalid Destination",
            title: "Invalid Destination",
            message: &format!(
                "The destination email '{}' does not exist. Please create the account first in the Accounts section.",
                form.destination
            ),
            back_url: "/aliases/new",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    
    let source = form.source.clone();
    let destination = form.destination.clone();
    let create_result = state
        .blocking_db(move |db| {
            db.create_alias(domain_id, &source, &destination, tracking, sort_order)
        })
        .await;
    match create_result {
        Ok(id) => {
            info!(
                "[web] alias created successfully: {} -> {} (id={}, domain_id={})",
                form.source, form.destination, id, domain_id
            );
            regen_configs(&state).await;
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
                status_code: 500,
                status_text: "Error",
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
    let alias = match state.blocking_db(move |db| db.get_alias(id)).await {
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
    let source = form.source.clone();
    let destination = form.destination.clone();
    state
        .blocking_db(move |db| {
            db.update_alias(id, &source, &destination, active, tracking, sort_order)
        })
        .await;
    regen_configs(&state).await;
    Redirect::to("/aliases").into_response()
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /aliases/{}/delete — deleting alias", id);
    state.blocking_db(move |db| db.delete_alias(id)).await;
    regen_configs(&state).await;
    Redirect::to("/aliases").into_response()
}
