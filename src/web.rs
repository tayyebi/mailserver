use axum::{
    extract::{FromRef, FromRequestParts, Path, Query, State},
    http::{header, request::Parts, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use log::{info, warn, error, debug};
use std::collections::HashMap;
use serde::Deserialize;
use tower_http::services::ServeDir;

// ── Shared State ──

#[derive(Clone)]
pub struct AppState {
    pub db: crate::db::Database,
    pub hostname: String,
    pub admin_port: u16,
}

// ── HTML Helpers ──

fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
}

fn layout(title: &str, nav_active: &str, content: &str, flash: Option<&str>) -> String {
    let nav_items = [
        ("Dashboard", "/"),
        ("Domains", "/domains"),
        ("Accounts", "/accounts"),
        ("Aliases", "/aliases"),
        ("Tracking", "/tracking"),
        ("Settings", "/settings"),
    ];
    let mut nav_html = String::new();
    for (label, href) in &nav_items {
        let aria = if *label == nav_active { " aria-current=\"page\"" } else { "" };
        nav_html.push_str(&format!("<a href=\"{}\"{}>{}</a>", esc(href), aria, esc(label)));
    }
    let flash_html = match flash {
        Some(msg) => format!("<output>{}</output>", esc(msg)),
        None => String::new(),
    };
    format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="/static/style.css">
<link rel="stylesheet" href="/static/desktop.css" media="(min-width: 768px)">
</head>
<body>
<header>
  <strong>Mailserver</strong>
  <small>Control Panel</small>
  <nav>{nav_html}</nav>
  <mark>Online</mark>
</header>
<main>
{flash_html}
{content}
</main>
<footer><small>Mailserver Admin</small></footer>
</body>
</html>"#,
        title = esc(title),
        nav_html = nav_html,
        flash_html = flash_html,
        content = content,
    )
}

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

// ── Basic Auth Extractor ──

struct AuthAdmin {
    admin: crate::db::Admin,
}

fn unauthorized() -> Response {
    warn!("[web] unauthorized access attempt");
    (
        StatusCode::UNAUTHORIZED,
        [(header::WWW_AUTHENTICATE, "Basic realm=\"Mailserver Admin\"")],
        "Unauthorized",
    )
        .into_response()
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for AuthAdmin
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        debug!("[web] authenticating request to {}", parts.uri);

        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                warn!("[web] missing Authorization header for {}", parts.uri);
                unauthorized()
            })?;

        if !auth_header.starts_with("Basic ") {
            warn!("[web] invalid Authorization scheme for {}", parts.uri);
            return Err(unauthorized());
        }

        let decoded = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &auth_header[6..],
        )
        .map_err(|_| {
            warn!("[web] failed to decode base64 credentials for {}", parts.uri);
            unauthorized()
        })?;
        let credentials = String::from_utf8(decoded).map_err(|_| {
            warn!("[web] invalid UTF-8 in credentials for {}", parts.uri);
            unauthorized()
        })?;
        let (username, password) = credentials.split_once(':').ok_or_else(|| {
            warn!("[web] malformed credentials (no colon) for {}", parts.uri);
            unauthorized()
        })?;

        debug!("[web] auth attempt for username={}", username);

        let admin = app_state
            .db
            .get_admin_by_username(username)
            .ok_or_else(|| {
                warn!("[web] authentication failed — unknown username={}", username);
                unauthorized()
            })?;

        if admin.totp_enabled {
            debug!("[web] TOTP enabled for username={}, verifying password+TOTP", username);
            if password.len() < 6 {
                warn!("[web] authentication failed — password too short for TOTP for username={}", username);
                return Err(unauthorized());
            }
            let (base_password, totp_code) = password.split_at(password.len() - 6);
            if !crate::auth::verify_password(base_password, &admin.password_hash) {
                warn!("[web] authentication failed — wrong password for username={}", username);
                return Err(unauthorized());
            }
            let secret = admin.totp_secret.as_deref().ok_or_else(|| {
                error!("[web] TOTP enabled but no secret stored for username={}", username);
                unauthorized()
            })?;
            if !crate::auth::verify_totp(secret, totp_code) {
                warn!("[web] authentication failed — invalid TOTP code for username={}", username);
                return Err(unauthorized());
            }
        } else if !crate::auth::verify_password(password, &admin.password_hash) {
            warn!("[web] authentication failed — wrong password for username={}", username);
            return Err(unauthorized());
        }

        info!("[web] authentication succeeded for username={}", username);
        Ok(AuthAdmin { admin })
    }
}

// ── Form Structs ──

#[derive(Deserialize)]
struct DomainForm {
    domain: String,
    #[serde(default)]
    footer_html: String,
}

#[derive(Deserialize)]
struct DomainEditForm {
    domain: String,
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    footer_html: String,
}

#[derive(Deserialize)]
struct AccountForm {
    domain_id: i64,
    username: String,
    password: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    quota: Option<i64>,
}

#[derive(Deserialize)]
struct AccountEditForm {
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    quota: Option<i64>,
}

#[derive(Deserialize)]
struct AliasForm {
    domain_id: i64,
    source: String,
    destination: String,
    #[serde(default)]
    tracking_enabled: Option<String>,
}

#[derive(Deserialize)]
struct AliasEditForm {
    source: String,
    destination: String,
    #[serde(default)]
    active: Option<String>,
    #[serde(default)]
    tracking_enabled: Option<String>,
}

#[derive(Deserialize)]
struct PasswordForm {
    current_password: String,
    new_password: String,
    confirm_password: String,
}

#[derive(Deserialize)]
struct TotpEnableForm {
    secret: String,
    code: String,
}

#[derive(Deserialize)]
struct PixelQuery {
    #[serde(default)]
    id: String,
}

// ── Server ──

