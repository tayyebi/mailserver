use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use log::{debug, info, warn};

use crate::web::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/bimi/:domain/logo.svg", get(bimi_logo_handler))
}

async fn bimi_logo_handler(State(state): State<AppState>, Path(domain): Path<String>) -> Response {
    debug!("[web] GET /bimi/{}/logo.svg — BIMI logo requested", domain);

    let domain_log = domain.clone();
    let svg = state
        .blocking_db(move |db| db.get_bimi_svg_for_domain(&domain))
        .await;

    match svg {
        Some(svg_content) => {
            info!("[web] serving BIMI SVG for domain={}", domain_log);
            (
                StatusCode::OK,
                [
                    (header::CONTENT_TYPE, "image/svg+xml"),
                    (header::CACHE_CONTROL, "public, max-age=86400"),
                ],
                svg_content,
            )
                .into_response()
        }
        None => {
            warn!("[web] no BIMI SVG found for domain={}", domain_log);
            StatusCode::NOT_FOUND.into_response()
        }
    }
}
