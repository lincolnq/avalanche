# 22 — Adminbot

> **Status:** Partial — adminbot runs on every deployment: the `#admins` group, auto-invites, an expiry cap, update checks, and manifest-based Project install are built. Rule-based routing, the full command surface, officialness, and join-event catch-up are not. Superuser bootstrapping has a P0 escalation bug.
> **Last verified against code:** 2026-10-03

## Summary

Adminbot is the first-party Project that administers a homeserver through chat. Two foundations:

1. **Superuser authority is membership in the reserved `adminbot` Project.** Every `/v1/admin/*` endpoint requires the caller to be a bot linked to that Project.
2. **The `#admins` group is the admin roster.** It is an ordinary E2E group; its membership *is* the set of human administrators. The server can't read it, so the **server database doesn't reveal who has admin authority**. Humans post commands; adminbot checks the sender is in `#admins` and acts through its superuser endpoints.

Adminbot is a Node/TypeScript process (`node/packages/adminbot/src/index.ts`) on `@theavalanche/app-core`. The deploy bundle runs it on the homeserver box with the master registration secret.

## Current design

### The model: two authorities, and why adminbot is privileged

- **Operator authority** — install a Project, link its bots, grant server capabilities, mark it official. Not seizure-sensitive (installed Projects are public by construction), so it lives in the server database.
- **Social admin authority** — who may moderate, kick, or add people to channels. Seizure-sensitive ("who can target whom"), so it lives in encrypted `#admins` membership.

> **The threat decides the home.** Access to the server's own resources → a server-enforced capability. Seizure-sensitive social authority → E2E state. A trust signal that must be verified offline or across servers → a signature rooted in a cold key.

The server can't see who the admins are, so something must bridge "an admin authorized this" (E2E) to "perform this privileged action" (server). That bridge has to read `#admins`, so it must be a member, and the server has to trust it. Adminbot is that bridge. Humans never hold superuser; they exercise it through adminbot. The price is concentration: a compromised adminbot is a compromised server. So adminbot holds only the specific endpoints it needs, and privileged commands are issued in `#admins`, where they are legible in group history.

### Superuser authority

**Built**, with a P0 flaw (*Known gaps*).

- The server seeds a reserved Project with slug `adminbot` at startup (`core/crates/server/src/main.rs` → `db::projects::ensure_adminbot_project`).
- `AuthAdminbot` (`core/crates/server/src/middleware/auth.rs`) admits a session only if its account is linked to that Project. Authority is the link, not a DID.
- The admin API refuses to link or unlink bots on the `adminbot` Project and refuses to install or uninstall it (`routes/admin.rs`, `resolve_mutable_project`). **The only way into the superuser Project is registering with a bootstrap token that names it** (`routes/registration.rs`, `gate_registration`). A comment in `auth.rs` mentions seeding from an `ADMINBOT_DIDS` config; no such code exists.
- On first run adminbot registers as `did:local:adminbot` with a bootstrap token `{s: server_url, k: REGISTRATION_SHARED_SECRET, p: "adminbot"}` (`AppCore.bootstrapToken`), which links it into the superuser Project.
- The bootstrap secret is honored only while **no** `registration.gatekeeper` Project is installed (`gate_registration`). After that, only signed gatekeeper invites admit registrations.

### Coordination is data-carried, not bot-to-bot

Adminbot calls no other bot and exposes no API to them. Coordination rides durable data: server events, signed tokens and catch-up. Adminbot learns of new accounts from the `AccountJoined` push and can read the registering token to decide where to route people (`24`). Bots depend on the server and on signed artifacts, never on each other's uptime.

### Deployment shape

Adminbot has **no web UI and no inbound network surface**. It is an ordinary client that opens an outbound WebSocket and HTTP connection to the homeserver, so it can run anywhere with outbound connectivity.

That makes **off-box** operation possible and attractive: nothing public routes to it (its `did:local:` DID has no PLC endpoint, and it serves no origin), so an adversary has to seize the homeserver and then trace a source IP to find it. Its private keys stay off the seized box too.

**In practice the deploy bundle runs adminbot on the homeserver box** (`infra/deploy/bundle/lib/common.sh`, `write_bot_env`) and relies on it for startup manifest installs. On a standard install, seizing the server seizes adminbot. Off-box operation is supported but not the default. The same location-independence also argues against making anything depend on adminbot's uptime; capability records and the directory live on the server.

### What adminbot does today

