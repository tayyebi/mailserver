use axum::{
    extract::{FromRef, FromRequestParts, Path, Query, State},
    http::{header, request::Parts, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    routing::{get, post},
    Form, Router,
};
use log::{info, warn, error, debug};
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
        let class = if *label == nav_active { " class=\"active\"" } else { "" };
        nav_html.push_str(&format!("<a href=\"{}\"{}>{}</a> ", esc(href), class, esc(label)));
    }
    let flash_html = match flash {
        Some(msg) => format!("<div class=\"flash\">{}</div>", esc(msg)),
        None => String::new(),
    };
    format!(
        r#"<!DOCTYPE html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="stylesheet" href="/static/style.css">
</head>
<body>
<nav>{nav_html}</nav>
{flash_html}
<main>{content}</main>
<footer>Mailserver Admin</footer>
</body>
</html>"#,
        title = esc(title),
        nav_html = nav_html,
        flash_html = flash_html,
        content = content,
    )
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
}

#[derive(Deserialize)]
struct DomainEditForm {
    domain: String,
    #[serde(default)]
    active: Option<String>,
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
        .expect("Failed to bind address");
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
    debug!("[web] dashboard stats: domains={}, accounts={}, aliases={}, tracked={}, opens={}",
        stats.domain_count, stats.account_count, stats.alias_count, stats.tracked_count, stats.open_count);
    let content = format!(
        r#"<h1>Dashboard</h1>
<table>
<tr><th>Domains</th><td>{}</td></tr>
<tr><th>Accounts</th><td>{}</td></tr>
<tr><th>Aliases</th><td>{}</td></tr>
<tr><th>Tracked Messages</th><td>{}</td></tr>
<tr><th>Pixel Opens</th><td>{}</td></tr>
</table>
<h2>Server Info</h2>
<table>
<tr><th>Hostname</th><td>{}</td></tr>
<tr><th>SMTP</th><td>Port 25 / 587 (STARTTLS) / 465 (SSL)</td></tr>
<tr><th>IMAP</th><td>Port 143 (STARTTLS) / 993 (SSL)</td></tr>
<tr><th>POP3</th><td>Port 110 (STARTTLS) / 995 (SSL)</td></tr>
</table>"#,
        stats.domain_count,
        stats.account_count,
        stats.alias_count,
        stats.tracked_count,
        stats.open_count,
        esc(&state.hostname),
    );
    Html(layout("Dashboard", "Dashboard", &content, None))
}

// ── Domains ──

async fn list_domains(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /domains — listing domains");
    let domains = state.db.list_domains();
    debug!("[web] found {} domains", domains.len());
    let mut rows = String::new();
    for d in &domains {
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>\
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
            d.id, d.id, d.id, d.id,
        ));
    }
    let content = format!(
        r#"<h1>Domains</h1>
<p><a href="/domains/new">Add Domain</a></p>
<table>
<tr><th>Domain</th><th>Active</th><th>DKIM</th><th>Actions</th></tr>
{rows}
</table>"#,
        rows = rows,
    );
    Html(layout("Domains", "Domains", &content, None))
}

async fn new_domain_form(_auth: AuthAdmin) -> Html<String> {
    debug!("[web] GET /domains/new — new domain form");
    let content = r#"<h1>Add Domain</h1>
<form method="post" action="/domains">
<label>Domain Name<br><input type="text" name="domain" required></label><br>
<button type="submit">Create</button>
</form>"#;
    Html(layout("Add Domain", "Domains", content, None))
}

