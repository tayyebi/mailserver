-- Replica node registry: peers this node syncs with
CREATE TABLE IF NOT EXISTS replica_nodes (
    id BIGSERIAL PRIMARY KEY,
    node_id TEXT NOT NULL UNIQUE,
    peer_url TEXT NOT NULL,
    shared_secret TEXT NOT NULL,
    active BOOLEAN NOT NULL DEFAULT TRUE,
    last_seen_at TEXT,
    created_at TEXT NOT NULL
);

-- Per-node key/value state (logical clock, stable node_id, etc.)
CREATE TABLE IF NOT EXISTS node_state (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- Append-only log of every local write; drives outbound replication
CREATE TABLE IF NOT EXISTS replication_changelog (
    id BIGSERIAL PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    version_id TEXT NOT NULL UNIQUE,
    node_id TEXT NOT NULL,
    logical_clock BIGINT NOT NULL,
    operation TEXT NOT NULL,
    payload TEXT NOT NULL,
    created_at TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS replication_changelog_entity_idx
    ON replication_changelog (entity_type, entity_id);

CREATE INDEX IF NOT EXISTS replication_changelog_lc_idx
    ON replication_changelog (logical_clock DESC, node_id DESC);

-- Per-peer cursor: highest changelog id already sent to that peer
CREATE TABLE IF NOT EXISTS replication_checkpoint (
    peer_node_id TEXT PRIMARY KEY,
    last_sent_change_id BIGINT NOT NULL DEFAULT 0
);
