CREATE TABLE IF NOT EXISTS caldav_calendars (
    id BIGSERIAL PRIMARY KEY,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    slug TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT 'Calendar',
    description TEXT NOT NULL DEFAULT '',
    color TEXT NOT NULL DEFAULT '#0000FF',
    ctag TEXT NOT NULL DEFAULT '',
    created_at TEXT,
    updated_at TEXT,
    UNIQUE(account_id, slug)
);

CREATE TABLE IF NOT EXISTS caldav_objects (
    id BIGSERIAL PRIMARY KEY,
    calendar_id BIGINT NOT NULL REFERENCES caldav_calendars(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    filename TEXT NOT NULL,
    etag TEXT NOT NULL DEFAULT '',
    data TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE(calendar_id, filename)
);
