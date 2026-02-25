CREATE TABLE IF NOT EXISTS mcp_logs (
    id BIGSERIAL PRIMARY KEY,
    method TEXT NOT NULL,
    tool TEXT,
    success BOOLEAN NOT NULL DEFAULT TRUE,
    error TEXT,
    duration_ms BIGINT,
    created_at TEXT NOT NULL
);
