-- Group-message store-and-forward queue (Stage 5, sealed-sender path).
-- See docs/03-groups.md.
--
-- Separate from the DM `message_queue` because the routing model is
-- fundamentally different: group messages are addressed by per-(group,
-- member) `recipient_group_pseudonym` rather than `device_pk`. The
-- pseudonym is what the server sees on a sealed-sender envelope; it has
-- no link back to a device or DID (the pseudonym ↔ EMI mapping lives
-- only in `member_credentials`, and EMI is the group-encrypted form of
-- the recipient's identity).
--
-- Schema-discipline annotations (§9 invariant 1):
--   public    server uses freely, no privacy weight
--   opaque    server stores but cannot decrypt
--   ephemeral row deleted when the resource resolves

CREATE TABLE group_message_queue (
    id                          BIGSERIAL PRIMARY KEY,                       -- public
    recipient_group_pseudonym   BYTEA NOT NULL,                              -- opaque
    group_id                    BYTEA NOT NULL,                              -- public
    ciphertext                  BYTEA NOT NULL,                              -- opaque
    enqueued_at                 TIMESTAMPTZ NOT NULL DEFAULT now(),          -- public
    expires_at                  TIMESTAMPTZ NOT NULL                         -- public
);

-- Fan-out lookups go by pseudonym; results are oldest-first.
CREATE INDEX group_message_queue_recipient
    ON group_message_queue (recipient_group_pseudonym, enqueued_at);

-- The expiry-sweeper runs against this.
CREATE INDEX group_message_queue_expires
    ON group_message_queue (expires_at);
