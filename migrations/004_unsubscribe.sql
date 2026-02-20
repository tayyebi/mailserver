ALTER TABLE domains ADD COLUMN IF NOT EXISTS unsubscribe_enabled BOOLEAN DEFAULT FALSE;

CREATE TABLE IF NOT EXISTS unsubscribe_tokens (
    id BIGSERIAL PRIMARY KEY,
    token TEXT UNIQUE NOT NULL,
    recipient_email TEXT NOT NULL,
    sender_domain TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS unsubscribe_list (
    id BIGSERIAL PRIMARY KEY,
    email TEXT NOT NULL,
    domain TEXT NOT NULL,
    created_at TEXT NOT NULL,
    UNIQUE(email, domain)
);
