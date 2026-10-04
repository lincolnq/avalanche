# 22 — Adminbot

> **Status:** Partial — adminbot runs on every deployment: the `#admins` group, auto-invites, an expiry cap, update checks, and manifest-based Project install are built. Rule-based routing, the full command surface, officialness, and join-event catch-up are not. The superuser bootstrap was rebuilt to close the S-01 escalation (branch `lincoln/bot-signup-keys`, pending merge). By design, every human who signs up joins `#admins` and can use admin commands (S-29).
> **Last verified against code:** 2026-10-03

## Summary

Adminbot is the first-party Project that administers a homeserver through chat. Two foundations:

1. **Superuser authority is membership in the reserved `adminbot` Project.** Every `/v1/admin/*` endpoint requires the caller to be a bot linked to that Project.
2. **The `#admins` group is the admin roster.** It is an ordinary E2E group; its membership *is* the set of human administrators. The server can't read it, so the **server database doesn't reveal who has admin authority**. Humans post commands; adminbot checks the sender is in `#admins` and acts through its superuser endpoints.

Adminbot is a Node/TypeScript process (`node/packages/adminbot/src/index.ts`) on `@theavalanche/app-core`. The deploy bundle runs it on the homeserver box with the superuser bootstrap secret, which is generated on the box.

## Current design

### The model: two authorities, and why adminbot is privileged

- **Operator authority** — install a Project, link its bots, grant server capabilities, mark it official. Not seizure-sensitive (installed Projects are public by construction), so it lives in the server database.
- **Social admin authority** — who may moderate, kick, or add people to channels. Seizure-sensitive ("who can target whom"), so it lives in encrypted `#admins` membership.

> **The threat decides the home.** Access to the server's own resources → a server-enforced capability. Seizure-sensitive social authority → E2E state. A trust signal that must be verified offline or across servers → a signature rooted in a cold key.

The server can't see who the admins are, so something must bridge "an admin authorized this" (E2E) to "perform this privileged action" (server). That bridge has to read `#admins`, so it must be a member, and the server has to trust it. Adminbot is that bridge. Humans never hold superuser; they exercise it through adminbot. The price is concentration: a compromised adminbot is a compromised server. So adminbot holds only the specific endpoints it needs, and privileged commands are issued in `#admins`, where they are legible in group history.

### Superuser authority

**Built** (branch `lincoln/bot-signup-keys`; closes S-01).

