use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};
use serde::Deserialize;

use crate::db::Account;
use crate::web::auth::AuthAdmin;
use crate::web::fire_webhook;
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

// ── DNS check structures ──

struct SpfRecord {
    domain: String,
    raw: String,
    depth: usize,
}

struct DnsCheckResult {
    resolved_ip: String,
    ptr_hostname: String,
    ptr_matches: bool,
    ptr_status: String,
    spf_chain: Vec<SpfRecord>,
    spf_error: String,
}

// ── DNS helpers ──

const MAX_SPF_RECURSION: usize = 10;

fn query_txt_records(domain: &str) -> Vec<String> {
    let output = match std::process::Command::new("nslookup")
        .args(["-type=TXT", domain])
        .output()
    {
        Ok(o) => o,
        Err(_) => return vec![],
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut results = Vec::new();
    for line in stdout.lines() {
        let line = line.trim();
        if line.contains("text = ") {
            // Extract content between outermost quotes
            if let Some(start) = line.find('"') {
                let rest = &line[start + 1..];
                if let Some(end) = rest.rfind('"') {
                    results.push(rest[..end].to_string());
                }
            }
        }
    }
    results
}

fn query_ptr_record(ip: &str) -> Option<String> {
    let output = std::process::Command::new("nslookup")
        .arg(ip)
        .output()
        .ok()?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let line = line.trim();
        if let Some(pos) = line.find("name = ") {
            let name = line[pos + 7..].trim().trim_end_matches('.');
            if !name.is_empty() {
                return Some(name.to_string());
            }
        }
    }
    None
}

fn spf_chain_recursive(domain: &str, depth: usize) -> Vec<SpfRecord> {
    if depth >= MAX_SPF_RECURSION {
        return vec![];
    }
    let txts = query_txt_records(domain);
    let spf_raw = match txts.into_iter().find(|t| t.starts_with("v=spf1")) {
        Some(r) => r,
        None => return vec![],
    };

    let mut result = vec![SpfRecord {
        domain: domain.to_string(),
        raw: spf_raw.clone(),
        depth,
    }];

    for token in spf_raw.split_whitespace().skip(1) {
        if let Some(inc) = token.strip_prefix("include:") {
            result.extend(spf_chain_recursive(inc, depth + 1));
        } else if let Some(redir) = token.strip_prefix("redirect=") {
            result.extend(spf_chain_recursive(redir, depth + 1));
        }
    }
    result
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

/// View-model for the DNS runbook page.
///
/// `dmarc_rua` and `dmarc_ruf` are the fully-qualified RFC 5321 mailbox addresses
/// (`local-part@domain`, §4.1.2) that will be embedded as `mailto:` URIs in the
/// `_dmarc` TXT record (RFC 7489 §6.3).  When `None` the record falls back to
/// `postmaster@<domain>` (RFC 5321 §4.5.1).
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
    bimi_logo_url: String,
    has_bimi: bool,
    /// `rua=mailto:<rua>` aggregate-report destination (RFC 7489 §6.3).
    dmarc_rua: Option<String>,
    /// `ruf=mailto:<ruf>` failure-report destination (RFC 7489 §6.3 / RFC 6591).
    dmarc_ruf: Option<String>,
    dmarc_inbox: Option<crate::db::DmarcInbox>,
    domain_accounts: Vec<Account>,
}

#[derive(Deserialize)]
pub struct DnsCheckQuery {
    #[serde(rename = "type")]
    pub check_type: Option<String>,
}

#[derive(Deserialize)]
pub struct SetDmarcForm {
    pub account_id: i64,
}