- **`#admins @ <hostname>`** is created at bootstrap. DIDs in `ADMINBOT_INITIAL_ADMINS` are invited.
- **Auto-invite.** On each live `AccountJoined` push for a human account, adminbot invites the new account into every group where it is currently an admin, `#admins` included. Any group that promotes adminbot to admin thereby becomes an onboarding target. Bots are never auto-invited.
- **Bot announcements.** New bot accounts are announced in `#admins` with a contact card, except display names in `UNANNOUNCED_BOT_NAMES` (`Testbot`).
- **Expiry cap.** When added to a group as admin, adminbot clamps the disappearing-message timer to at most 4 weeks, treating "off" as exceeding the cap. Later timer changes are only caught by `/audit`.
- **Update check.** Daily and at startup, it compares the deployment's `VERSION` file with the latest GitHub release and posts to `#admins` once per new release.
- **Manifest install at startup.** Every `*.json` in `ADMINBOT_MANIFEST_DIR` (default `<dirname(state dir)>/manifests`) is installed non-interactively, auto-granting every requested permission except `registration.gatekeeper`. This is how the deploy bundle configures web Projects.
- **State:** `ADMINBOT_STATE_DIR` holds the SQLCipher store and a `state.json` sidecar. Losing it means re-registration, which needs the old `did:local:adminbot` account row deleted server-side first.

### Commands

Accepted in `#admins` and in 1:1 DMs.

| Command | Gate | Effect |
|---|---|---|
| `/whoami`, `/help` | none | Echo DID; help text |
| `/audit` | **none** | Refresh every group adminbot is in; report admin status, counts and timer; clamp timers |
| `/check` | none | Check for a newer release |
| `/install-project` | `#admins` member | DM interview: paste a manifest (or URL), authorize permissions and web entries, install |
| `/list-projects` | `#admins` member | List installed Projects, capabilities, bots |

Confirmations are typed (`yes`); the Node layer can't receive reaction events yet. Membership is checked against freshly fetched `#admins` state (`requireAdminsMember`).

### Installing a Project

**Built.** `/install-project` reacts with an eyes emoji, DMs the operator to paste a manifest (schema in `20-project-security.md` §The manifest document), shows requested permissions and web entries in plain language, then:

1. `POST /v1/admin/projects` — create, or update an existing slug (install is an upsert, including OAuth registration).
2. `POST /v1/admin/capabilities` for each approved permission.
3. `PUT /v1/admin/projects/{slug}/directory` for the manifest's `webEntries` (replace semantics, stored non-official).
4. DM back a **setup code** for the Project's bot: a bootstrap token `{s, k: REGISTRATION_SHARED_SECRET, p: <slug>}`. A bot registering with it is linked to the Project.

It flips the reaction to a check or a cross when done. There is no `PROJECTS` env var: a Project appears in the Network tab only through a manifest install.

### Project capabilities

The catalog of what a Project may request and how it is enforced lives in `20-project-security.md` §Project permissions. Adminbot is the only grantor; `granted_by` records its DID so a grant can be cross-referenced with the `#admins` thread that authorized it. Adminbot holds every capability implicitly through the superuser link.

### Join event API (push + catch-up)

**Built** server-side (`routes/registration.rs`, `routes/admin.rs`, `infra/migrations/017_server_events.sql`).

- **Push.** On every registration the server sends `AccountJoined { did, joined_at_ms, invite_token }` over the WebSocket to every connected session holding `accounts.read`.
- **Durable log.** The same event is appended to `server_events` (30-day retention, swept in `server/src/tasks/mod.rs`).
- **Catch-up.** `GET /v1/admin/events?since=<id>&kind=account_joined` (500 per page) for any bot holding `accounts.read`.
- **Roster snapshot.** `GET /v1/admin/accounts?after=<did>` returns `{ accounts: [{did, display_name?, is_bot, created_at_ms}], next }`, gated on the same capability.

**Adminbot uses only the live push.** Neither app-core nor adminbot calls the catch-up endpoint, so joins that happen while adminbot is disconnected are never routed.

### Privacy posture

The server already knows every account it registers, so showing that to a bot the operator installed adds no new leak, and there is deliberately no group linkage (`03` §3.9 intact). A compromised `accounts.read` bot gets a real-time roster of joins with timing; the threat model accepts this. **Exception:** the events carry the raw `invite_token`, which today can contain the master secret (*Known gaps*). Planned: carry parsed issuer and routing claims only.

### `did:local:` DID scheme

**Not built as decided.** Bot accounts without a PLC DID get `did:local:<suffix>`: either a caller-chosen suffix (3–32 lowercase alphanumerics, first come first served) or one derived from the identity key (`registration.rs`, `generate_local_did`). Adminbot still registers the fixed literal `did:local:adminbot`.

**Decided: random per-server DIDs, no well-known literal.** The fixed literal is the same string on every server, but the client's per-identity store keys contacts, profiles and conversations by DID. A user on two servers therefore merges the two adminbots into one row: block one and you block both, and their DMs interleave (`37`). Random DIDs make each server's adminbot a distinct identity. Server authorization is unaffected, since authority is the Project link. Finding "this server's adminbot" without a literal (role discovery) is deferred until a concrete need appears.

