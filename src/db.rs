use log::{debug, error, info, warn};
use postgres::{Client, NoTls};
use serde::Serialize;
use std::sync::{Arc, Mutex, MutexGuard};

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
    pub unsubscribe_enabled: bool,
}

#[derive(Clone, Serialize)]
pub struct UnsubscribeEntry {
    pub id: i64,
    pub email: String,
    pub domain: String,
    pub created_at: String,
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
pub struct Forwarding {
    pub id: i64,
    pub domain_id: i64,
    pub source: String,
    pub destination: String,
    pub active: bool,
    pub keep_copy: bool,
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
    pub forwarding_count: i64,
    pub tracked_count: i64,
    pub open_count: i64,
    pub banned_count: i64,
    pub webhook_count: i64,
    pub unsubscribe_count: i64,
    pub dkim_ready_count: i64,
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

#[derive(Clone, Serialize)]
pub struct SpamblList {
    pub id: i64,
    pub name: String,
    pub hostname: String,
    pub enabled: bool,
}

#[derive(Clone, Serialize)]
pub struct WebhookLog {
    pub id: i64,
    pub url: String,
    pub request_body: String,
    pub response_status: Option<i32>,
    pub response_body: String,
    pub error: String,
    pub duration_ms: Option<i64>,
    pub sender: String,
    pub subject: String,
    pub created_at: String,
}

#[derive(Clone, Serialize)]
pub struct DmarcInbox {
    pub id: i64,
    pub account_id: i64,
    pub label: String,
    pub created_at: String,
    pub account_username: Option<String>,
    pub account_domain: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct OutboundRelay {
    pub id: i64,
    pub name: String,
    pub host: String,
    pub port: i32,
    pub auth_type: String,
    pub username: Option<String>,
    pub password: Option<String>,
    pub active: bool,
}

#[derive(Clone, Serialize)]
pub struct OutboundRelayAssignment {
    pub id: i64,
    pub relay_id: i64,
    pub assignment_type: String,
    pub pattern: String,
    pub relay_name: Option<String>,
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
        Self::try_open(url).unwrap_or_else(|e| {
            panic!("Failed to connect to PostgreSQL: {}", e);
        })
    }

    /// Try to open a database connection, returning an error on failure
    /// instead of panicking. Used by short-lived processes (e.g. the
    /// content filter) where a connection failure should be handled
    /// gracefully rather than crashing.
    pub fn try_open(url: &str) -> Result<Self, String> {
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
                        return Err(format!("Failed to connect to PostgreSQL after {} retries: {}", max_retries, e));
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
        Ok(Database {
            conn: Arc::new(Mutex::new(client)),
        })
    }

    /// Acquire the database connection, recovering from mutex poisoning.
    fn conn(&self) -> MutexGuard<'_, Client> {
        self.conn.lock().unwrap_or_else(|e| {
            warn!("[db] mutex was poisoned, recovering connection");
            e.into_inner()
        })
    }

    // ── Admin methods ──

