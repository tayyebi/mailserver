#![allow(dead_code)]

use log::{debug, error, info, warn};
use postgres::{Client, NoTls};
use serde::Serialize;
use std::sync::{Arc, Mutex};

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Client>>,
}

#[derive(Clone, Serialize)]
pub struct Admin {
    pub id: i64,
    pub username: String,
    pub password_hash: String,
    pub totp_secret: Option<String>,
    pub totp_enabled: bool,
}

#[derive(Clone, Serialize)]
pub struct Domain {
    pub id: i64,
    pub domain: String,
    pub active: bool,
    pub dkim_selector: String,
    pub dkim_private_key: Option<String>,
    pub dkim_public_key: Option<String>,
    pub footer_html: Option<String>,
    pub bimi_svg: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Account {
    pub id: i64,
    pub domain_id: i64,
    pub username: String,
    pub password_hash: String,
    pub name: String,
    pub active: bool,
    pub quota: i64,
    pub domain_name: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct Alias {
    pub id: i64,
    pub domain_id: i64,
    pub source: String,
    pub destination: String,
    pub active: bool,
    pub tracking_enabled: bool,
    pub sort_order: i64,
    pub domain_name: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct TrackedMessage {
    pub id: i64,
    pub message_id: String,
    pub sender: String,
    pub recipient: String,
    pub subject: String,
    pub alias_id: Option<i64>,
    pub created_at: String,
}

#[derive(Clone, Serialize)]
pub struct PixelOpen {
    pub id: i64,
    pub message_id: String,
    pub client_ip: String,
    pub user_agent: String,
    pub opened_at: String,
}

#[derive(Clone, Serialize)]
pub struct Stats {
    pub domain_count: i64,
    pub account_count: i64,
    pub alias_count: i64,
    pub tracked_count: i64,
    pub open_count: i64,
    pub banned_count: i64,
}

#[derive(Clone, Serialize)]
pub struct Fail2banSetting {
    pub id: i64,
    pub service: String,
    pub max_attempts: i32,
    pub ban_duration_minutes: i32,
    pub find_time_minutes: i32,
    pub enabled: bool,
}

#[derive(Clone, Serialize)]
pub struct Fail2banBanned {
    pub id: i64,
    pub ip_address: String,
    pub service: String,
    pub reason: String,
    pub attempts: i32,
    pub banned_at: String,
    pub expires_at: Option<String>,
    pub permanent: bool,
}

#[derive(Clone, Serialize)]
pub struct Fail2banWhitelist {
    pub id: i64,
    pub ip_address: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Clone, Serialize)]
pub struct Fail2banBlacklist {
    pub id: i64,
    pub ip_address: String,
    pub description: String,
    pub created_at: String,
}

#[derive(Clone, Serialize)]
pub struct Fail2banLogEntry {
    pub id: i64,
    pub ip_address: String,
    pub service: String,
    pub action: String,
    pub details: String,
    pub created_at: String,
}


fn load_available_migrations() -> Vec<(String, String)> {
    let mut migrations = Vec::new();
    let paths = vec!["migrations", "/app/migrations"];
    let mut found_any = false;

    for base_path in paths {
        let path = std::path::Path::new(base_path);
        if !path.exists() || !path.is_dir() {
            continue;
        }

        found_any = true;
        if let Ok(entries) = std::fs::read_dir(path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() 
                   && path.extension().and_then(|ext| ext.to_str()) == Some("sql") {
                    if let Some(filename) = path.file_stem().and_then(|stem| stem.to_str()) {
                        if let Ok(sql) = std::fs::read_to_string(&path) {
                            migrations.push((filename.to_string(), sql));
                        }
                    }
                }
            }
        }
        // If we found a valid directory, we stop looking at other paths to avoid duplicates
        // or mixing environments (unless we want to merge, but typically one source is enough)
        break;
    }

    if !found_any {
        warn!("[db] no migrations directory found (checked ./migrations and /app/migrations)");
    }
    
    // Sort by filename to ensure correct order
    migrations.sort_by(|a, b| a.0.cmp(&b.0));
    migrations
}

fn run_migrations(client: &mut Client) {
    info!("[db] checking for database migrations");

    // 1. Create _migrations table if it doesn't exist
    client
        .execute(
            "CREATE TABLE IF NOT EXISTS _migrations (
                id SERIAL PRIMARY KEY,
                name TEXT UNIQUE NOT NULL,
                applied_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
            )",
            &[],
        )
        .expect("Failed to create _migrations table");

