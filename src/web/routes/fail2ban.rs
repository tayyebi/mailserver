use askama::Template;
use axum::{
    extract::{Path, State},
    http::{header, HeaderMap, StatusCode},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{debug, error, info, warn};

use crate::web::auth::AuthAdmin;
use crate::web::fire_webhook;
use crate::web::forms::{
    Fail2banBanForm, Fail2banGlobalToggleForm, Fail2banListForm, Fail2banSettingForm,
};
use crate::web::AppState;

fn same_origin(headers: &HeaderMap) -> bool {
    let host = match headers.get(header::HOST).and_then(|v| v.to_str().ok()) {
        Some(v) => v,
        None => return false,
    };
    let matches_host = |value: &str| {
        let rest = match value.split_once("://") {
            Some((_, rest)) => rest,
            None => return false,
        };
        let authority = rest.split('/').next().unwrap_or(rest);
        let authority = authority.rsplit('@').next().unwrap_or(authority);
        authority.eq_ignore_ascii_case(host)
    };

    if let Some(origin) = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()) {
        return matches_host(origin);
    }
    if let Some(referer) = headers.get(header::REFERER).and_then(|v| v.to_str().ok()) {
        return matches_host(referer);
    }
    false
}

fn is_valid_ip_or_cidr(input: &str) -> bool {
    let trimmed = input.trim();
    if trimmed.is_empty() {
        return false;
    }
    // Allow IP addresses and CIDR notation (e.g., 192.168.1.0/24)
    let ip_part = if let Some((ip, prefix)) = trimmed.split_once('/') {
        if let Ok(p) = prefix.parse::<u8>() {
            if p > 128 {
                return false;
            }
        } else {
            return false;
        }
        ip
    } else {
        trimmed
    };

    // Validate IPv4
    let parts: Vec<&str> = ip_part.split('.').collect();
    if parts.len() == 4 {
        return parts.iter().all(|p| p.parse::<u8>().is_ok());
    }

    // Validate IPv6 (basic check)
    if ip_part.contains(':') {
        return ip_part.split(':').count() >= 2;
    }

    false
}

// ── Templates ──

#[derive(Template)]
#[template(path = "fail2ban/overview.html")]
struct Fail2banOverviewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    fail2ban_enabled: bool,
    settings: Vec<crate::db::Fail2banSetting>,
    banned: Vec<crate::db::Fail2banBanned>,
    whitelist: Vec<crate::db::Fail2banWhitelist>,
    blacklist: Vec<crate::db::Fail2banBlacklist>,
    log_entries: Vec<crate::db::Fail2banLogEntry>,
    banned_count: i64,
    whitelist_count: usize,
    blacklist_count: usize,
}

#[derive(Template)]
#[template(path = "fail2ban/edit_setting.html")]
struct EditSettingTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    setting: crate::db::Fail2banSetting,
}

// ── Handlers ──

pub async fn overview(auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    debug!(
        "[web] GET /fail2ban — fail2ban overview for username={}",
        auth.admin.username
    );

    let settings_fut = state.blocking_db(|db| db.list_fail2ban_settings());
    let banned_fut = state.blocking_db(|db| db.list_fail2ban_banned());
    let whitelist_fut = state.blocking_db(|db| db.list_fail2ban_whitelist());
    let blacklist_fut = state.blocking_db(|db| db.list_fail2ban_blacklist());
    let log_fut = state.blocking_db(|db| db.list_fail2ban_log(50));
    let enabled_fut = state.blocking_db(|db| db.is_fail2ban_enabled());

    let (settings, banned, whitelist, blacklist, log_entries, fail2ban_enabled) = tokio::join!(
        settings_fut,
        banned_fut,
        whitelist_fut,
        blacklist_fut,
        log_fut,
        enabled_fut
    );

    let banned_count = banned.len() as i64;
    let whitelist_count = whitelist.len();
    let blacklist_count = blacklist.len();

    let tmpl = Fail2banOverviewTemplate {
        nav_active: "Fail2ban",
        flash: None,
        fail2ban_enabled,
        settings,
        banned,
        whitelist,
        blacklist,
        log_entries,
        banned_count,
        whitelist_count,
        blacklist_count,
    };
    match tmpl.render() {
        Ok(html) => Html(html),
        Err(e) => {
            error!("[web] failed to render fail2ban template: {}", e);
            crate::web::errors::render_error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Template Error",
                "Failed to render fail2ban page.",
                "/",
                "Dashboard",
            )
        }
    }
}

