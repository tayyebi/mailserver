#![allow(dead_code)]

use log::{info, warn, error, debug};
use rusqlite::{params, Connection};
use serde::Serialize;
use std::sync::{Arc, Mutex};

fn now() -> String {
    chrono::Utc::now().format("%Y-%m-%d %H:%M:%S").to_string()
}

#[derive(Clone)]
pub struct Database {
    conn: Arc<Mutex<Connection>>,
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
pub struct Stats {
    pub domain_count: i64,
    pub account_count: i64,
    pub alias_count: i64,
    pub tracked_count: i64,
    pub open_count: i64,
}

impl Database {
    pub fn open(path: &str) -> Self {
        info!("[db] opening database at path={}", path);
        let conn = Connection::open(path).expect("Failed to open database");
        debug!("[db] setting pragmas: journal_mode=WAL, foreign_keys=ON");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .expect("Failed to set pragmas");

        debug!("[db] creating tables if not exists");
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS admins (
                id INTEGER PRIMARY KEY,
                username TEXT UNIQUE NOT NULL,
                password_hash TEXT NOT NULL,
                totp_secret TEXT,
                totp_enabled INTEGER DEFAULT 0,
                created_at TEXT,
                updated_at TEXT
            );

            CREATE TABLE IF NOT EXISTS domains (
                id INTEGER PRIMARY KEY,
                domain TEXT UNIQUE NOT NULL,
                active INTEGER DEFAULT 1,
                dkim_selector TEXT DEFAULT 'mail',
                dkim_private_key TEXT,
                dkim_public_key TEXT,
                footer_html TEXT DEFAULT '',
                created_at TEXT,
                updated_at TEXT
            );

            CREATE TABLE IF NOT EXISTS accounts (
                id INTEGER PRIMARY KEY,
                domain_id INTEGER REFERENCES domains(id) ON DELETE CASCADE,
                username TEXT NOT NULL,
                password_hash TEXT NOT NULL,
                name TEXT DEFAULT '',
                active INTEGER DEFAULT 1,
                quota INTEGER DEFAULT 0,
                created_at TEXT,
                updated_at TEXT,
                UNIQUE(username, domain_id)
            );

            CREATE TABLE IF NOT EXISTS aliases (
                id INTEGER PRIMARY KEY,
                domain_id INTEGER REFERENCES domains(id) ON DELETE CASCADE,
                source TEXT NOT NULL,
                destination TEXT NOT NULL,
                active INTEGER DEFAULT 1,
                tracking_enabled INTEGER DEFAULT 0,
                sort_order INTEGER DEFAULT 0,
                created_at TEXT,
                updated_at TEXT
            );

            CREATE TABLE IF NOT EXISTS tracked_messages (
                id INTEGER PRIMARY KEY,
                message_id TEXT UNIQUE NOT NULL,
                sender TEXT,
                recipient TEXT,
                subject TEXT,
                alias_id INTEGER,
                created_at TEXT
            );

            CREATE TABLE IF NOT EXISTS pixel_opens (
                id INTEGER PRIMARY KEY,
                message_id TEXT NOT NULL,
                client_ip TEXT,
                user_agent TEXT,
                opened_at TEXT
            );",
        )
        .expect("Failed to create tables");

        // Backfill columns for pre-existing databases (ignore errors if they already exist)
        let _ = conn.execute("ALTER TABLE domains ADD COLUMN footer_html TEXT DEFAULT ''", params![]);
        let _ = conn.execute("ALTER TABLE aliases ADD COLUMN sort_order INTEGER DEFAULT 0", params![]);

        info!("[db] database opened and schema initialized successfully");
        Database {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    // ── Admin methods ──

    pub fn get_admin_by_username(&self, username: &str) -> Option<Admin> {
        debug!("[db] looking up admin username={}", username);
        let conn = self.conn.lock().unwrap();
        let result = conn.query_row(
            "SELECT id, username, password_hash, totp_secret, totp_enabled FROM admins WHERE username = ?1",
            params![username],
            |row| {
                Ok(Admin {
                    id: row.get(0)?,
                    username: row.get(1)?,
                    password_hash: row.get(2)?,
                    totp_secret: row.get(3)?,
                    totp_enabled: row.get::<_, i64>(4)? != 0,
                })
            },
        )
        .ok();
        if result.is_some() {
            debug!("[db] admin found: username={}", username);
        } else {
            warn!("[db] admin not found: username={}", username);
        }
        result
    }

    pub fn update_admin_password(&self, id: i64, hash: &str) {
        info!("[db] updating admin password id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE admins SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![hash, now(), id],
        )
        .ok();
    }

    pub fn update_admin_totp(&self, id: i64, secret: Option<&str>, enabled: bool) {
        info!("[db] updating admin TOTP id={}, enabled={}", id, enabled);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE admins SET totp_secret = ?1, totp_enabled = ?2, updated_at = ?3 WHERE id = ?4",
            params![secret, enabled as i64, now(), id],
        )
        .ok();
    }