pub async fn start_server(state: AppState) {
    let port = state.admin_port;

    info!("[web] initializing admin web server on port {}", port);

    let static_dir = if std::path::Path::new("/app/static").exists() {
        info!("[web] serving static files from /app/static");
        "/app/static"
    } else {
        info!("[web] serving static files from ./static");
        "./static"
    };

    let pixel_routes = Router::new().route("/pixel", get(pixel_handler));

    let auth_routes = Router::new()
        .route("/", get(dashboard))
        .route("/domains", get(list_domains).post(create_domain))
        .route("/domains/new", get(new_domain_form))
        .route("/domains/{id}/edit", get(edit_domain_form))
        .route("/domains/{id}", post(update_domain))
        .route("/domains/{id}/delete", post(delete_domain))
        .route("/domains/{id}/dkim", post(generate_dkim))
        .route("/domains/{id}/dns", get(dns_info))
        .route("/accounts", get(list_accounts).post(create_account))
        .route("/accounts/new", get(new_account_form))
        .route("/accounts/{id}/edit", get(edit_account_form))
        .route("/accounts/{id}", post(update_account))
        .route("/accounts/{id}/delete", post(delete_account))
        .route("/aliases", get(list_aliases).post(create_alias))
        .route("/aliases/new", get(new_alias_form))
        .route("/aliases/{id}/edit", get(edit_alias_form))
        .route("/aliases/{id}", post(update_alias))
        .route("/aliases/{id}/delete", post(delete_alias))
        .route("/tracking", get(list_tracking))
        .route("/tracking/{msg_id}", get(tracking_detail))
        .route("/settings", get(settings_page))
        .route("/settings/password", post(change_password))
        .route("/settings/2fa", get(setup_2fa))
        .route("/settings/2fa/enable", post(enable_2fa))
        .route("/settings/2fa/disable", post(disable_2fa));

    let app = Router::new()
        .merge(pixel_routes)
        .merge(auth_routes)
        .nest_service("/static", ServeDir::new(static_dir))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind address {}: {}", addr, e));
    info!("[web] admin dashboard listening on {}", addr);
    axum::serve(listener, app).await.expect("Server error");
}

fn regen_configs(state: &AppState) {
    info!("[web] regenerating mail service configs");
    crate::config::generate_all_configs(&state.db, &state.hostname);
}

// ── Dashboard ──

async fn dashboard(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET / — dashboard requested");
    let stats = state.db.get_stats();
        debug!(
                "[web] dashboard stats: domains={}, accounts={}, aliases={}, tracked={}, opens={}",
                stats.domain_count,
                stats.account_count,
                stats.alias_count,
                stats.tracked_count,
                stats.open_count
        );

        let domains = state.db.list_domains();
        let aliases = state.db.list_all_aliases_with_domain();
        let mut catch_all_map: HashMap<i64, &crate::db::Alias> = HashMap::new();
        for alias in &aliases {
                if is_catch_all(&alias.source, alias.domain_name.as_deref()) {
                        catch_all_map.insert(alias.domain_id, alias);
                }
        }

        let hero = format!(
                r#"<section>
    <hgroup>
        <small>Deliverability overview</small>
        <h1>Mailserver Control Panel</h1>
    </hgroup>
    <p>Track DNS, DKIM, and alias coverage for {host}. Catch issues before they page you.</p>
    <nav>
        <a href=\"/domains\"><strong>Manage domains</strong></a>
        <a href=\"/aliases\">Tune aliases</a>
    </nav>
    <dl>
        <dt>Hostname</dt><dd>{host}</dd>
        <dt>SMTP</dt><dd>25 / 587 / 465</dd>
        <dt>IMAP</dt><dd>143 / 993</dd>
        <dt>POP3</dt><dd>110 / 995</dd>
    </dl>
</section>"#,
                host = esc(&state.hostname),
        );

        let stat_definitions = [
                ("Domains", stats.domain_count, "Managed zones"),
                ("Accounts", stats.account_count, "Provisioned mailboxes"),
                ("Aliases", stats.alias_count, "Routing rules"),
                ("Tracked Messages", stats.tracked_count, "Pixels injected"),
                ("Pixel Opens", stats.open_count, "Engagement events"),
        ];
        let mut stat_cards = String::new();
        for (label, value, caption) in &stat_definitions {
                stat_cards.push_str(&format!(
                        "<article><data value=\"{}\">{}</data><strong>{}</strong><small>{}</small></article>",
                        value, value,
                        esc(label),
                        esc(caption)
                ));
        }
        let stats_section = format!(
                r#"<section>
    <hgroup>
        <small>Platform health</small>
        <h2>Live counters</h2>
    </hgroup>
    <details>
        <summary>How do these refresh?</summary>
        <p>Counts are queried live from SQLite each time you load the console.</p>
    </details>
    <div>{stat_cards}</div>
</section>"#,
                stat_cards = stat_cards,
        );

        let mut domain_cards = String::new();
        if domains.is_empty() {
                domain_cards.push_str(
                        "<p><em>No domains yet. Add your first domain to unlock DNS guidance.</em></p>"
                );
        } else {
                for domain in &domains {
                        let active_label = if domain.active { "Accepting mail" } else { "Suspended" };
                        let dkim_ready = domain.dkim_public_key.is_some();
                        let dkim_label = if dkim_ready {
                                format!("Selector {} ready", esc(&domain.dkim_selector))
                        } else {
                                "Missing DKIM key".to_string()
                        };
                        let catch_html = match catch_all_map.get(&domain.id) {
                                Some(alias) if alias.active => format!(
                                        "<mark>Catch-all → {}</mark>",
                                        esc(&alias.destination)
                                ),
                                Some(alias) => format!(
                                        "<mark>Catch-all disabled ({})</mark>",
                                        esc(&alias.destination)
                                ),
                                None => "<mark>No catch-all alias</mark>".to_string(),
                        };
                        let footer_label = match domain.footer_html.as_deref() {
                                Some(html) if !html.trim().is_empty() => "Footer injected",
                                _ => "No footer",
                        };
                        domain_cards.push_str(&format!(
                                r#"<article>
    <header>
        <h3>{domain}</h3>
        <mark>{active_label}</mark>
    </header>
    <dl>
        <dt>DKIM</dt><dd>{dkim_label}</dd>
        <dt>Catch-all</dt><dd>{catch_html}</dd>
        <dt>Footer</dt><dd>{footer_label}</dd>
    </dl>
    <nav>
        <a href=\"/domains/{id}/dns\"><small>DNS runbook</small></a>
        <a href=\"/aliases\"><small>Alias registry</small></a>
    </nav>
</article>"#,
                                domain = esc(&domain.domain),
                                active_label = active_label,
                                dkim_label = dkim_label,
                                catch_html = catch_html,
                                footer_label = footer_label,
                                id = domain.id,
                        ));
                }
        }
        let domains_section = format!(
                r#"<section>
    <hgroup>
        <small>DNS &amp; DKIM posture</small>
        <h2>Per-domain status</h2>
    </hgroup>
    <details>
        <summary>Why this matters</summary>
        <p>Missing DKIM keys or catch-all routing gaps often surface as delivery failures. Review each domain regularly.</p>
    </details>
    <div>{domain_cards}</div>
</section>"#,
                domain_cards = domain_cards,
        );

        let mut catch_rows = String::new();
        if domains.is_empty() {
                catch_rows.push_str("<tr><td colspan=\"4\">Add a domain to start tracking catch-all coverage.</td></tr>");
        } else {
                for domain in &domains {
                        let catch = catch_all_map.get(&domain.id);
                        let (status, destination) = match catch {
                                Some(alias) if alias.active => (
                                        "Protected",
                                        format!("{}", esc(&alias.destination)),
                                ),
                                Some(alias) => (
                                        "Disabled",
                                        format!("{}", esc(&alias.destination)),
                                ),
                        None => ("Uncovered", "&mdash;".to_string()),
                        };
                        catch_rows.push_str(&format!(
                                "<tr><td>{}</td><td><mark>{}</mark></td><td>{}</td><td><a href=\"/aliases\">Create / edit</a></td></tr>",
                                esc(&domain.domain),
                                status,
                                destination,
                        ));
                }
        }
        let alias_focus = format!(
		r#"<section>
    <hgroup>
        <small>Alias coverage</small>
        <h2>Catch-all readiness</h2>
    </hgroup>
    <details>
        <summary>How catch-all works</summary>
        <p>Define an alias like <code>*@example.com</code> to collect any recipient the directory does not know about.</p>
    </details>
    <table>
        <thead>
            <tr><th>Domain</th><th>Status</th><th>Route target</th><th>Action</th></tr>
        </thead>
        <tbody>{catch_rows}</tbody>
    </table>
</section>"#,
		catch_rows = catch_rows,
	);

        let content = format!("{hero}{stats}{domains}{aliases_doc}", hero = hero, stats = stats_section, domains = domains_section, aliases_doc = alias_focus);
    Html(layout("Dashboard", "Dashboard", &content, None))
}

