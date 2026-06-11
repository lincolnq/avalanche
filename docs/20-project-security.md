# Project Security Model

This document describes the security model for Projects — standalone services that serve web UIs and operate bot accounts on the actnet platform.

## What a Project is

A Project is a standalone service that:

1. **Serves a web UI** that the mobile app opens in a webview.
2. **Owns bot accounts** that are full Signal protocol participants — they register on the homeserver, hold their own identity keys, and send/receive encrypted messages like any other account.

Because all groups and DMs are E2E encrypted, any Project that touches message content or manages group membership **must** operate through bot accounts. The homeserver cannot mediate these operations — it doesn't have keys. This means every non-trivial Project follows the same pattern: a standalone service with bots.

## Trust model

### The trust chain

```
User trusts their homeserver admin
  → Admin installs and configures a Project on the homeserver
    → User implicitly trusts that Project
```

This is analogous to a Slack workspace admin installing apps. The admin is the gatekeeper; users trust the admin's judgment.

### Who are the actors?

- **User**: has an account on the homeserver, uses the mobile app, opens Project UIs.
- **Homeserver admin**: controls which Projects are available, configures them.
- **Project service**: a standalone process that serves web pages and operates bot accounts. Runs in the admin's trust domain (same server, same infrastructure).
- **Bot accounts**: registered on the homeserver by the Project service. Visible to all group members. Hold their own Signal keys.
- **Attacker**: anyone not in the trust chain — other users, external actors, compromised services.

### What the homeserver knows

The homeserver sees routing metadata (who messages whom, when, device IPs) but cannot read message content. This is unchanged by Projects — bot accounts are just accounts from the homeserver's perspective.

The homeserver also knows which users have requested Project tokens (via `POST /v1/project-token`), revealing that the user opened a specific Project. It does not see what the user does within the Project after that — all subsequent traffic goes directly between the webview and the Project service.

### What a Project knows

A Project sees:
- The DID of users who interact with it (from verified Project tokens).
- The decrypted content of messages its bots receive (the bot has keys).
- Whatever state users provide through its web UI (form submissions, location data, etc.).

A Project cannot see:
- Messages in groups/DMs where it has no bot.
- Other Projects' data.
- The user's local database, keys, or conversations.

Bot visibility is a critical design invariant: **a bot's presence in a group is always visible to all members.** There is no silent observer mode. If a Project is reading your messages, you can see its bot in the member list.

## Authentication: homeserver-issued Project tokens

### The problem

The Project serves a web UI and an HTTP API. The mobile app opens the web UI in a webview. The Project needs to know the user's identity (DID) in a way that can't be spoofed. DIDs are public identifiers — anyone who knows a DID could call the Project's API and impersonate that user.

**Attack scenarios without authentication:**
- **Spam**: call the chatbot's `text-me` endpoint with a victim's DID, flooding them with bot messages.
- **Impersonation**: sign someone up for a team, upload fake location data as them, trigger actions on their behalf.
- **Resource exhaustion**: create thousands of bots by hitting the API repeatedly.

### The solution: opaque tokens with a verification endpoint

The homeserver is already the auth authority. Before opening a Project webview, the app requests a short-lived, Project-scoped token from the homeserver. The Project verifies this token with the homeserver before acting on any request.

### Flow

```
Mobile App                    Homeserver                   Project Service
    │                              │                              │
    │  POST /v1/project-token      │                              │
    │  Auth: Bearer <session>      │                              │
    │  { project_url: "..." }      │                              │
    │─────────────────────────────▶│                              │
    │  { token: "x9f2k..." }      │                              │
    │◀─────────────────────────────│                              │
    │                              │                              │
    │  Open webview: project_url/?token=x9f2k...                  │
    │─────────────────────────────────────────────────────────────▶│
    │                              │                              │
    │                              │  GET /v1/project-token/verify│
    │                              │  ?token=x9f2k...             │
    │                              │◀─────────────────────────────│
    │                              │  { did: "did:plc:abc",       │
    │                              │    project_url: "..." }      │
    │                              │─────────────────────────────▶│
    │                              │                              │
    │                     200 OK (web page / API response)        │
    │◀────────────────────────────────────────────────────────────│
```