    pub fn seed_admin(&self, username: &str, password_hash: &str) {
        info!("[db] seeding admin user: {}", username);
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT OR IGNORE INTO admins (username, password_hash, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![username, password_hash, ts, ts],
        )
        .ok();
    }

    // ── Domain methods ──

    pub fn list_domains(&self) -> Vec<Domain> {
        debug!("[db] listing all domains");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html FROM domains ORDER BY domain",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Domain {
                id: row.get(0)?,
                domain: row.get(1)?,
                active: row.get::<_, i64>(2)? != 0,
                dkim_selector: row.get(3)?,
                dkim_private_key: row.get(4)?,
                dkim_public_key: row.get(5)?,
                footer_html: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_domain(&self, id: i64) -> Option<Domain> {
        debug!("[db] getting domain id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key, footer_html FROM domains WHERE id = ?1",
            params![id],
            |row| {
                Ok(Domain {
                    id: row.get(0)?,
                    domain: row.get(1)?,
                    active: row.get::<_, i64>(2)? != 0,
                    dkim_selector: row.get(3)?,
                    dkim_private_key: row.get(4)?,
                    dkim_public_key: row.get(5)?,
                    footer_html: row.get(6)?,
                })
            },
        )
        .ok()
    }

    pub fn create_domain(&self, domain: &str, footer_html: &str) -> Result<i64, String> {
        info!("[db] creating domain: {}", domain);
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT INTO domains (domain, footer_html, created_at, updated_at) VALUES (?1, ?2, ?3, ?4)",
            params![domain, footer_html, ts, ts],
        )
        .map_err(|e| {
            error!("[db] failed to create domain {}: {}", domain, e);
            e.to_string()
        })?;
        let id = conn.last_insert_rowid();
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE domains SET domain = ?1, active = ?2, footer_html = ?3, updated_at = ?4 WHERE id = ?5",
            params![domain, active as i64, footer_html, now(), id],
        )
        .ok();
    }

    pub fn delete_domain(&self, id: i64) {
        warn!("[db] deleting domain id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM domains WHERE id = ?1", params![id])
            .ok();
    }

