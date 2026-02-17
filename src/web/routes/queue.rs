use askama::Template;
use axum::{extract::State, response::Html};
use log::debug;
use std::process::Command;

use crate::web::AppState;
use crate::web::auth::AuthAdmin;

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

    let (queue_output, error) = match Command::new("postqueue").arg("-p").output() {
        Ok(output) if output.status.success() => {
            (String::from_utf8_lossy(&output.stdout).to_string(), None)
        }
        Ok(output) => (
            String::new(),
            Some(String::from_utf8_lossy(&output.stderr).to_string()),
        ),
        Err(e) => (String::new(), Some(format!("Failed to run postqueue: {}", e))),
    };

    let tmpl = QueueTemplate {
        nav_active: "Queue",
        flash: None,
        queue_output,
        error,
    };

    Html(tmpl.render().unwrap())
}
