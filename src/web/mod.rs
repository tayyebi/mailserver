mod auth;
mod errors;
mod forms;
mod routes;

use axum::http::{StatusCode, Uri};
use axum::response::Response;
use axum::routing::get_service;
use axum::Router;
use log::info;
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
    let auth_routes = routes::auth_routes();

    let static_service = get_service(ServeDir::new(static_dir));

    let app = Router::new()
        .merge(pixel_routes)
        .merge(bimi_routes)
        .merge(unsubscribe_routes)
        .merge(auth_routes)
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