- The server seeds a reserved Project with slug `adminbot` at startup (`core/crates/server/src/main.rs` → `db::projects::ensure_adminbot_project`).
- `AuthAdminbot` (`core/crates/server/src/middleware/auth.rs`) admits a session only if its account is linked to that Project. Authority is the link, not a DID.
- The admin API refuses to link or unlink bots on the `adminbot` Project, refuses to mint a bot signup key for it, and refuses to install or uninstall it (`routes/admin.rs`, `resolve_mutable_project`). A comment in `auth.rs` mentions seeding from an `ADMINBOT_DIDS` config; no such code exists.
- **The only way into the superuser Project is a one-time claim.** A registration presenting a bootstrap token `{s, k: SUPERUSER_BOOTSTRAP_SECRET, p: "adminbot"}` is linked into it, but only while the Project has no linked account (`routes/registration.rs`, `gate_registration`; `db::projects::link_bot_if_unclaimed`). A second claim is refused even with the right secret. The claim works whether or not a gatekeeper is installed.
- `SUPERUSER_BOOTSTRAP_SECRET` is the operator's root credential. The deploy generates it on the box and writes it only to the server's and adminbot's env (`infra/deploy/bundle/lib/common.sh`, `migrate_env_files`). It never appears in the configure tool's output, the cloud-init, or any invite (`42`).
- The shareable `REGISTRATION_SHARED_SECRET` (the configure tool's first-members invite) can never link a Project: any bootstrap token carrying it plus a `p` gets a 403.
- On first run adminbot registers as `did:local:adminbot` and claims superuser with `AppCore.bootstrapToken(server, SUPERUSER_BOOTSTRAP_SECRET, "adminbot")`.
- **Recovery.** If adminbot's state is lost, it can't re-claim on its own. The operator runs `avalanche-reset-adminbot` on the server host: it stops adminbot, runs `avalanche-server reset-adminbot` (deletes the old `did:local:adminbot` account with `db::accounts::delete_account` and clears the claim), moves the old state dir aside, and restarts adminbot, which re-registers and claims again. The new adminbot creates a fresh `#admins`; admins must be re-invited (or listed in `ADMINBOT_INITIAL_ADMINS`).

### Coordination is data-carried, not bot-to-bot

Adminbot calls no other bot and exposes no API to them. Coordination rides durable data: server events, signed tokens and catch-up. Adminbot learns of new accounts from the `AccountJoined` push. Routing by the registering token's issuer and tags is Planned, as parsed claims carried in the event (`24`); events never carry the raw token. Bots depend on the server and on signed artifacts, never on each other's uptime.

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
- **Manifest install at startup.** Every `*.json` in `ADMINBOT_MANIFEST_DIR` (default `<dirname(state dir)>/manifests`) is installed non-interactively, auto-granting every requested permission except `registration.gatekeeper`. This is how the deploy bundle configures web Projects. For each, adminbot also writes the Project's bot signup key to `<ADMINBOT_BOT_SIGNUP_KEY_DIR>/<slug>.key` (mode 0600; default `<dirname(state dir)>/bot-signup-keys`), only if that file doesn't exist yet, so restarts don't rotate it. First-party bots such as testbot read their key from there.
- **State:** `ADMINBOT_STATE_DIR` holds the SQLCipher store and a `state.json` sidecar. Losing it means running `avalanche-reset-adminbot` (*Superuser authority*).

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
4. `POST /v1/admin/projects/{slug}/bot-signup-key` and DM back the Project's **bot signup key**. A bot registering with it is admitted (even on a closed server, even after a gatekeeper is installed) and linked to this Project. One key per Project, reusable; running `/install-project` again mints a new key and revokes the old one (`24`, `51`).

It flips the reaction to a check or a cross when done. There is no `PROJECTS` env var: a Project appears in the Network tab only through a manifest install.

### Project capabilities

The catalog of what a Project may request and how it is enforced lives in `20-project-security.md` §Project permissions. Adminbot is the only grantor; `granted_by` records its DID so a grant can be cross-referenced with the `#admins` thread that authorized it. Adminbot holds every capability implicitly through the superuser link.

### Join event API (push + catch-up)

**Built** server-side (`routes/registration.rs`, `routes/admin.rs`, `infra/migrations/017_server_events.sql`).

- **Push.** On every registration the server sends `AccountJoined { did, joined_at_ms }` over the WebSocket to every connected session holding `accounts.read`. The proto's `invite_token` field (3) is never populated (S-01).
- **Durable log.** The same event is appended to `server_events` (30-day retention, swept in `server/src/tasks/mod.rs`).
- **Catch-up.** `GET /v1/admin/events?since=<id>&kind=account_joined` (500 per page) for any bot holding `accounts.read`.
- **Roster snapshot.** `GET /v1/admin/accounts?after=<did>` returns `{ accounts: [{did, display_name?, is_bot, created_at_ms}], next }`, gated on the same capability.

**Adminbot uses only the live push.** Neither app-core nor adminbot calls the catch-up endpoint, so joins that happen while adminbot is disconnected are never routed.

### Privacy posture

The server already knows every account it registers, so showing that to a bot the operator installed adds no new leak, and there is deliberately no group linkage (`03` §3.9 intact). A compromised `accounts.read` bot gets a real-time roster of joins with timing; the threat model accepts this. Events never carry the raw registration token, which could contain a registration secret; migration 026 purged stored ones. When routing is built, events will carry parsed issuer and routing claims only.

### `did:local:` DID scheme

**Not built as decided.** Bot accounts without a PLC DID get `did:local:<suffix>`: either a caller-chosen suffix (3–32 lowercase alphanumerics, first come first served) or one derived from the identity key (`registration.rs`, `generate_local_did`). Adminbot still registers the fixed literal `did:local:adminbot`.

**Decided: random per-server DIDs, no well-known literal.** The fixed literal is the same string on every server, but the client's per-identity store keys contacts, profiles and conversations by DID. A user on two servers therefore merges the two adminbots into one row: block one and you block both, and their DMs interleave (`37`). Random DIDs make each server's adminbot a distinct identity. Server authorization is unaffected, since authority is the Project link. Finding "this server's adminbot" without a literal (role discovery) is deferred until a concrete need appears.

**Rejected:** a host-scoped shape such as `did:local:{hostname}:adminbot` (couples identity to a hostname), and client-side conversation-key rewriting (fixes one table, leaves the footgun for the next).

## Known gaps

Security items are also in `09-security-posture.md`; todos in `02`.

1. **Open admin needs onboarding (S-29, by design).** Auto-invite targets every group adminbot admins, `#admins` included (`inviteToAdminGroups`), and `/install-project` and `/list-projects` check `#admins` membership (`requireAdminsMember`). So everyone who signs up is an admin. **This is intended:** in a new org's early days nobody needs to control who has admin, and it keeps setup effortless. It is consistent with the model above: `#admins` *is* the admin roster, and on a young server the roster is everyone. The gap is that nothing tells the operator this is happening, or when and how to close it (before sharing the invite widely, or installing a gatekeeper).
2. **Reserved `did:local:` names can be squatted (S-30).** Bots choose their own suffix, first come first served, so on a fresh server a holder of an admission credential could register `did:local:adminbot` before adminbot does.
3. **`/audit` has no `#admins` check.** Anyone who can DM adminbot gets a listing of every group adminbot is in (titles, counts, timers) and triggers timer clamps.
4. **No catch-up.** Joins while adminbot is down are never routed (*Join event API*).
5. **Fixed `did:local:adminbot`** still merges adminbots across servers in multi-homed clients.
6. **Stale comment:** `ADMINBOT_DIDS` in `middleware/auth.rs` describes config seeding that doesn't exist.

## Planned

- **Onboarding for open admin (S-29):** adminbot's welcome to `#admins` and the configure page explain that everyone is an admin for now and how to change it; a simple command to stop auto-inviting new members into `#admins` (e.g. `/admins closed`), and a nudge when the server grows or a gatekeeper is installed (*Known gaps* 1).
- **Join events carry parsed claims** (issuer slug, purpose, routing tags) when routing is built.
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
- **Recovery ladder:** restart from state → rotate keys (authority is the Project link, not a key) → `avalanche-reset-adminbot` (built) → keep `#admins` membership across a reset instead of recreating it.
- **Backup recovery identity** for the "every admin left" case. Today the answer is operator shell access.

## Rationale and rejected alternatives

- **Two bootstrap secrets, not one (S-01).** The configure tool's first-members invite carries the registration secret and is shared by design, so that secret must grant nothing beyond signing up. Superuser needs a secret that is never shared, generated on the box, and usable once. Claim-once means a leaked superuser secret still can't add a second superuser after adminbot claims.
- **Per-Project bot signup keys instead of setup codes.** A setup code was a bootstrap token naming a Project, so it carried the master secret and its holder could rename the Project to `adminbot`. A bot signup key is resolved server-side by its hash, so it can't choose its Project. It is reusable rather than single-use because testbot creates a new bot account for every user; a leaked key adds bots to one Project only, and re-minting revokes it.
- **Rejected: a bot-to-bot RPC / service mesh with discovery.** It makes every bot depend on every other bot being live. Everything needed so far fits data-carried coordination. A genuinely synchronous need should be one Project calling another's ordinary HTTP API with its own auth, an explicit trust edge, not an ambient mesh.
- **Rejected: the gatekeeper asking adminbot to add a user to channels.** Imperative cross-bot RPC; the token carries routing tags instead (`24`).
- **Rejected: officialness as a signed attestation.** Decomposes into a plain operator-set flag (same-server only) plus an ordinary scope (`20`).
- **Rejected: per-admin server-verified credentials instead of the delegate.** The server would see which admin DID made each privileged call, accumulating a partial roster and eroding the property `#admins` protects. The delegate keeps the roster invisible.
- **Non-goals.** Adminbot is not a general bot framework, not an RPC hub, and not federation-aware. It can only add people to groups it is itself an admin of, under the group's normal policy.
