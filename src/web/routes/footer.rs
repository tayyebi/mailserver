use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, IntoResponse, Redirect, Response},
    Form,
};
use log::{info, warn};

use crate::web::auth::AuthAdmin;
use crate::web::forms::{FooterContentForm, TrackingPatternForm, TrackingRuleForm};
use crate::web::AppState;

use serde_json;

// ── Templates ──

#[derive(Template)]
#[template(path = "footer/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    footer_html: String,
    patterns: Vec<crate::db::FooterPattern>,
    rules: Vec<crate::db::FooterRule>,
}

// ── Handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /footer — listing footer patterns and rules");
    let footer_html = state
        .blocking_db(|db| db.get_setting("footer_html").unwrap_or_default())
        .await;
    let patterns = state.blocking_db(|db| db.list_footer_patterns()).await;
    let rules = state.blocking_db(|db| db.list_footer_rules()).await;
    let tmpl = ListTemplate {
        nav_active: "Footer",
        flash: None,
        footer_html,
        patterns,
        rules,
    };
    Html(tmpl.render().unwrap())
}

pub async fn update_content(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<FooterContentForm>,
) -> Response {
    info!("[web] POST /footer/content — updating footer HTML");
    let html = form.footer_html.clone();
    state
        .blocking_db(move |db| db.set_setting("footer_html", &html))
        .await;
    Redirect::to("/footer").into_response()
}

pub async fn create_pattern(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TrackingPatternForm>,
) -> Response {
    info!("[web] POST /footer/patterns — creating pattern={}", form.pattern);
    let pattern = form.pattern.trim().to_string();
    state
        .blocking_db(move |db| db.create_footer_pattern(&pattern))
        .await
        .ok();
    Redirect::to("/footer").into_response()
}

pub async fn delete_pattern(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /footer/patterns/{}/delete — deleting pattern", id);
    state
        .blocking_db(move |db| db.delete_footer_pattern(id))
        .await;
    Redirect::to("/footer").into_response()
}

pub async fn create_rule(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<TrackingRuleForm>,
) -> Response {
    info!("[web] POST /footer/rules — creating rule name={}", form.name);
    let name = form.name.trim().to_string();
    let match_mode = if form.match_mode == "OR" { "OR" } else { "AND" }.to_string();
    let conditions: Vec<crate::db::TrackingCondition> =
        serde_json::from_str(&form.conditions_json).unwrap_or_default();
    state
        .blocking_db(move |db| db.create_footer_rule(&name, &match_mode, &conditions))
        .await
        .ok();
    Redirect::to("/footer").into_response()
}

pub async fn delete_rule(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /footer/rules/{}/delete — deleting rule", id);
    state
        .blocking_db(move |db| db.delete_footer_rule(id))
        .await;
    Redirect::to("/footer").into_response()
}
