use axum::{
    extract::{FromRef, FromRequestParts},
    http::{header, request::Parts, StatusCode},
    response::Response,
};
use log::{debug, error, info, warn};

use super::AppState;
use crate::web::errors::render_error_page;

pub struct AuthAdmin {
    pub admin: crate::db::Admin,
}

fn unauthorized() -> Response {
    warn!("[web] unauthorized access attempt");
    let body = render_error_page(
        StatusCode::UNAUTHORIZED,
        "Unauthorized",
        "Valid admin credentials are required to reach this section.",
        "/",
        "Dashboard",
    );
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, "Basic realm=\"Mailserver Admin\"")
        .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
        .body(axum::body::Body::from(body.0))
        .expect("Failed to build unauthorized response")
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
            warn!(
                "[web] failed to decode base64 credentials for {}",
                parts.uri
            );
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
                warn!(
                    "[web] authentication failed — unknown username={}",
                    username
                );
                unauthorized()
            })?;

        if admin.totp_enabled {
            debug!(
                "[web] TOTP enabled for username={}, verifying password+TOTP",
                username
            );
            if password.len() < 6 {
                warn!(
                    "[web] authentication failed — password too short for TOTP for username={}",
                    username
                );
                return Err(unauthorized());
            }
            let (base_password, totp_code) = password.split_at(password.len() - 6);
            if !crate::auth::verify_password(base_password, &admin.password_hash) {
                warn!(
                    "[web] authentication failed — wrong password for username={}",
                    username
                );
                return Err(unauthorized());
            }
            let secret = admin.totp_secret.as_deref().ok_or_else(|| {
                error!(
                    "[web] TOTP enabled but no secret stored for username={}",
                    username
                );
                unauthorized()
            })?;
            if !crate::auth::verify_totp(secret, totp_code) {
                warn!(
                    "[web] authentication failed — invalid TOTP code for username={}",
                    username
                );
                return Err(unauthorized());
            }
        } else if !crate::auth::verify_password(password, &admin.password_hash) {
            warn!(
                "[web] authentication failed — wrong password for username={}",
                username
            );
            return Err(unauthorized());
        }

        info!("[web] authentication succeeded for username={}", username);
        Ok(AuthAdmin { admin })
    }
}