    // 2. Load migrations from files
    let migrations = load_available_migrations();

    if migrations.is_empty() {
        warn!("[db] no migration files found");
        return;
    }

    // 3. Apply pending migrations
    for (name, sql) in migrations {
        let rows = client
            .query("SELECT id FROM _migrations WHERE name = $1", &[&name])
            .expect("Failed to query _migrations");

        if rows.is_empty() {
            info!("[db] applying migration: {}", name);
            let mut transaction = client.transaction().expect("Failed to start transaction");
            transaction.batch_execute(&sql).expect("Failed to execute migration script");
            transaction
                .execute("INSERT INTO _migrations (name) VALUES ($1)", &[&name])
                .expect("Failed to record migration");
            transaction.commit().expect("Failed to commit transaction");
            info!("[db] migration {} applied successfully", name);
        } else {
            debug!("[db] migration {} already applied", name);
        }
    }
}

impl Database {
    pub fn open(url: &str) -> Self {
        info!("[db] opening PostgreSQL database at url={}", url);
        let mut retry_count = 0;
        let max_retries = 30;
        let mut client = loop {
            match Client::connect(url, NoTls) {
                Ok(c) => break c,
                Err(e) => {
                    retry_count += 1;
                    if retry_count >= max_retries {
                        error!(
                            "[db] failed to connect to PostgreSQL after {} retries: {}",
                            max_retries, e
                        );
                        panic!("Failed to connect to PostgreSQL: {}", e);
                    }
                    warn!(
                        "[db] failed to connect to PostgreSQL, retrying ({}/{}): {}",
                        retry_count, max_retries, e
                    );
                    std::thread::sleep(std::time::Duration::from_secs(1));
                }
            }
        };

        run_migrations(&mut client);

        info!("[db] PostgreSQL database opened and schema initialized successfully");
        Database {
            conn: Arc::new(Mutex::new(client)),
        }
    }

    // ── Admin methods ──

    pub fn get_admin_by_username(&self, username: &str) -> Option<Admin> {
        debug!("[db] looking up admin username={}", username);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let row = conn
            .query_opt(
                "SELECT id, username, password_hash, totp_secret, totp_enabled FROM admins WHERE username = $1",
                &[&username],
            )
            .ok()
            .flatten();

        let result = row.map(|row| Admin {
            id: row.get::<_, i64>(0),
            username: row.get::<_, String>(1),
            password_hash: row.get::<_, String>(2),
            totp_secret: row.get::<_, Option<String>>(3),
            totp_enabled: row.get::<_, Option<bool>>(4).unwrap_or(false),
        });

        if result.is_some() {
            debug!("[db] admin found: username={}", username);
        } else {
            warn!("[db] admin not found: username={}", username);
        }
        result
    }

    pub fn update_admin_password(&self, id: i64, hash: &str) {
        info!("[db] updating admin password id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE admins SET password_hash = $1, updated_at = $2 WHERE id = $3",
            &[&hash, &now(), &id],
        );
    }

