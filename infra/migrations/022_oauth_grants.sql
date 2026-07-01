-- OAuth grants for "Sign in with Avalanche" (docs/25-project-login.md).
--
-- One table holds both OAuth grant kinds behind a `grant_type` discriminator:
--   - 'auth_code'   — same-device Authorization Code + PKCE (RFC 6749 / 7636).
--                     Created already bound to the account (the app is
--                     session-authenticated and the user has just consented),
--                     carrying the PKCE challenge; single-use at token exchange.
--   - 'device_code' — cross-device Device Authorization Grant (RFC 8628).
--                     Created 'pending' with no account; the phone binds an
--                     account when it approves the matching user_code.
--
-- The minted access token is a `project_tokens` row (see 002_project_tokens.sql),
-- so the existing `GET /v1/project-token/verify` resolves the DID unchanged.
--
-- The table carries only `account_id` + `client_id` — the same, already-accepted
-- account<->Project linkage that `project_tokens` has. No group or DID-set
-- linkage, so the group membership-opacity discipline (docs/03 §3.9) is intact.

CREATE TABLE oauth_grants (
    code                  TEXT PRIMARY KEY,              -- the auth code or device_code (high-entropy, opaque)
    grant_type            TEXT NOT NULL,                 -- 'auth_code' | 'device_code'
    user_code             TEXT,                          -- device flow only: short human-typeable code
    client_id             TEXT NOT NULL,                 -- registered Project OAuth client
    project_url           TEXT NOT NULL,                 -- token audience (matches project_tokens.project_url)
    redirect_uri          TEXT,                          -- auth_code flow only; validated against the registry
    code_challenge        TEXT,                          -- PKCE (auth_code flow)
    code_challenge_method TEXT,                          -- 'S256'
    scope                 TEXT,
    account_id            BIGINT REFERENCES accounts(id), -- NULL until a device grant is approved
    status                TEXT NOT NULL DEFAULT 'pending', -- 'pending' | 'approved' | 'consumed' | 'denied'
    access_token          TEXT,                          -- minted project token (device flow: set at approve)
    auth_time             TIMESTAMPTZ,                   -- when the user proved identity (consent/approve)
    created_at            TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at            TIMESTAMPTZ NOT NULL,
    last_polled_at        TIMESTAMPTZ                    -- device flow poll throttle (RFC 8628 slow_down)
);

CREATE INDEX idx_oauth_grants_expires ON oauth_grants (expires_at);
CREATE INDEX idx_oauth_grants_user_code ON oauth_grants (user_code) WHERE user_code IS NOT NULL;
