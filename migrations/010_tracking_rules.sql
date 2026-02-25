CREATE TABLE IF NOT EXISTS tracking_rules (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    match_mode TEXT NOT NULL DEFAULT 'AND',
    conditions_json TEXT NOT NULL DEFAULT '[]',
    created_at TEXT
);
