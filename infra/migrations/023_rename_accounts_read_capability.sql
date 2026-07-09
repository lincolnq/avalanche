-- Rename the `subscribe.account_joined` capability to `accounts.read`.
--
-- Project permissions are being unified onto a single dot-separated namespace
-- (docs/20 §Project permissions). This capability also grew a second use — the
-- account-roster snapshot `GET /v1/admin/accounts` — so it is renamed to a
-- clearer view-permission name that subsumes the join/leave feeds and the
-- roster. `registration.gatekeeper` was already dot-separated and is unchanged.
--
-- adminbot holds no capability row (it is the superuser pin), so only bots the
-- operator explicitly granted the old capability are affected; their grant is
-- rewritten in place so it keeps working.
UPDATE project_capabilities
   SET capability = 'accounts.read'
 WHERE capability = 'subscribe.account_joined';
