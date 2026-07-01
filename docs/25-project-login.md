# Project Login ("Sign in with Avalanche")

This document specifies how a user signs in to a Project with their Avalanche
account: proving they control a DID **and** hold an authenticated account on a
given homeserver, so the Project can bind a web/session identity to that DID
(and then DM them / add them to groups through its bot).

It builds directly on the Project-token model in `20-project-security.md`; read
that first. This doc adds only the *login ceremony* — issuance and the two
front-ends — on top of the existing token/verify machinery.

## What login proves (v1 scope)

**Membership = "an authenticated account exists on this homeserver."** That fact
is already established by the existing auth gate: to obtain any homeserver
credential a device must complete the challenge-response in
`10-server-implementation.md` (sign a server nonce with the Ed25519 identity
key), and only then can it mint a Project token. So a valid token already proves
*"controls this DID and holds a registered, authenticated account here."* Login
surfaces that proof to a Project in an OAuth-standard shape; it invents no new
server-side membership state.

Explicitly **out of scope for v1** (see *Non-goals*): any notion of "good
standing" beyond account existence (there is no suspension/ban concept on the
server today), roles, group-membership claims, pseudonymous/anonymous
disclosure, and offline (signed-credential) verification. The default and only
disclosure tier here is the **real DID**, because the common integration is a
bot that must DM the user and add them to groups (`20` scopes `dm:initiate`,
`invites:auto-accept`).

## Trust model

Unchanged from `20`: the homeserver admin approves ("installs") every Project,
so a Project is inside the user's trust chain. Login therefore does **not**
introduce a per-scope runtime consent prompt (that would re-litigate the admin's
decision — `20` *Project permissions*). What login *does* add is a **consent
screen**, but its meaning is narrower: it is the user's act of *choosing to sign
in to this Project as this identity*, not an approval of scopes. The granted
capabilities are shown for legibility only.

## OAuth 2.0 mapping

Login is OAuth 2.0, so Projects can use off-the-shelf OAuth client libraries:

- **Authorization endpoint** = the **Avalanche app** (native consent), reached
  by a Universal Link / App Link (`https://<invite_domain>/authorize?...`). There
  is deliberately **no server-rendered login page** — that would put a web UI and
  a broader surface on the homeserver, against the `20` "keep the server small"
  posture.
- **Token endpoint** = the homeserver (`POST /v1/oauth/token`).
- **Access token** = a **Project token** (opaque; reuses `project_tokens`), so
  the Project resolves the DID via the existing, unchanged
  `GET /v1/project-token/verify`. No new introspection endpoint, no JWT, no
  signing key.

Two front-ends share this one back-end (consent + issue + verify):

### A. Same-device — Authorization Code + PKCE (RFC 6749 + RFC 7636)

For a user whose browser is on the same device as the app (a phone browser, or
later a desktop with the app installed).

```
Project            Browser                 Avalanche app            Homeserver
  │  build authorize URL                        │                       │
  │  (client_id, redirect_uri, state,           │                       │
  │   code_challenge=S256(verifier))            │                       │
  │────"Sign in"────▶│                          │                       │
  │                  │── universal link ───────▶│  validate client_id + │
  │                  │                          │  redirect_uri ───────▶│
  │                  │                 CONSENT (sign in as Alice?)       │
  │                  │                          │  POST /oauth/authorize-code
  │                  │                          │  (session-auth, +challenge)
  │                  │                          │──────────────────────▶│
  │                  │                          │◀──── { code } ────────│
  │                  │◀ open redirect_uri?code=&state= ─│                │
  │  exchange: POST /oauth/token                │                       │
  │  (grant=authorization_code, code, verifier, redirect_uri, client_id)│
  │────────────────────────────────────────────────────────────────────▶
  │◀──── { access_token, token_type, expires_in, auth_time } ───────────│
  │  GET /v1/project-token/verify?token=access_token → { did } ─────────▶│
```

PKCE closes the redirect-interception hole: the `code` is useless without the
`code_verifier`, which never leaves the Project backend.

### B. Cross-device — Device Authorization Grant (RFC 8628)

The mobile-first headline case: the user is at the Project's site in a **desktop
browser** and has **no Avalanche desktop app**. They authorize with their phone.

