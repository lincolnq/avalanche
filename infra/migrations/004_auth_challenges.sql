-- Auth challenges: short-lived nonces for identity key signature verification.
-- A client requests a challenge before authenticating; the server stores the
-- nonce so it can verify the client signed it with its identity key.
-- Challenges are single-use (consumed atomically on redemption) and expire
-- after a short TTL.

CREATE TABLE auth_challenges (
    nonce      TEXT PRIMARY KEY,
    device_pk  BIGINT NOT NULL REFERENCES devices(id),
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_auth_challenges_expires ON auth_challenges (expires_at);
