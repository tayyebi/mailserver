mod auth;
mod forms;
mod routes;

use axum::Router;
use tower_http::services::ServeDir;
use log::info;

// ── Shared State ──

#[derive(Clone)]
pub struct AppState {
    pub db: crate::db::Database,
    pub hostname: String,
    pub admin_port: u16,
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
    let auth_routes = routes::auth_routes();

    let app = Router::new()
        .merge(pixel_routes)
        .merge(auth_routes)
        .nest_service("/static", ServeDir::new(static_dir))
        .with_state(state);

    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .unwrap_or_else(|e| panic!("Failed to bind address {}: {}", addr, e));
    info!("[web] admin dashboard listening on {}", addr);
    axum::serve(listener, app).await.expect("Server error");
}

pub(crate) fn regen_configs(state: &AppState) {
    info!("[web] regenerating mail service configs");
    crate::config::generate_all_configs(&state.db, &state.hostname);
}