// ── Domains ──

async fn list_domains(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /domains — listing domains");
    let domains = state.db.list_domains();
    debug!("[web] found {} domains", domains.len());
    let mut rows = String::new();
    for d in &domains {
        let footer_badge = match d.footer_html.as_deref() {
            Some(html) if !html.trim().is_empty() => "Yes",
            _ => "No",
        };
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>\
             <a href=\"/domains/{}/edit\">Edit</a> \
             <a href=\"/domains/{}/dns\">DNS</a> \
             <form method=\"post\" action=\"/domains/{}/dkim\" style=\"display:inline\">\
             <button type=\"submit\">Gen DKIM</button></form> \
             <form method=\"post\" action=\"/domains/{}/delete\" style=\"display:inline\" \
             onsubmit=\"return confirm('Delete this domain?')\">\
             <button type=\"submit\">Delete</button></form>\
             </td></tr>",
            esc(&d.domain),
            if d.active { "Yes" } else { "No" },
            if d.dkim_public_key.is_some() { "Yes" } else { "No" },
            footer_badge,
            d.id, d.id, d.id, d.id,
        ));
    }
    let content = format!(
        r#"<h1>Domains</h1>
<p><a href="/domains/new">Add Domain</a></p>
<table>
<thead><tr><th>Domain</th><th>Active</th><th>DKIM</th><th>Footer</th><th>Actions</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        rows = rows,
    );
    Html(layout("Domains", "Domains", &content, None))
}

async fn new_domain_form(_auth: AuthAdmin) -> Html<String> {
    debug!("[web] GET /domains/new — new domain form");
        let content = r#"<h1>Add Domain</h1>
<aside>
    <h2>Domain footer</h2>
    <p>Add a trusted HTML snippet that will be appended to every outbound HTML email for this domain.</p>
    <p><small>Leave blank to skip footer injection. Plain-text emails will receive a simplified version.</small></p>
</aside>
<form method="post" action="/domains">
<label>Domain Name<br><input type="text" name="domain" required></label>
<label>Footer HTML (optional)<br><textarea name="footer_html" rows="5" placeholder="&lt;p&gt;Confidential notice&lt;/p&gt;"></textarea></label>
<small>Supports inline styles. Avoid external scripts.</small>
<button type="submit">Create</button>
</form>"#;
    Html(layout("Add Domain", "Domains", content, None))
}

async fn create_domain(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<DomainForm>,
) -> Response {
    info!(
        "[web] POST /domains — creating domain={}, footer_present={}",
        form.domain,
        !form.footer_html.trim().is_empty()
    );
    match state
        .db
        .create_domain(&form.domain, &form.footer_html)
    {
        Ok(id) => {
            info!("[web] domain created successfully: {} (id={})", form.domain, id);
            regen_configs(&state);
            Redirect::to("/domains").into_response()
        }
        Err(e) => {
            error!("[web] failed to create domain {}: {}", form.domain, e);
            let content = format!(
                "<h1>Error</h1><p>{}</p><p><a href=\"/domains/new\">Back</a></p>",
                esc(&e)
            );
            Html(layout("Error", "Domains", &content, None)).into_response()
        }
    }
}

