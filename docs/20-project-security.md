# 20 — Project Security Model

> **Status:** Partial — Project tokens, the manifest install, two server-enforced capabilities, the DB-backed directory, and OAuth login are built. The client-honored scope catalog, Project surfaces in conversations, token audience enforcement, and pseudonymous identity are not.
> **Last verified against code:** 2026-10-03

## Summary

A Project is a standalone service that serves a web UI (opened in an app webview) and, usually, runs bot accounts that are ordinary E2E participants. Because the homeserver has no message keys, anything that touches content or group membership goes through a visible bot. Projects authenticate users with short-lived opaque **Project tokens** minted by the homeserver. An operator installs a Project by handing adminbot a **manifest**; the server records the Project, its bots, and any **server-enforced capabilities** granted.

The trust model is the Slack-workspace one: users trust their homeserver's admin, and the admin vets the Projects. Several real gaps exist today (see *Known gaps*). By design, every member of a small server running adminbot is an admin and can install Projects (S-29, `22`). The older setup-code escalation (S-01) is fixed in code (not yet deployed).

## Current design

### What a Project is

1. **Serves a web UI** that the app opens in a webview (`mobile/ios/Actnet/Sources/Views/Network/ProjectWebView.swift`).
2. **Owns bot accounts** — full Signal-protocol participants with their own keys, built on `@theavalanche/app-core` (`node/packages/`). Its bots register with the Project's **bot signup key**, which adminbot mints at install; the key admits bots only and links each to this Project (`24` §Trust and gating model).

Server-side, a Project is a row in `projects` (`slug`, `name`, `url`, optional token-signing key, optional OAuth client registration), with bots linked through `project_bots` (one Project per bot) and grants in `project_capabilities` (`infra/migrations/015_projects.sql`, `025_projects_oauth.sql`).

### Trust model

```
User trusts their homeserver admin
  → Admin installs and configures a Project on the homeserver
    → User implicitly trusts that Project
```

Actors:

- **User** — has an account on the homeserver, opens Project UIs.
- **Operator / admins** — decide which Projects are installed, through adminbot (`22-adminbot.md`).
- **Project service** — a separate process (and usually a separate origin) that serves web pages and runs bots.
- **Bot accounts** — registered on the homeserver, visible to every group member, holding their own keys.

**What the homeserver learns from Projects:** that a user asked for a token for a given `project_url` (`project_tokens` rows: `account_id`, `project_url`), and which accounts completed an OAuth login to which client (`oauth_grants`). It does not see webview traffic, which goes directly between the webview and the Project.

**What a Project learns:** the DID of any user whose token it verifies; the decrypted content its bots receive; whatever users submit through its UI. It cannot see conversations its bots are not in, other Projects' data, or the app's local store or keys.

**Bot visibility is a design invariant: a bot's presence in a group is always visible to all members.** There is no silent observer mode.

### Authentication: homeserver-issued Project tokens

**Built** (`core/crates/server/src/routes/projects.rs`, `infra/migrations/002_project_tokens.sql`).

```
App                           Homeserver                    Project
 │ POST /v1/project-token        │                              │
 │ (session auth) {project_url}  │                              │
 │──────────────────────────────▶│                              │
 │◀──── { token, expires_at } ───│                              │
 │ open webview: project_url/?token=…                           │
 │─────────────────────────────────────────────────────────────▶│
 │                               │ GET /v1/project-token/verify │
 │                               │◀─────────────────────────────│
 │                               │── { did, project_url } ─────▶│
```

- `POST /v1/project-token` (session-authenticated): 32 random bytes, base64url, stored with the caller's `account_id` and the **caller-supplied** `project_url`, default TTL 1 hour (`PROJECT_TOKEN_LIFETIME_SECS`). Expired rows are swept by the background task (`server/src/tasks/mod.rs`).
- `GET /v1/project-token/verify?token=…` (unauthenticated): returns `{ did, project_url }` or 401.
- Tokens are multi-use for their lifetime.
- The webview reads the token from the URL and sends it as `Authorization: Bearer` on API calls. The Project verifies it with one HTTP call and may cache `token → DID` for a few minutes.
- An OAuth access token (`25-project-login.md`) **is** a Project token, so the same `verify` serves both.

| Property | Value |
|---|---|
| Format | Opaque random 32 bytes, base64url |
| TTL | 1 hour (configurable) |
| Multi-use | Yes |
| Audience | Stored and returned by `verify`, **not enforced** by the server (see *Known gaps*) |
| Revocation | Delete the row |
| Identity disclosed | Always the real DID |

### Webview: no bridge, deep links out

**Built** on iOS, Android and Desktop. A Project page runs in a standard sandboxed webview (`WKWebView` / `WebView` / Tauri window). There is **no JS bridge**: input is URL parameters, output is navigation to an intercepted deep link.

