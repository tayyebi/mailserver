pub mod accounts;
pub mod aliases;
pub mod bimi;
pub mod configs;
pub mod dashboard;
pub mod dmarc;
pub mod domains;
pub mod fail2ban;
pub mod forwarding;
pub mod pixel;
pub mod queue;
pub mod relays;
pub mod settings;
pub mod spambl;
pub mod tracking;
pub mod unsubscribe;
pub mod webhook;
pub mod webdav;
pub mod webmail;

use super::AppState;
use axum::{
    routing::{get, post},
    Router,
};

pub fn auth_routes() -> Router<AppState> {
    Router::new()
        .route("/", get(dashboard::page))
        .route("/domains", get(domains::list).post(domains::create))
        .route("/domains/new", get(domains::new_form))
        .route("/domains/:id/edit", get(domains::edit_form))
        .route("/domains/:id/delete", post(domains::delete))
        .route("/domains/:id/dkim", post(domains::generate_dkim))
        .route("/domains/:id/dns", get(domains::dns_info))
        .route("/domains/:id/check", get(domains::dns_check_run))
        .route("/domains/:id", post(domains::update))
        .route("/accounts/new", get(accounts::new_form))
        .route("/accounts", get(accounts::list).post(accounts::create))
        .route("/accounts/:id/edit", get(accounts::edit_form))
        .route("/accounts/:id/delete", post(accounts::delete))
        .route("/accounts/:id", post(accounts::update))
        .route("/aliases/new", get(aliases::new_form))
        .route("/aliases", get(aliases::list).post(aliases::create))
        .route("/aliases/:id/edit", get(aliases::edit_form))
        .route("/aliases/:id/delete", post(aliases::delete))
        .route("/aliases/:id", post(aliases::update))
        .route("/forwarding/new", get(forwarding::new_form))
        .route(
            "/forwarding",
            get(forwarding::list).post(forwarding::create),
        )
        .route("/forwarding/:id/edit", get(forwarding::edit_form))
        .route("/forwarding/:id/delete", post(forwarding::delete))
        .route("/forwarding/:id", post(forwarding::update))
        .route("/tracking", get(tracking::list))
        .route("/tracking/:msg_id", get(tracking::detail))
        .route("/queue", get(queue::list))
        .route("/queue/flush", post(queue::flush))
        .route("/queue/purge", post(queue::purge))
        .route("/queue/:id/delete", post(queue::delete_message))
        .route("/queue/:id/flush", post(queue::flush_message))
        .route("/webmail", get(webmail::inbox))
        .route("/webmail/view/:filename", get(webmail::view_email))
        .route("/webmail/download/:filename", get(webmail::download_email))
        .route("/webmail/reply/:filename", get(webmail::reply_email))
        .route("/webmail/delete/:filename", post(webmail::delete_email))
        .route("/webmail/compose", get(webmail::compose))
        .route("/webmail/send", post(webmail::send_email))
        .route("/settings", get(settings::page))
        .route("/settings/password", post(settings::change_password))
        .route("/settings/2fa", get(settings::setup_2fa))
        .route("/settings/2fa/enable", post(settings::enable_2fa))
        .route("/settings/2fa/disable", post(settings::disable_2fa))
        .route("/settings/pixel", post(settings::update_pixel))
        .route("/settings/features", post(settings::update_features))
        .route("/settings/tls/regenerate", post(settings::regenerate_tls))
        .route("/settings/tls/cert.pem", get(settings::download_cert))
        .route("/settings/tls/key.pem", get(settings::download_key))
        .route(
            "/settings/restart-services",
            post(settings::restart_services),
        )
        .route(
            "/settings/restart-container",
            post(settings::restart_container),
        )
        .route("/configs", get(configs::page))
        .route("/fail2ban", get(fail2ban::overview))
        .route("/fail2ban/toggle", post(fail2ban::toggle_system))
        .route("/fail2ban/ban", post(fail2ban::ban_ip))
        .route("/fail2ban/unban/:id", post(fail2ban::unban_ip))
        .route(
            "/fail2ban/settings/:id/edit",
            get(fail2ban::edit_setting_form),
        )
        .route("/fail2ban/settings/:id", post(fail2ban::update_setting))
        .route("/fail2ban/whitelist", post(fail2ban::add_whitelist))
        .route(
            "/fail2ban/whitelist/:id/delete",
            post(fail2ban::remove_whitelist),
        )
        .route("/fail2ban/blacklist", post(fail2ban::add_blacklist))
        .route(
            "/fail2ban/blacklist/:id/delete",
            post(fail2ban::remove_blacklist),
        )
        .route("/unsubscribe/list", get(unsubscribe::list))
        .route("/unsubscribe/:id/delete", post(unsubscribe::delete))
        .route("/spambl", get(spambl::list))
        .route("/spambl/toggle", post(spambl::toggle))
        .route("/webhooks", get(webhook::list))
        .route("/webhooks/settings", post(webhook::update_webhook))
        .route("/webhooks/test", post(webhook::test_webhook))
        .route("/dmarc", get(dmarc::list).post(dmarc::create))
        .route("/dmarc/:id/delete", post(dmarc::delete))
        .route("/dmarc/:id/reports", get(dmarc::reports))
        .route("/relays/new", get(relays::new_form))
        .route("/relays", get(relays::list).post(relays::create))
        .route("/relays/:id/edit", get(relays::edit_form))
        .route("/relays/:id/delete", post(relays::delete))
        .route("/relays/:id", post(relays::update))
        .route("/relays/:id/assignments", post(relays::add_assignment))
        .route("/relays/:id/assignments/:aid/delete",
            post(relays::remove_assignment),
        )
        .route("/webdav", get(webdav::list))
        .route("/webdav/settings", post(webdav::update_settings))
        .route("/webdav/:id/delete", post(webdav::admin_delete_file))
}
