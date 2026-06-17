-- Single-use tracking for Project-signed registration tokens (docs/24).
--
-- A token carries a unique `jti`; the PRIMARY KEY makes redemption atomic —
-- INSERT-as-gate before account creation, so a replayed token conflicts and is
-- rejected. Named generically (`token_redemptions`, with a `purpose` column)
-- so a future bot-signup token (`purpose = 'bot'`) shares the same table as the
-- human-invite token (`purpose = 'invite'`).
CREATE TABLE token_redemptions (
    jti             TEXT PRIMARY KEY,
    issuer_slug     TEXT NOT NULL,
    purpose         TEXT NOT NULL,
    redeemed_by_did TEXT NOT NULL,
    redeemed_at     TIMESTAMPTZ NOT NULL DEFAULT now()
);
