use askama::Template;
use axum::{
    extract::{Path, Query, State},
    response::{Html, Redirect, Response, IntoResponse},
    Form,
};
use log::{info, warn, error, debug};
use serde::Deserialize;

use crate::db::{Account, Alias, Domain};
use crate::web::AppState;
use crate::web::auth::AuthAdmin;
use crate::web::forms::{AccountForm, AccountEditForm};
use crate::web::regen_configs;

// ── Query parameters ──

#[derive(Deserialize)]
pub struct PaginationQuery {
    pub page: Option<i64>,
}

// ── View models ──

struct AccountListRow {
    id: i64,
    email: String,
    name: String,
    active: bool,
    quota_display: String,
    mailbox_path: String,
}

// ── Templates ──

#[derive(Template)]
#[template(path = "accounts/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    account_rows: Vec<AccountListRow>,
    current_page: u32,
    total_pages: u32,
    total_count: u32,
}

#[derive(Template)]
#[template(path = "accounts/new.html")]
struct NewTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    domains: Vec<Domain>,
}

#[derive(Template)]
#[template(path = "accounts/edit.html")]
struct EditTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    account: Account,
    send_as_aliases: Vec<Alias>,
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    title: &'a str,
    message: &'a str,
    back_url: &'a str,
    back_label: &'a str,
}

// ── Handlers ──

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>, Query(params): Query<PaginationQuery>) -> Html<String> {
    info!("[web] GET /accounts — listing accounts");
    let all_accounts = state.db.list_all_accounts_with_domain();
    debug!("[web] found {} accounts", all_accounts.len());
    
    let total_count = all_accounts.len() as u32;
    let page_num = std::cmp::max(params.page.unwrap_or(1), 1) as u32;
    let per_page = 20u32;
    let total_pages = (total_count + per_page - 1) / per_page;
    let page_num = std::cmp::min(page_num, std::cmp::max(total_pages, 1));
    let offset = ((page_num - 1) * per_page) as usize;
    let limit = per_page as usize;
    
    let account_rows: Vec<AccountListRow> = all_accounts.iter()
        .skip(offset)
        .take(limit)
        .map(|a| {
        let email = format!("{}@{}", a.username, a.domain_name.as_deref().unwrap_or("?"));
        let quota_display = if a.quota > 0 {
            let quota_gb = a.quota as f64 / 1_000_000_000.0;
            format!("{:.2} GB", quota_gb)
        } else {
            "∞".to_string()
        };
        let mailbox_path = format!("/var/mail/vhosts/{}/{}", 
            a.domain_name.as_deref().unwrap_or("?"), 
            a.username);
        AccountListRow {
            id: a.id,
            email,
            name: a.name.clone(),
            active: a.active,
            quota_display,
            mailbox_path,
        }
    }).collect();
    
    let tmpl = ListTemplate { 
        nav_active: "Accounts", 
        flash: None, 
        account_rows,
        current_page: page_num,
        total_pages,
        total_count,
    };
    Html(tmpl.render().unwrap())
}

pub async fn new_form(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    debug!("[web] GET /accounts/new — new account form");
    let domains = state.db.list_domains();
    let tmpl = NewTemplate { nav_active: "Accounts", flash: None, domains };
    Html(tmpl.render().unwrap())
}

pub async fn create(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Form(form): Form<AccountForm>,
) -> Response {
    info!("[web] POST /accounts — creating account username={}, domain_id={}", form.username, form.domain_id);
    let db_hash = crate::auth::hash_password(&form.password);
    let quota = form.quota.unwrap_or(0);
    match state.db.create_account(form.domain_id, &form.username, &db_hash, &form.name, quota) {
        Ok(id) => {
            info!("[web] account created successfully: {} (id={})", form.username, id);
            regen_configs(&state);
            Redirect::to("/accounts").into_response()
        }
        Err(e) => {
            error!("[web] failed to create account {}: {}", form.username, e);
            let tmpl = ErrorTemplate {
                nav_active: "Accounts", flash: None,
                title: "Error", message: &e,
                back_url: "/accounts/new", back_label: "Back",
            };
            Html(tmpl.render().unwrap()).into_response()
        }
    }
}

pub async fn edit_form(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    debug!("[web] GET /accounts/{}/edit — edit account form", id);
    let account = match state.db.get_account(id) {
        Some(a) => a,
        None => {
            warn!("[web] account id={} not found for edit", id);
            return Redirect::to("/accounts").into_response();
        }
    };

    // Find aliases on the same domain that the account can send as
    let all_aliases = state.db.list_all_aliases_with_domain();
    let send_as_aliases: Vec<Alias> = all_aliases
        .into_iter()
        .filter(|a| a.domain_id == account.domain_id && a.active)
        .collect();

    let tmpl = EditTemplate { nav_active: "Accounts", flash: None, account, send_as_aliases };
    Html(tmpl.render().unwrap()).into_response()
}

pub async fn update(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Form(form): Form<AccountEditForm>,
) -> Response {
    let active = form.active.is_some();
    let quota = form.quota.unwrap_or(0);
    info!("[web] POST /accounts/{} — updating account active={}, quota={}", id, active, quota);
    state.db.update_account(id, &form.name, active, quota);

    // Only update password if field is not empty
    if let Some(ref pw) = form.password {
        if !pw.is_empty() {
            info!("[web] updating password for account id={}", id);
            let db_hash = crate::auth::hash_password(pw);
            state.db.update_account_password(id, &db_hash);
        }
    }

    regen_configs(&state);
    Redirect::to("/accounts").into_response()
}

pub async fn delete(
    _auth: AuthAdmin,
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Response {
    warn!("[web] POST /accounts/{}/delete — deleting account", id);
    state.db.delete_account(id);
    regen_configs(&state);
    Redirect::to("/accounts").into_response()
}
