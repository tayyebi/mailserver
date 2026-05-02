mod auth;
mod config;
mod db;
mod fail2ban;
mod filter;
mod hlc;
mod provision;
mod replication;
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

            // Bootstrap cluster: join seed peers if CLUSTER_SEEDS is set
            let instance_id = database.get_local_node_id();
            let cluster_seeds: Vec<String> = env::var("CLUSTER_SEEDS")
                .unwrap_or_default()
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();

            if !cluster_seeds.is_empty() {
                info!(
                    "[main] bootstrapping cluster with {} seed(s): {:?}",
                    cluster_seeds.len(),
                    cluster_seeds
                );
                bootstrap_cluster(&database, &instance_id, &cluster_seeds, port);
            }

            info!("[main] starting mailserver admin on port {}", port);

            let state = web::AppState {
                db: database.clone(),
                hostname,
                admin_port: port,
                mcp_guard: std::sync::Arc::new(std::sync::Mutex::new(web::McpGuard::new())),
                idle_registry: std::sync::Arc::new(std::sync::Mutex::new(
                    std::collections::HashMap::new(),
                )),
                cluster_start_time: std::time::SystemTime::now(),
                nonce_cache: std::sync::Arc::new(std::sync::Mutex::new(
                    web::routes::cluster::NonceCache::new(),
                )),
            };

            // Start fail2ban log watcher in a background thread
            info!("[main] starting fail2ban log watcher");
            fail2ban::start_watcher(database.clone());

            // Start outbound replication service
            info!("[main] starting replication service");
            replication::start(database);

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
                    let h = env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
                    let p = env::var("ADMIN_PORT")
                        .ok()
                        .and_then(|v| v.parse::<u16>().ok())
                        .unwrap_or(8080);
                    let url = if p == 443 {
                        format!("https://{}/pixel?id=", h)
                    } else if p == 80 {
                        format!("http://{}/pixel?id=", h)
                    } else {
                        format!("https://{}:{}/pixel?id=", h, p)
                    };
                    warn!("[filter] PIXEL_BASE_URL not set, defaulting to {}", url);
                    url
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
        "provision" => {
            // Collect arguments that follow the "provision" token
            let sub_args: Vec<String> = args[2..].to_vec();

            info!("[provision] starting SSH auto-provisioning");

            let rt = tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
                .expect("failed to build Tokio runtime");

            rt.block_on(async move {
                if let Err(e) = provision::run(&sub_args).await {
                    error!("[provision] provisioning failed: {}", e);
                    std::process::exit(1);
                }
            });
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
            println!("  mailserver provision  Auto-provision a remote server via SSH");
            println!();
            println!("Environment variables:");
            println!("  ADMIN_PORT       Dashboard port (default: 8080)");
            println!("  HOSTNAME         Mail server hostname (default: localhost)");
            println!("  DATABASE_URL    PostgreSQL connection string (required)");
            println!("  PIXEL_BASE_URL   Base URL for tracking pixels");
            println!("  SEED_USER        Default admin username (default: admin)");
            println!("  SEED_PASS        Default admin password (default: admin)");
            println!("  CLUSTER_SEEDS    Comma-separated seed URLs for cluster bootstrap");
            println!();
            println!("Run 'mailserver provision' without arguments for provisioning help.");
        }
    }
}

