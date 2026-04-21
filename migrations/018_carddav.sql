CREATE TABLE IF NOT EXISTS carddav_addressbooks (
    id BIGSERIAL PRIMARY KEY,
    account_id BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    slug TEXT NOT NULL,
    display_name TEXT NOT NULL DEFAULT 'Address Book',
    description TEXT NOT NULL DEFAULT '',
    ctag TEXT NOT NULL DEFAULT '',
    created_at TEXT,
    updated_at TEXT,
    UNIQUE(account_id, slug)
);

CREATE TABLE IF NOT EXISTS carddav_objects (
    id BIGSERIAL PRIMARY KEY,
    addressbook_id BIGINT NOT NULL REFERENCES carddav_addressbooks(id) ON DELETE CASCADE,
    uid TEXT NOT NULL,
    filename TEXT NOT NULL,
    etag TEXT NOT NULL DEFAULT '',
    data TEXT NOT NULL,
    created_at TEXT,
    updated_at TEXT,
    UNIQUE(addressbook_id, filename)
);