### Homeserver implementation

**New table:**
```sql
CREATE TABLE project_tokens (
    token       TEXT PRIMARY KEY,
    account_id  BIGINT NOT NULL REFERENCES accounts(id),
    project_url TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL
);
```

**`POST /v1/project-token`** (authenticated — existing session token middleware):
- Input: `{ "project_url": "http://localhost:3001" }`
- Generate 32 random bytes, base64url encode.
- Store in `project_tokens` with the user's account ID and 1-hour expiry.
- Return: `{ "token": "x9f2k...", "expires_at": "..." }`

**`GET /v1/project-token/verify?token=x9f2k...`** (unauthenticated):
- Look up token in `project_tokens`.
- If valid and not expired: join with `accounts` to get DID, return `{ "did": "did:plc:abc", "project_url": "http://localhost:3001" }`.
- If invalid or expired: return 401.

Add expired-token cleanup to the existing background garbage-collection task.

### Token properties

| Property | Value | Rationale |
|----------|-------|-----------|
| Format | Opaque (random 32 bytes, base64url) | No crypto libraries needed on the Project side |
| TTL | 1 hour | Long enough for a webview session; short enough to limit leaked-token damage |
| Multi-use | Yes | The webview makes many API calls per session |
| Scoped to Project URL | Yes (stored, for future enforcement) | Prevents cross-Project token reuse |
| Revocation | Delete from table | Trivial with opaque tokens |

### How the web page uses the token

1. The webview opens `http://project-url/?token=x9f2k...`.
2. The web page's JavaScript reads the token from the URL query parameter.
3. On all subsequent API calls, the page includes it as `Authorization: Bearer x9f2k...`.
4. The token appears in the URL once (initial page load). This is acceptable — the URL is not shared or logged outside the app.

### How the Project verifies the token

On each API request:
1. Read the token from the `Authorization: Bearer` header.
2. Call `GET http://homeserver:3000/v1/project-token/verify?token=<token>`.
3. If 200: proceed with the DID from the response.
4. If 401: reject the request.

The Project can cache `token → DID` mappings for a few minutes to avoid a round-trip on every request. The token is valid for an hour, so caching for 5 minutes is safe.

**For Project developers, the entire auth implementation is one HTTP call.** No crypto, no JWT parsing, no shared secrets.

### Why opaque tokens (not JWT)

- No signing key to manage or distribute to Projects.
- No JWT library needed on the Project side.
- Revocation is trivial (delete from DB).
- The verification round-trip adds negligible latency for web UI interactions.
- Can upgrade to JWT later without changing the external flow — the token format is opaque to the Project either way.

### Why not proxy through the homeserver

An alternative design: the homeserver acts as a reverse proxy for Projects, forwarding requests with an `X-User-DID` header. This eliminates the three-legged auth flow.

We chose **not** to do this because:

- **Metadata exposure**: the homeserver would see all Project traffic (form submissions, location data, page views). The design is optimized for server seizure — the homeserver should learn as little as possible.
- **Plaintext channel**: the proxy introduces a new path where user data flows through the homeserver in plaintext. Encrypted DMs are opaque to the server; Project web traffic through a proxy would not be.
- **Blast radius**: a single session token would grant access to messaging AND all Projects. Scoped Project tokens limit exposure.
- **Attack surface**: the homeserver stays a focused messaging server, not a general-purpose reverse proxy.
- **Single point of failure**: the homeserver doesn't need to handle Project web traffic.

The three-legged approach keeps the homeserver small. The only new surface is two endpoints (issue and verify tokens).

## Threat: webview capabilities

### What the webview can do

The Project's web page runs in a `WKWebView` (iOS) / `WebView` (Android). Standard webview sandboxing applies:

