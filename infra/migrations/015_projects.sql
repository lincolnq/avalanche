-- Projects: first-class server entity for installed Projects (docs/20, 24).
--
-- A Project owns zero or more bot accounts (one-to-many via project_bots),
-- holds named capabilities (project_capabilities), and — if it is a gatekeeper
-- — a token-signing public key the server pins to validate invite/registration
-- tokens locally (the server never calls the Project; docs/24 §Trust-and-gating).
--
-- `slug` is the stable external identifier: it is the token `iss` stamp and the
-- admin API path segment. The numeric `id` is internal (BIGSERIAL is not stable
-- across databases, so it is never used as an external name).
--
-- `signing_public_key` is the Project's token-signing key in general (not
-- semantically welded to one capability): set when registration.gatekeeper is
-- granted today, but a future bot-provisioning capability can sign with the same
-- key — the server gates by the token's `purpose` -> capability, not by the key.
CREATE TABLE projects (
    id                 BIGSERIAL PRIMARY KEY,
    slug               TEXT UNIQUE NOT NULL,
    name               TEXT NOT NULL,
    url                TEXT,
    signing_public_key BYTEA,
    created_at         TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Bot accounts belonging to a Project. `account_id` is the PK, so a bot belongs
-- to at most one Project; `project_id` is non-unique, so a Project has many
-- bots. Capability resolution is therefore deterministic: account -> (<=1)
-- project -> capabilities.
CREATE TABLE project_bots (
    account_id BIGINT PRIMARY KEY REFERENCES accounts(id),
    project_id BIGINT NOT NULL REFERENCES projects(id),
    linked_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_project_bots_project ON project_bots (project_id);

-- Named capabilities granted to a Project. Capabilities are the operator
-- authority made concrete (docs/22 §Project-capabilities); the only grantor is
-- adminbot, recorded in `granted_by` for later cross-referencing with the
-- #admins chat thread.
CREATE TABLE project_capabilities (
    id          BIGSERIAL PRIMARY KEY,
    project_id  BIGINT NOT NULL REFERENCES projects(id),
    capability  TEXT NOT NULL,
    granted_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    granted_by  TEXT NOT NULL,
    UNIQUE (project_id, capability)
);
CREATE INDEX idx_project_capabilities_project ON project_capabilities (project_id);
