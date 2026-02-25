CREATE TABLE IF NOT EXISTS tracking_patterns (
    id BIGSERIAL PRIMARY KEY,
    pattern TEXT NOT NULL UNIQUE,
    created_at TEXT
);