/// Bootstrap the cluster by posting to each seed until one succeeds.
/// Back-off schedule: [1, 2, 5, 10, 10, 10] seconds.
fn bootstrap_cluster(
    db: &db::Database,
    instance_id: &str,
    seeds: &[String],
    admin_port: u16,
) {
    use std::time::Duration;

    let boot_hlc = db.get_node_state("hlc_high_water").unwrap_or_else(|| {
        let h = hlc::Hlc::new(instance_id);
        h.now()
    });

    let self_url = {
        let hostname = env::var("HOSTNAME").unwrap_or_else(|_| "localhost".to_string());
        format!("http://{}:{}", hostname, admin_port)
    };
    let region = env::var("CLUSTER_REGION").ok();

    let (_, verifying) = web::routes::cluster::load_or_generate_keypair();
    let pub_key = web::routes::cluster::public_key_b64(&verifying);

    let body = serde_json::json!({
        "instance_id": instance_id,
        "url": self_url,
        "region": region,
        "public_key": pub_key,
        "first_seen_hlc": boot_hlc,
    })
    .to_string();

    let backoff = [1u64, 2, 5, 10, 10, 10];
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .build()
        .expect("HTTP client");

    for seed in seeds {
        let url = format!("{}/cluster/join", seed.trim_end_matches('/'));
        info!("[main] posting cluster join to {}", url);

        let cluster_secret = db.get_setting("cluster_secret").unwrap_or_default();

        for (attempt, &wait) in backoff.iter().enumerate() {
            if attempt > 0 {
                std::thread::sleep(Duration::from_secs(wait));
            }

            let mut req = client
                .post(&url)
                .header("Content-Type", "application/json")
                .body(body.clone());
            if !cluster_secret.is_empty() {
                req = req.header("X-Cluster-Secret", &cluster_secret);
            }

            match req.send() {
                Ok(resp) if resp.status().is_success() => {
                    #[derive(serde::Deserialize)]
                    struct JoinResp {
                        peer_list: Vec<db::Peer>,
                        hlc_high_water: String,
                    }
                    if let Ok(jr) = resp.json::<JoinResp>() {
                        info!(
                            "[main] joined cluster via {}: {} peer(s), high_water={}",
                            seed,
                            jr.peer_list.len(),
                            jr.hlc_high_water
                        );
                        // Hydrate peers table
                        for peer in &jr.peer_list {
                            let _ = db.upsert_peer(
                                &peer.instance_id,
                                &peer.url,
                                peer.region.as_deref(),
                                peer.peer_public_key.as_deref(),
                                peer.first_seen_hlc.as_deref(),
                            );
                        }
                        // Run catch-up barrier
                        catchup_barrier(db, instance_id, &jr.hlc_high_water);
                    }
                    break;
                }
                Ok(resp) => {
                    warn!(
                        "[main] cluster join to {} returned HTTP {} (attempt {})",
                        seed,
                        resp.status(),
                        attempt + 1
                    );
                }
                Err(e) => {
                    warn!(
                        "[main] cluster join to {} failed (attempt {}): {}",
                        seed,
                        attempt + 1,
                        e
                    );
                }
            }
        }
    }
}

/// Catch-up barrier: block until the local HLC high-water is within 30 seconds of the
/// fleet high-water, or until the hard timeout (30s) expires.
fn catchup_barrier(db: &db::Database, instance_id: &str, fleet_high_water: &str) {
    use std::time::{Duration, Instant};

    let fleet_ms = hlc::physical_ms(fleet_high_water).unwrap_or(0);
    if fleet_ms == 0 {
        return;
    }

    let deadline = Instant::now() + Duration::from_secs(30);
    let hlc = hlc::Hlc::new(instance_id);

    loop {
        let local_hw = db.get_hlc_high_water();
        let local_ms = hlc::physical_ms(&local_hw).unwrap_or(0);
        let gap_ms = fleet_ms.saturating_sub(local_ms);

        if gap_ms <= 30_000 {
            info!(
                "[main] catch-up barrier passed: gap={}ms local_hw='{}' fleet_hw='{}'",
                gap_ms, local_hw, fleet_high_water
            );
            return;
        }

        if Instant::now() >= deadline {
            warn!(
                "[main] catch-up barrier timed out: gap={}ms — proceeding anyway",
                gap_ms
            );
            return;
        }

        // Gossip-pull in parallel from all peers once
        let peers = db.list_online_peers();
        info!(
            "[main] catch-up barrier: gap={}ms — pulling from {} peer(s)",
            gap_ms,
            peers.len()
        );
        for peer in &peers {
            replication::gossip_pull_from(db, &hlc, &peer.instance_id, &peer.url);
        }
        std::thread::sleep(Duration::from_millis(500));
    }
}