async fn edit_domain_form(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    debug!("[web] GET /domains/{}/edit — edit domain form", id);
    let domain = match state.db.get_domain(id) {
        Some(d) => d,
        None => {
            warn!("[web] domain id={} not found for edit", id);
            return Redirect::to("/domains").into_response();
        }
    };
    let checked = if domain.active { " checked" } else { "" };
    let footer_value = esc(domain.footer_html.as_deref().unwrap_or(""));
    let content = format!(
        r#"<h1>Edit Domain</h1>
<form method="post" action="/domains/{id}">
<label>Domain Name<br><input type="text" name="domain" value="{domain}" required></label>
<label><input type="checkbox" name="active" value="on"{checked}> Active</label>
<label>Footer HTML (optional)<br><textarea name="footer_html" rows="6">{footer}</textarea></label>
<small>Leave blank to disable footer injection for this domain.</small>
<button type="submit">Save</button>
</form>"#,
        id = domain.id,
        domain = esc(&domain.domain),
        checked = checked,
        footer = footer_value,
    );
    Html(layout("Edit Domain", "Domains", &content, None)).into_response()
}

async fn update_domain(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<DomainEditForm>,
) -> Response {
    let active = form.active.is_some();
    info!(
        "[web] POST /domains/{} — updating domain={}, active={}, footer_present={}",
        id,
        form.domain,
        active,
        !form.footer_html.trim().is_empty()
    );
    state
        .db
        .update_domain(id, &form.domain, active, &form.footer_html);
    regen_configs(&state);
    Redirect::to("/domains").into_response()
}

async fn delete_domain(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /domains/{}/delete — deleting domain", id);
    state.db.delete_domain(id);
    regen_configs(&state);
    Redirect::to("/domains").into_response()
}

async fn generate_dkim(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!("[web] POST /domains/{}/dkim — generating DKIM keys", id);
    let domain = match state.db.get_domain(id) {
        Some(d) => d,
        None => {
            warn!("[web] domain id={} not found for DKIM generation", id);
            return Redirect::to("/domains").into_response();
        }
    };

    // Generate RSA 2048 private key
    debug!("[web] generating RSA 2048 private key for domain={}", domain.domain);
    let priv_output = std::process::Command::new("openssl")
        .args(["genrsa", "2048"])
        .output();
    let private_key = match priv_output {
        Ok(o) if o.status.success() => {
            debug!("[web] DKIM private key generated for domain={}", domain.domain);
            String::from_utf8_lossy(&o.stdout).to_string()
        }
        Ok(o) => {
            error!("[web] openssl genrsa failed for domain={}: {}", domain.domain, String::from_utf8_lossy(&o.stderr));
            let content = "<h1>Error</h1><p>Failed to generate DKIM private key.</p>";
            return Html(layout("Error", "Domains", content, None)).into_response();
        }
        Err(e) => {
            error!("[web] failed to run openssl genrsa for domain={}: {}", domain.domain, e);
            let content = "<h1>Error</h1><p>Failed to generate DKIM private key.</p>";
            return Html(layout("Error", "Domains", content, None)).into_response();
        }
    };

    // Extract public key
    debug!("[web] extracting public key from DKIM private key for domain={}", domain.domain);
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
            debug!("[web] DKIM public key extracted for domain={}", domain.domain);
            String::from_utf8_lossy(&o.stdout).to_string()
        }
        Ok(o) => {
            error!("[web] openssl rsa -pubout failed for domain={}: {}", domain.domain, String::from_utf8_lossy(&o.stderr));
            let content = "<h1>Error</h1><p>Failed to extract DKIM public key.</p>";
            return Html(layout("Error", "Domains", content, None)).into_response();
        }
        Err(e) => {
            error!("[web] failed to run openssl rsa -pubout for domain={}: {}", domain.domain, e);
            let content = "<h1>Error</h1><p>Failed to extract DKIM public key.</p>";
            return Html(layout("Error", "Domains", content, None)).into_response();
        }
    };

    info!("[web] DKIM keys generated successfully for domain={}", domain.domain);

    state
        .db
        .update_domain_dkim(id, &domain.dkim_selector, &private_key, &public_key);
    regen_configs(&state);

    Redirect::to(&format!("/domains/{}/dns", id)).into_response()
}

