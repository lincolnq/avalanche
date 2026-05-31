-- Action-bound groups (Stage 5). See docs/03-groups.md §3.2 for the design.
--
-- Schema-discipline note (§9 invariant 1): every column below carries a
-- trailing `-- public | -- opaque | -- ephemeral | -- exempt` annotation.
--   public    server uses freely, no privacy weight
--   opaque    server stores but cannot decrypt
--   ephemeral row deleted when the resource resolves
--   exempt    known-leak the design has accepted; explain why in a comment

-- Per-group root. The encrypted state blob is opaque to the server; the
-- policy columns are server-readable so the server can enforce per-action
-- minimum-role checks (§3.3 step 4) without seeing identities.
CREATE TABLE groups (
    group_id                     BYTEA NOT NULL PRIMARY KEY,                  -- public
    server_public_params_version INTEGER NOT NULL,                            -- public
    group_public_params          BYTEA NOT NULL,                              -- public
    current_revision             BIGINT NOT NULL DEFAULT 0,                   -- public
    encrypted_state              BYTEA NOT NULL,                              -- opaque

    -- Policy (§3.1 / §3.3). Roles: 0 = Member, 1 = Admin. join_policy:
    -- 0 = Closed, 1 = RequestToJoin, 2 = OpenLink. modify_policy and
    -- modify_member_role are protocol-fixed Admin regardless of policy and
    -- therefore intentionally absent from this table.
    policy_invite_members_role     SMALLINT NOT NULL DEFAULT 1,               -- public
    policy_remove_members_role     SMALLINT NOT NULL DEFAULT 1,               -- public
    policy_modify_title_role       SMALLINT NOT NULL DEFAULT 1,               -- public
    policy_modify_description_role SMALLINT NOT NULL DEFAULT 1,               -- public
    policy_modify_expiry_role      SMALLINT NOT NULL DEFAULT 1,               -- public
    policy_join_policy             SMALLINT NOT NULL DEFAULT 0,               -- public
    policy_invite_link_password    BYTEA,                                     -- public
    policy_announcement_only       BOOLEAN NOT NULL DEFAULT FALSE,            -- public

    created_at                   TIMESTAMPTZ NOT NULL DEFAULT now()           -- public
);

-- Ring buffer of the last 256 revisions per group. Older rows are GC'd by a
-- background task so clients catching up from a recent revision can fetch
-- only the deltas (§3.4). Newest revision is also kept on `groups`; this
-- table stores the older snapshots.
CREATE TABLE group_state_history (
    group_id        BYTEA NOT NULL REFERENCES groups(group_id) ON DELETE CASCADE,  -- public
    revision        BIGINT NOT NULL,                                                -- public
    encrypted_state BYTEA NOT NULL,                                                 -- opaque
    -- Actions blob (§3.3) is small enough to inline here; clients fetching
    -- changes need both the snapshot and the diff to display them.
    actions         BYTEA NOT NULL,                                                 -- opaque
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),                             -- public
    PRIMARY KEY (group_id, revision)
);

-- Active members. `encrypted_member_id` is the per-group opaque routing id;
-- see docs/03-groups.md §2.3 and §3.2. No DID column — §3.9 rule 1.
-- `group_push_pseudonym` is a random per-group pseudonym the client chose at
-- join time; the relay independently holds `(pseudonym → device_token)`.
CREATE TABLE member_credentials (
    group_id             BYTEA NOT NULL REFERENCES groups(group_id) ON DELETE CASCADE,  -- public
    encrypted_member_id  BYTEA NOT NULL,                                                 -- opaque
    role                 SMALLINT NOT NULL,                                              -- public
    group_push_pseudonym BYTEA NOT NULL,                                                 -- opaque
    PRIMARY KEY (group_id, encrypted_member_id)
);
-- Pseudonym lookups happen on WebSocket subscribe and on group send fan-out;
-- both are per-group, but we also need to refuse cross-group claim-squatting
-- with a fast existence check, so index globally as well.
CREATE INDEX idx_member_credentials_pseudonym
    ON member_credentials (group_push_pseudonym);

-- Invited-but-not-yet-accepted members. Resolved by `promote_pending_members`
-- (self-action) or `decline_invite` (self-action), at which point the row is
-- removed — hence the `ephemeral` annotation on the timestamp.
--
-- `day_aligned_invited_at` is bucketed to day-resolution per §3.9 rule 5:
-- precise timestamps correlate with account-creation/registration events
-- and would de-anonymize. Use `floor(now() / 86400) * 86400` at insertion.
CREATE TABLE members_pending (
    group_id               BYTEA NOT NULL REFERENCES groups(group_id) ON DELETE CASCADE,  -- public
    encrypted_member_id    BYTEA NOT NULL,                                                 -- opaque
    role                   SMALLINT NOT NULL,                                              -- public
    day_aligned_invited_at TIMESTAMPTZ NOT NULL,                                            -- ephemeral
    PRIMARY KEY (group_id, encrypted_member_id)
);

-- Join requesters awaiting admin approval. Resolved by `approve_join_request`
-- / `deny_join_request` (admin) or `cancel_join_request` (self), removing
-- the row. Requester supplies their `group_push_pseudonym` at request time
-- so the admin's approval doesn't need it (§3.10).
CREATE TABLE members_pending_approval (
    group_id                 BYTEA NOT NULL REFERENCES groups(group_id) ON DELETE CASCADE,  -- public
    encrypted_member_id      BYTEA NOT NULL,                                                 -- opaque
    group_push_pseudonym     BYTEA NOT NULL,                                                 -- opaque
    day_aligned_requested_at TIMESTAMPTZ NOT NULL,                                            -- ephemeral
    PRIMARY KEY (group_id, encrypted_member_id)
);
