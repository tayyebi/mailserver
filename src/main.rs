mod db;
mod auth;
mod config;
mod filter;
mod web;

use std::env;

#[tokio::main]
async fn main() {
    let args: Vec<String> = env::args().collect();
    let command = args.get(1).map(|s| s.as_str()).unwrap_or("help");

    match command {
        "serve" => {
            let port: u16 = env::var("ADMIN_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(8080);
            let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| "/data/db/mailserver.sqlite".to_string());

            let database = db::Database::open(&db_path);

            config::generate_all_configs(&database, &hostname);

            println!("Starting mailserver admin on port {}", port);

            let state = web::AppState {
                db: database,
                hostname,
                admin_port: port,
            };

            web::start_server(state).await;
        }
        "filter" => {
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| "/data/db/mailserver.sqlite".to_string());
            let pixel_base_url = env::var("PIXEL_BASE_URL")
                .unwrap_or_else(|_| "https://localhost/pixel?id=".to_string());

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

            filter::run_filter(&db_path, &sender, &recipients, &pixel_base_url);
        }
        "seed" => {
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| "/data/db/mailserver.sqlite".to_string());
            let username = env::var("SEED_USER").unwrap_or_else(|_| "admin".to_string());
            let password = env::var("SEED_PASS").unwrap_or_else(|_| "admin".to_string());

            let database = db::Database::open(&db_path);
            let hash = auth::hash_password(&password);
            database.seed_admin(&username, &hash);
            println!("Seeded admin user: {}", username);
        }
        "genconfig" => {
            let db_path =
                env::var("DB_PATH").unwrap_or_else(|_| "/data/db/mailserver.sqlite".to_string());
            let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());

            let database = db::Database::open(&db_path);
            config::generate_all_configs(&database, &hostname);
            println!("Configuration files generated.");
        }
        _ => {
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
