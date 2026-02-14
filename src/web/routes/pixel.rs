use axum::{
    extract::{Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Router,
};
use log::{debug, info};

use crate::web::forms::PixelQuery;
use crate::web::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/pixel", get(pixel_handler))
}

async fn pixel_handler(
    State(state): State<AppState>,
    Query(params): Query<PixelQuery>,
    req: axum::http::Request<axum::body::Body>,
) -> Response {
    debug!(
        "[web] GET /pixel — pixel request id={}",
        if params.id.is_empty() {
            "(empty)"
        } else {
            &params.id
        }
    );
    if !params.id.is_empty() {
        let client_ip = req
            .headers()
            .get("x-forwarded-for")
            .and_then(|v| v.to_str().ok())
            .map(|s| s.split(',').next().unwrap_or("").trim().to_string())
            .or_else(|| {
                req.headers()
                    .get("x-real-ip")
                    .and_then(|v| v.to_str().ok())
                    .map(|s| s.to_string())
            })
            .unwrap_or_default();

        let user_agent = req
            .headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let message_id = params.id.clone();
        state
            .blocking_db(move |db| db.record_pixel_open(&message_id, &client_ip, &user_agent))
            .await;
        info!(
            "[web] pixel open recorded: message_id={}, client_ip={}, user_agent={}",
            message_id, client_ip, user_agent
        );
    }

    let gif: &[u8] = &[
        0x47, 0x49, 0x46, 0x38, 0x39, 0x61, 0x01, 0x00, 0x01, 0x00, 0x80, 0x00, 0x00, 0xff, 0xff,
        0xff, 0x00, 0x00, 0x00, 0x21, 0xf9, 0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x2c, 0x00, 0x00,
        0x00, 0x00, 0x01, 0x00, 0x01, 0x00, 0x00, 0x02, 0x02, 0x44, 0x01, 0x00, 0x3b,
    ];

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "image/gif")],
        gif.to_vec(),
    )
        .into_response()
}
