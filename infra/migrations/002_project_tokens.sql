-- Project tokens: short-lived opaque tokens used by Projects to verify user identity.
-- A user requests a token before opening a Project webview; the Project verifies
-- it with the homeserver to learn the user's DID.

CREATE TABLE project_tokens (
    token       TEXT PRIMARY KEY,
    account_id  BIGINT NOT NULL REFERENCES accounts(id),
    project_url TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_project_tokens_expires ON project_tokens (expires_at);
