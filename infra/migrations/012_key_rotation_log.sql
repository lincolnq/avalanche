CREATE TABLE key_rotation_log (
    id               BIGSERIAL PRIMARY KEY,
    account_id       BIGINT NOT NULL REFERENCES accounts(id) ON DELETE CASCADE,
    old_identity_key BYTEA,
    new_identity_key BYTEA NOT NULL,
    rotated_at       TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX key_rotation_log_account_idx ON key_rotation_log(account_id);
