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

/// Mask the last segment of an IP address for privacy.
/// IPv4: `192.168.1.100` → `192.168.1.x`
/// IPv6: `2001:db8::1`   → `2001:db8::x`
fn mask_ip(ip: &str) -> String {
    if ip.contains(':') {
        // IPv6: replace everything after the last ':' with 'x'
        if let Some(pos) = ip.rfind(':') {
            return format!("{}:x", &ip[..pos]);
        }
    } else if ip.contains('.') {
        // IPv4: replace last octet with 'x'
        if let Some(pos) = ip.rfind('.') {
            return format!("{}.x", &ip[..pos]);
        }
    }
    ip.to_string()
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

        // Mask last segment of IP for geo-location while preserving privacy
        let client_ip = mask_ip(&client_ip);

        let user_agent = req
            .headers()
            .get(header::USER_AGENT)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let message_id = params.id.clone();

        let db_message_id = message_id.clone();
        let db_client_ip = client_ip.clone();
        let db_user_agent = user_agent.clone();

        state
            .blocking_db(move |db| {
                db.record_pixel_open(&db_message_id, &db_client_ip, &db_user_agent)
            })
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

#[cfg(test)]
mod tests {
    use super::mask_ip;

    #[test]
    fn mask_ip_ipv4_last_octet() {
        assert_eq!(mask_ip("192.168.1.100"), "192.168.1.x");
        assert_eq!(mask_ip("10.0.0.1"), "10.0.0.x");
    }

    #[test]
    fn mask_ip_ipv6_last_group() {
        assert_eq!(mask_ip("2001:db8::1"), "2001:db8::x");
        assert_eq!(mask_ip("fe80::1"), "fe80::x");
        assert_eq!(mask_ip("::1"), "::x");
    }

    #[test]
    fn mask_ip_empty_unchanged() {
        assert_eq!(mask_ip(""), "");
    }
}
