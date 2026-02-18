CREATE TABLE IF NOT EXISTS fail2ban_settings (
    id BIGSERIAL PRIMARY KEY,
    service TEXT UNIQUE NOT NULL,
    max_attempts INTEGER NOT NULL DEFAULT 5,
    ban_duration_minutes INTEGER NOT NULL DEFAULT 60,
    find_time_minutes INTEGER NOT NULL DEFAULT 10,
    enabled BOOLEAN DEFAULT TRUE,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE IF NOT EXISTS fail2ban_banned (
    id BIGSERIAL PRIMARY KEY,
    ip_address TEXT NOT NULL,
    service TEXT NOT NULL DEFAULT 'all',
    reason TEXT DEFAULT '',
    attempts INTEGER DEFAULT 0,
    banned_at TEXT NOT NULL,
    expires_at TEXT,
    permanent BOOLEAN DEFAULT FALSE,
    created_at TEXT
);

CREATE TABLE IF NOT EXISTS fail2ban_whitelist (
    id BIGSERIAL PRIMARY KEY,
    ip_address TEXT NOT NULL,
    description TEXT DEFAULT '',
    created_at TEXT
);

CREATE TABLE IF NOT EXISTS fail2ban_blacklist (
    id BIGSERIAL PRIMARY KEY,
    ip_address TEXT NOT NULL,
    description TEXT DEFAULT '',
    created_at TEXT
);

CREATE TABLE IF NOT EXISTS fail2ban_log (
    id BIGSERIAL PRIMARY KEY,
    ip_address TEXT NOT NULL,
    service TEXT NOT NULL DEFAULT 'all',
    action TEXT NOT NULL,
    details TEXT DEFAULT '',
    created_at TEXT
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_fail2ban_banned_ip_service ON fail2ban_banned(ip_address, service);
CREATE UNIQUE INDEX IF NOT EXISTS idx_fail2ban_whitelist_ip ON fail2ban_whitelist(ip_address);
CREATE UNIQUE INDEX IF NOT EXISTS idx_fail2ban_blacklist_ip ON fail2ban_blacklist(ip_address);
CREATE INDEX IF NOT EXISTS idx_fail2ban_log_created ON fail2ban_log(created_at);

INSERT INTO fail2ban_settings (service, max_attempts, ban_duration_minutes, find_time_minutes, enabled, created_at, updated_at)
VALUES
    ('smtp', 5, 60, 10, true, NOW()::TEXT, NOW()::TEXT),
    ('imap', 5, 60, 10, true, NOW()::TEXT, NOW()::TEXT),
    ('pop3', 5, 60, 10, true, NOW()::TEXT, NOW()::TEXT),
    ('admin', 3, 120, 5, true, NOW()::TEXT, NOW()::TEXT)
ON CONFLICT (service) DO NOTHING;

-- Global fail2ban toggle: default is off
INSERT INTO settings (key, value) VALUES ('fail2ban_enabled', 'false')
ON CONFLICT (key) DO NOTHING;