**Canonical deep-link form: `https://go.theavalanche.net/<action>/<arg>`.** Each platform's navigation delegate matches the `go.theavalanche.net` host, cancels the navigation and routes it into the app's deep-link handler (iOS `ProjectWebView.swift` → `AppState.handleDeepLink`). Because interception is by host match, it does not depend on Universal Links firing inside the app's own webview. Routes handled today: `conversation/<did>`, `i/<token>` (legacy `invite/<token>`), and `authorize?…` (`25`). Any intercepted link also dismisses the webview, which is how a page "closes" itself. Webviews must not emit a custom scheme: although Desktop registers `avalanche://` with the OS for external launches (`desktop/src-tauri/tauri.conf.json`), the webview path is the host-matched HTTPS form.

Webview chrome always shows the Project name, so users can tell a Project view from native UI.

### Project permissions (admin-granted scopes)

The governing rule: **permissions are declared by the Project in its manifest and granted by the admin at install time**, default-deny, one permission per concrete capability. There is **no per-user runtime scope prompt.** Prompts would re-litigate the admin's decision and train reflexive "Allow"; for identity they would be theatre, since the admin's own server already knows the DID.

What the user does see:

- The **login consent screen** (`25`) — the user's act of signing in to this Project as this identity. Permissions are shown for legibility; it is not a scope approval.
- **First-use signposts** (Planned, with the surfaces in `23`) — an informational, attributed "you're opening <Project>" notice, not a grant.

Capabilities that only *look* like permissions are consented by the user's own action: sharing a profile is sharing the profile key in a message; disclosing a message to a long-press action is the tap.

All permission ids share one dot-separated `namespace.action` space, so a manifest's `permissions` array is homogeneous.

#### Server-enforced capabilities

**Built** (`core/crates/server/src/db/capabilities.rs`, `routes/admin.rs`). These gate the server's own facilities, so the server enforces them. Grants are validated against a known set, stored in `project_capabilities (project_id, capability, granted_at, granted_by)`, and resolved per bot as *account → Project → capability*. A bot in the reserved `adminbot` Project is a superuser and implicitly holds every capability (`22`).

| Capability | What it grants |
|---|---|
| `accounts.read` | The account roster (`GET /v1/admin/accounts`, DID-paginated) and the account-joined feed, live over the WebSocket and via catch-up `GET /v1/admin/events` (30-day retention). See `22-adminbot.md` §Join event API. |
| `registration.gatekeeper` | Mint signed invite tokens the server accepts under closed registration. Granting it pins the Project's Ed25519 signing key. See `24-vetted-onboarding-project.md`. |

These are the only two permissions the server knows. A manifest requesting anything else installs, but the unknown grant fails.

**Privacy posture.** The server already knows every account it registered, so showing the roster to a bot the operator installed adds no new leak; there is deliberately no group linkage (`03` §3.9 intact). A compromised `accounts.read` bot gets a real-time roster of joins with timing; the threat model accepts this. The join feed also carries the raw registration token, which today is a serious leak (see *Known gaps*).

#### The manifest document

**Built.** The manifest is what the operator's tooling reads at install time so the operator **authorizes** rather than retypes (`22-adminbot.md`, `/install-project`).

```json
{
  "slug": "beagle",
  "name": "Beagle",
  "description": "Posts a beagle fact on request.",
  "url": "https://beagle.example.org",
  "permissions": ["accounts.read"],
  "webEntries": [
    { "name": "Beagle", "url": "https://beagle.example.org/", "description": "Get a beagle fact." }
  ],
  "clientId": "beagle.example.org",
  "redirectUris": ["https://beagle.example.org/oauth/callback"]
}
```

- `slug` — stable identifier, 2–64 chars of `[a-z0-9-]`. Bot accounts link to it; its bot signup key admits them (`24`). `adminbot` is reserved (`routes/admin.rs`).
- `name` (1–100 chars), `description` (optional, shown at install).
- `url` — optional web origin; omitted for a headless bot.
- `permissions` — requested permission ids. Default-deny: the admin approves which to grant. Non-interactive installs from `ADMINBOT_MANIFEST_DIR` auto-grant everything requested except `registration.gatekeeper`.
- `webEntries` — optional Network-tab pages, stored in `directory_entries` with replace semantics and `ON DELETE CASCADE`. Untrusted input: at most 10 entries, `http(s)` URLs, name ≤ 100 and description ≤ 280 chars, control characters rejected. Always stored non-official.
- `clientId` / `redirectUris` — optional OAuth registration (`25`), stored on the `projects` row. `clientId` is unique across Projects (a duplicate fails the install); by convention it is the Project's domain. `redirectUris` is an exact-match allowlist (≤ 5). Both are self-declared and only affect this Project's own login.

