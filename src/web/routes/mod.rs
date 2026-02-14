pub mod dashboard;
pub mod domains;
pub mod accounts;
pub mod aliases;
pub mod tracking;
pub mod settings;
pub mod pixel;

use axum::{routing::{get, post}, Router};
use super::AppState;

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard::page))
        .route("/domains", get(domains::list).post(domains::create))
        .route("/domains/new", get(domains::new_form))
        .route("/domains/{id}/edit", get(domains::edit_form))
        .route("/domains/{id}", post(domains::update))
        .route("/domains/{id}/delete", post(domains::delete))
        .route("/domains/{id}/dkim", post(domains::generate_dkim))
        .route("/domains/{id}/dns", get(domains::dns_info))
        .route("/accounts", get(accounts::list).post(accounts::create))
        .route("/accounts/new", get(accounts::new_form))
        .route("/accounts/{id}/edit", get(accounts::edit_form))
        .route("/accounts/{id}", post(accounts::update))
        .route("/accounts/{id}/delete", post(accounts::delete))
        .route("/aliases", get(aliases::list).post(aliases::create))
        .route("/aliases/new", get(aliases::new_form))
        .route("/aliases/{id}/edit", get(aliases::edit_form))
        .route("/aliases/{id}", post(aliases::update))
        .route("/aliases/{id}/delete", post(aliases::delete))
        .route("/tracking", get(tracking::list))
        .route("/tracking/{msg_id}", get(tracking::detail))
        .route("/settings", get(settings::page))
        .route("/settings/password", post(settings::change_password))
        .route("/settings/2fa", get(settings::setup_2fa))
        .route("/settings/2fa/enable", post(settings::enable_2fa))
        .route("/settings/2fa/disable", post(settings::disable_2fa))
        .route("/settings/pixel", post(settings::update_pixel))
}
