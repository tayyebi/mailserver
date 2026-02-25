use askama::Template;
use axum::{extract::State, response::Html};
use log::info;

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Templates ──

#[derive(Template)]
#[template(path = "api_docs.html")]
struct ApiDocsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    hostname: &'a str,
    stats: crate::db::Stats,
}

// ── Handler ──

pub async fn page(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /api — REST documentation requested");
    let stats = state.blocking_db(|db| db.get_stats()).await;
    let tmpl = ApiDocsTemplate {
        nav_active: "API",
        flash: None,
        hostname: &state.hostname,
        stats,
    };
    Html(tmpl.render().unwrap())
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_docs_template_compiles() {
        // Verify the template struct can be constructed with valid data
        let stats = crate::db::Stats {
            domain_count: 1,
            account_count: 2,
            alias_count: 3,
            forwarding_count: 4,
            tracked_count: 5,
            open_count: 6,
            banned_count: 7,
            webhook_count: 8,
            unsubscribe_count: 9,
            dkim_ready_count: 1,
        };
        let tmpl = ApiDocsTemplate {
            nav_active: "API",
            flash: None,
            hostname: "mail.example.com",
            stats,
        };
        let rendered = tmpl.render().unwrap();
        assert!(rendered.contains("REST API"));
        assert!(rendered.contains("/api/emails"));
        assert!(rendered.contains("/pixel"));
        assert!(rendered.contains("/mcp"));
    }
}
