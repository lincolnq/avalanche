-- Directory entries: the client-facing "Network tab" project directory
-- (docs/20, docs/22). Moves the directory off the static PROJECTS env var and
-- into the DB so adminbot's /install-project manifest can control any number of
-- Network-tab web-page entries. `GET /v1/projects` reads this table; a one-time
-- startup seed migrates any existing PROJECTS env content in (see main.rs).
--
-- `project_id` is nullable on purpose:
--   * set    -> entry belongs to an installed Project; ON DELETE CASCADE drops
--               it when the Project is uninstalled.
--   * NULL   -> operator/seeded entry (from the old PROJECTS env var), managed
--               directly and not tied to any Project's lifecycle.
--
-- This table carries NO did/account_id column: it is directory metadata only,
-- so the membership-opacity discipline (docs/03 §3.9) is unaffected.
CREATE TABLE directory_entries (
    id          BIGSERIAL PRIMARY KEY,
    project_id  BIGINT REFERENCES projects(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    url         TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    -- OAuth "Sign in with Avalanche" client id (docs/25), if any. Preserved for
    -- seeded operator entries; NULL for manifest-driven entries (OAuth for
    -- manifest Projects is a deferred follow-up).
    client_id   TEXT,
    -- Server-vouched official flag (docs/54). Always false for manifest-driven
    -- entries (officialness is never self-declared); preserved for seeded rows.
    official    BOOLEAN NOT NULL DEFAULT false,
    position    INT NOT NULL DEFAULT 0,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_directory_entries_project ON directory_entries (project_id);
