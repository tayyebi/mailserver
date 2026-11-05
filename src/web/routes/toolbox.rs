use askama::Template;
use axum::{
    extract::Query,
    http::StatusCode,
    response::{Html, IntoResponse, Response},
};
use log::{debug, error};
use serde::Deserialize;

use crate::spf::{self, SystemResolver};
use crate::web::auth::AuthAdmin;

#[derive(Deserialize)]
pub struct ToolboxParams {
    #[serde(default)]
    domain: String,
}

struct LineRow {
    pad: String,
    class: &'static str,
    text: String,
}

#[derive(Template)]
#[template(path = "toolbox/index.html")]
struct ToolboxTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domain: String,
    input_error: Option<String>,
    searched: bool,
    lines: Vec<LineRow>,
    lookups: usize,
    lookup_limit: usize,
    over_limit: bool,
}

/// Accepts a plain hostname with at least two labels, such as `example.com`.
fn is_valid_domain(domain: &str) -> bool {
    if domain.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    labels.len() >= 2
        && labels.iter().all(|label| {
            !label.is_empty()
                && label.len() <= 63
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-')
        })
}

fn render(tmpl: ToolboxTemplate<'_>) -> Response {
    match tmpl.render() {
        Ok(html) => Html(html).into_response(),
        Err(e) => {
            error!("[web] failed to render toolbox template: {}", e);
            crate::web::errors::render_error_page(
                StatusCode::INTERNAL_SERVER_ERROR,
                "Template Error",
                "Failed to render the toolbox page. Please try again.",
                "/toolbox",
                "Toolbox",
            )
            .into_response()
        }
    }
}

pub async fn page(_auth: AuthAdmin, Query(params): Query<ToolboxParams>) -> Response {
    let domain = params
        .domain
        .trim()
        .trim_end_matches('.')
        .to_ascii_lowercase();
    debug!("[web] GET /toolbox — spf domain={:?}", domain);

    let empty = |input_error: Option<String>| ToolboxTemplate {
        nav_active: "Toolbox",
        flash: None,
        domain: domain.clone(),
        input_error,
        searched: false,
        lines: Vec::new(),
        lookups: 0,
        lookup_limit: spf::MAX_DNS_LOOKUPS,
        over_limit: false,
    };

    if domain.is_empty() {
        return render(empty(None));
    }
    if !is_valid_domain(&domain) {
        return render(empty(Some(format!(
            "\"{}\" is not a valid domain name.",
            domain
        ))));
    }

    let target = domain.clone();
    let result = tokio::task::spawn_blocking(move || spf::trace(&SystemResolver, &target)).await;
    match result {
        Ok(trace) => render(ToolboxTemplate {
            nav_active: "Toolbox",
            flash: None,
            domain: domain.clone(),
            input_error: None,
            searched: true,
            lines: trace
                .lines
                .into_iter()
                .map(|l| LineRow {
                    pad: " ".repeat(l.depth * 4),
                    class: l.class,
                    text: l.text,
                })
                .collect(),
            lookups: trace.lookups,
            lookup_limit: trace.limit,
            over_limit: trace.over_limit,
        }),
        Err(e) => {
            error!("[web] SPF lookup task failed: {}", e);
            render(empty(Some(format!("SPF lookup failed: {}", e))))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_domains() {
        assert!(is_valid_domain("example.com"));
        assert!(is_valid_domain("mail-1.example.co.uk"));
    }

    #[test]
    fn rejects_bad_domains() {
        assert!(!is_valid_domain("localhost"));
        assert!(!is_valid_domain("exa mple.com"));
        assert!(!is_valid_domain("-bad.example.com"));
        assert!(!is_valid_domain("bad-.example.com"));
        assert!(!is_valid_domain("a..b"));
        assert!(!is_valid_domain("_spf.example.com"));
        assert!(!is_valid_domain(&format!("{}.com", "a".repeat(64))));
    }
}
