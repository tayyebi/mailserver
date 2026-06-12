CREATE TABLE IF NOT EXISTS jmap_tokens (
    id BIGSERIAL PRIMARY KEY,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    token TEXT NOT NULL UNIQUE,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_jmap_tokens_token ON jmap_tokens(token);

CREATE TABLE IF NOT EXISTS jmap_state (
    account_id BIGINT PRIMARY KEY REFERENCES accounts(id) ON DELETE CASCADE,
    state TEXT NOT NULL DEFAULT '1',
    snapshot TEXT NOT NULL DEFAULT '{}'
);
