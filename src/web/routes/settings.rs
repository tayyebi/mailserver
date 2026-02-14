use askama::Template;
use axum::{
    extract::State,
    response::{Html, Response, IntoResponse},
    Form,
};
use log::{info, warn};

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

pub async fn page(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    log::debug!("[web] GET /settings — settings page for username={}", auth.admin.username);
    let tmpl = SettingsTemplate {
        nav_active: "Settings", flash: None,
        admin: auth.admin,
    };
    Html(tmpl.render().unwrap())
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
