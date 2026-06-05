-- Bounce inboxes — designate accounts to receive and parse DSN bounce
-- notifications (RFC 3464 – An Extensible Message Format for Delivery
-- Status Notifications).
--
-- When a message cannot be delivered the remote MTA generates a DSN, a
-- multipart/report message with report-type=delivery-status.  This table
-- records which accounts the administrator has designated as bounce
-- collection points so the admin UI can parse and surface structured
-- bounce data (status codes, remote MTA diagnostics, original recipients).
CREATE TABLE IF NOT EXISTS bounce_inboxes (
    id BIGSERIAL PRIMARY KEY,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    label TEXT NOT NULL DEFAULT '',
    created_at TEXT,
    UNIQUE(account_id)
);
