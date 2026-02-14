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
pub struct EmailLog {
    pub id: i64,
    pub message_id: String,
    pub sender: String,
    pub recipient: String,
    pub subject: String,
    pub direction: String, // "incoming" or "outgoing"
    pub raw_message: String,
    pub logged_at: String,
}

#[derive(Clone, Serialize)]
pub struct ConnectionLog {
    pub id: i64,
    pub log_type: String, // "login", "imap", "pop3", "smtp", etc.
    pub username: String,
    pub client_ip: String,
    pub status: String, // "success", "failure"
    pub details: Option<String>,
    pub logged_at: String,
}

#[derive(Clone, Serialize)]
pub struct Stats {
    pub domain_count: i64,
    pub account_count: i64,
    pub alias_count: i64,
    pub tracked_count: i64,
    pub open_count: i64,
}

impl Database {
    pub fn open(url: &str) -> Self {
        info!("[db] opening PostgreSQL database at url={}", url);
        let mut client = Client::connect(url, NoTls).expect("Failed to connect to PostgreSQL");

        debug!("[db] creating tables if not exists");
        client
            .batch_execute(
                "CREATE TABLE IF NOT EXISTS admins (
                    id BIGSERIAL PRIMARY KEY,
                    username TEXT UNIQUE NOT NULL,
                    password_hash TEXT NOT NULL,
                    totp_secret TEXT,
                    totp_enabled BOOLEAN DEFAULT FALSE,
                    created_at TEXT,
                    updated_at TEXT
                );

                CREATE TABLE IF NOT EXISTS domains (
                    id BIGSERIAL PRIMARY KEY,
                    domain TEXT UNIQUE NOT NULL,
                    active BOOLEAN DEFAULT TRUE,
                    dkim_selector TEXT DEFAULT 'mail',
                    dkim_private_key TEXT,
                    dkim_public_key TEXT,
                    footer_html TEXT DEFAULT '',
                    created_at TEXT,
                    updated_at TEXT
                );

                CREATE TABLE IF NOT EXISTS accounts (
                    id BIGSERIAL PRIMARY KEY,
                    domain_id BIGINT REFERENCES domains(id) ON DELETE CASCADE,
                    username TEXT NOT NULL,
                    password_hash TEXT NOT NULL,
                    name TEXT DEFAULT '',
                    active BOOLEAN DEFAULT TRUE,
                    quota BIGINT DEFAULT 0,
                    created_at TEXT,
                    updated_at TEXT,
                    UNIQUE(username, domain_id)
                );

                CREATE TABLE IF NOT EXISTS aliases (
                    id BIGSERIAL PRIMARY KEY,
                    domain_id BIGINT REFERENCES domains(id) ON DELETE CASCADE,
                    source TEXT NOT NULL,
                    destination TEXT NOT NULL,
                    active BOOLEAN DEFAULT TRUE,
                    tracking_enabled BOOLEAN DEFAULT FALSE,
                    sort_order BIGINT DEFAULT 0,
                    created_at TEXT,
                    updated_at TEXT
                );

                CREATE TABLE IF NOT EXISTS tracked_messages (
                    id BIGSERIAL PRIMARY KEY,
                    message_id TEXT UNIQUE NOT NULL,
                    sender TEXT,
                    recipient TEXT,
                    subject TEXT,
                    alias_id BIGINT,
                    created_at TEXT
                );

                CREATE TABLE IF NOT EXISTS pixel_opens (
                    id BIGSERIAL PRIMARY KEY,
                    message_id TEXT NOT NULL,
                    client_ip TEXT,
                    user_agent TEXT,
                    opened_at TEXT
                );

                CREATE TABLE IF NOT EXISTS email_logs (
                    id BIGSERIAL PRIMARY KEY,
                    message_id TEXT UNIQUE NOT NULL,
                    sender TEXT NOT NULL,
                    recipient TEXT NOT NULL,
                    subject TEXT,
                    direction TEXT DEFAULT 'incoming',
                    raw_message TEXT,
                    logged_at TEXT NOT NULL,
                    created_at TEXT
                );

                CREATE TABLE IF NOT EXISTS connection_logs (
                    id BIGSERIAL PRIMARY KEY,
                    log_type TEXT NOT NULL,
                    username TEXT,
                    client_ip TEXT,
                    status TEXT DEFAULT 'success',
                    details TEXT,
                    logged_at TEXT NOT NULL,
                    created_at TEXT
                );

                CREATE TABLE IF NOT EXISTS settings (
                    key TEXT PRIMARY KEY,
                    value TEXT
                );",
            )
            .expect("Failed to create tables");