    pub fn update_admin_totp(&self, id: i64, secret: Option<&str>, enabled: bool) {
        info!("[db] updating admin TOTP id={}, enabled={}", id, enabled);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE admins SET totp_secret = $1, totp_enabled = $2, updated_at = $3 WHERE id = $4",
            &[&secret, &enabled, &now(), &id],
        );
    }

    pub fn seed_admin(&self, username: &str, password_hash: &str) {
        info!("[db] seeding admin user: {}", username);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let ts = now();
        let _ = conn.execute(
            "INSERT INTO admins (username, password_hash, created_at, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (username) DO NOTHING",
            &[&username, &password_hash, &ts, &ts],
        );
    }

    // ── Domain methods ──

    pub fn list_domains(&self) -> Vec<Domain> {
        debug!("[db] listing all domains");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html, bimi_svg
                 FROM domains ORDER BY domain",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list domains: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| Domain {
                id: row.get(0),
                domain: row.get(1),
                active: row.get(2),
                dkim_selector: row.get(3),
                dkim_private_key: row.get(4),
                dkim_public_key: row.get(5),
                footer_html: row.get(6),
                bimi_svg: row.get(7),
            })
            .collect()
    }

    pub fn get_domain(&self, id: i64) -> Option<Domain> {
        debug!("[db] getting domain id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html, bimi_svg
             FROM domains WHERE id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| Domain {
            id: row.get(0),
            domain: row.get(1),
            active: row.get(2),
            dkim_selector: row.get(3),
            dkim_private_key: row.get(4),
            dkim_public_key: row.get(5),
            footer_html: row.get(6),
            bimi_svg: row.get(7),
        })
    }

    pub fn create_domain(&self, domain: &str, footer_html: &str, bimi_svg: &str) -> Result<i64, String> {
        info!("[db] creating domain: {}", domain);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO domains (domain, footer_html, bimi_svg, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5)
                 RETURNING id",
                &[&domain, &footer_html, &bimi_svg, &ts, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to create domain {}: {}", domain, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!("[db] domain created: {} (id={})", domain, id);
        Ok(id)
    }

    pub fn update_domain(&self, id: i64, domain: &str, active: bool, footer_html: &str, bimi_svg: &str) {
        info!(
            "[db] updating domain id={}, domain={}, active={}, footer_present={}, bimi_present={}",
            id,
            domain,
            active,
            !footer_html.trim().is_empty(),
            !bimi_svg.trim().is_empty()
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE domains
             SET domain = $1, active = $2, footer_html = $3, bimi_svg = $4, updated_at = $5
             WHERE id = $6",
            &[&domain, &active, &footer_html, &bimi_svg, &now(), &id],
        );
    }

    pub fn delete_domain(&self, id: i64) {
        warn!("[db] deleting domain id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute("DELETE FROM domains WHERE id = $1", &[&id]);
    }

    pub fn update_domain_dkim(&self, id: i64, selector: &str, private_key: &str, public_key: &str) {
        info!(
            "[db] updating DKIM for domain id={}, selector={}",
            id, selector
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE domains
             SET dkim_selector = $1, dkim_private_key = $2, dkim_public_key = $3, updated_at = $4
             WHERE id = $5",
            &[&selector, &private_key, &public_key, &now(), &id],
        );
    }

    pub fn get_bimi_svg_for_domain(&self, domain: &str) -> Option<String> {
        debug!("[db] looking up BIMI SVG for domain={}", domain);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT bimi_svg FROM domains
             WHERE lower(domain) = lower($1)
               AND bimi_svg IS NOT NULL
               AND bimi_svg <> ''",
            &[&domain],
        )
        .ok()
        .flatten()
        .map(|row| row.get(0))
    }

    pub fn get_footer_for_sender(&self, sender: &str) -> Option<String> {
        let domain_part = sender.split('@').nth(1)?.trim().to_lowercase();
        if domain_part.is_empty() {
            return None;
        }
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT footer_html FROM domains
             WHERE lower(domain) = lower($1)
               AND footer_html IS NOT NULL
               AND footer_html <> ''",
            &[&domain_part],
        )
        .ok()
        .flatten()
        .map(|row| row.get(0))
    }

    // ── Account methods ──

    pub fn list_accounts(&self) -> Vec<Account> {
        debug!("[db] listing all accounts");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, domain_id, username, password_hash, name, active, quota
                 FROM accounts ORDER BY username",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list accounts: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| Account {
                id: row.get(0),
                domain_id: row.get(1),
                username: row.get(2),
                password_hash: row.get(3),
                name: row.get(4),
                active: row.get(5),
                quota: row.get(6),
                domain_name: None,
            })
            .collect()
    }

    pub fn get_account(&self, id: i64) -> Option<Account> {
        debug!("[db] getting account id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT id, domain_id, username, password_hash, name, active, quota
             FROM accounts WHERE id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| Account {
            id: row.get(0),
            domain_id: row.get(1),
            username: row.get(2),
            password_hash: row.get(3),
            name: row.get(4),
            active: row.get(5),
            quota: row.get(6),
            domain_name: None,
        })
    }

    pub fn get_account_with_domain(&self, id: i64) -> Option<Account> {
        debug!("[db] getting account with domain info id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT a.id, a.domain_id, a.username, a.password_hash, a.name, a.active, a.quota, d.domain
             FROM accounts a
             LEFT JOIN domains d ON a.domain_id = d.id
             WHERE a.id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| Account {
            id: row.get(0),
            domain_id: row.get(1),
            username: row.get(2),
            password_hash: row.get(3),
            name: row.get(4),
            active: row.get(5),
            quota: row.get(6),
            domain_name: row.get(7),
        })
    }

    pub fn create_account(
        &self,
        domain_id: i64,
        username: &str,
        password_hash: &str,
        name: &str,
        quota: i64,
    ) -> Result<i64, String> {
        info!(
            "[db] creating account username={}, domain_id={}, quota={}",
            username, domain_id, quota
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO accounts (domain_id, username, password_hash, name, quota, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 RETURNING id",
                &[&domain_id, &username, &password_hash, &name, &quota, &ts, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to create account {}: {}", username, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!("[db] account created: {} (id={})", username, id);
        Ok(id)
    }

    pub fn update_account(&self, id: i64, name: &str, active: bool, quota: i64) {
        info!(
            "[db] updating account id={}, active={}, quota={}",
            id, active, quota
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE accounts
             SET name = $1, active = $2, quota = $3, updated_at = $4
             WHERE id = $5",
            &[&name, &active, &quota, &now(), &id],
        );
    }

    pub fn update_account_password(&self, id: i64, hash: &str) {
        info!("[db] updating account password id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE accounts SET password_hash = $1, updated_at = $2 WHERE id = $3",
            &[&hash, &now(), &id],
        );
    }

    pub fn delete_account(&self, id: i64) {
        warn!("[db] deleting account id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute("DELETE FROM accounts WHERE id = $1", &[&id]);
    }

    pub fn list_all_accounts_with_domain(&self) -> Vec<Account> {
        debug!("[db] listing all accounts with domain info");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT a.id, a.domain_id, a.username, a.password_hash, a.name, a.active, a.quota, d.domain
                 FROM accounts a
                 LEFT JOIN domains d ON a.domain_id = d.id
                 ORDER BY a.username",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list accounts with domain: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| Account {
                id: row.get(0),
                domain_id: row.get(1),
                username: row.get(2),
                password_hash: row.get(3),
                name: row.get(4),
                active: row.get(5),
                quota: row.get(6),
                domain_name: row.get(7),
            })
            .collect()
    }

    // ── Alias methods ──

    pub fn list_aliases(&self) -> Vec<Alias> {
        debug!("[db] listing all aliases");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, domain_id, source, destination, active, tracking_enabled, sort_order
                 FROM aliases ORDER BY sort_order ASC, id ASC",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list aliases: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| Alias {
                id: row.get(0),
                domain_id: row.get(1),
                source: row.get(2),
                destination: row.get(3),
                active: row.get(4),
                tracking_enabled: row.get(5),
                sort_order: row.get(6),
                domain_name: None,
            })
            .collect()
    }

    pub fn get_alias(&self, id: i64) -> Option<Alias> {
        debug!("[db] getting alias id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT id, domain_id, source, destination, active, tracking_enabled, sort_order
             FROM aliases WHERE id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| Alias {
            id: row.get(0),
            domain_id: row.get(1),
            source: row.get(2),
            destination: row.get(3),
            active: row.get(4),
            tracking_enabled: row.get(5),
            sort_order: row.get(6),
            domain_name: None,
        })
    }

    pub fn create_alias(
        &self,
        domain_id: i64,
        source: &str,
        destination: &str,
        tracking: bool,
        sort_order: i64,
    ) -> Result<i64, String> {
        info!(
            "[db] creating alias source={}, destination={}, tracking={}, sort_order={}",
            source, destination, tracking, sort_order
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO aliases (domain_id, source, destination, tracking_enabled, sort_order, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7)
                 RETURNING id",
                &[&domain_id, &source, &destination, &tracking, &sort_order, &ts, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to create alias {} -> {}: {}", source, destination, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!(
            "[db] alias created: {} -> {} (id={})",
            source, destination, id
        );
        Ok(id)
    }

    pub fn update_alias(
        &self,
        id: i64,
        source: &str,
        destination: &str,
        active: bool,
        tracking: bool,
        sort_order: i64,
    ) {
        info!("[db] updating alias id={}, source={}, destination={}, active={}, tracking={}, sort_order={}", id, source, destination, active, tracking, sort_order);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE aliases
             SET source = $1, destination = $2, active = $3, tracking_enabled = $4, sort_order = $5, updated_at = $6
             WHERE id = $7",
            &[&source, &destination, &active, &tracking, &sort_order, &now(), &id],
        );
    }

    pub fn delete_alias(&self, id: i64) {
        warn!("[db] deleting alias id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute("DELETE FROM aliases WHERE id = $1", &[&id]);
    }

    pub fn list_all_aliases_with_domain(&self) -> Vec<Alias> {
        debug!("[db] listing all aliases with domain info");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT a.id, a.domain_id, a.source, a.destination, a.active, a.tracking_enabled, a.sort_order, d.domain
                 FROM aliases a
                 LEFT JOIN domains d ON a.domain_id = d.id
                 ORDER BY a.sort_order ASC, a.id ASC",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list aliases with domain: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| Alias {
                id: row.get(0),
                domain_id: row.get(1),
                source: row.get(2),
                destination: row.get(3),
                active: row.get(4),
                tracking_enabled: row.get(5),
                sort_order: row.get(6),
                domain_name: row.get(7),
            })
            .collect()
    }

    pub fn is_tracking_enabled_for_sender(&self, sender: &str) -> bool {
        debug!("[db] checking tracking status for sender={}", sender);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM aliases WHERE source = $1 AND active = TRUE AND tracking_enabled = TRUE",
                &[&sender],
            )
            .map(|row| row.get(0))
            .unwrap_or(0);
        let enabled = count > 0;
        debug!("[db] tracking enabled for sender={}: {}", sender, enabled);
        enabled
    }

    /// Returns a list of (alias_source, account_email) for building sender_login_maps.
    /// An account owns an alias if they share the same domain_id.
    pub fn get_sender_login_map(&self) -> Vec<(String, String)> {
        debug!("[db] building sender login map");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT al.source, (ac.username || '@' || d.domain) AS account_email
                 FROM aliases al
                 JOIN domains d ON al.domain_id = d.id
                 JOIN accounts ac ON ac.domain_id = al.domain_id
                 WHERE al.active = TRUE AND ac.active = TRUE
                 ORDER BY al.source, account_email",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to build sender login map: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| (row.get(0), row.get(1)))
            .collect()
    }

    /// Check if an email address exists as an active account
    pub fn email_exists(&self, email: &str) -> bool {
        debug!("[db] checking if email exists: {}", email);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        
        // Parse email into username and domain
        let parts: Vec<&str> = email.split('@').collect();
        if parts.len() != 2 {
            warn!("[db] invalid email format: {}", email);
            return false;
        }
        
        let username = parts[0];
        let domain = parts[1];
        
        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM accounts ac
                 JOIN domains d ON ac.domain_id = d.id
                 WHERE ac.username = $1 AND d.domain = $2 AND ac.active = TRUE AND d.active = TRUE",
                &[&username, &domain],
            )
            .map(|row| row.get(0))
            .unwrap_or(0);
        
        let exists = count > 0;
        debug!("[db] email {} exists: {}", email, exists);
        exists
    }

    // ── Tracking methods ──

    pub fn create_tracked_message(
        &self,
        message_id: &str,
        sender: &str,
        recipient: &str,
        subject: &str,
        alias_id: Option<i64>,
    ) {
        info!(
            "[db] creating tracked message id={}, sender={}, recipient={}",
            message_id, sender, recipient
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "INSERT INTO tracked_messages (message_id, sender, recipient, subject, alias_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[&message_id, &sender, &recipient, &subject, &alias_id, &now()],
        );
    }

    pub fn record_pixel_open(&self, message_id: &str, client_ip: &str, user_agent: &str) {
        info!(
            "[db] recording pixel open message_id={}, client_ip={}",
            message_id, client_ip
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "INSERT INTO pixel_opens (message_id, client_ip, user_agent, opened_at)
             VALUES ($1, $2, $3, $4)",
            &[&message_id, &client_ip, &user_agent, &now()],
        );
    }

    pub fn list_tracked_messages(&self, limit: i64) -> Vec<TrackedMessage> {
        debug!("[db] listing tracked messages limit={}", limit);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, message_id, sender, recipient, subject, alias_id, created_at
                 FROM tracked_messages
                 ORDER BY created_at DESC
                 LIMIT $1",
                &[&limit],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list tracked messages: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| TrackedMessage {
                id: row.get(0),
                message_id: row.get(1),
                sender: row.get(2),
                recipient: row.get(3),
                subject: row.get(4),
                alias_id: row.get(5),
                created_at: row.get(6),
            })
            .collect()
    }

    pub fn get_tracked_message(&self, message_id: &str) -> Option<TrackedMessage> {
        debug!("[db] getting tracked message id={}", message_id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT id, message_id, sender, recipient, subject, alias_id, created_at
             FROM tracked_messages WHERE message_id = $1",
            &[&message_id],
        )
        .ok()
        .flatten()
        .map(|row| TrackedMessage {
            id: row.get(0),
            message_id: row.get(1),
            sender: row.get(2),
            recipient: row.get(3),
            subject: row.get(4),
            alias_id: row.get(5),
            created_at: row.get(6),
        })
    }

    pub fn get_opens_for_message(&self, message_id: &str) -> Vec<PixelOpen> {
        debug!("[db] getting opens for message id={}", message_id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, message_id, client_ip, user_agent, opened_at
                 FROM pixel_opens WHERE message_id = $1
                 ORDER BY opened_at DESC",
                &[&message_id],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to get opens for message: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| PixelOpen {
                id: row.get(0),
                message_id: row.get(1),
                client_ip: row.get(2),
                user_agent: row.get(3),
                opened_at: row.get(4),
            })
            .collect()
    }

    // ── Generic settings storage (key/value) ──

    pub fn set_setting(&self, key: &str, value: &str) {
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "INSERT INTO settings (key, value)
             VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            &[&key, &value],
        );
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt("SELECT value FROM settings WHERE key = $1", &[&key])
            .ok()
            .flatten()
            .map(|row| row.get(0))
    }

    pub fn is_fail2ban_enabled(&self) -> bool {
        self.get_setting("fail2ban_enabled")
            .map(|v| v == "true")
            .unwrap_or(false)
    }

    pub fn get_stats(&self) -> Stats {
        debug!("[db] fetching aggregate stats");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });

        let domain_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM domains", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);
        let account_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM accounts", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);
        let alias_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM aliases", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);
        let tracked_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM tracked_messages", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);
        let open_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM pixel_opens", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);

        let banned_count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM fail2ban_banned WHERE permanent = TRUE OR expires_at > $1",
                &[&now()],
            )
            .map(|row| row.get(0))
            .unwrap_or(0);

        Stats {
            domain_count,
            account_count,
            alias_count,
            tracked_count,
            open_count,
            banned_count,
        }
    }

    // ── Fail2ban methods ──

    pub fn list_fail2ban_settings(&self) -> Vec<Fail2banSetting> {
        debug!("[db] listing fail2ban settings");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, service, max_attempts, ban_duration_minutes, find_time_minutes, enabled
                 FROM fail2ban_settings ORDER BY service",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list fail2ban settings: {}", e);
                Vec::new()
            });

        rows.into_iter()
            .map(|row| Fail2banSetting {
                id: row.get(0),
                service: row.get(1),
                max_attempts: row.get(2),
                ban_duration_minutes: row.get(3),
                find_time_minutes: row.get(4),
                enabled: row.get(5),
            })
            .collect()
    }

    pub fn get_fail2ban_setting(&self, id: i64) -> Option<Fail2banSetting> {
        debug!("[db] getting fail2ban setting id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT id, service, max_attempts, ban_duration_minutes, find_time_minutes, enabled
             FROM fail2ban_settings WHERE id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| Fail2banSetting {
            id: row.get(0),
            service: row.get(1),
            max_attempts: row.get(2),
            ban_duration_minutes: row.get(3),
            find_time_minutes: row.get(4),
            enabled: row.get(5),
        })
    }

    pub fn update_fail2ban_setting(
        &self,
        id: i64,
        max_attempts: i32,
        ban_duration_minutes: i32,
        find_time_minutes: i32,
        enabled: bool,
    ) {
        info!(
            "[db] updating fail2ban setting id={}, max_attempts={}, ban_duration={}, find_time={}, enabled={}",
            id, max_attempts, ban_duration_minutes, find_time_minutes, enabled
        );
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute(
            "UPDATE fail2ban_settings SET max_attempts = $1, ban_duration_minutes = $2, find_time_minutes = $3, enabled = $4, updated_at = $5 WHERE id = $6",
            &[&max_attempts, &ban_duration_minutes, &find_time_minutes, &enabled, &now(), &id],
        );
    }

    pub fn list_fail2ban_banned(&self) -> Vec<Fail2banBanned> {
        debug!("[db] listing banned IPs");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, ip_address, service, reason, attempts, banned_at, expires_at, permanent
                 FROM fail2ban_banned
                 WHERE permanent = TRUE OR expires_at > $1
                 ORDER BY banned_at DESC",
                &[&now()],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list banned IPs: {}", e);
                Vec::new()
            });

        rows.into_iter()
            .map(|row| Fail2banBanned {
                id: row.get(0),
                ip_address: row.get(1),
                service: row.get(2),
                reason: row.get::<_, Option<String>>(3).unwrap_or_default(),
                attempts: row.get(4),
                banned_at: row.get(5),
                expires_at: row.get(6),
                permanent: row.get(7),
            })
            .collect()
    }

    pub fn ban_ip(&self, ip_address: &str, service: &str, reason: &str, duration_minutes: i32, permanent: bool) -> Result<i64, String> {
        info!("[db] banning IP={} service={} permanent={}", ip_address, service, permanent);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let ts = now();
        let expires = if permanent {
            None
        } else {
            Some(
                (chrono::Utc::now() + chrono::Duration::minutes(duration_minutes as i64))
                    .format("%Y-%m-%d %H:%M:%S")
                    .to_string(),
            )
        };
        let row = conn
            .query_one(
                "INSERT INTO fail2ban_banned (ip_address, service, reason, attempts, banned_at, expires_at, permanent, created_at)
                 VALUES ($1, $2, $3, 0, $4, $5, $6, $7)
                 ON CONFLICT (ip_address, service) DO UPDATE SET reason = EXCLUDED.reason, banned_at = EXCLUDED.banned_at, expires_at = EXCLUDED.expires_at, permanent = EXCLUDED.permanent
                 RETURNING id",
                &[&ip_address, &service, &reason, &ts, &expires, &permanent, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to ban IP {}: {}", ip_address, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);

        // Log the action
        let _ = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, $2, 'ban', $3, $4)",
            &[&ip_address, &service, &reason, &ts],
        );

        info!("[db] IP banned: {} (id={})", ip_address, id);
        Ok(id)
    }

    pub fn unban_ip(&self, id: i64) {
        info!("[db] unbanning IP id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        // Get IP for logging before delete
        let ip_info = conn
            .query_opt("SELECT ip_address, service FROM fail2ban_banned WHERE id = $1", &[&id])
            .ok()
            .flatten();
        let _ = conn.execute("DELETE FROM fail2ban_banned WHERE id = $1", &[&id]);
        if let Some(row) = ip_info {
            let ip: String = row.get(0);
            let service: String = row.get(1);
            let _ = conn.execute(
                "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, $2, 'unban', 'Manual unban from admin', $3)",
                &[&ip, &service, &now()],
            );
        }
    }

    pub fn list_fail2ban_whitelist(&self) -> Vec<Fail2banWhitelist> {
        debug!("[db] listing fail2ban whitelist");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, ip_address, description, created_at FROM fail2ban_whitelist ORDER BY created_at DESC",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list whitelist: {}", e);
                Vec::new()
            });

        rows.into_iter()
            .map(|row| Fail2banWhitelist {
                id: row.get(0),
                ip_address: row.get(1),
                description: row.get::<_, Option<String>>(2).unwrap_or_default(),
                created_at: row.get::<_, Option<String>>(3).unwrap_or_default(),
            })
            .collect()
    }

    pub fn add_to_whitelist(&self, ip_address: &str, description: &str) -> Result<i64, String> {
        info!("[db] adding IP to whitelist: {}", ip_address);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO fail2ban_whitelist (ip_address, description, created_at) VALUES ($1, $2, $3)
                 ON CONFLICT (ip_address) DO UPDATE SET description = EXCLUDED.description
                 RETURNING id",
                &[&ip_address, &description, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to add to whitelist {}: {}", ip_address, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);

        // Also unban if currently banned
        let _ = conn.execute("DELETE FROM fail2ban_banned WHERE ip_address = $1", &[&ip_address]);

        let _ = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, 'all', 'whitelist', $2, $3)",
            &[&ip_address, &description, &ts],
        );

        Ok(id)
    }

    pub fn remove_from_whitelist(&self, id: i64) {
        info!("[db] removing from whitelist id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute("DELETE FROM fail2ban_whitelist WHERE id = $1", &[&id]);
    }

    pub fn list_fail2ban_blacklist(&self) -> Vec<Fail2banBlacklist> {
        debug!("[db] listing fail2ban blacklist");
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, ip_address, description, created_at FROM fail2ban_blacklist ORDER BY created_at DESC",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list blacklist: {}", e);
                Vec::new()
            });

        rows.into_iter()
            .map(|row| Fail2banBlacklist {
                id: row.get(0),
                ip_address: row.get(1),
                description: row.get::<_, Option<String>>(2).unwrap_or_default(),
                created_at: row.get::<_, Option<String>>(3).unwrap_or_default(),
            })
            .collect()
    }

    pub fn add_to_blacklist(&self, ip_address: &str, description: &str) -> Result<i64, String> {
        info!("[db] adding IP to blacklist: {}", ip_address);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO fail2ban_blacklist (ip_address, description, created_at) VALUES ($1, $2, $3)
                 ON CONFLICT (ip_address) DO UPDATE SET description = EXCLUDED.description
                 RETURNING id",
                &[&ip_address, &description, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to add to blacklist {}: {}", ip_address, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);

        // Also permanently ban this IP
        let _ = conn.execute(
            "INSERT INTO fail2ban_banned (ip_address, service, reason, attempts, banned_at, expires_at, permanent, created_at)
             VALUES ($1, 'all', 'Blacklisted', 0, $2, NULL, TRUE, $2)
             ON CONFLICT (ip_address, service) DO UPDATE SET permanent = TRUE, reason = 'Blacklisted', expires_at = NULL",
            &[&ip_address, &ts],
        );

        let _ = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, 'all', 'blacklist', $2, $3)",
            &[&ip_address, &description, &ts],
        );

        Ok(id)
    }

    pub fn remove_from_blacklist(&self, id: i64) {
        info!("[db] removing from blacklist id={}", id);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let _ = conn.execute("DELETE FROM fail2ban_blacklist WHERE id = $1", &[&id]);
    }

    pub fn list_fail2ban_log(&self, limit: i64) -> Vec<Fail2banLogEntry> {
        debug!("[db] listing fail2ban log limit={}", limit);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let rows = conn
            .query(
                "SELECT id, ip_address, service, action, details, created_at
                 FROM fail2ban_log ORDER BY created_at DESC LIMIT $1",
                &[&limit],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list fail2ban log: {}", e);
                Vec::new()
            });

        rows.into_iter()
            .map(|row| Fail2banLogEntry {
                id: row.get(0),
                ip_address: row.get(1),
                service: row.get(2),
                action: row.get(3),
                details: row.get::<_, Option<String>>(4).unwrap_or_default(),
                created_at: row.get::<_, Option<String>>(5).unwrap_or_default(),
            })
            .collect()
    }

    pub fn is_ip_whitelisted(&self, ip_address: &str) -> bool {
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let count: i64 = conn
            .query_one("SELECT COUNT(*) FROM fail2ban_whitelist WHERE ip_address = $1", &[&ip_address])
            .map(|row| row.get(0))
            .unwrap_or(0);
        count > 0
    }

    pub fn is_ip_banned(&self, ip_address: &str) -> bool {
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM fail2ban_banned WHERE ip_address = $1 AND (permanent = TRUE OR expires_at > $2)",
                &[&ip_address, &now()],
            )
            .map(|row| row.get(0))
            .unwrap_or(0);
        count > 0
    }

    pub fn get_fail2ban_setting_by_service(&self, service: &str) -> Option<Fail2banSetting> {
        debug!("[db] getting fail2ban setting for service={}", service);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        conn.query_opt(
            "SELECT id, service, max_attempts, ban_duration_minutes, find_time_minutes, enabled
             FROM fail2ban_settings WHERE service = $1",
            &[&service],
        )
        .ok()
        .flatten()
        .map(|row| Fail2banSetting {
            id: row.get(0),
            service: row.get(1),
            max_attempts: row.get(2),
            ban_duration_minutes: row.get(3),
            find_time_minutes: row.get(4),
            enabled: row.get(5),
        })
    }

    pub fn record_fail2ban_attempt(&self, ip_address: &str, service: &str, details: &str) {
        info!("[db] recording fail2ban attempt ip={} service={}", ip_address, service);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        if let Err(e) = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, $2, 'attempt', $3, $4)",
            &[&ip_address, &service, &details, &now()],
        ) {
            error!("[db] failed to record fail2ban attempt for ip={}: {}", ip_address, e);
        }
    }

    pub fn count_recent_attempts(&self, ip_address: &str, service: &str, minutes: i32) -> i64 {
        debug!("[db] counting recent attempts ip={} service={} window={}min", ip_address, service, minutes);
        let mut conn = self.conn.lock().unwrap_or_else(|e| { warn!("[db] mutex was poisoned, recovering connection"); e.into_inner() });
        let cutoff = (chrono::Utc::now() - chrono::Duration::minutes(minutes as i64))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM fail2ban_log WHERE ip_address = $1 AND service = $2 AND action = 'attempt' AND created_at > $3",
                &[&ip_address, &service, &cutoff],
            )
            .map(|row| row.get(0))
            .unwrap_or(0);
        count
    }
}
