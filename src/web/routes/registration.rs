use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
    Form,
};
use log::{info, warn};
use serde::Deserialize;

use crate::web::fire_webhook;
use crate::web::AppState;

// ── Forms ──

#[derive(Deserialize)]
pub struct RegisterForm {
    pub username: String,
    pub name: String,
    pub password: String,
    pub confirm_password: String,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "registration/form.html")]
struct RegisterFormTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domain: String,
    username: String,
    username_preview: String,
    name: String,
    error: Option<String>,
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

// ── Helpers ──

/// Validate a username against optional domain regex and basic sanity rules.
///
/// Rules applied in order:
/// 1. Must be 3–64 characters.
/// 2. Must contain only letters, digits, dots, hyphens, and underscores.
/// 3. If `regex_pattern` is non-empty it must also match the regex.
fn validate_username(username: &str, regex_pattern: &str) -> Result<(), String> {
    let len = username.len();
    if len < 3 || len > 64 {
        return Err("Username must be between 3 and 64 characters.".into());
    }
    if !username
        .chars()
        .all(|c| c.is_alphanumeric() || c == '.' || c == '-' || c == '_')
    {
        return Err(
            "Username may only contain letters, digits, dots, hyphens, and underscores.".into(),
        );
    }
    if !regex_pattern.is_empty() {
        match regex::Regex::new(regex_pattern) {
            Ok(re) => {
                if !re.is_match(username) {
                    return Err(format!(
                        "Username does not meet the requirements for this domain."
                    ));
                }
            }
            Err(_) => {
                // Misconfigured regex — fail open (don't block registration due to admin error)
                warn!(
                    "[register] domain has invalid username regex '{}', ignoring",
                    regex_pattern
                );
            }
        }
    }
    Ok(())
}

// ── Handlers ──

/// Show the public registration form for a domain.
pub async fn show_form(
    State(state): State<AppState>,
    Path(domain): Path<String>,
) -> Response {
    info!("[web] GET /register/{} — registration form", domain);

    let domain_lower = domain.to_ascii_lowercase();
    let domain_record = state
        .blocking_db(move |db| db.get_domain_by_name(&domain_lower))
        .await;

    match domain_record {
        Some(d) if d.active && d.registration_enabled => {
            let tmpl = RegisterFormTemplate {
                nav_active: "",
                flash: None,
                domain: d.domain.clone(),
                username: String::new(),
                username_preview: format!("@{}", d.domain),
                name: String::new(),
                error: None,
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        _ => {
            let tmpl = ErrorTemplate {
                nav_active: "",
                flash: None,
                status_code: 404,
                status_text: "Not Found",
                title: "Registration Unavailable",
                message: "Registration is not available for this domain.",
                back_url: "/",
                back_label: "Home",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

/// Handle the registration form submission.
pub async fn handle_form(
    State(state): State<AppState>,
    Path(domain): Path<String>,
    Form(form): Form<RegisterForm>,
) -> Response {
    info!(
        "[web] POST /register/{} — registration attempt username={}",
        domain, form.username
    );

    let domain_lower = domain.to_ascii_lowercase();
    let domain_record = state
        .blocking_db(move |db| db.get_domain_by_name(&domain_lower))
        .await;

    let domain_obj = match domain_record {
        Some(d) if d.active && d.registration_enabled => d,
        _ => {
            let tmpl = ErrorTemplate {
                nav_active: "",
                flash: None,
                status_code: 404,
                status_text: "Not Found",
                title: "Registration Unavailable",
                message: "Registration is not available for this domain.",
                back_url: "/",
                back_label: "Home",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };

    let username = form.username.trim().to_ascii_lowercase();
    let name = form.name.trim().to_string();

    // Validate username
    if let Err(reason) = validate_username(&username, &domain_obj.registration_username_regex) {
        let tmpl = RegisterFormTemplate {
            nav_active: "",
            flash: None,
            domain: domain_obj.domain.clone(),
            username: username.clone(),
            username_preview: format!("{}@{}", username, domain_obj.domain),
            name: name.clone(),
            error: Some(reason),
        };
        return Html(tmpl.render().unwrap()).into_response();
    }

    // Validate password
    if form.password != form.confirm_password {
        let tmpl = RegisterFormTemplate {
            nav_active: "",
            flash: None,
            domain: domain_obj.domain.clone(),
            username: username.clone(),
            username_preview: format!("{}@{}", username, domain_obj.domain),
            name: name.clone(),
            error: Some("Passwords do not match.".into()),
        };
        return Html(tmpl.render().unwrap()).into_response();
    }
    if form.password.len() < 8 {
        let tmpl = RegisterFormTemplate {
            nav_active: "",
            flash: None,
            domain: domain_obj.domain.clone(),
            username: username.clone(),
            username_preview: format!("{}@{}", username, domain_obj.domain),
            name: name.clone(),
            error: Some("Password must be at least 8 characters.".into()),
        };
        return Html(tmpl.render().unwrap()).into_response();
    }

    // Hash the password
    let hash = match crate::auth::hash_password(&form.password) {
        Ok(h) => h,
        Err(e) => {
            warn!("[register] failed to hash password: {}", e);
            let tmpl = ErrorTemplate {
                nav_active: "",
                flash: None,
                status_code: 500,
                status_text: "Internal Server Error",
                title: "Error",
                message: "Failed to process your registration. Please try again.",
                back_url: &format!("/register/{}", domain_obj.domain),
                back_label: "Try Again",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };

    let domain_id = domain_obj.id;
    let username_clone = username.clone();
    let name_clone = name.clone();
    let domain_name = domain_obj.domain.clone();

    let result = state
        .blocking_db(move |db| db.create_account(domain_id, &username_clone, &hash, &name_clone, 0))
        .await;

    match result {
        Ok(_id) => {
            info!(
                "[register] new account created: {}@{}",
                username, domain_name
            );
            fire_webhook(
                &state,
                "account.registered",
                serde_json::json!({
                    "username": username,
                    "domain": domain_name,
                }),
            );
            // Regenerate configs so the new mailbox is active immediately.
            crate::web::regen_configs(&state).await;

            let tmpl = ErrorTemplate {
                nav_active: "",
                flash: None,
                status_code: 200,
                status_text: "OK",
                title: "Account Created",
                message: &format!(
                    "Your mailbox {}@{} has been created successfully. You can now log in.",
                    username, domain_name
                ),
                back_url: "/",
                back_label: "Home",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
        Err(e) => {
            warn!(
                "[register] failed to create account {}@{}: {}",
                username, domain_name, e
            );
            let reason = if e.contains("23505") || e.to_lowercase().contains("unique") || e.to_lowercase().contains("duplicate") {
                "That username is already taken on this domain.".to_string()
            } else {
                "Registration failed. Please try again.".to_string()
            };
            let tmpl = RegisterFormTemplate {
                nav_active: "",
                flash: None,
                domain: domain_name.clone(),
                username: username.clone(),
                username_preview: format!("{}@{}", username, domain_name),
                name,
                error: Some(reason),
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}
