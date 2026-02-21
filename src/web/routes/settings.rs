use askama::Template;
use axum::{
    extract::State,
    http::header,
    response::{Html, IntoResponse, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::db::Admin;
use crate::web::AppState;
use crate::web::auth::AuthAdmin;
use crate::web::forms::{FeatureToggleForm, PasswordForm, TotpEnableForm, WebhookSettingsForm};

// ── Templates ──

#[derive(Template)]
#[template(path = "settings/main.html")]
struct SettingsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    admin: Admin,
    pixel_host: String,
    pixel_port: String,
    cert_subject: String,
    cert_issuer: String,
    cert_not_before: String,
    cert_not_after: String,
    cert_serial: String,
    filter_enabled: bool,
    milter_enabled: bool,
    filter_healthy: bool,
    milter_healthy: bool,
    unsubscribe_enabled: bool,
    webhook_url: String,
}

#[derive(Template)]
#[template(path = "settings/setup_2fa.html")]
struct Setup2faTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    secret: String,
    uri: String,
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

fn check_filter_health() -> bool {
    // The content filter is a pipe transport invoked by Postfix on demand.
    // It is healthy if the mailserver binary exists and is executable.
    let paths = ["/usr/local/bin/mailserver", "./target/release/mailserver", "./target/debug/mailserver"];
    paths.iter().any(|p| std::path::Path::new(p).exists())
}

fn check_milter_health() -> bool {
    // OpenDKIM milter listens on 127.0.0.1:8891.
    // Check connectivity by attempting a TCP connection.
    std::net::TcpStream::connect_timeout(
        &"127.0.0.1:8891".parse().unwrap(),
        std::time::Duration::from_secs(1),
    )
    .is_ok()
}

fn read_cert_info() -> (String, String, String, String, String) {
    let cert_path = "/data/ssl/cert.pem";
    if !std::path::Path::new(cert_path).exists() {
        return (String::new(), String::new(), String::new(), String::new(), String::new());
    }
    let output = std::process::Command::new("openssl")
        .args(["x509", "-in", cert_path, "-noout", "-subject", "-issuer", "-dates", "-serial"])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout).to_string();
            let mut subject = String::new();
            let mut issuer = String::new();
            let mut not_before = String::new();
            let mut not_after = String::new();
            let mut serial = String::new();
            for line in text.lines() {
                if let Some(v) = line.strip_prefix("subject=") {
                    subject = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("issuer=") {
                    issuer = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("notBefore=") {
                    not_before = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("notAfter=") {
                    not_after = v.trim().to_string();
                } else if let Some(v) = line.strip_prefix("serial=") {
                    serial = v.trim().to_string();
                }
            }
            (subject, issuer, not_before, not_after, serial)
        }
        _ => (String::new(), String::new(), String::new(), String::new(), String::new()),
    }
}

pub async fn page(auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    log::debug!(
        "[web] GET /settings — settings page for username={}",
        auth.admin.username
    );

    // determine pixel host/port from DB (fallback to env or server state)
    let default_host = state.hostname.clone();
    let default_port = state.admin_port.to_string();
    let mut pixel_host = default_host.clone();
    let mut pixel_port: String = default_port.clone();

    if let Some(base) = state
        .blocking_db(|db| db.get_setting("pixel_base_url"))
        .await
    {
        // remove scheme and /pixel?id= suffix if present
        let trimmed = base
            .trim_end_matches("/pixel?id=")
            .trim_end_matches("/pixel");
        let no_scheme = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .unwrap_or(&trimmed);
        let host_port = no_scheme.split('/').next().unwrap_or(no_scheme);
        if let Some((h, p)) = host_port.split_once(':') {
            pixel_host = h.to_string();
            pixel_port = p.to_string();
        } else {
            pixel_host = host_port.to_string();
            pixel_port = String::new();
        }
    } else if let Ok(env_val) = std::env::var("PIXEL_BASE_URL") {
        let trimmed = env_val
            .trim_end_matches("/pixel?id=")
            .trim_end_matches("/pixel");
        let no_scheme = trimmed
            .strip_prefix("http://")
            .or_else(|| trimmed.strip_prefix("https://"))
            .unwrap_or(&trimmed);
        let host_port = no_scheme.split('/').next().unwrap_or(no_scheme);
        if let Some((h, p)) = host_port.split_once(':') {
            pixel_host = h.to_string();
            pixel_port = p.to_string();
        } else {
            pixel_host = host_port.to_string();
            pixel_port = String::new();
        }
    }

    let (cert_subject, cert_issuer, cert_not_before, cert_not_after, cert_serial) = read_cert_info();

    // Load feature toggle states from DB (default: enabled)
    let filter_enabled = state
        .blocking_db(|db| db.get_setting("feature_filter_enabled"))
        .await
        .map(|v| v != "false")
        .unwrap_or(true);
    let milter_enabled = state
        .blocking_db(|db| db.get_setting("feature_milter_enabled"))
        .await
        .map(|v| v != "false")
        .unwrap_or(true);

    let unsubscribe_enabled = state
        .blocking_db(|db| db.get_setting("feature_unsubscribe_enabled"))
        .await
        .map(|v| v != "false")
        .unwrap_or(true);

    let filter_healthy = check_filter_health();
    let milter_healthy = check_milter_health();

    let webhook_url = state
        .blocking_db(|db| db.get_setting("webhook_url"))
        .await
        .unwrap_or_default();

    let tmpl = SettingsTemplate {
        nav_active: "Settings",
        flash: None,
        admin: auth.admin,
        pixel_host,
        pixel_port,
        cert_subject,
        cert_issuer,
        cert_not_before,
        cert_not_after,
        cert_serial,
        filter_enabled,
        milter_enabled,
        filter_healthy,
        milter_healthy,
        unsubscribe_enabled,
        webhook_url,
    };
    Html(tmpl.render().unwrap())
}

pub async fn update_pixel(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<crate::web::forms::PixelSettingsForm>,
) -> Response {
    info!(
        "[web] POST /settings/pixel — update pixel host/port for username={}",
        auth.admin.username
    );
    let host = form.pixel_host.trim();
    if host.is_empty() {
        let tmpl = ErrorTemplate {
            nav_active: "Settings",
            flash: None,
            status_code: 400,
            status_text: "Bad Request",
            title: "Error",
            message: "Host may not be empty.",
            back_url: "/settings",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let base = match form.pixel_port {
        Some(p) if p > 0 && p != 80 => format!("http://{}:{}/pixel?id=", host, p),
        _ => format!("http://{}/pixel?id=", host),
    };
    let base_for_db = base.clone();
    state
        .blocking_db(move |db| db.set_setting("pixel_base_url", &base_for_db))
        .await;
    info!(
        "[web] pixel_base_url updated to {} by user={}",
        base, auth.admin.username
    );
    let tmpl = ErrorTemplate {
        nav_active: "Settings",
        flash: None,
        status_code: 200,
        status_text: "OK",
        title: "Success",
        message: "Pixel tracker base URL updated.",
        back_url: "/settings",
        back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn update_features(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<FeatureToggleForm>,
) -> Response {
    info!(
        "[web] POST /settings/features — update feature toggles by username={}",
        auth.admin.username
    );

    let filter_enabled = form.filter_enabled.is_some();
    let milter_enabled = form.milter_enabled.is_some();
    let unsubscribe_enabled = form.unsubscribe_enabled.is_some();

    let filter_val = if filter_enabled { "true" } else { "false" }.to_string();
    let milter_val = if milter_enabled { "true" } else { "false" }.to_string();
    let unsub_val = if unsubscribe_enabled { "true" } else { "false" }.to_string();

    state
        .blocking_db(move |db| {
            db.set_setting("feature_filter_enabled", &filter_val);
            db.set_setting("feature_milter_enabled", &milter_val);
            db.set_setting("feature_unsubscribe_enabled", &unsub_val);
        })
        .await;

    info!(
        "[web] features updated: filter={}, milter={}, unsubscribe={} by user={}",
        filter_enabled, milter_enabled, unsubscribe_enabled, auth.admin.username
    );

    // Regenerate Postfix configs to apply feature toggle changes
    crate::web::regen_configs(&state).await;

    let tmpl = ErrorTemplate {
        nav_active: "Settings",
        flash: None,
        status_code: 200,
        status_text: "OK",
        title: "Success",
        message: "Feature settings updated successfully.",
        back_url: "/settings",
        back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn change_password(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<PasswordForm>,
) -> Response {
    info!(
        "[web] POST /settings/password — password change requested for username={}",
        auth.admin.username
    );
    if !crate::auth::verify_password(&form.current_password, &auth.admin.password_hash) {
        warn!(
            "[web] password change failed — current password incorrect for username={}",
            auth.admin.username
        );
        let tmpl = ErrorTemplate {
            nav_active: "Settings",
            flash: None,
            status_code: 400,
            status_text: "Bad Request",
            title: "Error",
            message: "Current password is incorrect.",
            back_url: "/settings",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    if form.new_password != form.confirm_password {
        warn!(
            "[web] password change failed — new passwords do not match for username={}",
            auth.admin.username
        );
        let tmpl = ErrorTemplate {
            nav_active: "Settings",
            flash: None,
            status_code: 400,
            status_text: "Bad Request",
            title: "Error",
            message: "New passwords do not match.",
            back_url: "/settings",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let hash = crate::auth::hash_password(&form.new_password);
    let admin_id = auth.admin.id;
    state
        .blocking_db(move |db| db.update_admin_password(admin_id, &hash))
        .await;
    info!(
        "[web] password changed successfully for username={}",
        auth.admin.username
    );
    let tmpl = ErrorTemplate {
        nav_active: "Settings",
        flash: None,
        status_code: 200,
        status_text: "OK",
        title: "Success",
        message: "Password changed successfully.",
        back_url: "/settings",
        back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn setup_2fa(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    info!(
        "[web] GET /settings/2fa — 2FA setup page for username={}",
        auth.admin.username
    );
    let secret = crate::auth::generate_totp_secret();
    let uri = crate::auth::totp_uri(&secret, &auth.admin.username);
    let tmpl = Setup2faTemplate {
        nav_active: "Settings",
        flash: None,
        secret,
        uri,
    };
    Html(tmpl.render().unwrap())
}

pub async fn enable_2fa(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TotpEnableForm>,
) -> Response {
    info!(
        "[web] POST /settings/2fa/enable — enabling 2FA for username={}",
        auth.admin.username
    );
    if !crate::auth::verify_totp(&form.secret, &form.code) {
        warn!(
            "[web] 2FA enable failed — invalid verification code for username={}",
            auth.admin.username
        );
        let tmpl = ErrorTemplate {
            nav_active: "Settings",
            flash: None,
            status_code: 400,
            status_text: "Bad Request",
            title: "Error",
            message: "Invalid verification code. Please try again.",
            back_url: "/settings/2fa",
            back_label: "Retry",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let admin_id = auth.admin.id;
    let secret = form.secret.clone();
    state
        .blocking_db(move |db| db.update_admin_totp(admin_id, Some(&secret), true))
        .await;
    info!(
        "[web] 2FA enabled successfully for username={}",
        auth.admin.username
    );
    let tmpl = ErrorTemplate {
        nav_active: "Settings",
        flash: None,
        status_code: 200,
        status_text: "OK",
        title: "Success",
        message: "Two-factor authentication has been enabled.",
        back_url: "/settings",
        back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn disable_2fa(auth: AuthAdmin, State(state): State<AppState>) -> Response {
    info!(
        "[web] POST /settings/2fa/disable — disabling 2FA for username={}",
        auth.admin.username
    );
    let admin_id = auth.admin.id;
    state
        .blocking_db(move |db| db.update_admin_totp(admin_id, None, false))
        .await;
    info!(
        "[web] 2FA disabled successfully for username={}",
        auth.admin.username
    );
    let tmpl = ErrorTemplate {
        nav_active: "Settings",
        flash: None,
        status_code: 200,
        status_text: "OK",
        title: "Success",
        message: "Two-factor authentication has been disabled.",
        back_url: "/settings",
        back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn regenerate_tls(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Response {
    info!("[web] POST /settings/tls/regenerate — regenerating self-signed TLS certificate by username={}", auth.admin.username);
    let hostname = &state.hostname;
    
    match crate::config::generate_all_certificates(hostname) {
        Ok(_) => {
            info!("[web] TLS certificates and DH parameters regenerated successfully for hostname={}", hostname);
            crate::config::reload_services();
            let tmpl = ErrorTemplate {
                nav_active: "Settings", flash: None,
                status_code: 200,
                status_text: "OK",
                title: "Success", message: "TLS certificates and DH parameters regenerated. Services have been reloaded.",
                back_url: "/settings", back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        Err(e) => {
            error!("[web] failed to regenerate TLS certificates: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "Settings", flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error", message: &format!("Failed to regenerate TLS certificates: {}", e),
                back_url: "/settings", back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn download_cert(auth: AuthAdmin) -> Response {
    debug!("[web] GET /settings/tls/cert.pem — certificate download by username={}", auth.admin.username);
    let cert_path = "/data/ssl/cert.pem";
    match std::fs::read(cert_path) {
        Ok(data) => {
            info!("[web] certificate downloaded by username={}", auth.admin.username);
            (
                [
                    (header::CONTENT_TYPE, "application/x-pem-file"),
                    (header::CONTENT_DISPOSITION, "attachment; filename=\"cert.pem\""),
                ],
                data,
            ).into_response()
        }
        Err(e) => {
            error!("[web] failed to read certificate file: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "Settings", flash: None,
                status_code: 404,
                status_text: "Not Found",
                title: "Error", message: "Certificate file not found.",
                back_url: "/settings", back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn restart_services(auth: AuthAdmin) -> Response {
    info!(
        "[web] POST /settings/restart-services — restarting mail services by username={}",
        auth.admin.username
    );

    match crate::config::restart_services() {
        Ok(details) => {
            info!(
                "[web] services restarted successfully by username={}: {}",
                auth.admin.username, details
            );
            let tmpl = ErrorTemplate {
                nav_active: "Settings",
                flash: None,
                status_code: 200,
                status_text: "OK",
                title: "Success",
                message: &format!("Mail services restarted. {}", details),
                back_url: "/settings",
                back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        Err(e) => {
            error!(
                "[web] failed to restart services by username={}: {}",
                auth.admin.username, e
            );
            let tmpl = ErrorTemplate {
                nav_active: "Settings",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: &format!("Failed to restart services: {}", e),
                back_url: "/settings",
                back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn restart_container(auth: AuthAdmin) -> Response {
    info!(
        "[web] POST /settings/restart-container — restarting Docker container by username={}",
        auth.admin.username
    );

    match crate::config::restart_container() {
        Ok(()) => {
            info!(
                "[web] container restart initiated by username={}",
                auth.admin.username
            );
            let tmpl = ErrorTemplate {
                nav_active: "Settings",
                flash: None,
                status_code: 200,
                status_text: "OK",
                title: "Restarting",
                message: "Container restart initiated. The page will be temporarily unavailable.",
                back_url: "/settings",
                back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        Err(e) => {
            error!(
                "[web] failed to restart container by username={}: {}",
                auth.admin.username, e
            );
            let tmpl = ErrorTemplate {
                nav_active: "Settings",
                flash: None,
                status_code: 500,
                status_text: "Error",
                title: "Error",
                message: &format!("Failed to restart container: {}", e),
                back_url: "/settings",
                back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn download_key(auth: AuthAdmin) -> Response {
    debug!("[web] GET /settings/tls/key.pem — private key download by username={}", auth.admin.username);
    let key_path = "/data/ssl/key.pem";
    match std::fs::read(key_path) {
        Ok(data) => {
            info!("[web] private key downloaded by username={}", auth.admin.username);
            (
                [
                    (header::CONTENT_TYPE, "application/x-pem-file"),
                    (header::CONTENT_DISPOSITION, "attachment; filename=\"key.pem\""),
                ],
                data,
            ).into_response()
        }
        Err(e) => {
            error!("[web] failed to read private key file: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "Settings", flash: None,
                status_code: 404,
                status_text: "Not Found",
                title: "Error", message: "Private key file not found.",
                back_url: "/settings", back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn update_webhook(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<WebhookSettingsForm>,
) -> Response {
    info!(
        "[web] POST /settings/webhook — update webhook URL by username={}",
        auth.admin.username
    );
    let url = form.webhook_url.trim().to_string();
    // Validate: must be empty or start with http:// or https://
    if !url.is_empty() && !url.starts_with("http://") && !url.starts_with("https://") {
        let tmpl = ErrorTemplate {
            nav_active: "Settings",
            flash: None,
            status_code: 400,
            status_text: "Bad Request",
            title: "Error",
            message: "Webhook URL must start with http:// or https://",
            back_url: "/settings",
            back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let url_for_db = url.clone();
    state
        .blocking_db(move |db| db.set_setting("webhook_url", &url_for_db))
        .await;
    info!(
        "[web] webhook_url updated by user={}",
        auth.admin.username
    );
    let tmpl = ErrorTemplate {
        nav_active: "Settings",
        flash: None,
        status_code: 200,
        status_text: "OK",
        title: "Success",
        message: "Webhook URL updated successfully.",
        back_url: "/settings",
        back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}
