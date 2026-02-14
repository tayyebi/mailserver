use askama::Template;
use axum::{
    extract::State,
    http::header,
    response::{Html, Response, IntoResponse},
    Form,
};
use log::{info, warn, error, debug};

use crate::db::Admin;
use crate::web::AppState;
use crate::web::auth::AuthAdmin;
use crate::web::forms::{PasswordForm, TotpEnableForm};

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
    title: &'a str,
    message: &'a str,
    back_url: &'a str,
    back_label: &'a str,
}

// ── Handlers ──

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
    log::debug!("[web] GET /settings — settings page for username={}", auth.admin.username);

    // determine pixel host/port from DB (fallback to env or server state)
    let default_host = state.hostname.clone();
    let default_port = state.admin_port.to_string();
    let mut pixel_host = default_host.clone();
    let mut pixel_port: String = default_port.clone();

    if let Some(base) = state.db.get_setting("pixel_base_url") {
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
        let trimmed = env_val.trim_end_matches("/pixel?id=").trim_end_matches("/pixel");
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

    let tmpl = SettingsTemplate {
        nav_active: "Settings", flash: None,
        admin: auth.admin,
        pixel_host,
        pixel_port,
        cert_subject,
        cert_issuer,
        cert_not_before,
        cert_not_after,
        cert_serial,
    };
    Html(tmpl.render().unwrap())
}