The manifest is untrusted, Project-authored input: sanitize and length-limit strings and always attribute resulting surfaces to their `(server, Project)`. It is delivered out-of-band today (pasted into adminbot, or written to the manifest directory by the deploy bundle).

#### Identity is derived from the scope set, not chosen freely

Today every Project token reveals the user's **real DID**. A pseudonymous per-(user, Project) identity is only coherent for a Project that touches the user solely through its webview: any Project that talks to the user through a bot learns the real DID through the messaging channel anyway. So there are two archetypes:

- **Webview-only Projects** (compose helpers, read-only pages, landing pages) could be pseudonymous. Not built (*Speculative*).
- **Bot-bearing Projects** are always real-DID.

#### Officialness

There is no signed "official" trust primitive. The intended signal is a plain, operator-set `official` flag shown as a checkmark. In code today the flag exists only on `directory_entries` (`024_directory_entries.sql`), no write path ever sets it true, and account records carry no official flag (`routes/accounts.rs` returns `is_bot` only). So **no bot ever shows a checkmark**, and the login consent screen's badge is always off. See `54-bot-presentation.md` and *Planned*.

### Threat: malicious bot behavior

A bot is a full account. It can DM users, be added to groups, and keep whatever it decrypts. Mitigations in place: bot visibility in member lists; bot accounts render with distinct chrome (`54`); the same rate limits as any account. Bot status (`is_bot`) is self-declared at registration, so it is a label, not a guarantee (`54`).

### Threat: Project-to-Project isolation

- **Separate processes and storage.** Projects share no database or memory.
- **Separate bot accounts.** A bot belongs to at most one Project (`project_bots` primary key).
- **Origin isolation in webviews** for cookies and storage, provided each Project has its own origin. Projects behind the deploy bundle's Caddy share the homeserver origin under `/p/<name>/` paths (testbot: `/p/testbot/`), so they share an origin and therefore cookies and storage.
- **Tokens are not audience-isolated.** A Project that receives a user's token can replay it to another Project for up to an hour; the victim Project accepts it unless it checks the `project_url` returned by `verify`, which the reference testbot does not (`node/packages/testbot/src/index.ts`, `verifyProjectToken`).

### Threat: multiple homeservers and client-visible Project surfaces

Multi-account is shipped (`53`), so the client routinely holds accounts on several homeservers at once, each with its own admin and its own vetted Projects. Trust does not pool across them: server A's admin vouches only for A's Projects.

**Rule: a conversation lives on exactly one homeserver via one account, and any Project affordance shown in it comes only from that homeserver's Projects.** No surface, token or manifest may cross accounts. Today the only Project surfaces are the Network tab and deep links, and both have identity-scoping bugs (see *Known gaps*). When conversation surfaces from `23` arrive, each must be tagged with its `(account, server, Project)` and shown only there:

| Affordance | Appears in |
|---|---|
| Slash autocomplete, in-conversation entries (bot-backed) | only conversations where that Project's bot is a member |
| Participant long-press entries | only groups where that Project's bot is a member |
| Compose helpers ("+" entries, no bot) | conversations of the account whose server installed the Project |
| Custom emoji packs | conversations of the installing account's server |

Manifests, labels and assets from Projects are hostile input: sanitize, length-limit, homoglyph-guard names, size-cap and pre-fetch assets (no per-render remote loads), and never let a surface pose as native UI.

## Known gaps

Security gaps are also tracked in `09-security-posture.md`; todos in `02`.

1. **Everyone is an admin on a server running adminbot (S-29, by design).** Adminbot auto-invites every new human into `#admins`, so any member can install Projects. Intended for small new orgs; operators need onboarding that tells them how to tighten it as they grow (`22`).
2. **No token audience enforcement (P1).** `issue` accepts any `project_url`; `verify` requires no audience and checks none. Combined with the shared `/p/` origin, any Project can replay a user's token to another for an hour.
3. **Tokens travel in the URL query string.** They end up in Project access logs and browser history.
4. **Wrong identity for Projects (P1).** The Network tab mints the token from the first account on that server (`NetworkView.swift`, `openProject`), never showing which identity is used. A `conversation/<did>` deep link from any webview opens a DM from `accounts.first` (`AppState.swift`, `handleDeepLink`), so a Project on server B can start a conversation from identity A.
5. **Webview not hardened.** iOS uses a default `WKWebView` (shared default data store, no navigation lock to the Project origin, no content restrictions). Non-deep-link navigations are all allowed.
6. **Officialness is unsettable**, so the checkmark that `25`'s phishing mitigation and `54`'s impersonation defence rely on is always absent.
7. **Self-declared bots bypass the message-request gate** (`core/crates/app-core/src/messaging.rs`, `SenderGate::passes`). See `54`.
8. **The `invites.auto-accept` scope has no effect yet.** Since S-04, a group invite auto-joins only from an accepted contact or a Project bot on your server; anyone else's is a request. A Project bot's invites therefore already auto-join, so the scope would only matter for a Project that invites through a non-bot account.

