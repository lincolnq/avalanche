-- Abuse reports submitted to this homeserver (docs/12 §3).
--
-- A report is account-level and contains NO message content — only the reported
-- DID and a reason enum. Persisted for operator review. The account-level
-- enforcement ladder (docs/12 §4) and cross-server signed forwarding (§3) are
-- deferred until federation lands, so in v1 a report never crosses the network:
-- it stays on the reporter's own homeserver.
--
-- `reporter_account` is the local account that filed it, kept for per-reporter
-- rate-limiting and operator audit. The reporter's own homeserver already knows
-- it is DMing the reported DID, so storing this linkage here adds no new leak.
CREATE TABLE abuse_reports (
    id               BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    reported_did     TEXT NOT NULL,
    reason           TEXT NOT NULL,
    reporter_account BIGINT NOT NULL REFERENCES accounts(id),
    reported_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_abuse_reports_reported_did ON abuse_reports (reported_did);
CREATE INDEX idx_abuse_reports_reporter ON abuse_reports (reporter_account);
