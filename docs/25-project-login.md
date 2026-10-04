# 25 — Project Login ("Sign in with Avalanche")

> **Status:** Built on iOS and Android with the server endpoints: OAuth 2.0 auth-code + PKCE (same device) and the RFC 8628 device grant (phone authorizes a desktop browser). Desktop as an authorizer is not built. OIDC conformance is **Proposed**.
> **Last verified against code:** 2026-10-03

## Summary

A Project can sign a user in with their Avalanche account. Login proves the user controls a DID **and** holds an authenticated account on a given homeserver, so the Project can bind its own web session to that DID and then reach the user through its bot. It is layered on Project tokens (`20-project-security.md`): the **app is the authorization endpoint**, the **homeserver is the token endpoint**, and the **access token is a Project token**, so Projects resolve the DID with the existing `GET /v1/project-token/verify`. No JWT, no signing key, no new introspection endpoint.

## Current design

### What login proves

**"An authenticated account exists on this homeserver."** Minting any Project token already requires a session obtained through challenge-response with the identity key, so a token proves "controls this DID and has a registered, authenticated account here". Login presents that in an OAuth shape and adds no new server membership state.

The disclosed identity is always the **real DID**. Out of scope: "good standing" (the server has no ban concept), roles, group-membership claims, pseudonymous disclosure, offline verification.

### Trust model

As in `20`: the admin installed the Project, so it is in the user's trust chain, and there is no per-scope prompt. The login **consent screen** means "sign in to this Project as this identity". Granted permissions are shown for legibility only.

### OAuth 2.0 mapping

