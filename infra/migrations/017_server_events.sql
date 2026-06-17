-- Durable server-event log for bot catch-up (docs/22 join-event API).
--
-- Append-only. Drained by `GET /v1/admin/events?since=<id>&kind=...` and pushed
-- live over the WebSocket to bots holding the matching capability. Lets a bot
-- that was disconnected at registration time recover the events it missed, so
-- routing is deferred, not lost.
--
-- Privacy (docs/22 §Privacy posture): the server already knows every account it
-- registered, so disclosing `did` + the registering `invite_token` to an
-- operator-installed bot adds no new leak. There is deliberately NO group
-- linkage here, preserving the §3.9 membership-opacity discipline.
CREATE TABLE server_events (
    id            BIGSERIAL PRIMARY KEY,
    kind          TEXT NOT NULL,
    did           TEXT NOT NULL,
    invite_token  TEXT,
    joined_at_ms  BIGINT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_server_events_kind_id ON server_events (kind, id);
CREATE INDEX idx_server_events_created ON server_events (created_at);
