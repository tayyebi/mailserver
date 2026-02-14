use askama::Template;
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use log::{debug, info};
use serde::Deserialize;
use tokio::join;

use crate::web::auth::AuthAdmin;
use crate::web::errors::status_response;
use crate::web::AppState;

// ── Query parameters ──

#[derive(Deserialize)]
pub struct PaginationQuery {
    pub page: Option<i64>,
}

// ── View models ──

struct EmailLogRow {
    id: i64,
    message_id: String,
    sender: String,
    recipient: String,
    subject: String,
    direction: String,
    logged_at: String,
}

struct ConnectionLogRow {
    id: i64,
    log_type: String,
    username: String,
    client_ip: String,
    status: String,
    details: Option<String>,
    logged_at: String,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "logs/email.html")]
struct EmailLogsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    email_rows: Vec<EmailLogRow>,
    current_page: i64,
    total_pages: i64,
    total_count: i64,
}

#[derive(Template)]
#[template(path = "logs/connection.html")]
struct ConnectionLogsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    connection_rows: Vec<ConnectionLogRow>,
    current_page: i64,
    total_pages: i64,
    total_count: i64,
}

// ── Handlers ──

pub async fn email_logs(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(params): Query<PaginationQuery>,
) -> Html<String> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = 20i64;
    let offset = (page - 1) * per_page;

    info!("[web] GET /logs/email — page={}", page);

    let total_count_fut = state.blocking_db(|db| db.count_email_logs());
    let emails_fut = state.blocking_db(move |db| db.list_email_logs(per_page, offset));
    let (total_count, emails) = join!(total_count_fut, emails_fut);
    let total_pages = (total_count + per_page - 1) / per_page;
    debug!("[web] found {} email logs", emails.len());

    let email_rows: Vec<EmailLogRow> = emails
        .iter()
        .map(|e| EmailLogRow {
            id: e.id,
            message_id: e.message_id.clone(),
            sender: e.sender.clone(),
            recipient: e.recipient.clone(),
            subject: e.subject.clone(),
            direction: e.direction.clone(),
            logged_at: e.logged_at.clone(),
        })
        .collect();

    let tmpl = EmailLogsTemplate {
        nav_active: "Email Logs",
        flash: None,
        email_rows,
        current_page: page,
        total_pages,
        total_count,
    };
    Html(tmpl.render().unwrap())
}

pub async fn connection_logs(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Query(params): Query<PaginationQuery>,
) -> Html<String> {
    let page = params.page.unwrap_or(1).max(1);
    let per_page = 20i64;
    let offset = (page - 1) * per_page;

    info!("[web] GET /logs/connection — page={}", page);

    let total_count_fut = state.blocking_db(|db| db.count_connection_logs());
    let connections_fut = state.blocking_db(move |db| db.list_connection_logs(per_page, offset));
    let (total_count, connections) = join!(total_count_fut, connections_fut);
    let total_pages = (total_count + per_page - 1) / per_page;
    debug!("[web] found {} connection logs", connections.len());

    let connection_rows: Vec<ConnectionLogRow> = connections
        .iter()
        .map(|c| ConnectionLogRow {
            id: c.id,
            log_type: c.log_type.clone(),
            username: c.username.clone(),
            client_ip: c.client_ip.clone(),
            status: c.status.clone(),
            details: c.details.clone(),
            logged_at: c.logged_at.clone(),
        })
        .collect();

    let tmpl = ConnectionLogsTemplate {
        nav_active: "Connection Logs",
        flash: None,
        connection_rows,
        current_page: page,
        total_pages,
        total_count,
    };
    Html(tmpl.render().unwrap())
}

pub async fn email_download(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    info!("[web] GET /logs/email/{}/download — downloading email", id);
    let email = state.blocking_db(move |db| db.get_email_log(id)).await;
    match email {
        Some(email) => (
            StatusCode::OK,
            [
                ("Content-Type", "message/rfc822"),
                ("Content-Disposition", "attachment; filename=\"email.eml\""),
            ],
            email.raw_message,
        )
            .into_response(),
        None => status_response(
            StatusCode::NOT_FOUND,
            "Email not found",
            "We could not locate an email log with that identifier.",
            "/logs/email",
            "Email logs",
        ),
    }
}
