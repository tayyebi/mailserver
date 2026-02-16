use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, IntoResponse, Response},
};
use log::{debug, info};
use serde::Deserialize;

use crate::web::auth::AuthAdmin;
use crate::web::AppState;

// ── Query parameters ──

#[derive(Deserialize)]
pub struct TableViewQuery {
    #[serde(default = "default_page")]
    page: i64,
}

fn default_page() -> i64 {
    1
}

// ── Templates ──

#[derive(Template)]
#[template(path = "database/list.html")]
struct ListTablesTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    tables: Vec<String>,
}

#[derive(Template)]
#[template(path = "database/view.html")]
struct ViewTableTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    table_name: String,
    columns: Vec<String>,
    rows: Vec<Vec<String>>,
    page: i64,
    per_page: i64,
    total_rows: i64,
    total_pages: i64,
    has_prev: bool,
    has_next: bool,
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

pub async fn list_tables(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /database — listing all tables");
    let tables = state.blocking_db(|db| db.list_tables()).await;
    debug!("[web] found {} tables", tables.len());

    let tmpl = ListTablesTemplate {
        nav_active: "Database",
        flash: None,
        tables,
    };
    Html(tmpl.render().unwrap())
}

pub async fn view_table(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(table_name): Path<String>,
    Query(query): Query<TableViewQuery>,
) -> Response {
    info!(
        "[web] GET /database/{} — viewing table page={}",
        table_name, query.page
    );

    // Validate table exists
    let table_name_clone = table_name.clone();
    let tables = state.blocking_db(|db| db.list_tables()).await;
    if !tables.contains(&table_name) {
        let tmpl = ErrorTemplate {
            nav_active: "Database",
            flash: None,
            status_code: 404,
            status_text: "Not Found",
            title: "Table Not Found",
            message: &format!("Table '{}' does not exist", table_name),
            back_url: "/database",
            back_label: "Back to Tables",
        };
        return Html(tmpl.render().unwrap()).into_response();
    }

    let per_page = 50i64;
    let page = query.page.max(1);

    // Get table info
    let table_name_for_columns = table_name_clone.clone();
    let columns_with_types = state
        .blocking_db(move |db| db.get_table_columns(&table_name_for_columns))
        .await;
    let columns: Vec<String> = columns_with_types.iter().map(|(name, _)| name.clone()).collect();

    let table_name_for_count = table_name_clone.clone();
    let total_rows = state
        .blocking_db(move |db| db.count_table_rows(&table_name_for_count))
        .await;

    let table_name_for_data = table_name_clone.clone();
    let rows = state
        .blocking_db(move |db| db.get_table_data(&table_name_for_data, page, per_page))
        .await;

    let total_pages = (total_rows as f64 / per_page as f64).ceil() as i64;
    let has_prev = page > 1;
    let has_next = page < total_pages;

    debug!(
        "[web] table {} has {} rows, showing page {} of {}",
        table_name_clone, total_rows, page, total_pages
    );

    let tmpl = ViewTableTemplate {
        nav_active: "Database",
        flash: None,
        table_name: table_name_clone,
        columns,
        rows,
        page,
        per_page,
        total_rows,
        total_pages,
        has_prev,
        has_next,
    };
    Html(tmpl.render().unwrap()).into_response()
}
