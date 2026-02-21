CREATE TABLE IF NOT EXISTS webhook_logs (
    id BIGSERIAL PRIMARY KEY,
    url TEXT NOT NULL,
    request_body TEXT,
    response_status INTEGER,
    response_body TEXT,
    error TEXT,
    duration_ms BIGINT,
    sender TEXT,
    subject TEXT,
    created_at TEXT NOT NULL
);