#[derive(Template)]
#[template(path = "domains/check.html")]
struct DnsCheckTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domain_id: i64,
    domain_name: String,
    hostname: &'a str,
    check_type: String,
    dns_check: DnsCheckResult,
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
    let bimi_svg = form.bimi_svg.clone();
    let unsubscribe_enabled = form.unsubscribe_enabled.is_some();
    let create_result = state
        .blocking_db(move |db| {
            db.create_domain(&domain, &footer_html, &bimi_svg, unsubscribe_enabled)
        })
        .await;
    match create_result {
        Ok(id) => {
            info!(
                "[web] domain created successfully: {} (id={})",
                form.domain, id
            );
            regen_configs(&state).await;
            fire_webhook(
                &state,
                "domain.created",
                serde_json::json!({"domain": form.domain}),
            );
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
    let bimi_svg = form.bimi_svg.clone();
    let unsubscribe_enabled = form.unsubscribe_enabled.is_some();
    state
        .blocking_db(move |db| {
            db.update_domain(
                id,
                &domain,
                active,
                &footer_html,
                &bimi_svg,
                unsubscribe_enabled,
            )
        })
        .await;
    regen_configs(&state).await;
    fire_webhook(
        &state,
        "domain.updated",
        serde_json::json!({"id": id, "domain": form.domain}),
    );
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
    fire_webhook(&state, "domain.deleted", serde_json::json!({"id": id}));
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
    fire_webhook(
        &state,
        "domain.dkim_generated",
        serde_json::json!({"id": id}),
    );
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

    let has_bimi = domain
        .bimi_svg
        .as_ref()
        .map(|s| !s.trim().is_empty())
        .unwrap_or(false);
    let bimi_logo_url = format!("https://{}/bimi/{}/logo.svg", state.hostname, domain.domain);

    let domain_id_copy = domain.id;
    let dmarc_inbox = state
        .blocking_db(move |db| db.get_dmarc_inbox_by_domain_id(domain_id_copy))
        .await;
    let dmarc_rua = dmarc_inbox.as_ref().and_then(|inbox| {
        let username = inbox.account_username.as_ref()?;
        let dom = inbox.account_domain.as_ref()?;
        Some(format!("{}@{}", username, dom))
    });
    let dmarc_ruf = dmarc_inbox.as_ref().and_then(|inbox| {
        let username = inbox.ruf_account_username.as_ref()?;
        let dom = inbox.ruf_account_domain.as_ref()?;
        Some(format!("{}@{}", username, dom))
    });
    let domain_id_for_accounts = domain.id;
    let domain_accounts = state
        .blocking_db(move |db| db.list_accounts_by_domain(domain_id_for_accounts))
        .await;

    let tmpl = DnsTemplate {
        nav_active: "Domains",
        flash: None,
        domain_id: domain.id,
        domain_name: domain.domain.clone(),
        dkim_selector: domain.dkim_selector.clone(),
        hostname: &state.hostname,
        dkim_record,
        bimi_logo_url,
        has_bimi,
        dmarc_rua,
        dmarc_ruf,
        dmarc_inbox,
        domain_accounts,
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn set_dmarc_inbox(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<SetDmarcForm>,
) -> Response {
    info!(
        "[web] POST /domains/{}/dmarc — setting DMARC inbox account_id={}",
        id, form.account_id
    );
    let account_id = form.account_id;
    let existing = state
        .blocking_db(move |db| db.get_dmarc_inbox_by_domain_id(id))
        .await;
    if let Some(existing_inbox) = existing {
        let existing_id = existing_inbox.id;
        state
            .blocking_db(move |db| db.delete_dmarc_inbox(existing_id))
            .await;
    }
    let _ = state
        .blocking_db(move |db| db.create_dmarc_inbox(account_id, ""))
        .await;
    Redirect::to(&format!("/domains/{}/dns", id)).into_response()
}

pub async fn remove_dmarc_inbox(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!(
        "[web] POST /domains/{}/dmarc/delete — removing DMARC inbox",
        id
    );
    let existing = state
        .blocking_db(move |db| db.get_dmarc_inbox_by_domain_id(id))
        .await;
    if let Some(existing_inbox) = existing {
        let existing_id = existing_inbox.id;
        state
            .blocking_db(move |db| db.delete_dmarc_inbox(existing_id))
            .await;
    }
    Redirect::to(&format!("/domains/{}/dns", id)).into_response()
}

/// Set the `ruf` (failure report) inbox for a domain.
///
/// Updates the `ruf=` tag in the generated `_dmarc` TXT record.  DMARC failure reports
/// (RFC 7489 §7.3) are forensic per-message reports delivered by the receiving MTA to the
/// `ruf=mailto:<address>` URI over SMTP (RFC 5321 §3.1).  The address must be an RFC 5321
/// mailbox (`local-part@domain`, §4.1.2) reachable via standard SMTP delivery (RFC 5321 §5).
/// Reports are formatted as ARF messages per RFC 6591 / RFC 5965.
pub async fn set_dmarc_ruf_inbox(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<SetDmarcForm>,
) -> Response {
    info!(
        "[web] POST /domains/{}/dmarc/ruf — setting DMARC ruf inbox account_id={}",
        id, form.account_id
    );
    let account_id = form.account_id;
    let existing = state
        .blocking_db(move |db| db.get_dmarc_inbox_by_domain_id(id))
        .await;
    if let Some(existing_inbox) = existing {
        let existing_id = existing_inbox.id;
        state
            .blocking_db(move |db| db.set_dmarc_inbox_ruf(existing_id, Some(account_id)))
            .await;
    }
    Redirect::to(&format!("/domains/{}/dns", id)).into_response()
}

/// Remove the `ruf` failure-report inbox for a domain.
///
/// Clears the explicit `ruf=` inbox so the generated `_dmarc` TXT record reverts to the
/// required RFC 5321 fallback address `postmaster@<domain>` (RFC 5321 §4.5.1), which every
/// conforming SMTP server must accept.
pub async fn remove_dmarc_ruf_inbox(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!(
        "[web] POST /domains/{}/dmarc/ruf/delete — removing DMARC ruf inbox",
        id
    );
    let existing = state
        .blocking_db(move |db| db.get_dmarc_inbox_by_domain_id(id))
        .await;
    if let Some(existing_inbox) = existing {
        let existing_id = existing_inbox.id;
        state
            .blocking_db(move |db| db.set_dmarc_inbox_ruf(existing_id, None))
            .await;
    }
    Redirect::to(&format!("/domains/{}/dns", id)).into_response()
}

pub async fn dns_check_run(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Query(query): Query<DnsCheckQuery>,
) -> Response {
    let check_type = query.check_type.as_deref().unwrap_or("ptr").to_lowercase();
    debug!(
        "[web] GET /domains/{}/check?type={} — running DNS check",
        id, check_type
    );

    let domain = match state.blocking_db(move |db| db.get_domain(id)).await {
        Some(d) => d,
        None => {
            warn!("[web] domain id={} not found for DNS check", id);
            return Redirect::to("/domains").into_response();
        }
    };

    let dns_check = match check_type.as_str() {
        "spf" => {
            let spf_chain = spf_chain_recursive(&domain.domain, 0);
            let spf_error = if spf_chain.is_empty() {
                format!("No SPF record found for {}", domain.domain)
            } else {
                String::new()
            };
            DnsCheckResult {
                resolved_ip: String::new(),
                ptr_hostname: String::new(),
                ptr_matches: false,
                ptr_status: String::new(),
                spf_chain,
                spf_error,
            }
        }
        _ => {
            // Default: PTR
            let host_socket_addr = format!("{}:0", state.hostname);
            let resolved_ip = host_socket_addr
                .parse::<std::net::SocketAddr>()
                .map(|a| a.ip().to_string())
                .unwrap_or_else(|_| {
                    use std::net::ToSocketAddrs;
                    host_socket_addr
                        .to_socket_addrs()
                        .ok()
                        .and_then(|mut it| it.next())
                        .map(|a| a.ip().to_string())
                        .unwrap_or_default()
                });
            let (ptr_hostname, ptr_matches, ptr_status) = if resolved_ip.is_empty() {
                (
                    String::new(),
                    false,
                    "Could not resolve hostname to IP".to_string(),
                )
            } else {
                match query_ptr_record(&resolved_ip) {
                    Some(ptr) => {
                        let matches = ptr.eq_ignore_ascii_case(&state.hostname);
                        let status = if matches {
                            format!("OK — {} → {}", resolved_ip, ptr)
                        } else {
                            format!(
                                "Mismatch — PTR is \"{}\", expected \"{}\"",
                                ptr, state.hostname
                            )
                        };
                        (ptr, matches, status)
                    }
                    None => (
                        String::new(),
                        false,
                        format!("No PTR record for {}", resolved_ip),
                    ),
                }
            };
            DnsCheckResult {
                resolved_ip,
                ptr_hostname,
                ptr_matches,
                ptr_status,
                spf_chain: Vec::new(),
                spf_error: String::new(),
            }
        }
    };

    let tmpl = DnsCheckTemplate {
        nav_active: "Domains",
        flash: None,
        domain_id: domain.id,
        domain_name: domain.domain.clone(),
        hostname: &state.hostname,
        check_type,
        dns_check,
    };
    Html(tmpl.render().unwrap()).into_response()
}