async fn dns_info(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    debug!("[web] GET /domains/{}/dns — DNS info requested", id);
    let domain = match state.db.get_domain(id) {
        Some(d) => d,
        None => {
            warn!("[web] domain id={} not found for DNS info", id);
            return Redirect::to("/domains").into_response();
        }
    };
    let hostname = &state.hostname;

        let mut rows = String::new();
        rows.push_str(&format!(
                "<tr><td>MX</td><td>@</td><td><code>10 {hostname}.</code></td><td>Primary mail exchanger</td></tr>",
                hostname = esc(hostname),
        ));
        rows.push_str(
                "<tr><td>TXT</td><td>@</td><td><code>v=spf1 a mx ~all</code></td><td>Baseline SPF policy</td></tr>",
        );

        let mut dkim_block = String::from(
                "<p><em>Generate a DKIM key to unlock signing coverage.</em></p>",
        );
        if let Some(ref pub_key) = domain.dkim_public_key {
                let key_b64: String = pub_key
                        .lines()
                        .filter(|l| !l.starts_with("-----"))
                        .collect::<Vec<_>>()
                        .join("");
                rows.push_str(&format!(
                        "<tr><td>TXT</td><td><code>{selector}._domainkey</code></td><td><code>v=DKIM1; k=rsa; p={key}</code></td><td>DKIM signing key</td></tr>",
                        selector = esc(&domain.dkim_selector),
                        key = esc(&key_b64),
                ));
                dkim_block = format!(
                        r#"<figure>
<figcaption><small>selector: {selector}</small></figcaption>
<pre>v=DKIM1; k=rsa; p={key}</pre>
</figure>"#,
                        selector = esc(&domain.dkim_selector),
                        key = esc(&key_b64),
                );
        }

        rows.push_str(&format!(
                "<tr><td>TXT</td><td>_dmarc</td><td><code>v=DMARC1; p=none; rua=mailto:postmaster@{domain}</code></td><td>DMARC monitoring</td></tr>",
                domain = esc(&domain.domain),
        ));

        let content = format!(
                r#"<section>
    <hgroup>
        <small>DNS runbook</small>
        <h1>{domain}</h1>
    </hgroup>
    <p>Apply these records to Route 53 (or your DNS provider) to keep the domain aligned.</p>
    <form method="post" action="/domains/{id}/dkim">
        <button type="submit">Generate DKIM key</button>
    </form>
</section>
<aside>
    <h2>Deployment checklist</h2>
    <ol>
        <li>Update MX and SPF first to establish routing.</li>
        <li>Publish DKIM selector <code>{selector}</code> and wait for propagation.</li>
        <li>Add DMARC with <code>p=none</code> until you trust reports.</li>
        <li>Verify reverse DNS points back to <code>{hostname}</code>.</li>
    </ol>
    <small>Catch-all aliases rely on MX reaching this host. Keep DNS fresh.</small>
</aside>
<table>
    <thead><tr><th>Type</th><th>Name</th><th>Value</th><th>Purpose</th></tr></thead>
    <tbody>{rows}</tbody>
</table>
<section>
    <hgroup>
        <small>DKIM payload</small>
        <h2>Selector {selector}</h2>
    </hgroup>
    {dkim_block}
</section>
<aside>
    <h2>Reverse DNS</h2>
    <p>Ask your provider to point the PTR record for your public IP back to <code>{hostname}</code>.</p>
    <p><a href="/domains">Back to Domains</a></p>
</aside>"#,
                domain = esc(&domain.domain),
                id = domain.id,
                selector = esc(&domain.dkim_selector),
                hostname = esc(hostname),
                rows = rows,
                dkim_block = dkim_block,
        );

        Html(layout("DNS Records", "Domains", &content, None)).into_response()
}

// ── Accounts ──

async fn list_accounts(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /accounts — listing accounts");
    let accounts = state.db.list_all_accounts_with_domain();
    debug!("[web] found {} accounts", accounts.len());
    let mut rows = String::new();
    for a in &accounts {
        let domain = a.domain_name.as_deref().unwrap_or("-");
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>\
             <a href=\"/accounts/{}/edit\">Edit</a> \
             <form method=\"post\" action=\"/accounts/{}/delete\" style=\"display:inline\" \
             onsubmit=\"return confirm('Delete this account?')\">\
             <button type=\"submit\">Delete</button></form>\
             </td></tr>",
            esc(&a.username),
            esc(domain),
            esc(&a.name),
            if a.active { "Yes" } else { "No" },
            a.quota,
            a.id,
            a.id,
        ));
    }
    let content = format!(
        r#"<h1>Accounts</h1>
<p><a href="/accounts/new">Add Account</a></p>
<table>
<thead><tr><th>Username</th><th>Domain</th><th>Name</th><th>Active</th><th>Quota (MB)</th><th>Actions</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        rows = rows,
    );
    Html(layout("Accounts", "Accounts", &content, None))
}

async fn new_account_form(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    debug!("[web] GET /accounts/new — new account form");
    let domains = state.db.list_domains();
    let mut options = String::new();
    for d in &domains {
        options.push_str(&format!(
            "<option value=\"{}\">{}</option>",
            d.id,
            esc(&d.domain)
        ));
    }
    let content = format!(
        r#"<h1>Add Account</h1>
<form method="post" action="/accounts">
<label>Domain<br><select name="domain_id" required>{options}</select></label>
<label>Username<br><input type="text" name="username" required></label>
<label>Password<br><input type="password" name="password" required></label>
<label>Display Name<br><input type="text" name="name"></label>
<label>Quota (MB, 0 = unlimited)<br><input type="number" name="quota" value="0"></label>
<button type="submit">Create</button>
</form>"#,
        options = options,
    );
    Html(layout("Add Account", "Accounts", &content, None))
}

async fn create_account(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<AccountForm>,
) -> Response {
    info!("[web] POST /accounts — creating account username={}, domain_id={}", form.username, form.domain_id);
    let db_hash = crate::auth::hash_password(&form.password);
    let quota = form.quota.unwrap_or(0);
    match state
        .db
        .create_account(form.domain_id, &form.username, &db_hash, &form.name, quota)
    {
        Ok(id) => {
            info!("[web] account created successfully: {} (id={})", form.username, id);
            regen_configs(&state);
            Redirect::to("/accounts").into_response()
        }
        Err(e) => {
            error!("[web] failed to create account {}: {}", form.username, e);
            let content = format!(
                "<h1>Error</h1><p>{}</p><p><a href=\"/accounts/new\">Back</a></p>",
                esc(&e)
            );
            Html(layout("Error", "Accounts", &content, None)).into_response()
        }
    }
}

async fn edit_account_form(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    debug!("[web] GET /accounts/{}/edit — edit account form", id);
    let account = match state.db.get_account(id) {
        Some(a) => a,
        None => {
            warn!("[web] account id={} not found for edit", id);
            return Redirect::to("/accounts").into_response();
        }
    };
    let checked = if account.active { " checked" } else { "" };
    let content = format!(
        r#"<h1>Edit Account</h1>
<form method="post" action="/accounts/{id}">
<label>Display Name<br><input type="text" name="name" value="{name}"></label>
<label>New Password (leave blank to keep)<br><input type="password" name="password"></label>
<label><input type="checkbox" name="active" value="on"{checked}> Active</label>
<label>Quota (MB)<br><input type="number" name="quota" value="{quota}"></label>
<button type="submit">Save</button>
</form>"#,
        id = account.id,
        name = esc(&account.name),
        checked = checked,
        quota = account.quota,
    );
    Html(layout("Edit Account", "Accounts", &content, None)).into_response()
}

