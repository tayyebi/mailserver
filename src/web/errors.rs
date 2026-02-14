use askama::Template;
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Response};

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    status_code: u16,
    status_text: &'a str,
    title: &'a str,
    message: &'a str,
    back_label: &'a str,
    back_url: &'a str,
}

fn render_status_html(
    status: StatusCode,
    title: &str,
    message: &str,
    back_url: &str,
    back_label: &str,
) -> Html<String> {
    let template = ErrorTemplate {
        nav_active: "",
        flash: None,
        status_code: status.as_u16(),
        status_text: status.canonical_reason().unwrap_or("Error"),
        title,
        message,
        back_label,
        back_url,
    };
    Html(template.render().expect("Failed to render error page"))
}

pub fn render_error_page(
    status: StatusCode,
    title: &str,
    message: &str,
    back_url: &str,
    back_label: &str,
) -> Html<String> {
    render_status_html(status, title, message, back_url, back_label)
}

pub fn status_response(
    status: StatusCode,
    title: &str,
    message: &str,
    back_url: &str,
    back_label: &str,
) -> Response {
    (
        status,
        render_status_html(status, title, message, back_url, back_label),
    )
        .into_response()
}
