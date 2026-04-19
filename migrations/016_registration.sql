-- Per-domain self-registration settings.
-- When registration_enabled is TRUE, unauthenticated users may create
-- a mailbox on that domain via the public /register endpoint.
-- registration_username_regex constrains which usernames are accepted
-- (empty string = allow any valid username).
ALTER TABLE domains
    ADD COLUMN IF NOT EXISTS registration_enabled BOOLEAN DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS registration_username_regex TEXT DEFAULT '';