pub async fn toggle_system(
    auth: AuthAdmin,
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<Fail2banGlobalToggleForm>,
) -> Response {
    info!(
        "[web] POST /fail2ban/toggle — toggle fail2ban system for username={}",
        auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban toggle blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    let enabled = form.enabled.as_deref() == Some("on");
    let value = if enabled { "true" } else { "false" };
    state
        .blocking_db(move |db| db.set_setting("fail2ban_enabled", value))
        .await;

    info!("[web] fail2ban system toggled to: {}", value);
    fire_webhook(
        &state,
        "fail2ban.toggled",
        serde_json::json!({"enabled": enabled}),
    );
    Redirect::to("/fail2ban").into_response()
}

pub async fn edit_setting_form(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    debug!(
        "[web] GET /fail2ban/settings/{} — edit form for username={}",
        id, auth.admin.username
    );
    let setting = state
        .blocking_db(move |db| db.get_fail2ban_setting(id))
        .await;
    match setting {
        Some(setting) => {
            let tmpl = EditSettingTemplate {
                nav_active: "Fail2ban",
                flash: None,
                setting,
            };
            match tmpl.render() {
                Ok(html) => Html(html).into_response(),
                Err(e) => {
                    error!("[web] failed to render edit setting template: {}", e);
                    crate::web::errors::status_response(
                        StatusCode::INTERNAL_SERVER_ERROR,
                        "Template Error",
                        "Failed to render page.",
                        "/fail2ban",
                        "Back to Fail2ban",
                    )
                }
            }
        }
        None => crate::web::errors::status_response(
            StatusCode::NOT_FOUND,
            "Setting Not Found",
            "The requested fail2ban setting was not found.",
            "/fail2ban",
            "Back to Fail2ban",
        ),
    }
}

pub async fn update_setting(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
    Form(form): Form<Fail2banSettingForm>,
) -> Response {
    info!(
        "[web] POST /fail2ban/settings/{} — update setting for username={}",
        id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban setting update blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    if form.max_attempts < 1 || form.ban_duration_minutes < 1 || form.find_time_minutes < 1 {
        return crate::web::errors::status_response(
            StatusCode::BAD_REQUEST,
            "Invalid Values",
            "All threshold values must be at least 1.",
            &format!("/fail2ban/settings/{}/edit", id),
            "Back",
        );
    }

    let enabled = form.enabled.as_deref() == Some("on");
    state
        .blocking_db(move |db| {
            db.update_fail2ban_setting(
                id,
                form.max_attempts,
                form.ban_duration_minutes,
                form.find_time_minutes,
                enabled,
            )
        })
        .await;

    Redirect::to("/fail2ban").into_response()
}

pub async fn ban_ip(
    auth: AuthAdmin,
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<Fail2banBanForm>,
) -> Response {
    info!(
        "[web] POST /fail2ban/ban — ban IP for username={}",
        auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban ban blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    let ip = form.ip_address.trim().to_string();
    if !is_valid_ip_or_cidr(&ip) {
        return crate::web::errors::status_response(
            StatusCode::BAD_REQUEST,
            "Invalid IP Address",
            "Please enter a valid IP address or CIDR range.",
            "/fail2ban",
            "Back",
        );
    }

    let service = if form.service.trim().is_empty() {
        "all".to_string()
    } else {
        form.service.trim().to_string()
    };
    let reason = form.reason.trim().to_string();
    let permanent = form.permanent.as_deref() == Some("on");
    let duration = form.duration_minutes.unwrap_or(60);

    let ip_for_webhook = form.ip_address.trim().to_string();
    let service_for_webhook = service.clone();
    state
        .blocking_db(move |db| db.ban_ip(&ip, &service, &reason, duration, permanent))
        .await
        .ok();

    fire_webhook(
        &state,
        "fail2ban.ip_banned",
        serde_json::json!({"ip": ip_for_webhook, "service": service_for_webhook}),
    );
    Redirect::to("/fail2ban").into_response()
}

pub async fn unban_ip(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    info!(
        "[web] POST /fail2ban/unban/{} — unban IP for username={}",
        id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban unban blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    state.blocking_db(move |db| db.unban_ip(id)).await;
    fire_webhook(
        &state,
        "fail2ban.ip_unbanned",
        serde_json::json!({"id": id}),
    );
    Redirect::to("/fail2ban").into_response()
}

pub async fn add_whitelist(
    auth: AuthAdmin,
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<Fail2banListForm>,
) -> Response {
    info!(
        "[web] POST /fail2ban/whitelist — add to whitelist for username={}",
        auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban whitelist add blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    let ip = form.ip_address.trim().to_string();
    if !is_valid_ip_or_cidr(&ip) {
        return crate::web::errors::status_response(
            StatusCode::BAD_REQUEST,
            "Invalid IP Address",
            "Please enter a valid IP address or CIDR range.",
            "/fail2ban",
            "Back",
        );
    }

    let description = form.description.trim().to_string();
    state
        .blocking_db(move |db| db.add_to_whitelist(&ip, &description))
        .await
        .ok();

    Redirect::to("/fail2ban").into_response()
}

pub async fn remove_whitelist(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    info!(
        "[web] POST /fail2ban/whitelist/{}/delete for username={}",
        id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban whitelist remove blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    state
        .blocking_db(move |db| db.remove_from_whitelist(id))
        .await;
    Redirect::to("/fail2ban").into_response()
}

pub async fn add_blacklist(
    auth: AuthAdmin,
    State(state): State<AppState>,
    headers: HeaderMap,
    Form(form): Form<Fail2banListForm>,
) -> Response {
    info!(
        "[web] POST /fail2ban/blacklist — add to blacklist for username={}",
        auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban blacklist add blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    let ip = form.ip_address.trim().to_string();
    if !is_valid_ip_or_cidr(&ip) {
        return crate::web::errors::status_response(
            StatusCode::BAD_REQUEST,
            "Invalid IP Address",
            "Please enter a valid IP address or CIDR range.",
            "/fail2ban",
            "Back",
        );
    }

    let description = form.description.trim().to_string();
    state
        .blocking_db(move |db| db.add_to_blacklist(&ip, &description))
        .await
        .ok();

    Redirect::to("/fail2ban").into_response()
}

pub async fn remove_blacklist(
    auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    headers: HeaderMap,
) -> Response {
    info!(
        "[web] POST /fail2ban/blacklist/{}/delete for username={}",
        id, auth.admin.username
    );

    if !same_origin(&headers) {
        warn!("[web] fail2ban blacklist remove blocked: non same-origin request");
        return StatusCode::FORBIDDEN.into_response();
    }

    state
        .blocking_db(move |db| db.remove_from_blacklist(id))
        .await;
    Redirect::to("/fail2ban").into_response()
}

#[cfg(test)]
mod tests {
    use super::is_valid_ip_or_cidr;

    #[test]
    fn valid_ipv4() {
        assert!(is_valid_ip_or_cidr("192.168.1.1"));
        assert!(is_valid_ip_or_cidr("10.0.0.1"));
        assert!(is_valid_ip_or_cidr("0.0.0.0"));
        assert!(is_valid_ip_or_cidr("255.255.255.255"));
    }

    #[test]
    fn valid_cidr() {
        assert!(is_valid_ip_or_cidr("192.168.1.0/24"));
        assert!(is_valid_ip_or_cidr("10.0.0.0/8"));
        assert!(is_valid_ip_or_cidr("172.16.0.0/12"));
    }

    #[test]
    fn valid_ipv6() {
        assert!(is_valid_ip_or_cidr("::1"));
        assert!(is_valid_ip_or_cidr("2001:db8::1"));
        assert!(is_valid_ip_or_cidr("fe80::1"));
    }

    #[test]
    fn invalid_ips() {
        assert!(!is_valid_ip_or_cidr(""));
        assert!(!is_valid_ip_or_cidr("not-an-ip"));
        assert!(!is_valid_ip_or_cidr("256.1.1.1"));
        assert!(!is_valid_ip_or_cidr("192.168.1.0/129"));
    }

    #[test]
    fn same_origin_checks() {
        use super::same_origin;
        use axum::http::{header, HeaderMap, HeaderValue};

        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, HeaderValue::from_static("mail.example.com"));
        headers.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://mail.example.com"),
        );
        assert!(same_origin(&headers));

        let mut headers2 = HeaderMap::new();
        headers2.insert(header::HOST, HeaderValue::from_static("mail.example.com"));
        headers2.insert(
            header::ORIGIN,
            HeaderValue::from_static("https://evil.example"),
        );
        assert!(!same_origin(&headers2));
    }
}
