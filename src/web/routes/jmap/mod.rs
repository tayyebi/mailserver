pub mod core;
pub mod email;
pub mod mailbox;
pub mod thread;

use axum::{
    extract::{FromRef, FromRequestParts, State},
    http::{header, request::Parts, StatusCode},
    response::{IntoResponse, Response},
    Json, Router,
};
use log::warn;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::super::AppState;

// ── Auth extractor ─────────────────────────────────────────────────────────────

pub struct JmapAuth {
    pub account: crate::db::Account,
    pub account_id: i64,
}

fn unauthorized() -> Response {
    Response::builder()
        .status(StatusCode::UNAUTHORIZED)
        .header(header::WWW_AUTHENTICATE, r#"Bearer realm="JMAP""#)
        .header(header::CONTENT_TYPE, "application/json; charset=utf-8")
        .body(axum::body::Body::from(
            r#"{"type":"urn:ietf:params:jmap:error:unauthorized","detail":"Valid token required"}"#,
        ))
        .unwrap()
}

#[axum::async_trait]
impl<S> FromRequestParts<S> for JmapAuth
where
    S: Send + Sync,
    AppState: FromRef<S>,
{
    type Rejection = Response;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let app_state = AppState::from_ref(state);

        let auth_header = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| {
                warn!("[jmap] missing Authorization header");
                unauthorized()
            })?;

        let token = auth_header.strip_prefix("Bearer ").ok_or_else(|| {
            warn!("[jmap] unsupported auth scheme");
            unauthorized()
        })?;

        let token = token.trim().to_string();

        let result = app_state
            .blocking_db(move |db| db.verify_jmap_token(&token))
            .await;

        match result {
            Some(account) => {
                let account_id = account.id;
                Ok(JmapAuth {
                    account,
                    account_id,
                })
            }
            None => {
                warn!("[jmap] invalid token");
                Err(unauthorized())
            }
        }
    }
}

// ── JMAP protocol types ────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct JmapRequest {
    #[allow(dead_code)]
    pub using: Vec<String>,
    #[serde(rename = "methodCalls")]
    pub method_calls: Vec<MethodCall>,
    #[serde(rename = "createdIds", default)]
    #[allow(dead_code)]
    pub created_ids: Option<serde_json::Map<String, Value>>,
}

#[derive(Debug, Deserialize)]
pub struct MethodCall {
    #[serde(rename = "0")]
    pub name: String,
    #[serde(rename = "1")]
    pub args: serde_json::Value,
    #[serde(rename = "2")]
    pub call_id: String,
}

#[derive(Debug, Serialize)]
pub struct JmapResponse {
    #[serde(rename = "sessionState")]
    pub session_state: String,
    #[serde(rename = "methodResponses")]
    pub method_responses: Vec<Vec<Value>>,
    #[serde(rename = "createdIds")]
    pub created_ids: serde_json::Map<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct JmapError {
    #[serde(rename = "type")]
    pub type_: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

// ── Router ────────────────────────────────────────────────────────────────────

pub fn jmap_routes() -> Router<AppState> {
    Router::new()
        .route("/.well-known/jmap", axum::routing::get(core::well_known))
        .route("/jmap/auth", axum::routing::post(core::auth_login))
        .route("/jap/session", axum::routing::get(core::session_resource))
        .route("/jap", axum::routing::post(api_handler))
        .route(
            "/jap/download/{account_id}/{blob_id}",
            axum::routing::get(core::download_blob),
        )
}

// ── API dispatcher ─────────────────────────────────────────────────────────────

async fn api_handler(
    auth: JmapAuth,
    State(state): State<AppState>,
    Json(req): Json<JmapRequest>,
) -> Response {
    let session_state = get_session_state(&state, auth.account_id).await;
    let mut responses = Vec::new();

    for mc in req.method_calls {
        let result = dispatch_method(&state, &auth, &mc.name, &mc.args).await;
        responses.push(vec![
            Value::String(result.name),
            result.args,
            Value::String(mc.call_id),
        ]);
    }

    let resp = JmapResponse {
        session_state,
        method_responses: responses,
        created_ids: serde_json::Map::new(),
    };

    (StatusCode::OK, Json(resp)).into_response()
}

pub(crate) struct DispatchResult {
    name: String,
    args: Value,
}

async fn dispatch_method(
    state: &AppState,
    auth: &JmapAuth,
    method: &str,
    args: &Value,
) -> DispatchResult {
    match method {
        "Mailbox/get" => mailbox::mailbox_get(state, auth, args).await,
        "Mailbox/getChanges" => mailbox::mailbox_get_changes(state, auth, args).await,
        "Email/get" => email::email_get(state, auth, args).await,
        "Email/query" => email::email_query(state, auth, args).await,
        "Email/getChanges" => email::email_get_changes(state, auth, args).await,
        "Thread/get" => thread::thread_get(state, auth, args).await,
        _ => DispatchResult {
            name: "error".to_string(),
            args: serde_json::to_value(JmapError {
                type_: "urn:ietf:params:jmap:error:unknownMethod".to_string(),
                description: Some(format!("Method not supported: {}", method)),
            })
            .unwrap(),
        },
    }
}

// ── State helpers ──────────────────────────────────────────────────────────────

pub async fn get_session_state(state: &AppState, account_id: i64) -> String {
    state
        .blocking_db(move |db| db.get_jmap_state(account_id).0)
        .await
}

pub fn account_email(account: &crate::db::Account) -> String {
    format!(
        "{}@{}",
        account.username,
        account.domain_name.as_deref().unwrap_or("")
    )
}

pub fn maildir_path(domain: &str, username: &str) -> String {
    format!("/data/mail/{}/{}/Maildir", domain, username)
}

pub fn mailbox_dir(maildir_base: &str, mailbox_id: &str) -> String {
    if mailbox_id == "mailbox:inbox" {
        maildir_base.to_string()
    } else if let Some(name) = mailbox_id.strip_prefix("mailbox:") {
        format!("{}/.{}", maildir_base, name)
    } else {
        maildir_base.to_string()
    }
}

pub fn mailbox_id_from_dir(name: &str) -> String {
    if name.is_empty() || name == "INBOX" {
        "mailbox:inbox".to_string()
    } else {
        format!("mailbox:{}", name)
    }
}

pub fn scan_maildir_folders(base: &str) -> Vec<(String, String)> {
    use std::fs;
    let mut folders = vec![("".to_string(), "INBOX".to_string())];

    let dir = match fs::read_dir(base) {
        Ok(d) => d,
        Err(_) => return folders,
    };

    for entry in dir.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        if !name.starts_with('.') || !entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
            continue;
        }
        // Check it's a Maildir (has cur/ subdir)
        let cur_path = entry.path().join("cur");
        if !cur_path.is_dir() {
            continue;
        }
        let display = name.strip_prefix('.').unwrap_or(&name);
        folders.push((name.clone(), display.to_string()));
    }

    folders
}