        info!("[db] PostgreSQL database opened and schema initialized successfully");
        Database {
            conn: Arc::new(Mutex::new(client)),
        }
    }

    // ── Admin methods ──

    pub fn get_admin_by_username(&self, username: &str) -> Option<Admin> {
        debug!("[db] looking up admin username={}", username);
        let mut conn = self.conn.lock().unwrap();
        let row = conn
            .query_opt(
                "SELECT id, username, password_hash, totp_secret, totp_enabled FROM admins WHERE username = $1",
                &[&username],
            )
            .ok()
            .flatten();

        let result = row.map(|row| Admin {
            id: row.get(0),
            username: row.get(1),
            password_hash: row.get(2),
            totp_secret: row.get(3),
            totp_enabled: row.get(4),
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
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE admins SET password_hash = $1, updated_at = $2 WHERE id = $3",
            &[&hash, &now(), &id],
        );
    }

    pub fn update_admin_totp(&self, id: i64, secret: Option<&str>, enabled: bool) {
        info!("[db] updating admin TOTP id={}, enabled={}", id, enabled);
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE admins SET totp_secret = $1, totp_enabled = $2, updated_at = $3 WHERE id = $4",
            &[&secret, &enabled, &now(), &id],
        );
    }

    pub fn seed_admin(&self, username: &str, password_hash: &str) {
        info!("[db] seeding admin user: {}", username);
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
        let rows = conn
            .query(
                "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html
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
            })
            .collect()
    }

    pub fn get_domain(&self, id: i64) -> Option<Domain> {
        debug!("[db] getting domain id={}", id);
        let mut conn = self.conn.lock().unwrap();
        conn.query_opt(
            "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html
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
        })
    }

    pub fn create_domain(&self, domain: &str, footer_html: &str) -> Result<i64, String> {
        info!("[db] creating domain: {}", domain);
        let mut conn = self.conn.lock().unwrap();
        let ts = now();
        let row = conn
            .query_one(
                "INSERT INTO domains (domain, footer_html, created_at, updated_at)
                 VALUES ($1, $2, $3, $4)
                 RETURNING id",
                &[&domain, &footer_html, &ts, &ts],
            )
            .map_err(|e| {
                error!("[db] failed to create domain {}: {}", domain, e);
                e.to_string()
            })?;
        let id: i64 = row.get(0);
        info!("[db] domain created: {} (id={})", domain, id);
        Ok(id)
    }

    pub fn update_domain(&self, id: i64, domain: &str, active: bool, footer_html: &str) {
        info!(
            "[db] updating domain id={}, domain={}, active={}, footer_present={}",
            id,
            domain,
            active,
            !footer_html.trim().is_empty()
        );
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE domains
             SET domain = $1, active = $2, footer_html = $3, updated_at = $4
             WHERE id = $5",
            &[&domain, &active, &footer_html, &now(), &id],
        );
    }

    pub fn delete_domain(&self, id: i64) {
        warn!("[db] deleting domain id={}", id);
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM domains WHERE id = $1", &[&id]);
    }

    pub fn update_domain_dkim(&self, id: i64, selector: &str, private_key: &str, public_key: &str) {
        info!(
            "[db] updating DKIM for domain id={}, selector={}",
            id, selector
        );
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE domains
             SET dkim_selector = $1, dkim_private_key = $2, dkim_public_key = $3, updated_at = $4
             WHERE id = $5",
            &[&selector, &private_key, &public_key, &now(), &id],
        );
    }

    pub fn get_footer_for_sender(&self, sender: &str) -> Option<String> {
        let domain_part = sender.split('@').nth(1)?.trim().to_lowercase();
        if domain_part.is_empty() {
            return None;
        }
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE accounts
             SET name = $1, active = $2, quota = $3, updated_at = $4
             WHERE id = $5",
            &[&name, &active, &quota, &now(), &id],
        );
    }

    pub fn update_account_password(&self, id: i64, hash: &str) {
        info!("[db] updating account password id={}", id);
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE accounts SET password_hash = $1, updated_at = $2 WHERE id = $3",
            &[&hash, &now(), &id],
        );
    }

    pub fn delete_account(&self, id: i64) {
        warn!("[db] deleting account id={}", id);
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM accounts WHERE id = $1", &[&id]);
    }

    pub fn list_all_accounts_with_domain(&self) -> Vec<Account> {
        debug!("[db] listing all accounts with domain info");
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE aliases
             SET source = $1, destination = $2, active = $3, tracking_enabled = $4, sort_order = $5, updated_at = $6
             WHERE id = $7",
            &[&source, &destination, &active, &tracking, &sort_order, &now(), &id],
        );
    }

    pub fn delete_alias(&self, id: i64) {
        warn!("[db] deleting alias id={}", id);
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute("DELETE FROM aliases WHERE id = $1", &[&id]);
    }

    pub fn list_all_aliases_with_domain(&self) -> Vec<Alias> {
        debug!("[db] listing all aliases with domain info");
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO pixel_opens (message_id, client_ip, user_agent, opened_at)
             VALUES ($1, $2, $3, $4)",
            &[&message_id, &client_ip, &user_agent, &now()],
        );
    }

    pub fn list_tracked_messages(&self, limit: i64) -> Vec<TrackedMessage> {
        debug!("[db] listing tracked messages limit={}", limit);
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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
        let mut conn = self.conn.lock().unwrap();
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

    // ── Email logging ──

    pub fn log_email(
        &self,
        message_id: &str,
        sender: &str,
        recipient: &str,
        subject: &str,
        direction: &str,
        raw_message: &str,
    ) {
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO email_logs (message_id, sender, recipient, subject, direction, raw_message, logged_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
             ON CONFLICT (message_id) DO NOTHING",
            &[&message_id, &sender, &recipient, &subject, &direction, &raw_message, &now(), &now()],
        );
    }

    pub fn list_email_logs(&self, limit: i64, offset: i64) -> Vec<EmailLog> {
        debug!("[db] listing email logs limit={}, offset={}", limit, offset);
        let mut conn = self.conn.lock().unwrap();
        let rows = conn
            .query(
                "SELECT id, message_id, sender, recipient, subject, direction, raw_message, logged_at
                 FROM email_logs
                 ORDER BY logged_at DESC
                 LIMIT $1 OFFSET $2",
                &[&limit, &offset],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list email logs: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| EmailLog {
                id: row.get(0),
                message_id: row.get(1),
                sender: row.get(2),
                recipient: row.get(3),
                subject: row.get(4),
                direction: row.get(5),
                raw_message: row.get(6),
                logged_at: row.get(7),
            })
            .collect()
    }

    pub fn get_email_log(&self, id: i64) -> Option<EmailLog> {
        debug!("[db] getting email log id={}", id);
        let mut conn = self.conn.lock().unwrap();
        conn.query_opt(
            "SELECT id, message_id, sender, recipient, subject, direction, raw_message, logged_at
             FROM email_logs WHERE id = $1",
            &[&id],
        )
        .ok()
        .flatten()
        .map(|row| EmailLog {
            id: row.get(0),
            message_id: row.get(1),
            sender: row.get(2),
            recipient: row.get(3),
            subject: row.get(4),
            direction: row.get(5),
            raw_message: row.get(6),
            logged_at: row.get(7),
        })
    }

    pub fn count_email_logs(&self) -> i64 {
        let mut conn = self.conn.lock().unwrap();
        conn.query_one("SELECT COUNT(*) FROM email_logs", &[])
            .map(|row| row.get(0))
            .unwrap_or(0)
    }

    // ── Connection logging ──

    pub fn log_connection(
        &self,
        log_type: &str,
        username: &str,
        client_ip: &str,
        status: &str,
        details: Option<&str>,
    ) {
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO connection_logs (log_type, username, client_ip, status, details, logged_at, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            &[&log_type, &username, &client_ip, &status, &details, &now(), &now()],
        );
    }

    pub fn list_connection_logs(&self, limit: i64, offset: i64) -> Vec<ConnectionLog> {
        debug!(
            "[db] listing connection logs limit={}, offset={}",
            limit, offset
        );
        let mut conn = self.conn.lock().unwrap();
        let rows = conn
            .query(
                "SELECT id, log_type, username, client_ip, status, details, logged_at
                 FROM connection_logs
                 ORDER BY logged_at DESC
                 LIMIT $1 OFFSET $2",
                &[&limit, &offset],
            )
            .unwrap_or_else(|e| {
                error!("[db] failed to list connection logs: {}", e);
                Vec::new()
            });

        rows
            .into_iter()
            .map(|row| ConnectionLog {
                id: row.get(0),
                log_type: row.get(1),
                username: row.get(2),
                client_ip: row.get(3),
                status: row.get(4),
                details: row.get(5),
                logged_at: row.get(6),
            })
            .collect()
    }

    pub fn count_connection_logs(&self) -> i64 {
        let mut conn = self.conn.lock().unwrap();
        conn.query_one("SELECT COUNT(*) FROM connection_logs", &[])
            .map(|row| row.get(0))
            .unwrap_or(0)
    }

    // ── Generic settings storage (key/value) ──

    pub fn set_setting(&self, key: &str, value: &str) {
        let mut conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO settings (key, value)
             VALUES ($1, $2)
             ON CONFLICT (key) DO UPDATE SET value = EXCLUDED.value",
            &[&key, &value],
        );
    }

    pub fn get_setting(&self, key: &str) -> Option<String> {
        let mut conn = self.conn.lock().unwrap();
        conn.query_opt("SELECT value FROM settings WHERE key = $1", &[&key])
            .ok()
            .flatten()
            .map(|row| row.get(0))
    }

    pub fn get_stats(&self) -> Stats {
        debug!("[db] fetching aggregate stats");
        let mut conn = self.conn.lock().unwrap();

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

        Stats {
            domain_count,
            account_count,
            alias_count,
            tracked_count,
            open_count,
        }
    }
}