**Can:**
- Execute arbitrary JavaScript.
- Make network requests to any origin (fetch, XHR, WebSocket).
- Store data in cookies and localStorage (scoped to the webview's origin).
- Display any UI (HTML/CSS).

**Cannot:**
- Access the device filesystem.
- Access the native app's data (SQLCipher DB, keys, conversations).
- Call native APIs (no JS bridge — through Stage 6 the design stays bridgeless: URL params in, deeplinks out; see `23-messaging-extensions.md`).
- Access other Projects' webview storage (origin isolation).

### Risks and mitigations

| Risk | Severity | Mitigation |
|------|----------|------------|
| Project tracks user activity | Low | DID is already public; this is expected behavior. Users choose to open a Project. |
| Project phones home with DID | Low | Same as above. The trust chain assumes the admin vetted the Project. |
| Project phishes user (mimics native UI) | Medium | Webview has visible chrome/header identifying it as a Project view, not native app UI. The user always knows they're in a webview. |
| XSS in Project's web page | Medium | The Project's problem, not the platform's. The webview sandbox limits blast radius — XSS can't escape to the native app. |
| Project serves malicious JS that exploits webview engine | Low | Keep OS/webview up to date. Standard platform security. |

### No JS bridge (for now)

The web page has no bridge to the native app. All actions go through the Project's own HTTP backend, which then operates through bot accounts. The way the web page gets the user back to the app is by opening a deep link for navigation in the app.

This is a deliberate security choice. A JS bridge would dramatically expand the attack surface. If a JS bridge is ever added, it must be gated by a scoped permission system: the Project declares what native capabilities it needs, the user explicitly approves, and the bridge only exposes approved capabilities. 

## Messaging-extension surfaces (Stage 6+)

`23-messaging-extensions.md` is the catalog of surfaces by which Projects extend messaging, and where the core-vs-Project line falls. This section records their **security posture**; it does not repeat the mechanics.

First, the boundary. Most features `23` discusses are **core, not Project** — reactions, `@`-mentions, simple polls, generic link unfurling, live location. These sit **outside the Project trust boundary**: there is no Project API to them, and a Project can neither read, mediate, nor inject them. A member bot sees reactions/replies/messages in a conversation it belongs to *exactly as any member does* (the visible-bot model above) — that is the only way a Project touches them, and it grants no new capability. "Lightweight bot actions" (react ✅ to approve) ride on this and add nothing to the attack surface.

The genuinely Project-facing surfaces, and what each newly exposes:

| Surface (see `23`) | New disclosure / trust | Mitigation |
|---|---|---|
| **Entry points** (`+` menu, message long-press, deep-link launcher) | A registered entry point is an attributed label that could phish ("Verify your account"); a message-action discloses *that one message* to the Project | Entries visibly attributed to their Project; admin vetting; message-action disclosure is per-message, consent-gated by the tap (signpost on first use per Project); a launcher may only open its owning Project (no open-redirect) |
| **Webview I/O** (URL params in, deeplinks out) | The return-content deeplink makes the client fetch a webview-chosen URL (SSRF/privacy) and proposes a message/attachment the user then sends | **No bridge** = no native API surface; the fetch is https-only, size-capped, origin-allowlisted (sender-side, like on-device unfurl); return-content is **proposed, never sent silently**; conversation posts go through the bot server-side. Inbound params never carry E2E data |
| **Slash-command manifest** | A bot advertises commands; autocomplete text is Project-supplied (mild phishing via misleading descriptions) | No bridge and no new wire format — a slash command is a plain message the bot reads (it's a member; expected). Attributed to the bot; manifest vetted by the admin |
| **Custom emoji / reaction asset pack** | Project-supplied images in the reaction picker (offensive content; remote-load tracking; oversized assets) | Fetched/cached like attachments (no per-render remote load), size-capped, admin-vetted |
| **Rich text authored by bots** | Link spans can spoof (display text ≠ destination) | Anti-spoof rule from `35-attachments.md` (show the real URL); render only the fixed inline style set |

The recurring theme: every Project-facing surface is an **explicit, attributed, consent-gated handoff**, and none grant a Project access to conversation content it isn't already a member of. The privacy-sensitive primitives stay in core, out of reach.

## Threat: malicious bot behavior

### The problem

A bot is a full account with its own keys. Once registered, it can:
- Send DMs to any user on the homeserver.
- Be added to groups (by an admin or through the Project framework).
- Accumulate and exfiltrate decrypted message content.

### Mitigations

**Bot visibility:** Bots are visible in every group they join. Users can see which bots are present and can leave groups with bots they don't trust. There is no hidden observer mode.

**Bot account marking:** Bot accounts should be distinguishable from human accounts. The homeserver can mark accounts as bot-owned (a flag set at registration time by the Project). The mobile app displays this clearly in the member list and conversation view.

**Rate limiting:** The homeserver applies the same rate limits to bot accounts as human accounts. A bot that spams messages gets throttled or suspended like any other account.

**Scope limitations (future):** In the full Project framework (Stage 6), bots would operate under scoped permissions — a bot might be allowed to read messages in specific groups but not send unsolicited DMs. For now, bots are just accounts with no special restrictions beyond the rate limits that apply to all accounts.

## Threat: Project-to-Project isolation

### The problem

If multiple Projects run on the same homeserver, can one Project interfere with another?

### Mitigations

**Separate processes:** Each Project is a standalone service. They share no state, no memory, no database. They communicate only through the homeserver's public API.

**Separate bot accounts:** Each Project's bots are distinct accounts. One Project cannot control another Project's bots.

**Origin isolation in webviews:** Each Project's web UI runs on a different origin (different host/port). Webview storage (cookies, localStorage) is isolated per origin.

**Scoped tokens:** A Project token issued for Project A cannot be verified by Project B — the token is scoped to a specific `project_url`, and the verification response includes this URL so the Project can check it matches.

**No shared API surface:** Projects have no way to discover or interact with each other except through the same mechanisms available to any user (sending messages, looking up DIDs).

## Threat: multiple homeservers and client-visible Project surfaces

Two developments make this bigger than the original "guest access is a Stage-9 federation problem" framing:

1. **Multi-account is here now.** Per `53-multi-account-ux.md`, a user can be logged into several homeservers at once, each a first-class account. So the client routinely **holds and renders Project surfaces from multiple trust domains simultaneously** — this is a normal state, not a deferred edge case.
2. **Projects now expose client-visible surfaces.** Per `23-messaging-extensions.md`, Projects advertise **slash-command manifests, entry-point labels/icons, custom-emoji packs, and bot-authored rich text** — data and assets the *client* parses and renders, not just bot messages a bot sends. That is a new surface reaching the client.

These two compound: client-rendered, Project-authored content, sourced from several servers of differing trust, shown side by side in one app.

### Three relationships, each × every homeserver you're on

- **Your homeserver's Projects (vetted).** Admin-vetted, full trust per the trust chain above — but you now have *one such chain per homeserver you hold an account on*. Trust does not pool across them: server A's admin vouches for A's Projects only.
- **Guest on a remote homeserver (Stage 9, deferred).** You participate in a Project/group on a server where you have **no account**, via a guest credential. That admin is not in your trust chain → untrusted.
- **No relationship → no reach.** A Project you've never encountered cannot project anything into your client; surfaces are gated (next).

### Gating: client-visible surfaces follow bot visibility / explicit invocation

The core invariant — *a Project only sees a conversation where its bot is a visible member* — extends to **what a Project can render in your client**. A Project may project UI into the client only where it has a **visible bot member** of that conversation (in-conversation slash autocomplete, entry points) or where the **user explicitly invoked** it (a compose helper like Giphy). A foreign Project cannot inject a slash command, entry point, or emoji into a conversation it isn't a member of. The new surfaces are therefore bounded by the same per-conversation visible-membership rule that already bounds message access.

### Appearance scope — where each affordance can show up

Bot-membership gates the bot-backed surfaces, but a **compose helper has no bot** (Giphy is invoked from "+", not a member of anything), so its appearance must be defined explicitly. The rule: **a conversation lives on exactly one homeserver via one account, and the affordances available in it come only from *that homeserver's* Projects.** Switching to a different account's conversation swaps the affordance set; nothing bleeds across accounts or servers.

| Affordance | Appears in | Scope |
|---|---|---|
| Slash autocomplete, in-conversation entry points (bot-backed) | only conversation(s) where that Project's **bot is a member** | per-conversation (⊂ one account) |
| Compose helpers / "+" entries (no bot — Giphy, meme maker) | the **"+" menu of conversations on the account/homeserver where the Project is installed** | per-account |
| Custom emoji / reaction packs | the **reaction picker in conversations on the installing account's homeserver** | per-account |
| Bot-posted content & rich text | wherever that **bot is a member** | per-conversation |

**The motivating example:** Giphy installed on homeserver A → its "+" entry appears in your **account-A** conversations only. Composing in an account-B (homeserver B) conversation shows server B's helpers, **not** A's Giphy — B's admin never vetted it, and the affordance does not follow you across servers. A pure compose-helper leaks little even if it did bleed (Giphy would see only "account A opened me, picked GIF X," never the B conversation), but per-account scoping keeps trust attribution clean and the "+" menu predictable. A cross-account "use everywhere" opt-in could be offered later — explicit and attributed, never a silent default.

### Scoping & isolation (needed now — Stage 6 / multi-account, not Stage 9)

This work lands with multi-account, ahead of federation:

- **Per-(account, server, conversation) scoping.** Every client-visible surface is tagged with where it came from and shown only there. A Project on server A must never appear in — or influence the rendering of — a conversation belonging to account B. No global, co-mingled command palette or entry-point list spanning accounts.
- **Per-account fetch.** Manifests, entry-point lists, and emoji packs sync over the **owning account's own connection**. Server A's Projects must learn nothing about account B's activity, and the client must not leak one account's state to another.
- **Manifests are untrusted input.** Command names/descriptions, entry-point labels, and emoji names/assets are Project-authored strings and binaries the client displays. Treat them as hostile: sanitize and length-limit text; guard against homoglyph/Unicode spoofing in command and Project names; size-cap and lazily fetch assets (no per-render remote load → no tracking beacon); rate/size-limit manifest sync (DoS); and **always attribute a surface to its (server, Project)** so one Project can't impersonate another or pose as native/system UI.

### Guest access (Stage 9, still deferred — but sharper now)

When the user is a *guest* on a remote homeserver (no account there), the remote admin isn't in the trust chain, so on top of the scoping above:

- Guest sessions get **reduced or no client-visible surface** from remote Projects — at most, surfaces the user explicitly opts into, clearly marked "remote / not vetted by your admin."
- A **guest credential** issued by the user's own homeserver vouches "valid user" without exposing the real DID; the homeserver-as-proxy / pseudonymous-DID option keeps the remote Project from learning identity.
- The remote Project accepts the credential and grants scoped access; the UX makes the remote, untrusted nature explicit.

## Summary: what we build now

For the chatbot Project (and the first iteration of the Project model):

1. **Homeserver Project token endpoints** — `POST /v1/project-token` and `GET /v1/project-token/verify`. Opaque tokens, 1-hour TTL, stored in DB.
2. **Project verifies tokens** on all API calls via one HTTP call to the homeserver. No unauthenticated actions.
3. **No JS bridge.** Web pages talk to their own backends only.
4. **Bot accounts marked as bots** at registration time. Displayed distinctly in the mobile app.
5. **Visible webview chrome.** Users always know when they're in a Project webview vs. native UI.
6. **Origin isolation.** Each Project on a different origin.

Items deferred:
- Scoped permissions for bots (Stage 6).
- Messaging-extension surfaces — entry points, slash-command manifests, custom-emoji packs, bot rich text — and their scopes (Stage 6); see `23-messaging-extensions.md` and *Messaging-extension surfaces* above.
- Webview return-content via intercepted deeplink + sender-side fetch — **no JS bridge** (Stage 6); specced in `23-messaging-extensions.md`.
- Per-(account, server, conversation) scoping and isolation of client-visible Project surfaces, and treating Project manifests as untrusted input (Stage 6, with multi-account); see *Threat: multiple homeservers and client-visible Project surfaces* above.
- Token scoping enforcement on verify endpoint (v2).
- Guest access to remote Projects, incl. reduced client-visible surface and pseudonymous credentials (Stage 9).
