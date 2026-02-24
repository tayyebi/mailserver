mod auth;
mod config;
mod db;
mod fail2ban;
mod filter;
mod web;

use log::{debug, error, info, warn};
use std::env;

fn main() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .format_timestamp_millis()
        .init();

    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    info!("[main] mailserver starting, command={}", command);

    match command {
        "serve" => {
            let port: u16 = env::var("ADMIN_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or_else(|| {
                    debug!("[main] ADMIN_PORT not set or invalid, defaulting to 8080");
                    8080
                });
            let hostname = env::var("HOSTNAME").unwrap_or_else(|_| {
                warn!("[main] HOSTNAME not set, defaulting to localhost");
                "localhost".to_string()
            });
            let db_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
                error!("[main] DATABASE_URL not set; ensure it is provided via environment");
                std::process::exit(1);
            });

            info!(
                "[main] serve: port={}, hostname={}, db_url={}",
                port, hostname, db_url
            );

            let database = db::Database::open(&db_url);

            info!("[main] generating initial mail service configs");
            config::generate_all_configs(&database, &hostname);

            info!("[main] starting mailserver admin on port {}", port);

            let state = web::AppState {
                db: database.clone(),
                hostname,
                admin_port: port,
            };

            // Start fail2ban log watcher in a background thread
            info!("[main] starting fail2ban log watcher");
            fail2ban::start_watcher(database);

            // Start Tokio runtime only for the HTTP server
            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("Failed to build Tokio runtime");
            rt.block_on(async move {
                web::start_server(state).await;
            });
        }
        "filter" => {
            let db_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
                error!("[filter] DATABASE_URL not set; ensure it is provided via environment");
                std::process::exit(75);
            });
            // Prefer pixel_base_url stored in the database (if set), fall back to env var, then default
            let database = db::Database::open(&db_url);
            let pixel_base_url = database
                .get_setting("pixel_base_url")
                .or_else(|| env::var("PIXEL_BASE_URL").ok())
                .unwrap_or_else(|| {
                    warn!(
                        "[filter] PIXEL_BASE_URL not set, defaulting to http://localhost/pixel?id="
                    );
                    "http://localhost/pixel?id=".to_string()
                });

            let mut sender = String::new();
            let mut recipients = Vec::new();
            let mut after_separator = false;
            let mut incoming = false;
            let mut i = 2;
            while i < args.len() {
                if args[i] == "--incoming" {
                    incoming = true;
                } else if args[i] == "-f" {
                    i += 1;
                    if i < args.len() {
                        sender = args[i].clone();
                    }
                } else if args[i] == "--" {
                    after_separator = true;
                } else if after_separator {
                    recipients.push(args[i].clone());
                }
                i += 1;
            }

            let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
            let admin_port = env::var("ADMIN_PORT")
                .ok()
                .and_then(|p| p.parse::<u16>().ok())
                .unwrap_or(8080);
            let unsubscribe_base_url = database
                .get_setting("unsubscribe_base_url")
                .or_else(|| env::var("UNSUBSCRIBE_BASE_URL").ok())
                .unwrap_or_else(|| {
                    if admin_port == 443 {
                        format!("https://{}", hostname)
                    } else if admin_port == 80 {
                        format!("http://{}", hostname)
                    } else {
                        format!("https://{}:{}", hostname, admin_port)
                    }
                });

            info!(
                "[filter] running content filter sender={}, recipients={}",
                sender,
                recipients.join(", ")
            );
            filter::run_filter(
                &db_url,
                &sender,
                &recipients,
                &pixel_base_url,
                &unsubscribe_base_url,
                incoming,
            );
            info!("[filter] content filter completed");
        }
        "seed" => {
            let db_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
                error!("[seed] DATABASE_URL not set; ensure it is provided via environment");
                std::process::exit(1);
            });
            let username = env::var("SEED_USER").unwrap_or_else(|_| {
                debug!("[seed] SEED_USER not set, defaulting to admin");
                "admin".to_string()
            });
            let password = env::var("SEED_PASS").unwrap_or_else(|_| {
                debug!("[seed] SEED_PASS not set, defaulting to admin");
                "admin".to_string()
            });

            info!("[seed] seeding admin user: {}", username);
            let database = db::Database::open(&db_url);
            let hash = auth::hash_password(&password).unwrap_or_else(|e| {
                error!("[seed] failed to hash password: {}", e);
                std::process::exit(1);
            });
            database.seed_admin(&username, &hash);
            info!("[seed] admin user seeded successfully: {}", username);
        }
        "genconfig" => {
            let db_url = env::var("DATABASE_URL").unwrap_or_else(|_| {
                error!("[genconfig] DATABASE_URL not set; ensure it is provided via environment");
                std::process::exit(1);
            });
            let hostname = env::var("HOSTNAME").unwrap_or_else(|_| {
                warn!("[genconfig] HOSTNAME not set, defaulting to localhost");
                "localhost".to_string()
            });

            info!("[genconfig] generating configs for hostname={}", hostname);
            let database = db::Database::open(&db_url);
            config::generate_all_configs(&database, &hostname);
            info!("[genconfig] configuration files generated successfully");
        }
        "gencerts" => {
            let hostname = env::var("HOSTNAME").unwrap_or_else(|_| {
                warn!("[gencerts] HOSTNAME not set, defaulting to localhost");
                "localhost".to_string()
            });

            info!(
                "[gencerts] generating certificates and DH parameters for hostname={}",
                hostname
            );
            match config::generate_all_certificates(&hostname, false) {
                Ok(_) => {
                    info!("[gencerts] certificates and DH parameters generated successfully");
                    config::reload_services();
                }
                Err(e) => {
                    error!("[gencerts] failed to generate certificates: {}", e);
                    std::process::exit(1);
                }
            }
        }
        other => {
            if other != "help" {
                error!("[main] unknown command: {}", other);
            }
            println!("Mailserver - Monolithic mail server admin");
            println!();
            println!("Usage:");
            println!("  mailserver serve      Start admin dashboard and pixel server");
            println!("  mailserver filter     Run as Postfix content filter");
            println!("  mailserver seed       Seed default admin user");
            println!("  mailserver genconfig  Generate mail service configs");
            println!("  mailserver gencerts   Generate TLS certificates and DH parameters");
            println!();
            println!("Environment variables:");
            println!("  ADMIN_PORT       Dashboard port (default: 8080)");
            println!("  HOSTNAME         Mail server hostname (default: localhost)");
            println!("  DATABASE_URL    PostgreSQL connection string (required)");
            println!("  PIXEL_BASE_URL   Base URL for tracking pixels");
            println!("  SEED_USER        Default admin username (default: admin)");
            println!("  SEED_PASS        Default admin password (default: admin)");
        }
    }
}
