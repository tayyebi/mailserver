#![allow(dead_code)]

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
        let conn = Connection::open(path).expect("Failed to open database");
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;")
            .expect("Failed to set pragmas");

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

        Database {
            conn: Arc::new(Mutex::new(conn)),
        }
    }

    // ── Admin methods ──

    pub fn get_admin_by_username(&self, username: &str) -> Option<Admin> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
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
        .ok()
    }

    pub fn update_admin_password(&self, id: i64, hash: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE admins SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![hash, now(), id],
        )
        .ok();
    }

    pub fn update_admin_totp(&self, id: i64, secret: Option<&str>, enabled: bool) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE admins SET totp_secret = ?1, totp_enabled = ?2, updated_at = ?3 WHERE id = ?4",
            params![secret, enabled as i64, now(), id],
        )
        .ok();
    }

    pub fn seed_admin(&self, username: &str, password_hash: &str) {
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key FROM domains ORDER BY domain")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Domain {
                id: row.get(0)?,
                domain: row.get(1)?,
                active: row.get::<_, i64>(2)? != 0,
                dkim_selector: row.get(3)?,
                dkim_private_key: row.get(4)?,
                dkim_public_key: row.get(5)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_domain(&self, id: i64) -> Option<Domain> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, domain, active, dkim_selector, dkim_private_key, dkim_public_key FROM domains WHERE id = ?1",
            params![id],
            |row| {
                Ok(Domain {
                    id: row.get(0)?,
                    domain: row.get(1)?,
                    active: row.get::<_, i64>(2)? != 0,
                    dkim_selector: row.get(3)?,
                    dkim_private_key: row.get(4)?,
                    dkim_public_key: row.get(5)?,
                })
            },
        )
        .ok()
    }

    pub fn create_domain(&self, domain: &str) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT INTO domains (domain, created_at, updated_at) VALUES (?1, ?2, ?3)",
            params![domain, ts, ts],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_domain(&self, id: i64, domain: &str, active: bool) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE domains SET domain = ?1, active = ?2, updated_at = ?3 WHERE id = ?4",
            params![domain, active as i64, now(), id],
        )
        .ok();
    }

    pub fn delete_domain(&self, id: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM domains WHERE id = ?1", params![id])
            .ok();
    }

    pub fn update_domain_dkim(&self, id: i64, selector: &str, private_key: &str, public_key: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE domains SET dkim_selector = ?1, dkim_private_key = ?2, dkim_public_key = ?3, updated_at = ?4 WHERE id = ?5",
            params![selector, private_key, public_key, now(), id],
        )
        .ok();
    }

    // ── Account methods ──

    pub fn list_accounts(&self) -> Vec<Account> {
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
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT INTO accounts (domain_id, username, password_hash, name, quota, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![domain_id, username, password_hash, name, quota, ts, ts],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_account(&self, id: i64, name: &str, active: bool, quota: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET name = ?1, active = ?2, quota = ?3, updated_at = ?4 WHERE id = ?5",
            params![name, active as i64, quota, now(), id],
        )
        .ok();
    }

    pub fn update_account_password(&self, id: i64, hash: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE accounts SET password_hash = ?1, updated_at = ?2 WHERE id = ?3",
            params![hash, now(), id],
        )
        .ok();
    }

    pub fn delete_account(&self, id: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM accounts WHERE id = ?1", params![id])
            .ok();
    }

    pub fn list_all_accounts_with_domain(&self) -> Vec<Account> {
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
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare("SELECT id, domain_id, source, destination, active, tracking_enabled FROM aliases ORDER BY source")
            .unwrap();
        stmt.query_map([], |row| {
            Ok(Alias {
                id: row.get(0)?,
                domain_id: row.get(1)?,
                source: row.get(2)?,
                destination: row.get(3)?,
                active: row.get::<_, i64>(4)? != 0,
                tracking_enabled: row.get::<_, i64>(5)? != 0,
                domain_name: None,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn get_alias(&self, id: i64) -> Option<Alias> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT id, domain_id, source, destination, active, tracking_enabled FROM aliases WHERE id = ?1",
            params![id],
            |row| {
                Ok(Alias {
                    id: row.get(0)?,
                    domain_id: row.get(1)?,
                    source: row.get(2)?,
                    destination: row.get(3)?,
                    active: row.get::<_, i64>(4)? != 0,
                    tracking_enabled: row.get::<_, i64>(5)? != 0,
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
    ) -> Result<i64, String> {
        let conn = self.conn.lock().unwrap();
        let ts = now();
        conn.execute(
            "INSERT INTO aliases (domain_id, source, destination, tracking_enabled, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![domain_id, source, destination, tracking as i64, ts, ts],
        )
        .map_err(|e| e.to_string())?;
        Ok(conn.last_insert_rowid())
    }

    pub fn update_alias(&self, id: i64, source: &str, destination: &str, active: bool, tracking: bool) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "UPDATE aliases SET source = ?1, destination = ?2, active = ?3, tracking_enabled = ?4, updated_at = ?5 WHERE id = ?6",
            params![source, destination, active as i64, tracking as i64, now(), id],
        )
        .ok();
    }

    pub fn delete_alias(&self, id: i64) {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM aliases WHERE id = ?1", params![id])
            .ok();
    }

    pub fn list_all_aliases_with_domain(&self) -> Vec<Alias> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn
            .prepare(
                "SELECT a.id, a.domain_id, a.source, a.destination, a.active, a.tracking_enabled, d.domain \
                 FROM aliases a LEFT JOIN domains d ON a.domain_id = d.id ORDER BY a.source",
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
                domain_name: row.get(6)?,
            })
        })
        .unwrap()
        .filter_map(|r| r.ok())
        .collect()
    }

    pub fn is_tracking_enabled_for_sender(&self, sender: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT COUNT(*) FROM aliases WHERE source = ?1 AND active = 1 AND tracking_enabled = 1",
            params![sender],
            |row| row.get::<_, i64>(0),
        )
        .unwrap_or(0)
            > 0
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
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO tracked_messages (message_id, sender, recipient, subject, alias_id, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![message_id, sender, recipient, subject, alias_id, now()],
        )
        .ok();
    }

    pub fn record_pixel_open(&self, message_id: &str, client_ip: &str, user_agent: &str) {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "INSERT INTO pixel_opens (message_id, client_ip, user_agent, opened_at) VALUES (?1, ?2, ?3, ?4)",
            params![message_id, client_ip, user_agent, now()],
        )
        .ok();
    }

    pub fn list_tracked_messages(&self, limit: i64) -> Vec<TrackedMessage> {
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