async fn update_account(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<AccountEditForm>,
) -> Response {
    let active = form.active.is_some();
    let quota = form.quota.unwrap_or(0);
    info!("[web] POST /accounts/{} — updating account active={}, quota={}", id, active, quota);
    state.db.update_account(id, &form.name, active, quota);

    if let Some(ref pw) = form.password {
        if !pw.is_empty() {
            info!("[web] updating password for account id={}", id);
            let db_hash = crate::auth::hash_password(pw);
            state.db.update_account_password(id, &db_hash);
        }
    }

    regen_configs(&state);
    Redirect::to("/accounts").into_response()
}

async fn delete_account(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /accounts/{}/delete — deleting account", id);
    state.db.delete_account(id);
    regen_configs(&state);
    Redirect::to("/accounts").into_response()
}

// ── Aliases ──

async fn list_aliases(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /aliases — listing aliases");
    let aliases = state.db.list_all_aliases_with_domain();
        debug!("[web] found {} aliases", aliases.len());
        let domains = state.db.list_domains();
        let mut catch_ready: HashMap<i64, bool> = HashMap::new();
        let mut rows = String::new();
        if aliases.is_empty() {
                rows.push_str("<tr><td colspan=\"7\">No aliases yet — create one to start routing mail.</td></tr>");
        } else {
                for a in &aliases {
                        let domain = a.domain_name.as_deref().unwrap_or("-");
                        let is_catch = is_catch_all(&a.source, a.domain_name.as_deref());
                        if is_catch && a.active {
                                catch_ready.insert(a.domain_id, true);
                        }
                        let alias_type = if is_catch {
                                "<mark>Catch-all</mark>"
                        } else {
                                "<em>Targeted</em>"
                        };
                        let tracking_badge = if a.tracking_enabled {
                                "<mark>On</mark>"
                        } else {
                                "<em>Off</em>"
                        };
                        let active_badge = if a.active {
                                "<mark>Active</mark>"
                        } else {
                                "<em>Disabled</em>"
                        };
                        rows.push_str(&format!(
                                r#"<tr>
    <td>{domain}</td>
    <td>{source}</td>
    <td>{destination}</td>
    <td>{alias_type}</td>
    <td>{tracking}</td>
    <td>{active}</td>
    <td>
        <a href="/aliases/{id}/edit">Edit</a>
        <form method="post" action="/aliases/{id}/delete" style="display:inline" onsubmit="return confirm('Delete this alias?')">
            <button type="submit">Delete</button>
        </form>
    </td>
</tr>"#,
                                domain = esc(domain),
                                source = esc(&a.source),
                                destination = esc(&a.destination),
                                alias_type = alias_type,
                                tracking = tracking_badge,
                                active = active_badge,
                                id = a.id,
                        ));
                }
        }

        let domain_total = domains.len() as f64;
        let coverage_pct = if domain_total > 0.0 {
                (catch_ready.len() as f64 / domain_total * 100.0).round()
        } else {
                0.0
        };
        let coverage_copy = if domain_total > 0.0 {
                format!("{} of {} domains have an active catch-all", catch_ready.len(), domains.len())
        } else {
                "Add a domain to calculate catch-all coverage".to_string()
        };

        let content = format!(
                r#"<section>
    <hgroup>
        <small>Routing intelligence</small>
        <h1>Aliases</h1>
    </hgroup>
    <p>Use direct aliases for known senders and catch-alls for the rest. Keep tracking toggled where compliance allows.</p>
    <a href="/aliases/new"><strong>Add alias</strong></a>
</section>
<aside>
    <h2>Catch-all coverage</h2>
    <p>{coverage_copy}</p>
    <progress value="{coverage_pct}" max="100"></progress>
    <ul>
        <li>Create <code>*@domain.com</code> to scoop stray recipients.</li>
        <li>Use tracking to learn which aliases are still receiving mail.</li>
        <li>Disable catch-all aliases temporarily instead of deleting them.</li>
    </ul>
</aside>
<table>
    <thead>
        <tr><th>Domain</th><th>Source</th><th>Destination</th><th>Type</th><th>Tracking</th><th>Status</th><th>Actions</th></tr>
    </thead>
    <tbody>{rows}</tbody>
</table>"#,
                coverage_copy = coverage_copy,
                coverage_pct = coverage_pct,
                rows = rows,
        );
    Html(layout("Aliases", "Aliases", &content, None))
}

async fn new_alias_form(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    debug!("[web] GET /aliases/new — new alias form");
    let domains = state.db.list_domains();
    let mut options = String::new();
    for d in &domains {
        options.push_str(&format!(
            "<option value=\"{}\">{}</option>",
            d.id,
            esc(&d.domain)
        ));
    }
    let content = format!(
        r#"<h1>Add Alias</h1>
<aside>
  <h2>When to use catch-all</h2>
  <p>Configure <code>*@domain.com</code> to collect unknown addresses, then forward them to a monitored mailbox.</p>
  <p><small>Tip: leave tracking enabled on new catch-alls for the first week.</small></p>
</aside>
<form method="post" action="/aliases">
<label>Domain<br><select name="domain_id" required>{options}</select></label>
<label>Source (full address)<br><input type="text" name="source" placeholder="*@example.com" required></label>
<small>Use an asterisk to build a catch-all; otherwise provide the exact mailbox.</small>
<label>Destination (full address)<br><input type="text" name="destination" placeholder="ops@example.com" required></label>
<small>This mailbox receives copies of anything that matches the alias.</small>
<label><input type="checkbox" name="tracking_enabled" value="on" checked> Enable Tracking</label>
<button type="submit">Create</button>
</form>"#,
        options = options,
    );
    Html(layout("Add Alias", "Aliases", &content, None))
}

