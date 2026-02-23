CREATE TABLE IF NOT EXISTS outbound_relays (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    host TEXT NOT NULL,
    port INTEGER NOT NULL DEFAULT 587,
    auth_type TEXT NOT NULL DEFAULT 'none',
    username TEXT,
    password TEXT,
    active BOOLEAN DEFAULT TRUE,
    created_at TEXT,
    updated_at TEXT
);

CREATE TABLE IF NOT EXISTS outbound_relay_assignments (
    id BIGSERIAL PRIMARY KEY,
    relay_id BIGINT NOT NULL REFERENCES outbound_relays(id) ON DELETE CASCADE,
    assignment_type TEXT NOT NULL,
    pattern TEXT NOT NULL,
    created_at TEXT
);
