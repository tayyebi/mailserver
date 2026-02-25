mod auth;
mod errors;
mod forms;
mod routes;

use axum::http::{StatusCode, Uri};
use axum::response::Response;
use axum::routing::get_service;
use axum::Router;
use log::{debug, info, warn};
use tower_http::services::ServeDir;

use crate::web::errors::status_response;

// ── Shared State ──

#[derive(Clone)]
pub struct AppState {
    pub db: crate::db::Database,
    pub hostname: String,
    pub admin_port: u16,
}

impl AppState {
    pub async fn blocking_db<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&crate::db::Database) -> R + Send + 'static,
        R: Send + 'static,
    {
        let db = self.db.clone();
        // Use std::thread instead of tokio::task::spawn_blocking to avoid "runtime within runtime" panic
        // because the synchronous postgres crate uses its own internal runtime which conflicts with
        // tokio's blocking thread pool context.
        let (tx, rx) = tokio::sync::oneshot::channel();

        std::thread::spawn(move || {
            let result = f(&db);
            let _ = tx.send(result);
        });

        rx.await.expect("Database thread panicked or was dropped")
    }
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

    let pixel_routes = routes::pixel::routes();
    let bimi_routes = routes::bimi::routes();
    let unsubscribe_routes = routes::unsubscribe::public_routes();
    let webdav_routes = routes::webdav::public_routes();
    let auth_routes = routes::auth_routes();

    let static_service = get_service(ServeDir::new(static_dir));

    let app = Router::new()
        .merge(pixel_routes)
        .merge(bimi_routes)
        .merge(unsubscribe_routes)
        .merge(webdav_routes)
        .merge(auth_routes)
        // CalDAV protocol handler — handles all HTTP methods on /caldav/{email}/...
        .route("/caldav/*path", axum::routing::any(routes::caldav::protocol_handler))
        // RFC 6764 well-known redirect for CalDAV auto-discovery
        .route(
            "/.well-known/caldav",
            axum::routing::any(|| async {
                axum::response::Redirect::permanent("/caldav/")
            }),
        )
        .nest_service("/static", static_service)
        .fallback(handle_not_found)
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind address {}: {}", addr, e));
    info!("[web] admin dashboard listening on {}", addr);
    axum::serve(listener, app).await.expect("Server error");
}

async fn handle_not_found(uri: Uri) -> Response {
    let message = format!("No page exists at {}", uri.path());
    status_response(
        StatusCode::NOT_FOUND,
        "Page not found",
        &message,
        "/",
        "Dashboard",
    )
}

pub(crate) async fn regen_configs(state: &AppState) {
    info!("[web] regenerating mail service configs");
    let db = state.db.clone();
    let hostname = state.hostname.clone();
    let (tx, rx) = tokio::sync::oneshot::channel();

    std::thread::spawn(move || {
        crate::config::generate_all_configs(&db, &hostname);
        let _ = tx.send(());
    });

    let _ = rx.await;
}

/// Fire a webhook notification for a system activity event.
///
/// This sends a POST request with a JSON payload to the configured webhook URL.
/// The call is non-blocking — it spawns a background thread so the HTTP response
/// to the admin is not delayed by the webhook delivery.
///
/// `event` — short event identifier (e.g. "domain.created", "account.deleted")
/// `details` — a JSON-serialisable value with event-specific information
pub(crate) fn fire_webhook(state: &AppState, event: &str, details: serde_json::Value) {
    let db = state.db.clone();
    let event = event.to_string();

    std::thread::spawn(move || {
        let webhook_url = db.get_setting("webhook_url").unwrap_or_default();
        if webhook_url.is_empty() {
            return;
        }

        let timestamp = chrono::Utc::now().to_rfc3339();
        let payload = serde_json::json!({
            "event": event,
            "timestamp": timestamp,
            "details": details,
        });
        let request_body = payload.to_string();

        debug!("[webhook] firing {} to {}", event, webhook_url);
        let start = std::time::Instant::now();

        let (response_status, response_body, error) = match reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
        {
            Ok(client) => match client.post(&webhook_url).json(&payload).send() {
                Ok(resp) => {
                    let status = resp.status().as_u16() as i32;
                    let body = resp.text().unwrap_or_default();
                    let body_truncated = if body.len() > 2048 {
                        let mut end = 2048;
                        while !body.is_char_boundary(end) {
                            end -= 1;
                        }
                        body[..end].to_string()
                    } else {
                        body
                    };
                    info!(
                        "[webhook] {} delivered to {} status={}",
                        event, webhook_url, status
                    );
                    (Some(status), body_truncated, String::new())
                }
                Err(e) => {
                    warn!(
                        "[webhook] {} delivery failed to {}: {}",
                        event, webhook_url, e
                    );
                    (None, String::new(), e.to_string())
                }
            },
            Err(e) => {
                warn!("[webhook] failed to build HTTP client: {}", e);
                (None, String::new(), e.to_string())
            }
        };

        let duration_ms = start.elapsed().as_millis() as i64;

        // Log the webhook execution (best-effort)
        db.log_webhook(
            &webhook_url,
            &request_body,
            response_status,
            &response_body,
            &error,
            duration_ms,
            &event,
            "",
        );
    });
}