async fn create_domain(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<DomainForm>,
) -> Response {
    info!("[web] POST /domains — creating domain={}", form.domain);
    match state.db.create_domain(&form.domain) {
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
    let content = format!(
        r#"<h1>Edit Domain</h1>
<form method="post" action="/domains/{id}">
<label>Domain Name<br><input type="text" name="domain" value="{domain}" required></label><br>
<label><input type="checkbox" name="active" value="on"{checked}> Active</label><br>
<button type="submit">Save</button>
</form>"#,
        id = domain.id,
        domain = esc(&domain.domain),
        checked = checked,
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
    info!("[web] POST /domains/{} — updating domain={}, active={}", id, form.domain, active);
    state.db.update_domain(id, &form.domain, active);
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

    let mut records = format!(
        r#"<h1>DNS Records for {domain}</h1>
<table>
<tr><th>Type</th><th>Name</th><th>Value</th></tr>
<tr><td>MX</td><td>@</td><td><code>10 {hostname}.</code></td></tr>
<tr><td>TXT</td><td>@</td><td><code>v=spf1 a mx ~all</code></td></tr>"#,
        domain = esc(&domain.domain),
        hostname = esc(hostname),
    );

    if let Some(ref pub_key) = domain.dkim_public_key {
        // Strip PEM headers/footers and whitespace to get base64-only
        let key_b64: String = pub_key
            .lines()
            .filter(|l| !l.starts_with("-----"))
            .collect::<Vec<_>>()
            .join("");
        records.push_str(&format!(
            "<tr><td>TXT</td><td><code>{}._domainkey</code></td>\
             <td><code>v=DKIM1; k=rsa; p={}</code></td></tr>",
            esc(&domain.dkim_selector),
            esc(&key_b64),
        ));
    }

    records.push_str(&format!(
        "<tr><td>TXT</td><td>_dmarc</td>\
         <td><code>v=DMARC1; p=none; rua=mailto:postmaster@{domain}</code></td></tr>\
         </table>\
         <h2>Reverse DNS (PTR)</h2>\
         <p>Set your server's PTR record to <code>{hostname}</code></p>\
         <p><a href=\"/domains\">Back to Domains</a></p>",
        domain = esc(&domain.domain),
        hostname = esc(hostname),
    ));

    Html(layout("DNS Records", "Domains", &records, None)).into_response()
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
<tr><th>Username</th><th>Domain</th><th>Name</th><th>Active</th><th>Quota (MB)</th><th>Actions</th></tr>
{rows}
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
<label>Domain<br><select name="domain_id" required>{options}</select></label><br>
<label>Username<br><input type="text" name="username" required></label><br>
<label>Password<br><input type="password" name="password" required></label><br>
<label>Display Name<br><input type="text" name="name"></label><br>
<label>Quota (MB, 0 = unlimited)<br><input type="number" name="quota" value="0"></label><br>
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
<label>Display Name<br><input type="text" name="name" value="{name}"></label><br>
<label>New Password (leave blank to keep)<br><input type="password" name="password"></label><br>
<label><input type="checkbox" name="active" value="on"{checked}> Active</label><br>
<label>Quota (MB)<br><input type="number" name="quota" value="{quota}"></label><br>
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
    let mut rows = String::new();
    for a in &aliases {
        let domain = a.domain_name.as_deref().unwrap_or("-");
        rows.push_str(&format!(
            "<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>\
             <a href=\"/aliases/{}/edit\">Edit</a> \
             <form method=\"post\" action=\"/aliases/{}/delete\" style=\"display:inline\" \
             onsubmit=\"return confirm('Delete this alias?')\">\
             <button type=\"submit\">Delete</button></form>\
             </td></tr>",
            esc(domain),
            esc(&a.source),
            esc(&a.destination),
            if a.tracking_enabled { "Yes" } else { "No" },
            if a.active { "Yes" } else { "No" },
            a.id,
            a.id,
        ));
    }
    let content = format!(
        r#"<h1>Aliases</h1>
<p><a href="/aliases/new">Add Alias</a></p>
<table>
<tr><th>Domain</th><th>Source</th><th>Destination</th><th>Tracking</th><th>Active</th><th>Actions</th></tr>
{rows}
</table>"#,
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
<form method="post" action="/aliases">
<label>Domain<br><select name="domain_id" required>{options}</select></label><br>
<label>Source (full address)<br><input type="text" name="source" required></label><br>
<label>Destination (full address)<br><input type="text" name="destination" required></label><br>
<label><input type="checkbox" name="tracking_enabled" value="on"> Enable Tracking</label><br>
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
<form method="post" action="/aliases/{id}">
<label>Source<br><input type="text" name="source" value="{source}" required></label><br>
<label>Destination<br><input type="text" name="destination" value="{destination}" required></label><br>
<label><input type="checkbox" name="active" value="on"{active_checked}> Active</label><br>
<label><input type="checkbox" name="tracking_enabled" value="on"{tracking_checked}> Enable Tracking</label><br>
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
<tr><th>Message ID</th><th>Sender</th><th>Recipient</th><th>Subject</th><th>Date</th><th>Opens</th></tr>
{rows}
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
<table>
<tr><th>Message ID</th><td>{msg_id}</td></tr>
<tr><th>Sender</th><td>{sender}</td></tr>
<tr><th>Recipient</th><td>{recipient}</td></tr>
<tr><th>Subject</th><td>{subject}</td></tr>
<tr><th>Date</th><td>{date}</td></tr>
</table>
<h2>Opens ({open_count})</h2>
<table>
<tr><th>IP Address</th><th>User Agent</th><th>Time</th></tr>
{open_rows}
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
<table>
<tr><th>Username</th><td>{username}</td></tr>
<tr><th>2FA Status</th><td>{totp_status}</td></tr>
</table>

<h2>Change Password</h2>
<form method="post" action="/settings/password">
<label>Current Password<br><input type="password" name="current_password" required></label><br>
<label>New Password<br><input type="password" name="new_password" required></label><br>
<label>Confirm Password<br><input type="password" name="confirm_password" required></label><br>
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
<label>Verification Code<br><input type="text" name="code" pattern="[0-9]{{6}}" maxlength="6" required></label><br>
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
