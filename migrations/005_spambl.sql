CREATE TABLE IF NOT EXISTS spambl_lists (
    id BIGSERIAL PRIMARY KEY,
    name TEXT NOT NULL,
    hostname TEXT UNIQUE NOT NULL,
    description TEXT DEFAULT '',
    enabled BOOLEAN DEFAULT FALSE,
    created_at TEXT,
    updated_at TEXT
);

INSERT INTO spambl_lists (name, hostname, enabled, created_at, updated_at) VALUES
    ('Spamhaus ZEN',       'zen.spamhaus.org',       FALSE, NOW()::TEXT, NOW()::TEXT),
    ('SpamCop',            'bl.spamcop.net',          FALSE, NOW()::TEXT, NOW()::TEXT),
    ('SORBS',              'dnsbl.sorbs.net',         FALSE, NOW()::TEXT, NOW()::TEXT),
    ('Barracuda BRBL',     'b.barracudacentral.org',  FALSE, NOW()::TEXT, NOW()::TEXT),
    ('UCEPROTECT Level 1', 'dnsbl-1.uceprotect.net',  FALSE, NOW()::TEXT, NOW()::TEXT),
    ('NiX Spam',           'ix.dnsbl.manitu.net',     FALSE, NOW()::TEXT, NOW()::TEXT),
    ('PSBL',               'psbl.surriel.com',        FALSE, NOW()::TEXT, NOW()::TEXT),
    ('SPFBL',              'dnsbl.spfbl.net',         FALSE, NOW()::TEXT, NOW()::TEXT)
ON CONFLICT (hostname) DO NOTHING;
