use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::web::auth::AuthAdmin;
use crate::web::forms::{DomainEditForm, DomainForm};
use crate::web::regen_configs;
use crate::web::AppState;

// ── View models ──

struct DomainRow {
    id: i64,
    domain: String,
    active_label: String,
    dkim_label: String,
    footer_label: String,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "domains/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domain_rows: Vec<DomainRow>,
}

#[derive(Template)]
#[template(path = "domains/new.html")]
struct NewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
}

#[derive(Template)]
#[template(path = "domains/edit.html")]
struct EditTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domain: crate::db::Domain,
}

#[derive(Template)]
#[template(path = "domains/dns.html")]
struct DnsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domain_id: i64,
    domain_name: String,
    dkim_selector: String,
    hostname: &'a str,
    dkim_record: String,
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
    info!("[web] GET /domains — listing domains");
    let domains = state.blocking_db(|db| db.list_domains()).await;
    debug!("[web] found {} domains", domains.len());

    let domain_rows: Vec<DomainRow> = domains
        .iter()
        .map(|d| DomainRow {
            id: d.id,
            domain: d.domain.clone(),
            active_label: if d.active { "Yes" } else { "No" }.to_string(),
            dkim_label: if d.dkim_public_key.is_some() {
                "Yes"
            } else {
                "No"
            }
            .to_string(),
            footer_label: match d.footer_html.as_deref() {
                Some(html) if !html.trim().is_empty() => "Yes",
                _ => "No",
            }
            .to_string(),
        })
        .collect();

    let tmpl = ListTemplate {
        nav_active: "Domains",
        flash: None,
        domain_rows,
    };
    Html(tmpl.render().unwrap())
}

pub async fn new_form(_auth: AuthAdmin) -> Html<String> {
    debug!("[web] GET /domains/new — new domain form");
    let tmpl = NewTemplate {
        nav_active: "Domains",
        flash: None,
    };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<DomainForm>,
) -> Response {
    info!("[web] POST /domains — creating domain={}", form.domain);
    let domain = form.domain.clone();
    let footer_html = form.footer_html.clone();
    let create_result = state
        .blocking_db(move |db| db.create_domain(&domain, &footer_html))
        .await;
    match create_result {
        Ok(id) => {
            info!(
                "[web] domain created successfully: {} (id={})",
                form.domain, id
            );
            regen_configs(&state).await;
            Redirect::to("/domains").into_response()
        }
        Err(e) => {
            error!("[web] failed to create domain {}: {}", form.domain, e);
            let tmpl = ErrorTemplate {
                nav_active: "Domains",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: &e,
                back_url: "/domains/new",
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
    debug!("[web] GET /domains/{}/edit — edit domain form", id);
    let domain = match state.blocking_db(move |db| db.get_domain(id)).await {
        Some(d) => d,
        None => {
            warn!("[web] domain id={} not found for edit", id);
            return Redirect::to("/domains").into_response();
        }
    };
    let tmpl = EditTemplate {
        nav_active: "Domains",
        flash: None,
        domain,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn update(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<DomainEditForm>,
) -> Response {
    let active = form.active.is_some();
    info!(
        "[web] POST /domains/{} — updating domain={}, active={}",
        id, form.domain, active
    );
    let domain = form.domain.clone();
    let footer_html = form.footer_html.clone();
    state
        .blocking_db(move |db| db.update_domain(id, &domain, active, &footer_html))
        .await;
    regen_configs(&state).await;
    Redirect::to("/domains").into_response()
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /domains/{}/delete — deleting domain", id);
    state.blocking_db(move |db| db.delete_domain(id)).await;
    regen_configs(&state).await;
    Redirect::to("/domains").into_response()
}

pub async fn generate_dkim(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!("[web] POST /domains/{}/dkim — generating DKIM keys", id);
    let domain = match state.blocking_db(move |db| db.get_domain(id)).await {
        Some(d) => d,
        None => {
            warn!("[web] domain id={} not found for DKIM generation", id);
            return Redirect::to("/domains").into_response();
        }
    };

    debug!(
        "[web] generating RSA 2048 private key for domain={}",
        domain.domain
    );
    let priv_output = std::process::Command::new("openssl")
        .args(["genrsa", "2048"])
        .output();
    let private_key = match priv_output {
        Ok(o) if o.status.success() => {
            debug!(
                "[web] DKIM private key generated for domain={}",
                domain.domain
            );
            String::from_utf8_lossy(&o.stdout).to_string()
        }
        Ok(o) => {
            error!(
                "[web] openssl genrsa failed for domain={}: {}",
                domain.domain,
                String::from_utf8_lossy(&o.stderr)
            );
            let tmpl = ErrorTemplate {
                nav_active: "Domains",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: "Failed to generate DKIM private key.",
                back_url: "/domains",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
        Err(e) => {
            error!(
                "[web] failed to run openssl genrsa for domain={}: {}",
                domain.domain, e
            );
            let tmpl = ErrorTemplate {
                nav_active: "Domains",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: "Failed to generate DKIM private key.",
                back_url: "/domains",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };

    debug!("[web] extracting public key for domain={}", domain.domain);
    let pub_output = std::process::Command::new("openssl")
        .args(["rsa", "-pubout"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(private_key.as_bytes()).ok();
            }
            child.wait_with_output()
        });
    let public_key = match pub_output {
        Ok(o) if o.status.success() => {
            debug!(
                "[web] DKIM public key extracted for domain={}",
                domain.domain
            );
            String::from_utf8_lossy(&o.stdout).to_string()
        }
        Ok(o) => {
            error!(
                "[web] openssl rsa -pubout failed for domain={}: {}",
                domain.domain,
                String::from_utf8_lossy(&o.stderr)
            );
            let tmpl = ErrorTemplate {
                nav_active: "Domains",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: "Failed to extract DKIM public key.",
                back_url: "/domains",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
        Err(e) => {
            error!(
                "[web] failed to run openssl rsa -pubout for domain={}: {}",
                domain.domain, e
            );
            let tmpl = ErrorTemplate {
                nav_active: "Domains",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: "Failed to extract DKIM public key.",
                back_url: "/domains",
                back_label: "Back",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };

    info!(
        "[web] DKIM keys generated successfully for domain={}",
        domain.domain
    );
    let selector = domain.dkim_selector.clone();
    state
        .blocking_db(move |db| db.update_domain_dkim(id, &selector, &private_key, &public_key))
        .await;
    regen_configs(&state).await;
    Redirect::to(&format!("/domains/{}/dns", id)).into_response()
}

pub async fn dns_info(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    debug!("[web] GET /domains/{}/dns — DNS info requested", id);
    let domain = match state.blocking_db(move |db| db.get_domain(id)).await {
        Some(d) => d,
        None => {
            warn!("[web] domain id={} not found for DNS info", id);
            return Redirect::to("/domains").into_response();
        }
    };

    let dkim_record = domain
        .dkim_public_key
        .as_ref()
        .map(|pub_key| {
            pub_key
                .lines()
                .filter(|l| !l.starts_with("-----"))
                .collect::<Vec<_>>()
                .join("")
        })
        .unwrap_or_default();

    let tmpl = DnsTemplate {
        nav_active: "Domains",
        flash: None,
        domain_id: domain.id,
        domain_name: domain.domain.clone(),
        dkim_selector: domain.dkim_selector.clone(),
        hostname: &state.hostname,
        dkim_record,
    };
    Html(tmpl.render().unwrap()).into_response()
}
