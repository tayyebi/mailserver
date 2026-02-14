CREATE TABLE IF NOT EXISTS admins (
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
);
