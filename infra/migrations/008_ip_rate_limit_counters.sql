-- Rate-limit counters keyed by client IP address.
--
-- Used for endpoints that cannot rely on account_id (unauthenticated, or where
-- the rate limit needs to apply before the account exists). Mirrors the schema
-- of rate_limit_counters but with a TEXT key instead of a FK to accounts.
CREATE TABLE ip_rate_limit_counters (
    ip           TEXT NOT NULL,
    action       TEXT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    count        INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (ip, action, window_start)
);