```
Project            Desktop browser          Phone (app)            Homeserver
  │  POST /oauth/device_authorization (client_id, scope) ───────────────▶
  │◀ { device_code, user_code, verification_uri,                         │
  │    verification_uri_complete, expires_in, interval } ────────────────│
  │  render QR(verification_uri_complete) ──▶│                           │
  │                  │        scan QR ───────▶│ (opens app; same         │
  │                  │                        │  authorize universal link)│
  │                  │                 CONSENT ("signing in on ANOTHER   │
  │                  │                  device — only continue if you    │
  │                  │                  started this yourself")          │
  │                  │                        │ POST /oauth/device/approve│
  │                  │                        │ (session-auth, user_code) │
  │                  │                        │──────────────────────────▶ binds
  │  poll: POST /oauth/token (grant=device_code, device_code) ──────────▶│ account,
  │◀ { error: "authorization_pending" } … then { access_token, … } ─────│ mints token
```

The QR encodes the **same** `authorize` Universal Link as front-end A (with the
device/user code embedded), so a phone-camera scan opens the app directly and the
app's own QR scanner parses the same string — one code path. **No secret ever
reaches the desktop**; the desktop/Project only ever holds the opaque
`device_code` (polled server-to-server) and the final access token, which is
audience-bound to `project_url` and useless without the Project backend. This is
why we deliberately do **not** reuse the device-linking ECDH mailbox handshake
(`04` §4) — there is no key material to protect in transit here.

## Session lifetime & re-authentication

The login ceremony is a **point-in-time identity bootstrap**, not an ongoing
session. Once the Project has resolved the DID it establishes **its own** session
(cookie); the platform imposes **no expiry** on that session. A Project that
wants a permanent "never sign in again" cookie is free to keep one — exactly like
any "Sign in with X" relying party.

Consequences, stated so Projects design correctly:

- The OAuth artifacts are short-lived (auth `code` ~60s; `access_token` = a
  Project token, ~1h) and exist only to *establish* identity. A Project must
  trade them for its own durable session immediately, not treat the access token
  as its long-term session.
- **Membership is asserted as-of-login.** With a permanent Project session, the
  "authenticated account exists" fact is checked once and never re-checked. For
  v1 this is a non-issue (there is no ban/revocation concept to propagate). *If*
  a future "good standing" feature lands, a Project wanting continuous
  enforcement (auto-logout on ban) must re-run the flow periodically or consume a
  future revocation signal — a permanent cookie by definition will not notice.
- **Re-authentication is always Project-initiated, never platform-forced.** When
  a Project does re-prompt, same-device paths keep it frictionless; the QR/phone
  ceremony only recurs if the desktop session expired *and* the user is back on
  desktop. Practical advice: set a long Project session so the QR ceremony is
  rare.
- The token/verify responses include an optional **`auth_time`** (unix seconds,
  when the user last proved identity) so a Project can implement its own max-age /
  step-up policy without any platform-side session management. Refresh tokens are
  intentionally not provided (a login does not need them).

## Client registration

A Project registers as an OAuth client at **admin-install time** by extending its
entry in the `PROJECTS` config (see `10`/`config.rs`). Fields added, all optional
(a Project that does not do login omits them and is unaffected):

- `client_id` — stable public identifier used in authorize/token/device requests.
- `redirect_uris` — exact-match allowlist for front-end A's `redirect_uri`
  (no open redirect; a `redirect_uri` not on the list is rejected).
- `official` — the verified-badge bit (`54`), shown on the consent screen.

There is **no client secret**: same-device is a public client protected by PKCE,
and the device grant relies on the high-entropy `device_code` returned only to
the initiating client (RFC 8628 public-client mode).

## Server surface

New endpoints (all under `/v1/oauth`); see `config.rs` for TTLs.

| Endpoint | Auth | Purpose |
|---|---|---|
| `POST /oauth/authorize-code` | session (`AuthDevice`) | App mints an auth code post-consent, bound to account + `code_challenge` + `client_id` + `redirect_uri`. |
| `POST /oauth/device_authorization` | none (client) | Start a device grant; returns `device_code`/`user_code`/QR link/`interval`. |
| `POST /oauth/device/approve` | session (`AuthDevice`) | App approves a `user_code`/`device_code` post-consent; binds account and mints the token. |
| `POST /oauth/token` | none (client) | Exchange (`authorization_code`) or poll (`device_code`) → `{ access_token, token_type, expires_in, auth_time }`. |

`access_token` is a `project_tokens` row, so `GET /v1/project-token/verify` is
unchanged. The token endpoint returns **RFC-shaped error bodies**
(`{"error":"authorization_pending"|"slow_down"|"expired_token"|"access_denied"|"invalid_grant"|"invalid_client"}`,
HTTP 400) rather than the generic `ServerError` responses, per the OAuth specs.

