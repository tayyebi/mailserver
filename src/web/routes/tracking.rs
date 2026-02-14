use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Response},
};
use log::{debug, info, warn};

use crate::db::PixelOpen;
use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── View models ──

struct TrackingRow {
    message_id: String,
    message_id_short: String,
    sender: String,
    recipient: String,
    subject: String,
    created_at: String,
    open_count: usize,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "tracking/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    messages: Vec<TrackingRow>,
}

#[derive(Template)]
#[template(path = "tracking/detail.html")]
struct DetailTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    message: crate::db::TrackedMessage,
    opens: Vec<PixelOpen>,
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

// ── Handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /tracking — listing tracked messages");
    let raw_messages = state.db.list_tracked_messages(100);
    debug!("[web] found {} tracked messages", raw_messages.len());

    let messages: Vec<TrackingRow> = raw_messages
        .into_iter()
        .map(|m| {
            let open_count = state.db.get_opens_for_message(&m.message_id).len();
            let message_id_short = if m.message_id.len() > 20 {
                m.message_id[..20].to_string()
            } else {
                m.message_id.clone()
            };
            TrackingRow {
                message_id: m.message_id,
                message_id_short,
                sender: m.sender,
                recipient: m.recipient,
                subject: m.subject,
                created_at: m.created_at,
                open_count,
            }
        })
        .collect();

    let tmpl = ListTemplate {
        nav_active: "Tracking",
        flash: None,
        messages,
    };
    Html(tmpl.render().unwrap())
}

pub async fn detail(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(msg_id): Path<String>,
) -> Response {
    debug!("[web] GET /tracking/{} — tracking detail requested", msg_id);
    let message = match state.db.get_tracked_message(&msg_id) {
        Some(m) => m,
        None => {
            warn!("[web] tracked message not found: {}", msg_id);
            let tmpl = ErrorTemplate {
                nav_active: "Tracking",
                flash: None,
                status_code: 404,
                status_text: "Not Found",
                title: "Not Found",
                message: "Message not found.",
                back_url: "/tracking",
                back_label: "Back to Tracking",
            };
            return Html(tmpl.render().unwrap()).into_response();
        }
    };
    let opens = state.db.get_opens_for_message(&msg_id);
    debug!("[web] tracked message {} has {} opens", msg_id, opens.len());

    let tmpl = DetailTemplate {
        nav_active: "Tracking",
        flash: None,
        message,
        opens,
    };
    Html(tmpl.render().unwrap()).into_response()
}
