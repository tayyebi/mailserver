CREATE TABLE IF NOT EXISTS footer_patterns (
    id BIGSERIAL PRIMARY KEY,
    pattern TEXT NOT NULL UNIQUE,
    created_at TEXT
);
CREATE TABLE IF NOT EXISTS footer_rules (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    match_mode TEXT NOT NULL DEFAULT 'AND',
    conditions_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT
);