### Data model

One new table `oauth_grants` (migration `022`) holding both grant kinds behind a
`grant_type` discriminator: the opaque `code` (PK; the device_code or auth code),
optional short `user_code`, `client_id`, `project_url`, optional `redirect_uri`,
optional PKCE `code_challenge`/`code_challenge_method`, `scope`, nullable
`account_id` (set on approval), `status` (`pending`|`approved`|`consumed`|`denied`),
nullable `access_token` (the minted Project token, set on approve/exchange),
`created_at`, `expires_at`, `last_polled_at` (for `slow_down`). Auth codes are
single-use (`status → consumed` on exchange). A GC task deletes expired rows,
alongside the existing token-expiry sweeps.

The table carries `account_id` + `client_id` only — the same, already-accepted
`account ↔ Project` linkage that `project_tokens` has (`20` "what the homeserver
knows"). No group or DID-set linkage, so the `03` §3.9 membership-opacity
discipline is unaffected.

## Client surface (app = authorization endpoint)

- The existing deep-link routers (iOS `AppState.handleDeepLink`, Android
  `AppViewModel.handleDeepLink`) gain an `authorize` route parsing the OAuth
  params + target `server_url`. The `go.theavalanche.net` AASA/App Links already
  wildcard all paths, so no entitlement/manifest change is needed.
- A **consent screen** names the Project (+ `official` badge, `54`), the identity
  being used (`display_name @ server`), and the granted capabilities for
  legibility. On the device-grant path it additionally warns *"You're signing in
  on another device — only continue if you started this yourself."*
- The **device-grant QR** is scanned with the existing QR scanner (`04` §4);
  short `user_code` entry is the copy-paste fallback.
- Two FFI methods drive the authed calls: `oauth_issue_code(...)` (front-end A)
  and `oauth_approve_device(...)` (front-end B). Both are per-account-context: the
  app selects the account matching the request's `server_url`.
- **No account on the requested homeserver → a structured, catchable failure**
  (`NoAccountOnServer { server_url }`) that the app (or a Project) can hook to an
  invitation/onboarding flow. Onboarding itself is out of scope here.

## Cross-device consent phishing

The one genuinely new threat (shared by every scan-to-login flow): an attacker
shows the victim a QR that authorizes the *attacker's* session; the victim scans
and approves, logging the attacker in as the victim. The phone cannot verify
where the desktop is. Mitigations (bounding, not eliminating):

- The consent screen states plainly that this signs in **on another device** and
  to continue only if the user started it.
- The Project's `official` badge is shown so a spoofed/unknown Project is legible.
- Short `device_code`/`user_code` TTL (a couple minutes), rate-limited; the QR
  carries the high-entropy `device_code` so there is no brute-forceable typed
  value (the human `user_code` gets tighter limits per RFC 8628).
- Blast radius is already bounded: one admin-approved Project, a scoped token, no
  key material, and a point-in-time assertion.

## Platform scope / parity

- **iOS** (reference) and **Android** implement the deep-link route, consent
  screen, QR entry, and the two FFI calls in the same change (mobile parity rule).
- **Desktop app: deferred.** The Avalanche desktop app registers no deep-link
  handler today, so it cannot be an authorizer without new infrastructure. Desktop
  *users* are fully served by front-end B (authorize from the phone), so nothing
  is blocked. "Desktop app as login authorizer" (and its prerequisite, a desktop
  deep-link handler that would also serve invite/conversation links) are tracked
  in `02-todos-deferred.md`. This is a deliberate, noted exception to the
  three-platform parity rule, justified by the missing prerequisite and the fact
  that the feature works *from* desktop via the phone.

## Non-goals (follow-on work)

- Any "good standing" / suspension / role / group-membership claim (v1 =
  "authenticated account exists").
- Offline / signed-credential / `.well-known`-rooted verification; pseudonymous
  or anonymous (zkgroup) disclosure tiers.
- OIDC conformance (`id_token`, discovery document, `userinfo`/`sub`) — Projects
  call `verify`. Revisit only if a Project ecosystem wants drop-in OIDC libraries.
- Refresh tokens (a login does not need them).
- The invitation/onboarding handler for the no-account case (only the structured
  failure is in scope).
- Avalanche desktop app as an authorizer, and the desktop deep-link handler it
  requires.
