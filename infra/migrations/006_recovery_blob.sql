-- Recovery blob: opaque encrypted ciphertext containing the user's rotation key,
-- identity keypair, and server list. Decryptable only with the user's passkey
-- (via WebAuthn PRF extension) or written-down recovery phrase.
-- Served publicly via GET /v1/recovery/{did} — safe because it's encrypted.

ALTER TABLE accounts
    ADD COLUMN recovery_blob BYTEA;
