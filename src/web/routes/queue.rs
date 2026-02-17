use askama::Template;
use axum::{extract::State, response::Html};
use log::{debug, error};
use std::path::Path;
use std::process::Command;

use crate::web::AppState;
use crate::web::auth::AuthAdmin;

const POSTQUEUE_PATHS: [&str; 2] = ["/usr/sbin/postqueue", "/usr/bin/postqueue"];

#[derive(Template)]
#[template(path = "queue/list.html")]
struct QueueTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    queue_output: String,
    error: Option<String>,
}

pub async fn list(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    debug!(
        "[web] GET /queue — queue page for username={}",
        auth.admin.username
    );

    let postqueue_bin = POSTQUEUE_PATHS
        .into_iter()
        .find(|path| Path::new(path).exists());

    let (queue_output, error) = match postqueue_bin {
        Some(postqueue_bin) => match Command::new(postqueue_bin).arg("-p").output() {
            Ok(output) if output.status.success() => {
                (String::from_utf8_lossy(&output.stdout).to_string(), None)
            }
            Ok(output) => {
                error!(
                    "[web] postqueue failed with status {}: {}",
                    output.status,
                    String::from_utf8_lossy(&output.stderr)
                );
                (
                    String::new(),
                    Some("Failed to read queue output from postqueue.".to_string()),
                )
            }
            Err(e) => {
                error!("[web] failed to run postqueue: {}", e);
                (
                    String::new(),
                    Some("Failed to run postqueue command.".to_string()),
                )
            }
        },
        None => (
            String::new(),
            Some("postqueue binary not found in /usr/sbin or /usr/bin.".to_string()),
        ),
    };

    let tmpl = QueueTemplate {
        nav_active: "Queue",
        flash: None,
        queue_output,
        error,
    };

    match tmpl.render() {
        Ok(html) => Html(html),
        Err(e) => {
            error!("[web] failed to render queue template: {}", e);
            crate::web::errors::render_error_page(
                axum::http::StatusCode::INTERNAL_SERVER_ERROR,
                "Template Error",
                "Failed to render queue page. Please try again.",
                "/",
                "Dashboard",
            )
        }
    }
}
