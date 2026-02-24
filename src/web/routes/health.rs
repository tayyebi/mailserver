use askama::Template;
use axum::{extract::State, response::Html};
use log::{error, info};
use std::path::Path;
use std::process::Command;

use crate::web::auth::AuthAdmin;
use crate::web::routes::queue::parse_queue_output;
use crate::web::AppState;

#[derive(Template)]
#[template(path = "health.html")]
struct HealthTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    outgoing_count: i64,
    open_count: i64,
    queue_summary: String,
    queue_entries: Vec<QueueRow>,
    queue_has_error: bool,
    queue_error: String,
    queue_has_entries: bool,
}

const POSTQUEUE_PATHS: [&str; 2] = ["/usr/sbin/postqueue", "/usr/bin/postqueue"];

#[derive(Clone)]
struct QueueRow {
    id: String,
    arrival_time: String,
    sender: String,
    recipients_display: String,
}

pub async fn page(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /health — delivery observability requested");
    let (outgoing_count, open_count) = state.blocking_db(|db| db.get_delivery_counters()).await;
    let (queue_entries, queue_summary, queue_error) = read_queue_snapshot();
    let queue_has_entries = !queue_entries.is_empty();

    let tmpl = HealthTemplate {
        nav_active: "Health",
        flash: None,
        outgoing_count,
        open_count,
        queue_summary,
        queue_entries,
        queue_has_error: queue_error.is_some(),
        queue_error: queue_error.unwrap_or_else(|| "".to_string()),
        queue_has_entries,
    };
    Html(tmpl.render().expect("failed to render health template"))
}

fn read_queue_snapshot() -> (Vec<QueueRow>, String, Option<String>) {
    let postqueue_bin = POSTQUEUE_PATHS
        .into_iter()
        .find(|path| Path::new(path).exists());

    match postqueue_bin {
        Some(bin) => match Command::new(bin).arg("-p").output() {
            Ok(output) if output.status.success() => {
                let raw = String::from_utf8_lossy(&output.stdout).to_string();
                let summary = raw
                    .lines()
                    .find(|l| l.starts_with("--"))
                    .unwrap_or("")
                    .trim_start_matches("--")
                    .trim()
                    .to_string();
                let mut entries = parse_queue_output(&raw)
                    .into_iter()
                    .map(|q| QueueRow {
                        id: q.id,
                        arrival_time: q.arrival_time,
                        sender: q.sender,
                        recipients_display: q.recipients.join(", "),
                    })
                    .collect::<Vec<_>>();
                if entries.len() > 5 {
                    entries.truncate(5);
                }
                (entries, summary, None)
            }
            Ok(output) => {
                error!(
                    "[health] postqueue failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
                (
                    Vec::new(),
                    String::new(),
                    Some("Failed to read queue output from postqueue.".to_string()),
                )
            }
            Err(e) => {
                error!("[health] failed to run postqueue: {}", e);
                (
                    Vec::new(),
                    String::new(),
                    Some("Failed to run postqueue command.".to_string()),
                )
            }
        },
        None => (
            Vec::new(),
            String::new(),
            Some("postqueue binary not found in /usr/sbin or /usr/bin.".to_string()),
        ),
    }
}
