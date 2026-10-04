-- Per-Project bot signup keys (docs/24, docs/09 S-01).
--
-- A bot signup key lets a Project's bots register on a closed server and links
-- each one to that Project — nothing else. It replaces the old "setup code",
-- which embedded the server's master registration secret and let its holder
-- name any Project (including the superuser Project) to be linked into.
--
-- One active key per Project: minting again replaces the row, which revokes the
-- previous key. Only the SHA-256 of the key is stored; the Project is found by
-- that hash, so a key can never select a different Project.
CREATE TABLE project_bot_signup_keys (
    project_id  BIGINT PRIMARY KEY REFERENCES projects(id) ON DELETE CASCADE,
    key_hash    BYTEA NOT NULL UNIQUE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Join events no longer carry the raw invite token (it could contain the master
-- secret, and every roster-reading Project received it). Purge stored copies.
-- The column stays for now so older binaries don't break; nothing writes it.
UPDATE server_events SET invite_token = NULL WHERE invite_token IS NOT NULL;
