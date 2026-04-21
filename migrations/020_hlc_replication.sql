-- HLC-based active-active replication tables

-- Peer registry: every node in the cluster (never hard-deleted)
CREATE TABLE IF NOT EXISTS peers (
    id                UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    instance_id       TEXT UNIQUE NOT NULL,
    url               TEXT NOT NULL,
    region            TEXT,
    peer_public_key   TEXT,               -- Ed25519 public key, PEM/base64
    status            TEXT NOT NULL DEFAULT 'online',
    suspicion_level   INT  NOT NULL DEFAULT 0,
    last_seen_at      TIMESTAMPTZ,
    first_seen_hlc    TEXT,
    decommissioned_at TIMESTAMPTZ,        -- explicit leave marker; never hard-deleted
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Append-only replication log: source of truth for every write
CREATE TABLE IF NOT EXISTS replication_log (
    seq            BIGSERIAL PRIMARY KEY,
    hlc            TEXT NOT NULL,
    origin_replica TEXT NOT NULL,
    entity_type    TEXT NOT NULL,
    entity_id      TEXT NOT NULL,
    op             TEXT NOT NULL CHECK (op IN ('upsert','tombstone','insert')),
    payload        JSONB NOT NULL,
    created_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE INDEX IF NOT EXISTS replication_log_hlc_idx
    ON replication_log (hlc);

CREATE INDEX IF NOT EXISTS replication_log_origin_hlc_idx
    ON replication_log (origin_replica, hlc);

CREATE INDEX IF NOT EXISTS replication_log_entity_idx
    ON replication_log (entity_type, entity_id);

-- Per-peer gossip cursor: how far the local replica has consumed from each remote peer
CREATE TABLE IF NOT EXISTS hlc_cursors (
    peer_instance_id TEXT PRIMARY KEY,
    last_applied_hlc TEXT NOT NULL DEFAULT '',
    last_gossip_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
