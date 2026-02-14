use askama::Template;
use axum::{
    extract::{Path, State},
    response::{Html, Redirect, Response, IntoResponse},
    Form,
};
use log::{info, warn, error, debug};

use crate::db::{Account, Alias, Domain};
use crate::web::AppState;
use crate::web::auth::AuthAdmin;
use crate::web::forms::{AccountForm, AccountEditForm};
use crate::web::regen_configs;

// ── Templates ──

#[derive(Template)]
#[template(path = "accounts/list.html")]
struct ListTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    accounts: Vec<Account>,
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

pub async fn list(_auth: AuthAdmin, State(state): State<AppState>) -> Html<String> {
    info!("[web] GET /accounts — listing accounts");
    let accounts = state.db.list_all_accounts_with_domain();
    debug!("[web] found {} accounts", accounts.len());
    let tmpl = ListTemplate { nav_active: "Accounts", flash: None, accounts };
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