pub async fn update_pixel(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<crate::web::forms::PixelSettingsForm>,
) -> Response {
    info!("[web] POST /settings/pixel — update pixel host/port for username={}", auth.admin.username);
    let host = form.pixel_host.trim();
    if host.is_empty() {
        let tmpl = ErrorTemplate {
            nav_active: "Settings", flash: None,
            title: "Error", message: "Host may not be empty.",
            back_url: "/settings", back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let base = match form.pixel_port {
        Some(p) if p > 0 && p != 80 => format!("http://{}:{}/pixel?id=", host, p),
        _ => format!("http://{}/pixel?id=", host),
    };
    state.db.set_setting("pixel_base_url", &base);
    info!("[web] pixel_base_url updated to {} by user={}", base, auth.admin.username);
    let tmpl = ErrorTemplate {
        nav_active: "Settings", flash: None,
        title: "Success", message: "Pixel tracker base URL updated.",
        back_url: "/settings", back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn change_password(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<PasswordForm>,
) -> Response {
    info!("[web] POST /settings/password — password change requested for username={}", auth.admin.username);
    if !crate::auth::verify_password(&form.current_password, &auth.admin.password_hash) {
        warn!("[web] password change failed — current password incorrect for username={}", auth.admin.username);
        let tmpl = ErrorTemplate {
            nav_active: "Settings", flash: None,
            title: "Error", message: "Current password is incorrect.",
            back_url: "/settings", back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    if form.new_password != form.confirm_password {
        warn!("[web] password change failed — new passwords do not match for username={}", auth.admin.username);
        let tmpl = ErrorTemplate {
            nav_active: "Settings", flash: None,
            title: "Error", message: "New passwords do not match.",
            back_url: "/settings", back_label: "Back",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let hash = crate::auth::hash_password(&form.new_password);
    state.db.update_admin_password(auth.admin.id, &hash);
    info!("[web] password changed successfully for username={}", auth.admin.username);
    let tmpl = ErrorTemplate {
        nav_active: "Settings", flash: None,
        title: "Success", message: "Password changed successfully.",
        back_url: "/settings", back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn setup_2fa(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    info!("[web] GET /settings/2fa — 2FA setup page for username={}", auth.admin.username);
    let secret = crate::auth::generate_totp_secret();
    let uri = crate::auth::totp_uri(&secret, &auth.admin.username);
    let tmpl = Setup2faTemplate {
        nav_active: "Settings", flash: None,
        secret, uri,
    };
    Html(tmpl.render().unwrap())
}

pub async fn enable_2fa(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TotpEnableForm>,
) -> Response {
    info!("[web] POST /settings/2fa/enable — enabling 2FA for username={}", auth.admin.username);
    if !crate::auth::verify_totp(&form.secret, &form.code) {
        warn!("[web] 2FA enable failed — invalid verification code for username={}", auth.admin.username);
        let tmpl = ErrorTemplate {
            nav_active: "Settings", flash: None,
            title: "Error", message: "Invalid verification code. Please try again.",
            back_url: "/settings/2fa", back_label: "Retry",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    state.db.update_admin_totp(auth.admin.id, Some(&form.secret), true);
    info!("[web] 2FA enabled successfully for username={}", auth.admin.username);
    let tmpl = ErrorTemplate {
        nav_active: "Settings", flash: None,
        title: "Success", message: "Two-factor authentication has been enabled.",
        back_url: "/settings", back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn disable_2fa(auth: AuthAdmin, State(state): State<AppState>) -> Response {
    info!("[web] POST /settings/2fa/disable — disabling 2FA for username={}", auth.admin.username);
    state.db.update_admin_totp(auth.admin.id, None, false);
    info!("[web] 2FA disabled successfully for username={}", auth.admin.username);
    let tmpl = ErrorTemplate {
        nav_active: "Settings", flash: None,
        title: "Success", message: "Two-factor authentication has been disabled.",
        back_url: "/settings", back_label: "Back to Settings",
    };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn regenerate_tls(
    auth: AuthAdmin,
    State(state): State<AppState>,
) -> Response {
    info!("[web] POST /settings/tls/regenerate — regenerating self-signed TLS certificate by username={}", auth.admin.username);
    let hostname = &state.hostname;
    // Sanitize hostname for use in certificate subject — only allow safe characters
    let safe_hostname: String = hostname.chars()
        .filter(|c| c.is_alphanumeric() || *c == '.' || *c == '-')
        .collect();
    if safe_hostname.is_empty() {
        error!("[web] hostname is empty or contains only invalid characters: {}", hostname);
        let tmpl = ErrorTemplate {
            nav_active: "Settings", flash: None,
            title: "Error", message: "Invalid hostname for certificate generation.",
            back_url: "/settings", back_label: "Back to Settings",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    let result = std::process::Command::new("openssl")
        .args([
            "req", "-new", "-newkey", "rsa:2048", "-days", "3650",
            "-nodes", "-x509",
            "-subj", &format!("/CN={}", safe_hostname),
            "-keyout", "/data/ssl/key.pem",
            "-out", "/data/ssl/cert.pem",
        ])
        .output();
    match result {
        Ok(o) if o.status.success() => {
            // Set secure permissions on the private key
            match std::process::Command::new("chmod").args(["600", "/data/ssl/key.pem"]).output() {
                Ok(o) if o.status.success() => debug!("[web] set key.pem permissions to 600"),
                Ok(o) => warn!("[web] chmod 600 key.pem exited with status {}", o.status),
                Err(e) => warn!("[web] failed to set key.pem permissions: {}", e),
            }
            info!("[web] TLS certificate regenerated successfully for hostname={}", safe_hostname);
            crate::config::reload_services();
            let tmpl = ErrorTemplate {
                nav_active: "Settings", flash: None,
                title: "Success", message: "TLS certificate regenerated. Services have been reloaded.",
                back_url: "/settings", back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            error!("[web] openssl failed to regenerate TLS certificate: {}", stderr);
            let tmpl = ErrorTemplate {
                nav_active: "Settings", flash: None,
                title: "Error", message: "Failed to regenerate TLS certificate.",
                back_url: "/settings", back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        Err(e) => {
            error!("[web] failed to run openssl for TLS regeneration: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "Settings", flash: None,
                title: "Error", message: "Failed to regenerate TLS certificate.",
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
                title: "Error", message: "Certificate file not found.",
                back_url: "/settings", back_label: "Back to Settings",
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
                title: "Error", message: "Private key file not found.",
                back_url: "/settings", back_label: "Back to Settings",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}