    pub fn get_admin_by_username(&self, username: &str) -> Option<Admin> {
        debug!("[db] looking up admin username={}", username);
        let mut conn = self.conn();
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
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE admins SET password_hash = $1, updated_at = $2 WHERE id = $3",
            &[&hash, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn update_admin_totp(&self, id: i64, secret: Option<&str>, enabled: bool) {
        info!("[db] updating admin TOTP id={}, enabled={}", id, enabled);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE admins SET totp_secret = $1, totp_enabled = $2, updated_at = $3 WHERE id = $4",
            &[&secret, &enabled, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn seed_admin(&self, username: &str, password_hash: &str) {
        info!("[db] seeding admin user: {}", username);
        let mut conn = self.conn();
        let ts = now();
        if let Err(e) = conn.execute(
            "INSERT INTO admins (username, password_hash, created_at, updated_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (username) DO NOTHING",
            &[&username, &password_hash, &ts, &ts],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    // ── Domain methods ──

    pub fn list_domains(&self) -> Vec<Domain> {
        debug!("[db] listing all domains");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html, bimi_svg, unsubscribe_enabled
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
                unsubscribe_enabled: row.get(8),
            })
            .collect()
    }

    pub fn get_domain(&self, id: i64) -> Option<Domain> {
        debug!("[db] getting domain id={}", id);
        let mut conn = self.conn();
        conn.query_opt(
            "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html, bimi_svg, unsubscribe_enabled
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
            unsubscribe_enabled: row.get(8),
        })
    }

    pub fn get_domain_by_name(&self, domain_name: &str) -> Option<Domain> {
        debug!("[db] getting domain by name={}", domain_name);
        let mut conn = self.conn();
        conn.query_opt(
            "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html, bimi_svg, unsubscribe_enabled
             FROM domains WHERE LOWER(domain) = LOWER($1)",
            &[&domain_name],
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
            unsubscribe_enabled: row.get(8),
        })
    }

    pub fn create_domain(&self, domain: &str, footer_html: &str, bimi_svg: &str, unsubscribe_enabled: bool) -> Result<i64, String> {
        info!("[db] creating domain: {}", domain);
        let mut conn = self.conn();
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO domains (domain, footer_html, bimi_svg, unsubscribe_enabled, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 RETURNING id",
                &[&domain, &footer_html, &bimi_svg, &unsubscribe_enabled, &ts, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to create domain {}: {}", domain, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!("[db] domain created: {} (id={})", domain, id);
        Ok(id)
    }

    pub fn update_domain(&self, id: i64, domain: &str, active: bool, footer_html: &str, bimi_svg: &str, unsubscribe_enabled: bool) {
        info!(
            "[db] updating domain id={}, domain={}, active={}, footer_present={}, bimi_present={}, unsubscribe_enabled={}",
            id,
            domain,
            active,
            !footer_html.trim().is_empty(),
            !bimi_svg.trim().is_empty(),
            unsubscribe_enabled
        );
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE domains
             SET domain = $1, active = $2, footer_html = $3, bimi_svg = $4, unsubscribe_enabled = $5, updated_at = $6
             WHERE id = $7",
            &[&domain, &active, &footer_html, &bimi_svg, &unsubscribe_enabled, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn delete_domain(&self, id: i64) {
        warn!("[db] deleting domain id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM domains WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn update_domain_dkim(&self, id: i64, selector: &str, private_key: &str, public_key: &str) {
        info!(
            "[db] updating DKIM for domain id={}, selector={}",
            id, selector
        );
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE domains
             SET dkim_selector = $1, dkim_private_key = $2, dkim_public_key = $3, updated_at = $4
             WHERE id = $5",
            &[&selector, &private_key, &public_key, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn get_bimi_svg_for_domain(&self, domain: &str) -> Option<String> {
        debug!("[db] looking up BIMI SVG for domain={}", domain);
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE accounts
             SET name = $1, active = $2, quota = $3, updated_at = $4
             WHERE id = $5",
            &[&name, &active, &quota, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn update_account_password(&self, id: i64, hash: &str) {
        info!("[db] updating account password id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE accounts SET password_hash = $1, updated_at = $2 WHERE id = $3",
            &[&hash, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn delete_account(&self, id: i64) {
        warn!("[db] deleting account id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM accounts WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn list_all_accounts_with_domain(&self) -> Vec<Account> {
        debug!("[db] listing all accounts with domain info");
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
    ) -> Result<i64, String> {
        info!(
            "[db] creating alias source={}, destination={}, tracking={}",
            source, destination, tracking
        );
        let mut conn = self.conn();
        let ts = now();
        
        // Calculate sort_order: 0 for specific addresses, 1 for catch-alls
        let sort_order: i64 = if source.trim().starts_with('*') { 1 } else { 0 };
        
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
    ) {
        info!("[db] updating alias id={}, source={}, destination={}, active={}, tracking={}", id, source, destination, active, tracking);
        let mut conn = self.conn();
        
        // Calculate sort_order: 0 for specific addresses, 1 for catch-alls
        let sort_order: i64 = if source.trim().starts_with('*') { 1 } else { 0 };
        
        if let Err(e) = conn.execute(
            "UPDATE aliases
             SET source = $1, destination = $2, active = $3, tracking_enabled = $4, sort_order = $5, updated_at = $6
             WHERE id = $7",
            &[&source, &destination, &active, &tracking, &sort_order, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn delete_alias(&self, id: i64) {
        warn!("[db] deleting alias id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM aliases WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn list_all_aliases_with_domain(&self) -> Vec<Alias> {
        debug!("[db] listing all aliases with domain info");
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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


    /// Check if an email address exists as an active account
    pub fn email_exists(&self, email: &str) -> bool {
        debug!("[db] checking if email exists: {}", email);
        let mut conn = self.conn();
        
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

    // ── Forwarding methods ──

    pub fn list_all_forwardings_with_domain(&self) -> Vec<Forwarding> {
        debug!("[db] listing all forwardings with domain info");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT f.id, f.domain_id, f.source, f.destination, f.active, f.keep_copy, d.domain
                 FROM forwardings f
                 LEFT JOIN domains d ON f.domain_id = d.id
                 ORDER BY f.id ASC",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list forwardings with domain: {}", e);
                Vec::new()
            });
        rows.into_iter()
            .map(|row| Forwarding {
                id: row.get(0),
                domain_id: row.get(1),
                source: row.get(2),
                destination: row.get(3),
                active: row.get(4),
                keep_copy: row.get(5),
                domain_name: row.get(6),
            })
            .collect()
    }

    pub fn get_forwarding(&self, id: i64) -> Option<Forwarding> {
        debug!("[db] getting forwarding id={}", id);
        let mut conn = self.conn();
        conn.query_opt(
            "SELECT f.id, f.domain_id, f.source, f.destination, f.active, f.keep_copy, d.domain
             FROM forwardings f
             LEFT JOIN domains d ON f.domain_id = d.id
             WHERE f.id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| Forwarding {
            id: row.get(0),
            domain_id: row.get(1),
            source: row.get(2),
            destination: row.get(3),
            active: row.get(4),
            keep_copy: row.get(5),
            domain_name: row.get(6),
        })
    }

    pub fn create_forwarding(
        &self,
        domain_id: i64,
        source: &str,
        destination: &str,
        keep_copy: bool,
    ) -> Result<i64, String> {
        info!(
            "[db] creating forwarding source={}, destination={}, keep_copy={}",
            source, destination, keep_copy
        );
        let mut conn = self.conn();
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO forwardings (domain_id, source, destination, keep_copy, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6)
                 RETURNING id",
                &[&domain_id, &source, &destination, &keep_copy, &ts, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to create forwarding {} -> {}: {}", source, destination, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!("[db] forwarding created: {} -> {} (id={})", source, destination, id);
        Ok(id)
    }

    pub fn update_forwarding(
        &self,
        id: i64,
        source: &str,
        destination: &str,
        active: bool,
        keep_copy: bool,
    ) {
        info!("[db] updating forwarding id={}, source={}, destination={}, active={}, keep_copy={}", id, source, destination, active, keep_copy);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE forwardings
             SET source = $1, destination = $2, active = $3, keep_copy = $4, updated_at = $5
             WHERE id = $6",
            &[&source, &destination, &active, &keep_copy, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn delete_forwarding(&self, id: i64) {
        warn!("[db] deleting forwarding id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM forwardings WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
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
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO tracked_messages (message_id, sender, recipient, subject, alias_id, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
            &[&message_id, &sender, &recipient, &subject, &alias_id, &now()],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn record_pixel_open(&self, message_id: &str, client_ip: &str, user_agent: &str) {
        info!(
            "[db] recording pixel open message_id={}, client_ip={}",
            message_id, client_ip
        );
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO pixel_opens (message_id, client_ip, user_agent, opened_at)
             VALUES ($1, $2, $3, $4)",
            &[&message_id, &client_ip, &user_agent, &now()],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn list_tracked_messages(&self, limit: i64) -> Vec<TrackedMessage> {
        debug!("[db] listing tracked messages limit={}", limit);
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO settings (key, value)
             VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            &[&key, &value],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let mut conn = self.conn();
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
        let mut conn = self.conn();

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
        let forwarding_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM forwardings", &[])
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

        let webhook_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM webhook_logs", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);

        let unsubscribe_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM unsubscribe_list", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);

        let dkim_ready_count: i64 = conn
            .query_one("SELECT COUNT(*) FROM domains WHERE dkim_public_key IS NOT NULL AND active = TRUE", &[])
            .map(|row| row.get(0))
            .unwrap_or(0);

        Stats {
            domain_count,
            account_count,
            alias_count,
            forwarding_count,
            tracked_count,
            open_count,
            banned_count,
            webhook_count,
            unsubscribe_count,
            dkim_ready_count,
        }
    }

    // ── Fail2ban methods ──

    pub fn list_fail2ban_settings(&self) -> Vec<Fail2banSetting> {
        debug!("[db] listing fail2ban settings");
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE fail2ban_settings SET max_attempts = $1, ban_duration_minutes = $2, find_time_minutes = $3, enabled = $4, updated_at = $5 WHERE id = $6",
            &[&max_attempts, &ban_duration_minutes, &find_time_minutes, &enabled, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn list_fail2ban_banned(&self) -> Vec<Fail2banBanned> {
        debug!("[db] listing banned IPs");
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        if let Err(e) = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, $2, 'ban', $3, $4)",
            &[&ip_address, &service, &reason, &ts],
        ) {
            error!("[db] failed to execute query: {}", e);
        }

        info!("[db] IP banned: {} (id={})", ip_address, id);
        Ok(id)
    }

    pub fn unban_ip(&self, id: i64) {
        info!("[db] unbanning IP id={}", id);
        let mut conn = self.conn();
        // Get IP for logging before delete
        let ip_info = conn
            .query_opt("SELECT ip_address, service FROM fail2ban_banned WHERE id = $1", &[&id])
            .ok()
            .flatten();
        if let Err(e) = conn.execute("DELETE FROM fail2ban_banned WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
        if let Some(row) = ip_info {
            let ip: String = row.get(0);
            let service: String = row.get(1);
            if let Err(e) = conn.execute(
                "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, $2, 'unban', 'Manual unban from admin', $3)",
                &[&ip, &service, &now()],
            ) {
                error!("[db] failed to execute query: {}", e);
            }
        }
    }

    pub fn list_fail2ban_whitelist(&self) -> Vec<Fail2banWhitelist> {
        debug!("[db] listing fail2ban whitelist");
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        if let Err(e) = conn.execute("DELETE FROM fail2ban_banned WHERE ip_address = $1", &[&ip_address]) {
            error!("[db] failed to execute query: {}", e);
        }

        if let Err(e) = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, 'all', 'whitelist', $2, $3)",
            &[&ip_address, &description, &ts],
        ) {
            error!("[db] failed to execute query: {}", e);
        }

        Ok(id)
    }

    pub fn remove_from_whitelist(&self, id: i64) {
        info!("[db] removing from whitelist id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM fail2ban_whitelist WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn list_fail2ban_blacklist(&self) -> Vec<Fail2banBlacklist> {
        debug!("[db] listing fail2ban blacklist");
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        if let Err(e) = conn.execute(
            "INSERT INTO fail2ban_banned (ip_address, service, reason, attempts, banned_at, expires_at, permanent, created_at)
             VALUES ($1, 'all', 'Blacklisted', 0, $2, NULL, TRUE, $2)
             ON CONFLICT (ip_address, service) DO UPDATE SET permanent = TRUE, reason = 'Blacklisted', expires_at = NULL",
            &[&ip_address, &ts],
        ) {
            error!("[db] failed to execute query: {}", e);
        }

        if let Err(e) = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, 'all', 'blacklist', $2, $3)",
            &[&ip_address, &description, &ts],
        ) {
            error!("[db] failed to execute query: {}", e);
        }

        Ok(id)
    }

    pub fn remove_from_blacklist(&self, id: i64) {
        info!("[db] removing from blacklist id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM fail2ban_blacklist WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn list_fail2ban_log(&self, limit: i64) -> Vec<Fail2banLogEntry> {
        debug!("[db] listing fail2ban log limit={}", limit);
        let mut conn = self.conn();
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
        let mut conn = self.conn();
        let count: i64 = conn
            .query_one("SELECT COUNT(*) FROM fail2ban_whitelist WHERE ip_address = $1", &[&ip_address])
            .map(|row| row.get(0))
            .unwrap_or(0);
        count > 0
    }

    pub fn is_ip_banned(&self, ip_address: &str) -> bool {
        let mut conn = self.conn();
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
        let mut conn = self.conn();
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
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO fail2ban_log (ip_address, service, action, details, created_at) VALUES ($1, $2, 'attempt', $3, $4)",
            &[&ip_address, &service, &details, &now()],
        ) {
            error!("[db] failed to record fail2ban attempt for ip={}: {}", ip_address, e);
        }
    }

    pub fn count_recent_attempts(&self, ip_address: &str, service: &str, minutes: i32) -> i64 {
        debug!("[db] counting recent attempts ip={} service={} window={}min", ip_address, service, minutes);
        let mut conn = self.conn();
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

    pub fn create_unsubscribe_token(&self, token: &str, recipient_email: &str, sender_domain: &str) {
        debug!("[db] creating unsubscribe token for recipient={} domain={}", recipient_email, sender_domain);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO unsubscribe_tokens (token, recipient_email, sender_domain, created_at)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (token) DO NOTHING",
            &[&token, &recipient_email, &sender_domain, &now()],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn get_unsubscribe_by_token(&self, token: &str) -> Option<(String, String)> {
        debug!("[db] looking up unsubscribe token={}", token);
        let mut conn = self.conn();
        conn.query_opt(
            "SELECT recipient_email, sender_domain FROM unsubscribe_tokens WHERE token = $1",
            &[&token],
        )
        .ok()
        .flatten()
        .map(|row| (row.get(0), row.get(1)))
    }

    pub fn record_unsubscribe(&self, email: &str, domain: &str) {
        info!("[db] recording unsubscribe email={} domain={}", email, domain);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO unsubscribe_list (email, domain, created_at)
             VALUES ($1, $2, $3)
             ON CONFLICT (email, domain) DO NOTHING",
            &[&email, &domain, &now()],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn is_unsubscribed(&self, email: &str, domain: &str) -> bool {
        let mut conn = self.conn();
        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM unsubscribe_list WHERE LOWER(email) = LOWER($1) AND LOWER(domain) = LOWER($2)",
                &[&email, &domain],
            )
            .map(|row| row.get(0))
            .unwrap_or(0);
        count > 0
    }

    pub fn list_unsubscribes(&self) -> Vec<UnsubscribeEntry> {
        debug!("[db] listing all unsubscribe entries");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT id, email, domain, created_at FROM unsubscribe_list ORDER BY created_at DESC",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list unsubscribes: {}", e);
                Vec::new()
            });
        rows.into_iter()
            .map(|row| UnsubscribeEntry {
                id: row.get(0),
                email: row.get(1),
                domain: row.get(2),
                created_at: row.get(3),
            })
            .collect()
    }

    pub fn delete_unsubscribe(&self, id: i64) {
        warn!("[db] deleting unsubscribe entry id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM unsubscribe_list WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn is_unsubscribe_enabled_for_domain(&self, sender_domain: &str) -> bool {
        let mut conn = self.conn();
        let count: i64 = conn
            .query_one(
                "SELECT COUNT(*) FROM domains WHERE LOWER(domain) = LOWER($1) AND unsubscribe_enabled = TRUE AND active = TRUE",
                &[&sender_domain],
            )
            .map(|row| row.get(0))
            .unwrap_or(0);
        count > 0
    }

    // ── Spambl methods ──

    pub fn list_spambl_lists(&self) -> Vec<SpamblList> {
        debug!("[db] listing spambl lists");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT id, name, hostname, enabled FROM spambl_lists ORDER BY id",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list spambl lists: {}", e);
                Vec::new()
            });

        rows.into_iter()
            .map(|row| SpamblList {
                id: row.get(0),
                name: row.get(1),
                hostname: row.get(2),
                enabled: row.get(3),
            })
            .collect()
    }

    pub fn set_spambl_enabled(&self, id: i64, enabled: bool) {
        info!("[db] setting spambl id={} enabled={}", id, enabled);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE spambl_lists SET enabled = $1, updated_at = $2 WHERE id = $3",
            &[&enabled, &now(), &id],
        ) {
            error!("[db] failed to set spambl id={} enabled={}: {}", id, enabled, e);
        }
    }

    pub fn list_enabled_spambl_hostnames(&self) -> Vec<String> {
        debug!("[db] listing enabled spambl hostnames");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT hostname FROM spambl_lists WHERE enabled = TRUE ORDER BY id",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list enabled spambl hostnames: {}", e);
                Vec::new()
            });

        rows.into_iter().map(|row| row.get(0)).collect()
    }

    // ── Webhook log methods ──

    pub fn log_webhook(
        &self,
        url: &str,
        request_body: &str,
        response_status: Option<i32>,
        response_body: &str,
        error: &str,
        duration_ms: i64,
        sender: &str,
        subject: &str,
    ) {
        debug!("[db] logging webhook execution url={}", url);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "INSERT INTO webhook_logs (url, request_body, response_status, response_body, error, duration_ms, sender, subject, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
            &[&url, &request_body, &response_status, &response_body, &error, &duration_ms, &sender, &subject, &now()],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn count_webhook_logs(&self) -> i64 {
        let mut conn = self.conn();
        conn.query_one("SELECT COUNT(*) FROM webhook_logs", &[])
            .map(|row| row.get(0))
            .unwrap_or(0)
    }

    pub fn list_webhook_logs(&self, limit: i64, offset: i64) -> Vec<WebhookLog> {
        debug!("[db] listing webhook logs limit={} offset={}", limit, offset);
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT id, url, request_body, response_status, response_body, error, duration_ms, sender, subject, created_at
                 FROM webhook_logs ORDER BY created_at DESC LIMIT $1 OFFSET $2",
                &[&limit, &offset],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list webhook logs: {}", e);
                Vec::new()
            });

        rows.into_iter()
            .map(|row| WebhookLog {
                id: row.get(0),
                url: row.get(1),
                request_body: row.get::<_, Option<String>>(2).unwrap_or_default(),
                response_status: row.get(3),
                response_body: row.get::<_, Option<String>>(4).unwrap_or_default(),
                error: row.get::<_, Option<String>>(5).unwrap_or_default(),
                duration_ms: row.get(6),
                sender: row.get::<_, Option<String>>(7).unwrap_or_default(),
                subject: row.get::<_, Option<String>>(8).unwrap_or_default(),
                created_at: row.get(9),
            })
            .collect()
    }

    // ── DMARC inbox methods ──

    pub fn list_dmarc_inboxes(&self) -> Vec<DmarcInbox> {
        debug!("[db] listing dmarc inboxes");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT di.id, di.account_id, di.label, di.created_at, a.username, d.domain
                 FROM dmarc_inboxes di
                 JOIN accounts a ON di.account_id = a.id
                 LEFT JOIN domains d ON a.domain_id = d.id
                 ORDER BY di.id ASC",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list dmarc inboxes: {}", e);
                Vec::new()
            });
        rows.into_iter()
            .map(|row| DmarcInbox {
                id: row.get(0),
                account_id: row.get(1),
                label: row.get::<_, Option<String>>(2).unwrap_or_default(),
                created_at: row.get::<_, Option<String>>(3).unwrap_or_default(),
                account_username: row.get(4),
                account_domain: row.get(5),
            })
            .collect()
    }

    pub fn get_dmarc_inbox(&self, id: i64) -> Option<DmarcInbox> {
        debug!("[db] getting dmarc inbox id={}", id);
        let mut conn = self.conn();
        conn.query_opt(
            "SELECT di.id, di.account_id, di.label, di.created_at, a.username, d.domain
             FROM dmarc_inboxes di
             JOIN accounts a ON di.account_id = a.id
             LEFT JOIN domains d ON a.domain_id = d.id
             WHERE di.id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| DmarcInbox {
            id: row.get(0),
            account_id: row.get(1),
            label: row.get::<_, Option<String>>(2).unwrap_or_default(),
            created_at: row.get::<_, Option<String>>(3).unwrap_or_default(),
            account_username: row.get(4),
            account_domain: row.get(5),
        })
    }

    pub fn get_dmarc_inbox_by_domain(&self, domain: &str) -> Option<DmarcInbox> {
        debug!("[db] getting dmarc inbox for domain={}", domain);
        let mut conn = self.conn();
        conn.query_opt(
            "SELECT di.id, di.account_id, di.label, di.created_at, a.username, d.domain
             FROM dmarc_inboxes di
             JOIN accounts a ON di.account_id = a.id
             JOIN domains d ON a.domain_id = d.id
             WHERE d.domain = $1
             LIMIT 1",
            &[&domain],
        )
        .ok()
        .flatten()
        .map(|row| DmarcInbox {
            id: row.get(0),
            account_id: row.get(1),
            label: row.get::<_, Option<String>>(2).unwrap_or_default(),
            created_at: row.get::<_, Option<String>>(3).unwrap_or_default(),
            account_username: row.get(4),
            account_domain: row.get(5),
        })
    }

    pub fn create_dmarc_inbox(&self, account_id: i64, label: &str) -> Result<i64, String> {
        info!("[db] creating dmarc inbox account_id={}", account_id);
        let mut conn = self.conn();
        let ts = now();
        conn.query_one(
            "INSERT INTO dmarc_inboxes (account_id, label, created_at)
             VALUES ($1, $2, $3) RETURNING id",
            &[&account_id, &label, &ts],
        )
        .map(|row| row.get(0))
        .map_err(|e| {
            error!("[db] failed to create dmarc inbox: {}", e);
            e.to_string()
        })
    }

    pub fn delete_dmarc_inbox(&self, id: i64) {
        warn!("[db] deleting dmarc inbox id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM dmarc_inboxes WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    // ── Outbound Relay methods ──

    pub fn list_outbound_relays(&self) -> Vec<OutboundRelay> {
        debug!("[db] listing outbound relays");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT id, name, host, port, auth_type, username, password, active
                 FROM outbound_relays ORDER BY name",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list outbound relays: {}", e);
                Vec::new()
            });
        rows.into_iter()
            .map(|row| OutboundRelay {
                id: row.get(0),
                name: row.get(1),
                host: row.get(2),
                port: row.get(3),
                auth_type: row.get(4),
                username: row.get(5),
                password: row.get(6),
                active: row.get(7),
            })
            .collect()
    }

    pub fn get_outbound_relay(&self, id: i64) -> Option<OutboundRelay> {
        debug!("[db] getting outbound relay id={}", id);
        let mut conn = self.conn();
        conn.query_opt(
            "SELECT id, name, host, port, auth_type, username, password, active
             FROM outbound_relays WHERE id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| OutboundRelay {
            id: row.get(0),
            name: row.get(1),
            host: row.get(2),
            port: row.get(3),
            auth_type: row.get(4),
            username: row.get(5),
            password: row.get(6),
            active: row.get(7),
        })
    }

    pub fn create_outbound_relay(
        &self,
        name: &str,
        host: &str,
        port: i32,
        auth_type: &str,
        username: Option<&str>,
        password: Option<&str>,
    ) -> Result<i64, String> {
        info!("[db] creating outbound relay name={} host={}:{}", name, host, port);
        let mut conn = self.conn();
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO outbound_relays (name, host, port, auth_type, username, password, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
                 RETURNING id",
                &[&name, &host, &port, &auth_type, &username, &password, &ts, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to create outbound relay {}: {}", name, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!("[db] outbound relay created: {} (id={})", name, id);
        Ok(id)
    }

    pub fn update_outbound_relay(
        &self,
        id: i64,
        name: &str,
        host: &str,
        port: i32,
        auth_type: &str,
        username: Option<&str>,
        password: Option<&str>,
        active: bool,
    ) {
        info!("[db] updating outbound relay id={} name={} host={}:{} active={}", id, name, host, port, active);
        let mut conn = self.conn();
        if let Err(e) = conn.execute(
            "UPDATE outbound_relays
             SET name = $1, host = $2, port = $3, auth_type = $4, username = $5, password = $6, active = $7, updated_at = $8
             WHERE id = $9",
            &[&name, &host, &port, &auth_type, &username, &password, &active, &now(), &id],
        ) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn delete_outbound_relay(&self, id: i64) {
        warn!("[db] deleting outbound relay id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM outbound_relays WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    pub fn list_relay_assignments(&self, relay_id: i64) -> Vec<OutboundRelayAssignment> {
        debug!("[db] listing assignments for relay id={}", relay_id);
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT a.id, a.relay_id, a.assignment_type, a.pattern, r.name
                 FROM outbound_relay_assignments a
                 JOIN outbound_relays r ON a.relay_id = r.id
                 WHERE a.relay_id = $1
                 ORDER BY a.assignment_type, a.pattern",
                &[&relay_id],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list relay assignments: {}", e);
                Vec::new()
            });
        rows.into_iter()
            .map(|row| OutboundRelayAssignment {
                id: row.get(0),
                relay_id: row.get(1),
                assignment_type: row.get(2),
                pattern: row.get(3),
                relay_name: row.get(4),
            })
            .collect()
    }

    pub fn list_all_relay_assignments(&self) -> Vec<OutboundRelayAssignment> {
        debug!("[db] listing all relay assignments");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT a.id, a.relay_id, a.assignment_type, a.pattern, r.name
                 FROM outbound_relay_assignments a
                 JOIN outbound_relays r ON a.relay_id = r.id
                 ORDER BY a.assignment_type, a.pattern",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list all relay assignments: {}", e);
                Vec::new()
            });
        rows.into_iter()
            .map(|row| OutboundRelayAssignment {
                id: row.get(0),
                relay_id: row.get(1),
                assignment_type: row.get(2),
                pattern: row.get(3),
                relay_name: row.get(4),
            })
            .collect()
    }

    pub fn create_relay_assignment(
        &self,
        relay_id: i64,
        assignment_type: &str,
        pattern: &str,
    ) -> Result<i64, String> {
        info!("[db] creating relay assignment relay_id={} type={} pattern={}", relay_id, assignment_type, pattern);
        let mut conn = self.conn();
        let row = conn
            .query_one(
                "INSERT INTO outbound_relay_assignments (relay_id, assignment_type, pattern, created_at)
                 VALUES ($1, $2, $3, $4)
                 RETURNING id",
                &[&relay_id, &assignment_type, &pattern, &now()],
            )
            .map_err(|e| {
                error!("[db] failed to create relay assignment: {}", e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!("[db] relay assignment created id={}", id);
        Ok(id)
    }

    pub fn delete_relay_assignment(&self, id: i64) {
        warn!("[db] deleting relay assignment id={}", id);
        let mut conn = self.conn();
        if let Err(e) = conn.execute("DELETE FROM outbound_relay_assignments WHERE id = $1", &[&id]) {
            error!("[db] failed to execute query: {}", e);
        }
    }

    /// Returns all active relay assignments joined with relay info for config generation.
    pub fn get_active_relay_assignments_with_relay(&self) -> Vec<(OutboundRelay, OutboundRelayAssignment)> {
        debug!("[db] getting active relay assignments with relay info");
        let mut conn = self.conn();
        let rows = conn
            .query(
                "SELECT r.id, r.name, r.host, r.port, r.auth_type, r.username, r.password, r.active,
                        a.id, a.relay_id, a.assignment_type, a.pattern
                 FROM outbound_relay_assignments a
                 JOIN outbound_relays r ON a.relay_id = r.id
                 WHERE r.active = TRUE
                 ORDER BY a.assignment_type, a.pattern",
                &[],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to get active relay assignments: {}", e);
                Vec::new()
            });
        rows.into_iter()
            .map(|row| {
                let relay = OutboundRelay {
                    id: row.get(0),
                    name: row.get(1),
                    host: row.get(2),
                    port: row.get(3),
                    auth_type: row.get(4),
                    username: row.get(5),
                    password: row.get(6),
                    active: row.get(7),
                };
                let assignment = OutboundRelayAssignment {
                    id: row.get(8),
                    relay_id: row.get(9),
                    assignment_type: row.get(10),
                    pattern: row.get(11),
                    relay_name: Some(relay.name.clone()),
                };
                (relay, assignment)
            })
            .collect()
    }
}
