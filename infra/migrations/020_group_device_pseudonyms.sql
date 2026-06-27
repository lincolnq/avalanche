-- Per-device group push pseudonyms (docs/03 §3.9, docs/04 multi-device groups).
--
-- Previously `member_credentials.group_push_pseudonym` held ONE pseudonym per
-- (group, member). Group delivery is a single-consumer queue keyed by pseudonym,
-- so a member's second device had no way to receive fan-out without stealing the
-- first device's messages. This splits routing pseudonyms into their own table
-- with N rows per (group, encrypted_member_id) — one per device — so each device
-- drains its own queue. Membership/role stays in `member_credentials`.
--
-- Schema-discipline note (§9 invariant 1): every column carries a trailing
-- `-- public | -- opaque | -- ephemeral | -- exempt` annotation.

CREATE TABLE group_member_pseudonyms (
    group_id             BYTEA NOT NULL REFERENCES groups(group_id) ON DELETE CASCADE,  -- public
    encrypted_member_id  BYTEA NOT NULL,                                                 -- opaque
    group_push_pseudonym BYTEA NOT NULL,                                                 -- opaque
    created_at           TIMESTAMPTZ NOT NULL DEFAULT now(),                             -- public
    -- A pseudonym is globally the routing key for the delivery queue, so it must
    -- be unique per group; random 24-byte values make collisions negligible.
    PRIMARY KEY (group_id, group_push_pseudonym)
);

-- Fan-out resolves a recipient's EMI to all of their device pseudonyms.
CREATE INDEX idx_group_member_pseudonyms_member
    ON group_member_pseudonyms (group_id, encrypted_member_id);

-- WebSocket subscribe and cross-group claim-squatting checks look up by the
-- pseudonym alone, mirroring the old member_credentials index.
CREATE INDEX idx_group_member_pseudonyms_pseudonym
    ON group_member_pseudonyms (group_push_pseudonym);

-- Carry each existing member's single pseudonym over as their first device's
-- binding so live groups keep delivering without a re-register.
INSERT INTO group_member_pseudonyms (group_id, encrypted_member_id, group_push_pseudonym)
    SELECT group_id, encrypted_member_id, group_push_pseudonym FROM member_credentials;

-- Routing now lives in the new table; membership/role stays here.
DROP INDEX IF EXISTS idx_member_credentials_pseudonym;
ALTER TABLE member_credentials DROP COLUMN group_push_pseudonym;

-- NOTE (exempt): the server now learns how many devices a member has (N rows
-- per EMI). The EMI remains zkgroup-opaque, so this cannot be linked to a DID;
-- it is the same device-count leak Signal accepts for multi-device fan-out.