    pub fn update_domain_dkim(&self, id: i64, selector: &str, private_key: &str, public_key: &str) {
        info!("[db] updating DKIM for domain id={}, selector={}", id, selector);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE domains SET dkim_selector = ?1, dkim_private_key = ?2, dkim_public_key = ?3, updated_at = ?4 WHERE id = ?5",
            params![selector, private_key, public_key, now(), id],
        )
        .ok();
    }

    pub fn get_footer_for_sender(&self, sender: &str) -> Option<String> {
        let domain_part = sender.split('@').nth(1)?.trim().to_lowercase();
        if domain_part.is_empty() {
            return None;
        }
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT footer_html FROM domains WHERE lower(domain) = ?1 AND footer_html IS NOT NULL AND footer_html != ''",
            params![domain_part],
            |row| row.get(0),
        )
        .ok()
    }

    // ── Account methods ──

    pub fn list_accounts(&self) -> Vec<Account> {
        debug!("[db] listing all accounts");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, domain_id, username, password_hash, name, active, quota FROM accounts ORDER BY username")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Account {
                id: row.get(0)?,
                domain_id: row.get(1)?,
                username: row.get(2)?,
                password_hash: row.get(3)?,
                name: row.get(4)?,
                active: row.get::<_, i64>(5)? != 0,
                quota: row.get(6)?,
                domain_name: None,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_account(&self, id: i64) -> Option<Account> {
        debug!("[db] getting account id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, domain_id, username, password_hash, name, active, quota FROM accounts WHERE id = ?1",
            params![id],
            |row| {
                Ok(Account {
                    id: row.get(0)?,
                    domain_id: row.get(1)?,
                    username: row.get(2)?,
                    password_hash: row.get(3)?,
                    name: row.get(4)?,
                    active: row.get::<_, i64>(5)? != 0,
                    quota: row.get(6)?,
                    domain_name: None,
                })
            },
        )
        .ok()
    }

    pub fn create_account(
        &self,
        domain_id: i64,
        username: &str,
        password_hash: &str,
        name: &str,
        quota: i64,
    ) -> Result<i64, String> {
        info!("[db] creating account username={}, domain_id={}, quota={}", username, domain_id, quota);
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT INTO accounts (domain_id, username, password_hash, name, quota, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![domain_id, username, password_hash, name, quota, ts, ts],
        )
        .map_err(|e| {
            error!("[db] failed to create account {}: {}", username, e);
            e.to_string()
        })?;
        let id = conn.last_insert_rowid();
        info!("[db] account created: {} (id={})", username, id);
        Ok(id)
    }

    pub fn update_account(&self, id: i64, name: &str, active: bool, quota: i64) {
        info!("[db] updating account id={}, active={}, quota={}", id, active, quota);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET name = ?1, active = ?2, quota = ?3, updated_at = ?4 WHERE id = ?5",
            params![name, active as i64, quota, now(), id],
        )
        .ok();
    }

    pub fn update_account_password(&self, id: i64, hash: &str) {
        info!("[db] updating account password id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![hash, now(), id],
        )
        .ok();
    }

    pub fn delete_account(&self, id: i64) {
        warn!("[db] deleting account id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])
            .ok();
    }

    pub fn list_all_accounts_with_domain(&self) -> Vec<Account> {
        debug!("[db] listing all accounts with domain info");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.domain_id, a.username, a.password_hash, a.name, a.active, a.quota, d.domain \
                 FROM accounts a LEFT JOIN domains d ON a.domain_id = d.id ORDER BY a.username",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Account {
                id: row.get(0)?,
                domain_id: row.get(1)?,
                username: row.get(2)?,
                password_hash: row.get(3)?,
                name: row.get(4)?,
                active: row.get::<_, i64>(5)? != 0,
                quota: row.get(6)?,
                domain_name: row.get(7)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    // ── Alias methods ──

    pub fn list_aliases(&self) -> Vec<Alias> {
        debug!("[db] listing all aliases");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, domain_id, source, destination, active, tracking_enabled, sort_order FROM aliases ORDER BY sort_order ASC, id ASC")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Alias {
                id: row.get(0)?,
                domain_id: row.get(1)?,
                source: row.get(2)?,
                destination: row.get(3)?,
                active: row.get::<_, i64>(4)? != 0,
                tracking_enabled: row.get::<_, i64>(5)? != 0,
                sort_order: row.get(6)?,
                domain_name: None,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_alias(&self, id: i64) -> Option<Alias> {
        debug!("[db] getting alias id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, domain_id, source, destination, active, tracking_enabled, sort_order FROM aliases WHERE id = ?1",
            params![id],
            |row| {
                Ok(Alias {
                    id: row.get(0)?,
                    domain_id: row.get(1)?,
                    source: row.get(2)?,
                    destination: row.get(3)?,
                    active: row.get::<_, i64>(4)? != 0,
                    tracking_enabled: row.get::<_, i64>(5)? != 0,
                    sort_order: row.get(6)?,
                    domain_name: None,
                })
            },
        )
        .ok()
    }

    pub fn create_alias(
        &self,
        domain_id: i64,
        source: &str,
        destination: &str,
        tracking: bool,
        sort_order: i64,
    ) -> Result<i64, String> {
        info!("[db] creating alias source={}, destination={}, tracking={}, sort_order={}", source, destination, tracking, sort_order);
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT INTO aliases (domain_id, source, destination, tracking_enabled, sort_order, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![domain_id, source, destination, tracking as i64, sort_order, ts, ts],
        )
        .map_err(|e| {
            error!("[db] failed to create alias {} -> {}: {}", source, destination, e);
            e.to_string()
        })?;
        let id = conn.last_insert_rowid();
        info!("[db] alias created: {} -> {} (id={})", source, destination, id);
        Ok(id)
    }

    pub fn update_alias(&self, id: i64, source: &str, destination: &str, active: bool, tracking: bool, sort_order: i64) {
        info!("[db] updating alias id={}, source={}, destination={}, active={}, tracking={}, sort_order={}", id, source, destination, active, tracking, sort_order);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE aliases SET source = ?1, destination = ?2, active = ?3, tracking_enabled = ?4, sort_order = ?5, updated_at = ?6 WHERE id = ?7",
            params![source, destination, active as i64, tracking as i64, sort_order, now(), id],
        )
        .ok();
    }

    pub fn delete_alias(&self, id: i64) {
        warn!("[db] deleting alias id={}", id);
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM aliases WHERE id = ?1", params![id])
            .ok();
    }

    pub fn list_all_aliases_with_domain(&self) -> Vec<Alias> {
        debug!("[db] listing all aliases with domain info");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.domain_id, a.source, a.destination, a.active, a.tracking_enabled, a.sort_order, d.domain \
                 FROM aliases a LEFT JOIN domains d ON a.domain_id = d.id ORDER BY a.sort_order ASC, a.id ASC",
            )
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Alias {
                id: row.get(0)?,
                domain_id: row.get(1)?,
                source: row.get(2)?,
                destination: row.get(3)?,
                active: row.get::<_, i64>(4)? != 0,
                tracking_enabled: row.get::<_, i64>(5)? != 0,
                sort_order: row.get(6)?,
                domain_name: row.get(7)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn is_tracking_enabled_for_sender(&self, sender: &str) -> bool {
        debug!("[db] checking tracking status for sender={}", sender);
        let conn = self.conn.lock().unwrap();
        let enabled = conn.query_row(
            "SELECT COUNT(*) FROM aliases WHERE source = ?1 AND active = 1 AND tracking_enabled = 1",
            params![sender],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
            > 0;
        debug!("[db] tracking enabled for sender={}: {}", sender, enabled);
        enabled
    }

    /// Returns a list of (alias_source, account_email) for building sender_login_maps.
    /// An account owns an alias if they share the same domain_id.
    pub fn get_sender_login_map(&self) -> Vec<(String, String)> {
        debug!("[db] building sender login map");
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT al.source, (ac.username || '@' || d.domain) AS account_email \
             FROM aliases al \
             JOIN domains d ON al.domain_id = d.id \
             JOIN accounts ac ON ac.domain_id = al.domain_id \
             WHERE al.active = 1 AND ac.active = 1 \
             ORDER BY al.source, account_email"
        ).unwrap();
        stmt.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .unwrap()
        .filter_map(|r| r.ok())
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
        info!("[db] creating tracked message id={}, sender={}, recipient={}", message_id, sender, recipient);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tracked_messages (message_id, sender, recipient, subject, alias_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![message_id, sender, recipient, subject, alias_id, now()],
        )
        .ok();
    }

    pub fn record_pixel_open(&self, message_id: &str, client_ip: &str, user_agent: &str) {
        info!("[db] recording pixel open message_id={}, client_ip={}", message_id, client_ip);
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pixel_opens (message_id, client_ip, user_agent, opened_at) VALUES (?1, ?2, ?3, ?4)",
            params![message_id, client_ip, user_agent, now()],
        )
        .ok();
    }

    pub fn list_tracked_messages(&self, limit: i64) -> Vec<TrackedMessage> {
        debug!("[db] listing tracked messages limit={}", limit);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, message_id, sender, recipient, subject, alias_id, created_at FROM tracked_messages ORDER BY created_at DESC LIMIT ?1")
            .unwrap();
        stmt.query_map(params![limit], |row| {
            Ok(TrackedMessage {
                id: row.get(0)?,
                message_id: row.get(1)?,
                sender: row.get(2)?,
                recipient: row.get(3)?,
                subject: row.get(4)?,
                alias_id: row.get(5)?,
                created_at: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_tracked_message(&self, message_id: &str) -> Option<TrackedMessage> {
        debug!("[db] getting tracked message id={}", message_id);
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, message_id, sender, recipient, subject, alias_id, created_at FROM tracked_messages WHERE message_id = ?1",
            params![message_id],
            |row| {
                Ok(TrackedMessage {
                    id: row.get(0)?,
                    message_id: row.get(1)?,
                    sender: row.get(2)?,
                    recipient: row.get(3)?,
                    subject: row.get(4)?,
                    alias_id: row.get(5)?,
                    created_at: row.get(6)?,
                })
            },
        )
        .ok()
    }

    pub fn get_opens_for_message(&self, message_id: &str) -> Vec<PixelOpen> {
        debug!("[db] getting opens for message id={}", message_id);
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, message_id, client_ip, user_agent, opened_at FROM pixel_opens WHERE message_id = ?1 ORDER BY opened_at DESC")
            .unwrap();
        stmt.query_map(params![message_id], |row| {
            Ok(PixelOpen {
                id: row.get(0)?,
                message_id: row.get(1)?,
                client_ip: row.get(2)?,
                user_agent: row.get(3)?,
                opened_at: row.get(4)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_stats(&self) -> Stats {
        debug!("[db] fetching aggregate stats");
        let conn = self.conn.lock().unwrap();
        let count = |sql: &str| -> i64 {
            conn.query_row(sql, [], |row| row.get(0)).unwrap_or(0)
        };
        Stats {
            domain_count: count("SELECT COUNT(*) FROM domains"),
            account_count: count("SELECT COUNT(*) FROM accounts"),
            alias_count: count("SELECT COUNT(*) FROM aliases"),
            tracked_count: count("SELECT COUNT(*) FROM tracked_messages"),
            open_count: count("SELECT COUNT(*) FROM pixel_opens"),
        }
    }
}
