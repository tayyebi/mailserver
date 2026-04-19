-- Rate-limit rules: evaluated per-message in the content filter.
-- When a message matches a rule's conditions, its sender is subject
-- to the configured rate limit (max_messages per window_seconds).
-- If exceeded, the message is rejected with a temporary error (EX_TEMPFAIL).
CREATE TABLE IF NOT EXISTS rate_limit_rules (
    id         BIGSERIAL PRIMARY KEY,
    name       TEXT NOT NULL DEFAULT '',
    match_mode TEXT NOT NULL DEFAULT 'AND',   -- 'AND' or 'OR'
    conditions_json TEXT NOT NULL DEFAULT '[]',
    max_messages  INTEGER NOT NULL DEFAULT 100,
    window_seconds INTEGER NOT NULL DEFAULT 3600,
    enabled    BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TEXT
);

-- Per-sender sliding-window counters for the rate-limit rules.
CREATE TABLE IF NOT EXISTS rate_limit_counts (
    id          BIGSERIAL PRIMARY KEY,
    rule_id     BIGINT NOT NULL REFERENCES rate_limit_rules(id) ON DELETE CASCADE,
    sender      TEXT NOT NULL,
    window_start TEXT NOT NULL,
    count       INTEGER NOT NULL DEFAULT 1,
    UNIQUE(rule_id, sender, window_start)
);