async fn create_alias(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<AliasForm>,
) -> Response {
    let tracking = form.tracking_enabled.is_some();
    info!("[web] POST /aliases — creating alias source={}, destination={}, tracking={}", form.source, form.destination, tracking);
    match state
        .db
        .create_alias(form.domain_id, &form.source, &form.destination, tracking)
    {
        Ok(id) => {
            info!("[web] alias created successfully: {} -> {} (id={})", form.source, form.destination, id);
            regen_configs(&state);
            Redirect::to("/aliases").into_response()
        }
        Err(e) => {
            error!("[web] failed to create alias {} -> {}: {}", form.source, form.destination, e);
            let content = format!(
                "<h1>Error</h1><p>{}</p><p><a href=\"/aliases/new\">Back</a></p>",
                esc(&e)
            );
            Html(layout("Error", "Aliases", &content, None)).into_response()
        }
    }
}

async fn edit_alias_form(
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
    let active_checked = if alias.active { " checked" } else { "" };
    let tracking_checked = if alias.tracking_enabled { " checked" } else { "" };
    let content = format!(
        r#"<h1>Edit Alias</h1>
<aside>
  <h2>Routing notes</h2>
  <p>Toggle <strong>Active</strong> instead of deleting when you want to pause a catch-all.</p>
  <p><small>Tracking helps confirm whether a legacy alias is still in use.</small></p>
</aside>
<form method="post" action="/aliases/{id}">
<label>Source<br><input type="text" name="source" value="{source}" required></label>
<small>Keep <code>*@domain</code> syntax for catch-alls.</small>
<label>Destination<br><input type="text" name="destination" value="{destination}" required></label>
<small>Use a shared mailbox or list for observability.</small>
<label><input type="checkbox" name="active" value="on"{active_checked}> Active</label>
<label><input type="checkbox" name="tracking_enabled" value="on"{tracking_checked}> Enable Tracking</label>
<button type="submit">Save</button>
</form>"#,
        id = alias.id,
        source = esc(&alias.source),
        destination = esc(&alias.destination),
        active_checked = active_checked,
        tracking_checked = tracking_checked,
    );
    Html(layout("Edit Alias", "Aliases", &content, None)).into_response()
}

async fn update_alias(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<AliasEditForm>,
) -> Response {
    let active = form.active.is_some();
    let tracking = form.tracking_enabled.is_some();
    info!("[web] POST /aliases/{} — updating alias source={}, destination={}, active={}, tracking={}", id, form.source, form.destination, active, tracking);
    state
        .db
        .update_alias(id, &form.source, &form.destination, active, tracking);
    regen_configs(&state);
    Redirect::to("/aliases").into_response()
}

async fn delete_alias(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /aliases/{}/delete — deleting alias", id);
    state.db.delete_alias(id);
    regen_configs(&state);
    Redirect::to("/aliases").into_response()
}

// ── Tracking ──

async fn list_tracking(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /tracking — listing tracked messages");
    let messages = state.db.list_tracked_messages(100);
    debug!("[web] found {} tracked messages", messages.len());
    let mut rows = String::new();
    for m in &messages {
        let opens = state.db.get_opens_for_message(&m.message_id);
        rows.push_str(&format!(
            "<tr><td><a href=\"/tracking/{msg_id}\">{msg_id_short}</a></td>\
             <td>{sender}</td><td>{recipient}</td><td>{subject}</td>\
             <td>{date}</td><td>{opens}</td></tr>",
            msg_id = esc(&m.message_id),
            msg_id_short = esc(if m.message_id.len() > 20 {
                &m.message_id[..20]
            } else {
                &m.message_id
            }),
            sender = esc(&m.sender),
            recipient = esc(&m.recipient),
            subject = esc(&m.subject),
            date = esc(&m.created_at),
            opens = opens.len(),
        ));
    }
    let content = format!(
        r#"<h1>Tracking</h1>
<table>
<thead><tr><th>Message ID</th><th>Sender</th><th>Recipient</th><th>Subject</th><th>Date</th><th>Opens</th></tr></thead>
<tbody>{rows}</tbody>
</table>"#,
        rows = rows,
    );
    Html(layout("Tracking", "Tracking", &content, None))
}

async fn tracking_detail(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(msg_id): Path<String>,
) -> Response {
    debug!("[web] GET /tracking/{} — tracking detail requested", msg_id);
    let message = match state.db.get_tracked_message(&msg_id) {
        Some(m) => m,
        None => {
            warn!("[web] tracked message not found: {}", msg_id);
            let content = "<h1>Not Found</h1><p>Message not found.</p>";
            return Html(layout("Not Found", "Tracking", content, None)).into_response();
        }
    };
    let opens = state.db.get_opens_for_message(&msg_id);
    debug!("[web] tracked message {} has {} opens", msg_id, opens.len());

    let mut open_rows = String::new();
    for o in &opens {
        open_rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td></tr>",
            esc(&o.client_ip),
            esc(&o.user_agent),
            esc(&o.opened_at),
        ));
    }

    let content = format!(
        r#"<h1>Message Details</h1>
<dl>
<dt>Message ID</dt><dd>{msg_id}</dd>
<dt>Sender</dt><dd>{sender}</dd>
<dt>Recipient</dt><dd>{recipient}</dd>
<dt>Subject</dt><dd>{subject}</dd>
<dt>Date</dt><dd>{date}</dd>
</dl>
<h2>Opens ({open_count})</h2>
<table>
<thead><tr><th>IP Address</th><th>User Agent</th><th>Time</th></tr></thead>
<tbody>{open_rows}</tbody>
</table>
<p><a href="/tracking">Back to Tracking</a></p>"#,
        msg_id = esc(&message.message_id),
        sender = esc(&message.sender),
        recipient = esc(&message.recipient),
        subject = esc(&message.subject),
        date = esc(&message.created_at),
        open_count = opens.len(),
        open_rows = open_rows,
    );
    Html(layout("Message Details", "Tracking", &content, None)).into_response()
}

// ── Settings ──

