CREATE TABLE IF NOT EXISTS forwardings (
    id BIGSERIAL PRIMARY KEY,
    domain_id BIGINT REFERENCES domains(id) ON DELETE CASCADE,
    source TEXT NOT NULL,
    destination TEXT NOT NULL,
    active BOOLEAN DEFAULT TRUE,
    keep_copy BOOLEAN DEFAULT FALSE,
    created_at TEXT,
    updated_at TEXT
);
