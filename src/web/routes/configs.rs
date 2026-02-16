use askama::Template;
use axum::{extract::State, response::Html};
use log::debug;
use std::fs;

use crate::web::AppState;
use crate::web::auth::AuthAdmin;

#[derive(Template)]
#[template(path = "configs/view.html")]
struct ConfigsTemplate<'a> {
    nav_active: &'a str,
    flash: Option<&'a str>,
    config_files: Vec<ConfigFile>,
}

#[derive(Debug)]
struct ConfigFile {
    name: String,
    path: String,
    content: String,
    error: Option<String>,
}

pub async fn page(auth: AuthAdmin, State(_state): State<AppState>) -> Html<String> {
    debug!(
        "[web] GET /configs — config files page for username={}",
        auth.admin.username
    );

    let config_paths = vec![
        ("Postfix Main Config", "/etc/postfix/main.cf"),
        ("Postfix Master Config", "/etc/postfix/master.cf"),
        ("Virtual Domains", "/etc/postfix/virtual_domains"),
        ("Virtual Mailboxes", "/etc/postfix/vmailbox"),
        ("Virtual Aliases", "/etc/postfix/virtual_aliases"),
        ("Sender Login Maps", "/etc/postfix/sender_login_maps"),
        ("Dovecot Config", "/etc/dovecot/dovecot.conf"),
        ("Dovecot Passwd", "/etc/dovecot/passwd"),
        ("OpenDKIM Config", "/etc/opendkim/opendkim.conf"),
        ("OpenDKIM KeyTable", "/etc/opendkim/KeyTable"),
        ("OpenDKIM SigningTable", "/etc/opendkim/SigningTable"),
        ("OpenDKIM TrustedHosts", "/etc/opendkim/TrustedHosts"),
    ];

    let mut config_files = Vec::new();

    for (name, path) in config_paths {
        let (content, error) = match fs::read_to_string(path) {
            Ok(content) => (content, None),
            Err(e) => (
                String::new(),
                Some(format!("Error reading file: {}", e)),
            ),
        };

        config_files.push(ConfigFile {
            name: name.to_string(),
            path: path.to_string(),
            content,
            error,
        });
    }

    let tmpl = ConfigsTemplate {
        nav_active: "Configs",
        flash: None,
        config_files,
    };

    Html(tmpl.render().unwrap())
}
