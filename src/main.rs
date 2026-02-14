mod db;
mod auth;
mod config;
mod filter;
mod web;

use log::{info, warn, debug, error};
use std::env;

#[tokio::main]
async fn main() {
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
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| {
                    debug!("[main] DB_PATH not set, defaulting to /data/db/mailserver.sqlite");
                    "/data/db/mailserver.sqlite".to_string()
                });

            info!("[main] serve: port={}, hostname={}, db_path={}", port, hostname, db_path);

            let database = db::Database::open(&db_path);

            info!("[main] generating initial mail service configs");
            config::generate_all_configs(&database, &hostname);

            info!("[main] starting mailserver admin on port {}", port);

            let state = web::AppState {
                db: database,
                hostname,
                admin_port: port,
            };

            web::start_server(state).await;
        }
        "filter" => {
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| {
                    debug!("[filter] DB_PATH not set, defaulting to /data/db/mailserver.sqlite");
                    "/data/db/mailserver.sqlite".to_string()
                });
            // Prefer pixel_base_url stored in the database (if set), fall back to env var, then default
            let database = db::Database::open(&db_path);
            let pixel_base_url = database
                .get_setting("pixel_base_url")
                .or_else(|| env::var("PIXEL_BASE_URL").ok())
                .unwrap_or_else(|| {
                    warn!("[filter] PIXEL_BASE_URL not set, defaulting to http://localhost/pixel?id=");
                    "http://localhost/pixel?id=".to_string()
                });

            let mut sender = String::new();
            let mut recipients = Vec::new();
            let mut after_separator = false;
            let mut i = 2;
            while i < args.len() {
                if args[i] == "-f" {
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

            info!("[filter] running content filter sender={}, recipients={}", sender, recipients.join(", "));
            filter::run_filter(&db_path, &sender, &recipients, &pixel_base_url);
            info!("[filter] content filter completed");
        }
        "seed" => {
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| {
                    debug!("[seed] DB_PATH not set, defaulting to /data/db/mailserver.sqlite");
                    "/data/db/mailserver.sqlite".to_string()
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
            let database = db::Database::open(&db_path);
            let hash = auth::hash_password(&password);
            database.seed_admin(&username, &hash);
            info!("[seed] admin user seeded successfully: {}", username);
        }
        "genconfig" => {
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| {
                    debug!("[genconfig] DB_PATH not set, defaulting to /data/db/mailserver.sqlite");
                    "/data/db/mailserver.sqlite".to_string()
                });
            let hostname = env::var("HOSTNAME").unwrap_or_else(|_| {
                warn!("[genconfig] HOSTNAME not set, defaulting to localhost");
                "localhost".to_string()
            });

            info!("[genconfig] generating configs for hostname={}", hostname);
            let database = db::Database::open(&db_path);
            config::generate_all_configs(&database, &hostname);
            info!("[genconfig] configuration files generated successfully");
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
            println!();
            println!("Environment variables:");
            println!("  ADMIN_PORT       Dashboard port (default: 8080)");
            println!("  HOSTNAME         Mail server hostname (default: localhost)");
            println!("  DB_PATH          SQLite database path (default: /data/db/mailserver.sqlite)");
            println!("  PIXEL_BASE_URL   Base URL for tracking pixels");
            println!("  SEED_USER        Default admin username (default: admin)");
            println!("  SEED_PASS        Default admin password (default: admin)");
        }
    }
}