## Planned

- **Token audience.** Additive: `verify` takes a required `audience` (the Project's `url`) and the server rejects a mismatch; `issue` only mints for origins of installed Projects. Update the reference Project to pass it.
- **Move tokens out of the query string** (Proposed: changes the Project interface contract; owner review), e.g. into the URL fragment, which never reaches the Project's server logs.
- **Identity scoping in the client.** Mint tokens from the account the user is viewing (and show it); route `conversation/<did>` links from a webview through the account that opened the webview.
- **Webview hardening.** A non-persistent or per-Project data store, navigation locked to the Project's origin plus an allowlist, and per-Project origins in the deploy bundle (subdomains, not `/p/` paths).
- **Provenance from installation.** Show the checkmark for a bot linked (via `project_bots`) to an installed Project the operator marked official, exposed on the account-info response. This makes the existing linkage the server-vouched signal `54` needs.
- **Manifest from a well-known URL**, same schema.

## Proposed

- **OIDC-conformant "Sign in with Avalanche" as the main developer story.** See `25` §Proposed. Needs project-owner review.

## Speculative

**Client-honored scopes.** None are implemented; the server rejects them as unknown capabilities. Kept as the vocabulary for the surfaces in `23`:

- Identity: `identity.pseudonymous` (per-Project pseudonym; webview-only Projects), `identity.real-did`, `identity.magic-links` (the tapping device mints a token for a vetted Project's link), `profile.read`.
- Messaging reach: `dm.initiate`, `dm.bypass-request` (escalated: skips the request gate), `invites.auto-accept` (same-server only; meaningful only once invites stop auto-accepting for everyone).
- Client surfaces: `surface.compose`, `surface.slash-commands`, `surface.emoji`, `message.context-on-action`, `participant.context-on-action` (bot-membership-gated; see `23`).

**Security posture of conversation surfaces** (if they are built; mechanics in `23`):

| Surface | New disclosure | Mitigation |
|---|---|---|
| Entry points ("+", message long-press) | An attributed label that could phish; a message action discloses that one message | Visible attribution; admin vetting; per-tap disclosure |
| Participant long-press | A third party's DID plus the actor's intent | Only where the Project's bot is already a member (it already holds the roster), so only intent is new |
| Magic links | Tapping mints the clicker's token; a per-share context id is a who-clicked beacon | No credential in the link; mint only for vetted Projects; coarse context ids |
| Return-content deep link | Client fetches a Project-chosen URL | https-only, size-capped, allowlisted, never auto-sent |
| Slash manifests, emoji packs | Project-authored strings and images | Attributed, sanitized, pre-fetched and size-capped |

**Profile sharing through the auth flow.** A webview-only Project with no bot (e.g. a forum) has no channel for the user's profile key. The client could pass it during token minting.

**Guest access to remote Projects.** Superseded in spirit by client-side federation (`13`): a user holds an account on each server whose Projects they use. If guests return, they get no client-visible surfaces from the remote server by default.

**A JS bridge.** Only behind its own scoped-permission system with explicit user approval.

## How to design a Project today

1. **Pick the interaction model.** Webview-only or bot-bearing. Bot-bearing Projects are real-DID.
2. **Request only what you need.** Most Projects need no server capability. `accounts.read` is for rosters and join routing.
3. **Verify every token server-side** with `GET /v1/project-token/verify`, act only on the returned DID, and **check that the returned `project_url` is yours** until the server enforces audience.
4. **Keep magic-link-style links credential-free**, and avoid per-recipient context ids that work as beacons.
5. **Run on your own HTTPS origin**, keep your own storage, and don't log message content or pass DIDs around.
6. **Attribute your UI** and never imitate native or system UI.

## Rationale and rejected alternatives

- **Opaque tokens, not JWT.** No signing key to distribute, no JWT library on the Project side, trivial revocation; the verify round-trip is cheap for web UI traffic. The format is opaque to Projects, so it can change later.
- **Rejected: reverse-proxying Projects through the homeserver with an `X-User-DID` header.** It would put all Project traffic (forms, locations) through the server in plaintext, widen the blast radius of a session token, and turn the server into a general proxy. The three-legged flow keeps the server small.
- **Admin-granted, not runtime-prompted, permissions** (see *Project permissions*).
- **One dot-separated permission namespace.** An earlier draft split scopes and capabilities with different separators, which made two near-identical concepts gratuitously inconsistent.
- **Rejected: officialness as a signed attestation.** Earlier drafts had adminbot or the server sign periodically re-issued official-bot attestations. It decomposes into a plain operator-set flag (same-server only) plus an ordinary scope; no signing, no recurring signer.
- **No in-feed interactive widgets** (`23`): the webview is the single interaction surface.