async fn settings_page(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    debug!("[web] GET /settings — settings page for username={}", auth.admin.username);
    let admin = &auth.admin;
    let totp_status = if admin.totp_enabled { "Enabled" } else { "Disabled" };
    let totp_action = if admin.totp_enabled {
        r#"<form method="post" action="/settings/2fa/disable">
<button type="submit">Disable 2FA</button>
</form>"#
    } else {
        r#"<p><a href="/settings/2fa">Enable 2FA</a></p>"#
    };

    let content = format!(
        r#"<h1>Settings</h1>
<h2>Admin Account</h2>
<dl>
<dt>Username</dt><dd>{username}</dd>
<dt>2FA Status</dt><dd>{totp_status}</dd>
</dl>

<h2>Change Password</h2>
<form method="post" action="/settings/password">
<label>Current Password<br><input type="password" name="current_password" required></label>
<label>New Password<br><input type="password" name="new_password" required></label>
<label>Confirm Password<br><input type="password" name="confirm_password" required></label>
<button type="submit">Change Password</button>
</form>

<h2>Two-Factor Authentication</h2>
{totp_action}"#,
        username = esc(&admin.username),
        totp_status = totp_status,
        totp_action = totp_action,
    );
    Html(layout("Settings", "Settings", &content, None))
}

async fn change_password(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<PasswordForm>,
) -> Response {
    info!("[web] POST /settings/password — password change requested for username={}", auth.admin.username);
    if !crate::auth::verify_password(&form.current_password, &auth.admin.password_hash) {
        warn!("[web] password change failed — current password incorrect for username={}", auth.admin.username);
        let content = "<h1>Error</h1><p>Current password is incorrect.</p>\
                       <p><a href=\"/settings\">Back</a></p>";
        return Html(layout("Error", "Settings", content, None)).into_response();
    }
    if form.new_password != form.confirm_password {
        warn!("[web] password change failed — new passwords do not match for username={}", auth.admin.username);
        let content = "<h1>Error</h1><p>New passwords do not match.</p>\
                       <p><a href=\"/settings\">Back</a></p>";
        return Html(layout("Error", "Settings", content, None)).into_response();
    }
    let hash = crate::auth::hash_password(&form.new_password);
    state.db.update_admin_password(auth.admin.id, &hash);
    info!("[web] password changed successfully for username={}", auth.admin.username);
    let content = "<h1>Success</h1><p>Password changed successfully.</p>\
                   <p><a href=\"/settings\">Back to Settings</a></p>";
    Html(layout("Password Changed", "Settings", content, None)).into_response()
}

async fn setup_2fa(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    info!("[web] GET /settings/2fa — 2FA setup page for username={}", auth.admin.username);
    let secret = crate::auth::generate_totp_secret();
    let uri = crate::auth::totp_uri(&secret, &auth.admin.username);
    let content = format!(
        r#"<h1>Setup Two-Factor Authentication</h1>
<p>Add this secret to your authenticator app:</p>
<p><code>{secret}</code></p>
<p>Or use this URI:</p>
<p><code>{uri}</code></p>
<form method="post" action="/settings/2fa/enable">
<input type="hidden" name="secret" value="{secret}">
<label>Verification Code<br><input type="text" name="code" pattern="[0-9]{{6}}" maxlength="6" required></label>
<button type="submit">Verify &amp; Enable</button>
</form>
<p><a href="/settings">Cancel</a></p>"#,
        secret = esc(&secret),
        uri = esc(&uri),
    );
    Html(layout("Setup 2FA", "Settings", &content, None))
}

async fn enable_2fa(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TotpEnableForm>,
) -> Response {
    info!("[web] POST /settings/2fa/enable — enabling 2FA for username={}", auth.admin.username);
    if !crate::auth::verify_totp(&form.secret, &form.code) {
        warn!("[web] 2FA enable failed — invalid verification code for username={}", auth.admin.username);
        let content = "<h1>Error</h1><p>Invalid verification code. Please try again.</p>\
                       <p><a href=\"/settings/2fa\">Retry</a></p>";
        return Html(layout("Error", "Settings", content, None)).into_response();
    }
    state
        .db
        .update_admin_totp(auth.admin.id, Some(&form.secret), true);
    info!("[web] 2FA enabled successfully for username={}", auth.admin.username);
    let content = "<h1>Success</h1><p>Two-factor authentication has been enabled.</p>\
                   <p><a href=\"/settings\">Back to Settings</a></p>";
    Html(layout("2FA Enabled", "Settings", content, None)).into_response()
}

async fn disable_2fa(auth: AuthAdmin, State(state): State<AppState>) -> Response {
    info!("[web] POST /settings/2fa/disable — disabling 2FA for username={}", auth.admin.username);
    state.db.update_admin_totp(auth.admin.id, None, false);
    info!("[web] 2FA disabled successfully for username={}", auth.admin.username);
    let content = "<h1>Success</h1><p>Two-factor authentication has been disabled.</p>\
                   <p><a href=\"/settings\">Back to Settings</a></p>";
    Html(layout("2FA Disabled", "Settings", content, None)).into_response()
}

// ── Pixel Endpoint (no auth) ──

async fn pixel_handler(
    State(state): State<AppState>,
    Query(params): Query<PixelQuery>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    debug!("[web] GET /pixel — pixel request id={}", if params.id.is_empty() { "(empty)" } else { &params.id });
    if !params.id.is_empty() {
        let client_ip = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
            .or_else(|| {
                req.headers()
                    .get("x-real-ip")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();

        let user_agent = req
            .headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        state
            .db
            .record_pixel_open(&params.id, &client_ip, &user_agent);
        info!("[web] pixel open recorded: message_id={}, client_ip={}, user_agent={}", params.id, client_ip, user_agent);
    }

    let gif: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff,
        0xff, 0xff, 0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2c,
        0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00,
        0x3b,
    ];

    (
        StatusCode::OK,
        [
            (header::CONTENT_TYPE, "image/gif"),
            (header::CACHE_CONTROL, "no-store, no-cache, must-revalidate"),
        ],
        gif.to_vec(),
    )
        .into_response()
}