**Rejected:** a host-scoped shape such as `did:local:{hostname}:adminbot` (couples identity to a hostname), and client-side conversation-key rewriting (fixes one table, leaves the footgun for the next).

## Known gaps

Security items are also in `09-security-posture.md`; todos in `02`.

1. **Setup codes grant superuser (P0).** A setup code is `base64url(JSON)` containing the master `REGISTRATION_SHARED_SECRET` (`index.ts`, `performInstall`; token format in `server/src/invite_token.rs`, `BootstrapToken`). Anyone holding one, i.e. any Project operator, can decode it, set `p` to `"adminbot"`, register, and be linked into the superuser Project (`gate_registration`). That is full server admin.
2. **The master secret leaks through join events (P0).** The raw registration token goes into `server_events` and to every `accounts.read` holder (`20` §Known gaps). Every bot registered with a setup code, or with testbot's plain bootstrap token, publishes the secret.
3. **The secret has a silent cliff.** Installing any gatekeeper retires the bootstrap path, so new Project bots can no longer register with setup codes, and adminbot can't re-register.
4. **`/audit` has no `#admins` check.** Anyone who can DM adminbot gets a listing of every group adminbot is in (titles, counts, timers) and triggers timer clamps.
5. **No catch-up.** Joins while adminbot is down are never routed (*Join event API*).
6. **Fixed `did:local:adminbot`** still merges adminbots across servers in multi-homed clients.
7. **Stale comment:** `ADMINBOT_DIDS` in `middleware/auth.rs` describes config seeding that doesn't exist.

## Planned

- **Bot enrollment tokens (fixes 1–3).** The server mints per-Project, single-use, short-lived enrollment tokens (`purpose: "bot"`, redeemed through `token_redemptions` like gatekeeper invites). Adminbot requests one via a new admin endpoint and hands it out instead of a bootstrap token. They work whether or not a gatekeeper is installed, and can never name the `adminbot` Project. Adminbot bootstraps itself from an operator-only path: the shared secret, scoped by the server to the `adminbot` Project and refused once adminbot exists, or a one-shot operator command on the box. Testbot gets an enrollment token too. Then remove `REGISTRATION_SHARED_SECRET` from every bot env except adminbot's first run.
- **Join events carry parsed claims, not raw tokens:** issuer slug, purpose and routing tags.
- **Gate `/audit`** on `#admins` membership.
- **Use catch-up:** persist the last processed event id and drain `GET /v1/admin/events` on connect.
- **Random `did:local:` for adminbot**, per the decision above.
- **Officialness.** An operator command sets an `official` flag for a Project; clients show the checkmark for that Project's linked bots and directory entries (`20` §Planned, `54`).
- **Uninstall and revoke from chat** (`/uninstall-project`, `/revoke`), reaction-based confirmation for destructive commands once the Node layer gets reaction events.

## Speculative

- **Rule-based routing config.** Per-server rules mapping invite-token tags to channels with default notification levels, editable from chat (`/add-rule`). This is the central-routing half of `24`'s post-join hand-off.
- **Default notifications on accept.** An E2E hint inside the invite's `GroupContext` ("muted for 7 days"), applied on accept. Always a hint, never a command.
- **Security-update awareness** beyond the version check: a curated or signed security manifest with severity-based nagging.
- **Fuller command surface:** `/grant`, `/revoke`, `/officialize`, `/pause`, `/kick <did> from <group>`, `/add`, `/seed-into <group>`.
- **Leave/rejoin for `#admins`** (DM a leaver a `/rejoin` path) and an **official-groups registry** to protect the reserved `#admins` title.
- **Recovery ladder:** restart from state → rotate keys (authority is the Project link, not a key) → re-bootstrap with a fresh DID → recreate `#admins` and re-invite.
- **Backup recovery identity** for the "every admin left" case. Today the answer is operator shell access.

## Rationale and rejected alternatives

- **Rejected: a bot-to-bot RPC / service mesh with discovery.** It makes every bot depend on every other bot being live. Everything needed so far fits data-carried coordination. A genuinely synchronous need should be one Project calling another's ordinary HTTP API with its own auth, an explicit trust edge, not an ambient mesh.
- **Rejected: the gatekeeper asking adminbot to add a user to channels.** Imperative cross-bot RPC; the token carries routing tags instead (`24`).
- **Rejected: officialness as a signed attestation.** Decomposes into a plain operator-set flag (same-server only) plus an ordinary scope (`20`).
- **Rejected: per-admin server-verified credentials instead of the delegate.** The server would see which admin DID made each privileged call, accumulating a partial roster and eroding the property `#admins` protects. The delegate keeps the roster invisible.
- **Non-goals.** Adminbot is not a general bot framework, not an RPC hub, and not federation-aware. It can only add people to groups it is itself an admin of, under the group's normal policy.