- **Authorization endpoint = the Avalanche app**, reached by the Universal Link / App Link `https://go.theavalanche.net/authorize?…` (the AASA and App Links files already wildcard all paths). The link carries the standard parameters plus `server_url`, naming the homeserver to authorize against. There is no server-rendered login page.
- **Token endpoint = the homeserver** (`POST /v1/oauth/token`).
- **Access token = a Project token** (`project_tokens` row, audience = the Project's `url`), ~1 hour.

#### A. Same device — Authorization Code + PKCE (RFC 6749 + RFC 7636)

The Project redirects to the `authorize` link with `client_id`, `redirect_uri`, `state` and `code_challenge` (S256). The app validates the client against the homeserver, shows consent, and calls `POST /v1/oauth/authorize-code` (session-auth), which binds a code to the account, challenge, client and redirect URI. The app opens `redirect_uri?code=…&state=…`. The Project backend exchanges the code plus verifier at `POST /v1/oauth/token` for `{ access_token, token_type, expires_in, auth_time }`, then calls `verify`. PKCE makes an intercepted code useless.

#### B. Cross-device — Device Authorization Grant (RFC 8628)

For a user at the Project's site in a desktop browser without the Avalanche desktop app. The Project calls `POST /v1/oauth/device_authorization` and renders a QR of `verification_uri_complete`, which is the same `authorize` Universal Link with the user code embedded. The phone opens it, shows consent with an **"another device"** warning, and calls `POST /v1/oauth/device/approve` (session-auth). The Project polls `/v1/oauth/token` (`authorization_pending`, `slow_down`, then the token). No secret reaches the desktop. That is why this doesn't reuse the device-linking ECDH mailbox (`04` §4): there's no key material to protect in transit.

### Server surface

`core/crates/server/src/routes/oauth.rs`, migration `022_oauth_grants.sql`.

| Endpoint | Auth | Purpose |
|---|---|---|
| `POST /v1/oauth/authorize-code` | session | App mints an auth code after consent |
| `POST /v1/oauth/device_authorization` | none (IP rate-limited) | Start a device grant |
| `POST /v1/oauth/device/approve` | session | App approves a device grant and mints the token |
| `POST /v1/oauth/token` | none (IP rate-limited) | Exchange or poll → `{ access_token, token_type, expires_in, auth_time }` |

The token endpoint returns RFC-shaped errors (`authorization_pending`, `slow_down`, `expired_token`, `access_denied`, `invalid_grant`, `invalid_client`, HTTP 400). Lifetimes are configurable (`OAUTH_AUTH_CODE_LIFETIME_SECS`, `OAUTH_DEVICE_CODE_LIFETIME_SECS`, `OAUTH_DEVICE_POLL_INTERVAL_SECS`); expired grants are swept by the background task.

`oauth_grants` holds both grant kinds behind a `grant_type` discriminator: code (primary key), optional `user_code`, `client_id`, `project_url`, optional `redirect_uri` and PKCE challenge, `scope`, nullable `account_id`, status (`pending` / `approved` / `consumed` / `denied`), the minted access token, `auth_time`, and timestamps. Auth codes are single-use. The table links `account_id` to `client_id` only, the same account↔Project linkage `project_tokens` already has, so `03` §3.9 is unaffected.

`auth_time` is returned by the **token** endpoint only, not by `verify`.

### Client registration

A login-capable Project declares `clientId` and `redirectUris` in its install manifest (`20` §The manifest document); they land on its `projects` row (`oauth_client_id` UNIQUE, `oauth_redirect_uris`), and `find_client` resolves requests against it. A duplicate `client_id` fails the install, so one manifest can't hijack another Project's login. There is no client secret: same-device is a public client protected by PKCE; the device grant relies on the high-entropy `device_code`.

### Client surface

- iOS `AppState.handleAuthorizeDeepLink` and Android `AppViewModel.handleDeepLink` parse the `authorize` route. Cold-launch links are staged until accounts are restored.
- The consent screen (`ProjectLoginConsentView.swift`) names the Project and its checkmark (resolved from the homeserver's directory by `client_id`), the identity being used, and on the device path the "another device" warning.
- FFI: `oauth_issue_code` and `oauth_approve_device` (`core/crates/app-core/src/lib.rs`).
- No account on the requested homeserver → a structured `noAccountOnServer` failure the UI surfaces.

### Session lifetime

Login is a **point-in-time identity bootstrap**. The Project then keeps its own session, and the platform imposes no expiry on it. Consequences: trade the access token for your own session immediately; membership is asserted as of login and never re-checked (harmless today, with no ban concept); re-authentication is always Project-initiated. Use `auth_time` for your own max-age policy. No refresh tokens.

## Known gaps

- **The checkmark is always absent.** It comes from the directory's `official` flag, which nothing sets (`20` §Officialness). The consent screen's anti-phishing badge is inert.
- **Identity choice.** The app uses the first account on the requested server (`handleAuthorizeDeepLink`). A user with two identities on one server can't choose, and isn't told which identity is used unless they read the consent screen.
- **Project name resolution** depends on the Project having a directory entry; a login-only Project without `webEntries` shows no verified name.
- **Desktop app as authorizer** is not built. Desktop registers an `avalanche://` OS handler (`desktop/src-tauri/tauri.conf.json`) but has no `authorize` route. Desktop users are served by flow B from their phone. This is a noted exception to the parity rule.
- **Audience** is recorded but not enforced by `verify` (`20` §Known gaps).

## Planned

- An identity picker on the consent screen when several identities are on the requested server.
- Show the checkmark once officialness is settable (`22` §Planned).
- `verify` with a required `audience` (`20` §Planned).

## Proposed

**OIDC conformance as the primary developer story.** Needs project-owner review; it reverses the earlier non-goal.

Most campaign tools (event signup sites, CRMs, anything on NextAuth, Auth0 or a stock OIDC library) can add an OIDC provider without custom code, but cannot easily add a bespoke OAuth flow that ends in a custom `verify` call. Making login OIDC-conformant turns "integrate with Avalanche" into a configuration step:

- **Discovery:** `/.well-known/openid-configuration` on the homeserver, listing the existing endpoints (authorization endpoint = the app's `authorize` link).
- **`id_token`:** a JWT signed by a per-homeserver key, with `sub` = DID, `iss` = the homeserver URL, `aud` = `client_id`, `auth_time`, `nonce`. This introduces the server signing key `20` avoided; the cost is one key with a published JWKS, and the opaque access token plus `verify` stay as they are.
- **`userinfo`:** returns `sub` and, only with consent, a display name.
- **Scope `openid`** gates the above; existing non-OIDC Projects are unaffected (additive).

Paired with the messenger-bot idea in `23` §Speculative, a campaign gets identity and messaging from Avalanche without hosting Signal state.

## Speculative

- Desktop app as authorizer (needs an `authorize` route in the desktop deep-link handler).
- "Good standing" claims and a revocation signal for continuous enforcement.
- Pseudonymous per-Project identities for webview-only Projects (`20`).
- Offline, signed-credential verification.

## Cross-device consent phishing

The one new threat, shared by every scan-to-login flow: an attacker shows the victim a QR that authorizes the *attacker's* desktop session; the victim approves and logs the attacker in as themselves. Mitigations bound it without eliminating it: consent copy that says this signs in **on another device** and to continue only if you started it; the Project's checkmark (inert until officialness ships); short code TTLs; IP rate limits on starting a grant and on the token endpoint; approval requires a logged-in session; and a bounded blast radius (one admin-installed Project, a scoped token, no key material).

## Rationale and rejected alternatives

- **Rejected: a server-rendered login page.** It would put a web UI and a broader attack surface on the homeserver, against `20`'s "keep the server small". The app is the natural authorization endpoint.
- **Rejected: reusing the device-linking mailbox for cross-device login.** Nothing secret crosses devices, so plain RFC 8628 suffices.
- **Access token = Project token**, so Project verification is unchanged and one verify path serves both webviews and login.
- **No refresh tokens.** Login bootstraps the Project's own session; it doesn't need them.
