CREATE TABLE IF NOT EXISTS abuse_inboxes (
    id BIGSERIAL PRIMARY KEY,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    label TEXT NOT NULL DEFAULT '',
    created_at TEXT,
    UNIQUE(account_id)
);
