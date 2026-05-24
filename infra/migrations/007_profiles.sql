-- Stage 4: encrypted profile blobs.
--
-- Stores opaque ciphertext keyed by account. The server never interprets the
-- contents; only contacts with the user's profile key can decrypt. A seized
-- server yields DIDs but not real names.
--
-- Separate table from `accounts` because the access patterns are independent:
-- auth/registration never reads profile blobs, and profile fetches never read
-- auth fields. Keeps the hot `accounts` table lean as profile blobs grow (with
-- future avatar/bio fields).

CREATE TABLE profiles (
    account_id     BIGINT PRIMARY KEY REFERENCES accounts(id),
    encrypted_blob BYTEA NOT NULL,
    updated_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
