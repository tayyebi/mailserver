CREATE TABLE IF NOT EXISTS webdav_files (
    id BIGSERIAL PRIMARY KEY,
    account_id BIGINT,
    owner TEXT NOT NULL,
    filename TEXT NOT NULL,
    content_type TEXT NOT NULL DEFAULT 'application/octet-stream',
    size BIGINT NOT NULL DEFAULT 0,
    token TEXT UNIQUE NOT NULL,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE(owner, filename)
);
