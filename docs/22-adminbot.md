# Adminbot

This document describes **adminbot**, the canonical first-party Project that runs on every homeserver to handle server administration through chat. The two foundational pieces are:

1. **One superuser identity** — adminbot's DID — pinned in server config. All privileged `/v1/admin/*` endpoints check that the caller is adminbot; nothing else.
2. **An `#admins` group** — a regular action-bound group, encrypted like any other, whose membership *is* the set of human administrators. The server can't read this membership (group state is E2E encrypted), so it has no opinion on which humans have admin authority. Adminbot mediates: humans post commands in `#admins`; adminbot verifies the sender is a group member and executes the command by calling its own superuser endpoints.

The big property this gives us: **the homeserver database doesn't reveal who has administrative authority.** Privilege sits in the encrypted member list of `#admins`, which adminbot can decrypt because it's a member but the server itself can't.

A second property, from adminbot's *shape* rather than its authority model: it is **headless and outbound-only** — no web UI, no inbound network surface, and no public routing record pointing at it. It can run anywhere with outbound connectivity (behind a firewall, on a laptop), and because nothing public routes to it, the admin control plane is **hard to locate or seize**: an adversary must seize the homeserver first, then chase a source IP to a device. This inherits the platform's server-seizure posture (`00-design.md`). See *Deployment shape* for the full picture and the caveats (the server still sees the live source IP; co-locating adminbot with the homeserver forfeits it).

This doc opens with **the model** and the **alternatives we rejected** (and why), then splits into a **v1** section describing what's built today and a **Future** section sketching design directions that motivated the v1 shape but are not yet implemented.

---

## The model: two authorities, and why adminbot is privileged

Two different kinds of authority run a server, and conflating them is the mistake the rest of this design is careful to avoid:

- **Operator authority** — the infrastructure / trust-root domain: install a Project, register its bot account, grant it server capabilities, mark it official. The operator holds the server's trust root. These facts are **not seizure-sensitive** — an installed Project and its officialness are public by construction (a ✓ badge exists to be seen). So they can live in the server's database without weakening the seizure posture.
- **Social admin authority** — who may moderate: kick a member, remove content, add someone to a channel. This **is** seizure-sensitive ("who can target whom"), so it lives in the encrypted `#admins` membership, which the server cannot read.

These are protected differently, and the general rule for where any bot capability lives is:

> **The threat decides the home.** Access to the *server's own* resources → a **server-enforced capability** (the bot is the constrained party; only the server can enforce a limit on what touches its facilities). Seizure-sensitive social authority → **E2E state** (the server is the threat). A trust signal that must be verified *offline or across servers* → a **signature rooted in the server's cold trust-root key**.

### Why adminbot needs the superuser pin

We want installing and permissioning Projects to happen **in the `#admins` group** — a conversational admin experience, not shelling into the box. But the server **cannot see who the admins are** (membership is E2E). So when a privileged action is requested, the server has no way to verify "a real admin authorized this."

Something must bridge *admin authority (E2E, invisible to the server)* → *privileged server action*. That bridge needs to (1) read `#admins` membership — so it must be a member — and (2) be trusted by the server — so the server must recognize it. **adminbot is that bridge, and the superuser pin (`caller == ADMINBOT_DID`) is property (2).** Humans never hold superuser; they exercise it *through* adminbot, gated by their `#admins` membership.

So **yes — adminbot has to be able to install Projects and grant capabilities.** That power lives in adminbot precisely because adminbot is the only thing that can both verify the E2E authorization *and* be trusted by the server to act on it. The price of "in-app admin channel + admins hidden from the server" is one privileged bridge bot.

Two disciplines keep that price bounded:

- **Least privilege for the delegate.** The pin is a concentration — a compromised adminbot is full server-admin compromise. So adminbot holds *specific* privileged endpoints (install, grant) and we avoid piling other powers onto adminbot. Note especially that adminbot may run on less-controlled hardware and isn't guaranteed to be up at all times.
- **Legible, confirm-gated commands.** Privileged actions are issued as `#admins` messages (auditable in group history); destructive ones gate on a reaction confirmation (see *Full chat-command surface*).

### Coordination is data-carried, not bot-to-bot

adminbot does not call other bots, and other bots do not call it. All coordination rides **durable signed data + server events + catch-up**, never a live dependency on a peer. Adminbot learns of new accounts via `AccountJoinedEvent` (which can be carried over the websocket or the catch-up endpoint), reads the registering invite token, and determines which groups to add them to based on the token's contents.

Through this property we push for keeping the bot dependency graph minimal: bots depend on the server and on signed artifacts, not on each other's uptime.

### Deployment shape: headless and outbound-only

Adminbot has **no web UI and no inbound network surface.** Unlike web-UI Projects (the chatbot, the gatekeeper) — which serve a webview, need a public HTTPS origin, and use the project-token flow — adminbot's entire interface is chat commands in `#admins`, carried over the E2E messaging substrate. It is just another client account: it opens an *outbound* WebSocket (plus HTTP) to the homeserver and serves nothing.

That makes it **location-independent.** It can run behind a firewall or NAT — on a server, a Raspberry Pi, or an admin's laptop — anywhere with outbound connectivity to the homeserver. No port to open, no origin to host, no TLS to terminate, no DNS. As long as the process stays up and can reach the server it does its job; if the device sleeps or drops off, adminbot is simply "down" and resumes when it returns (the only cost is missed events, which the `AccountJoinedEvent` catch-up endpoint covers — not a connectivity-exposure problem).

Three consequences worth noting:

- **Hard to seize — no public pointer to it.** This is the seizure angle, and it composes with the platform's server-seizure posture (`00-design.md`). *Nothing public routes to adminbot:* `did:local:adminbot` is server-scoped (no PLC service endpoint), and adminbot serves no origin, so there is no DNS, IP, or directory record an adversary can follow to find it. To locate it you must first seize the **homeserver**, recover adminbot's source IP from live connection state or retained logs (which may be behind NAT/VPN, dynamic, or not logged at all), and only then trace that IP to a physical device. Contrast a web-UI Project, whose public HTTPS origin is itself a persistent, findable routing pointer — and whose box, if seized, doesn't even require the homeserver as a first step. (Bonus: an off-box adminbot keeps its superuser *private* keys off the seized server too — only its public verification key is in the accounts table.) **Caveat:** this only holds if adminbot is deliberately run *off-box*; co-locating it with the homeserver — the simple `make dev-all` shape — forfeits the property: seize the server, seize the bot.
- **Smaller attack surface, but not zero.** With no inbound listener, adminbot isn't directly reachable from the network. Its real exposure is the untrusted *message* input it decrypts and parses from `#admins` and DMs, plus the security of whatever device it runs on. "Outbound-only" is genuine hardening, but the superuser pin still means treat the host with care.
- **It argues against making anything depend on adminbot's uptime.** A bot that may live on a laptop behind a firewall is the wrong place for a *liveness dependency*: anything other Projects or users rely on — capability records, the `official` flag, routing — lives on the always-available server, not on a process that might be asleep. The location-independence that makes adminbot easy to run is the same property that argues against letting anything *depend* on it being up.

---

## Rejected alternatives (and why)

- **A bot-to-bot RPC / service mesh, with discovery.** Tempting for "bots offering services to bots" (a signing bot, a directory bot, an "add-to-channel" service). Rejected: it makes *anybody depend on any other bot* being live — exactly the fragility we want to avoid. Everything we needed turned out to be expressible as data-carried coordination (signed tokens + server events + catch-up). If a genuinely *synchronous, interactive* bot-to-bot need ever appears, model it as one Project calling another's ordinary HTTP API (with its own auth) — a new, explicit trust edge — not an ambient mesh. A discovery layer (registry / directory bot) is deferred with it.
- **The gatekeeper asking adminbot to add a user to channels (imperative RPC).** See `24-vetted-onboarding-project.md`. Rejected in favor of the invite token carrying routing *tags* that adminbot maps to channels declaratively; a self-routing gatekeeper reads the same `AccountJoinedEvent` instead of calling adminbot. No cross-bot call.
- **Officialness as anything more than a flag + a scope.** Earlier drafts made it a bespoke trust primitive with a signed, periodically re-issued official-bot attestation (first signed by adminbot, then by the server). All rejected: officialness decomposes into a same-server `official` flag (the ✓ badge, read from the bot's account record) and the `invites:auto-accept` scope — both plain server records: no signing, no attestation, no recurring signer, no separate "declare official" channel (it's set at install through the normal grant flow). The full treatment lives in `20-project-security.md`.
- **Per-admin server-verified credentials instead of the pinned delegate.** An alternative bridge: give each admin a signed credential the server checks per request, so admins call privileged endpoints directly (no adminbot). Rejected for the base design: the server would then see *which admin DID* acted on each privileged call, accumulating a partial roster over time and eroding the seizure property `#admins` protects — plus per-admin credential provisioning. The pinned delegate keeps the roster fully invisible (the server only ever sees adminbot act).

---

## v1: what's implemented today

A minimal demo: `make dev-all` brings up the homeserver and a separate adminbot process together; the first human to register is invited to `#admins`; `/whoami` and `/help` work there. Everything in *Future* below is not yet built. What exists, vaguely:

- **A separate Node/TS process** (`node/packages/adminbot/`) on `@actnet/app-core`, talking to the server over HTTP+WS, coupled to it only by the reserved DID `did:local:adminbot` (server-scoped; the server pins `ADMINBOT_DID` and claims the identity key on first registration).
- **One privileged endpoint** — `GET /v1/admin/ping`, gated by `caller_did == ADMINBOT_DID` — the entire "superuser" surface so far, proving the pin middleware. No capability table yet.
- **The `#admins` group**, created at bootstrap; adminbot invites every newly-registered account (detected via an `AccountJoinedEvent` WS push — fire-and-forget, lost if adminbot is disconnected; no catch-up yet). Humans accept via the normal invite UI.
- **Two commands** in `#admins` — `/whoami`, `/help` — authorized by "sender is a current `#admins` member," which adminbot checks by decrypting the roster. No destructive commands yet, so no confirmation flow.
- **State** in `ADMINBOT_STATE_DIR` (SQLCipher store + `state.json` sidecar); config via `ADMINBOT_DID`, `ADMINBOT_SERVER_URL`, `ADMINBOT_DB_KEY`, and `ADMINBOT_INITIAL_ADMINS` (comma-separated DIDs to seed the admin set when retrofitting onto a server that already has users).
- **Recovery** is restart-from-state; if state is lost, the reserved DID is already claimed, so re-registration needs the `did:local:adminbot` accounts row deleted server-side first.

## Non-goals

- **Not a general bot framework.** Adminbot is one Project among many. The cross-cutting concepts in this doc — superuser pinning, the join-event API, capability grants — are framework hooks other Projects use too. None of that framework exists in v1.
- **Not a bot-to-bot RPC hub.** Adminbot exposes no callable API to other bots; coordination is data-carried (see *The model* and *Rejected alternatives*).
- **Not a free pass to invite anyone to anything.** Adminbot can only add users to groups *it is itself a member of*. The group's existing invite policy still applies; the bot acts as a regular admin, just on a hair trigger.
- **Not federation-aware.** Adminbot runs against the homeserver it's installed on, learns about local joins, and acts on local groups. Federated joins / multi-homed users follow `docs/13-federation.md` separately.

---

## Future / not yet built

Everything below is design sketch, not built. The v1 shape was deliberately chosen so each of these can be layered on without rework: adminbot is a regular client account, the server's only opinion is the DID pin, all policy lives in adminbot's process.

### Human-readable `did:local:` scheme

`did:local:` is already implemented as a random-suffix format. A future iteration could move to a human-readable, server-scoped shape like `did:local:{server_hostname}:adminbot` so the DID is recognizable at a glance. Trade-off is just operator UX vs. the current random suffix; both verify identically and use the same accounts-table resolution path. Defer until there's a concrete reason to break the random-DID invariant.

### Installing a Project and granting permission (the `#admins` flow)

This is the easy system for bringing a Project online — and it's the concrete realization of *operator authority exercised through the admin channel* (see *The model*). All of it is driven from `#admins`:

1. An admin posts `/install-project <name> <url>` in `#admins`.
2. Adminbot verifies the sender is a current `#admins` member (it decrypts the roster).
3. Adminbot registers the Project's bot account and records the install via its superuser endpoint (`POST /v1/admin/projects`), which creates the bot-account row and returns its DID.
4. Adminbot grants the capabilities the Project's manifest declares — one `POST /v1/admin/capabilities` per scope. **Default-deny:** only the declared, admin-approved scopes are granted. High-privilege grants (`registration.gatekeeper`, `subscribe.account_joined`) gate on a reaction confirmation in `#admins`.
5. Adminbot posts a summary back to `#admins` — what was installed, which scopes were granted — so the decision is legible and auditable.

### Project capabilities

A Project is installed by registering a bot account and granting it some set of **capabilities** — explicit, named permissions that gate access to server-side facilities most accounts cannot touch. Capabilities are the *operator authority* made concrete, and they live server-side because the bot is the constrained party (see *The model*: "access to the server's own resources → server-enforced capability").

Initial capability set (extensible as Projects need more):

| Capability | What it grants |
| --- | --- |
| `subscribe.account_joined` | Receive a notification each time a new account registers on this server (see "Join event API"). |
| `subscribe.account_left` | Receive a notification when an account is deleted from this server. |
| `registration.gatekeeper` | Construct invite tokens the server accepts under closed registration. Held by any number of Projects (one per invite flow); granting it registers the Project's token-signing public key with the server. See `24-vetted-onboarding-project.md`. |

Capabilities are **per-bot**, stored server-side in `project_capabilities (account_id, capability, granted_at, granted_by)`. The `granted_by` is always adminbot's DID — these endpoints only accept adminbot as caller — but the record is kept so we can later distinguish "which human admin's chat command authorized this" by cross-referencing the `#admins` thread.

A bot without `subscribe.account_joined` cannot learn about new users by any server-mediated mechanism. Adminbot itself is granted `subscribe.account_joined` (so it can act on new users) plus the implicit `superuser` pin. No other bot ever gets superuser.

### Join event API (push + catch-up)

The `subscribe.account_joined` capability lets a bot's account receive a server-side event each time a new account registers. The bot is responsible for inviting users to channels (using the normal `POST /v1/groups/{id}/changes` endpoint with `invite_members`) — the server never invites on a bot's behalf.

A long-lived authenticated stream on the bot's session. WebSocket is the existing transport for the bot's normal traffic; we extend `actnet.ws.WsFrame` with a new event-push variant:

```proto
// Server → bot: a new account registered on this server. Sent only to
// bots whose account has the `subscribe.account_joined` capability.
message AccountJoinedEvent {
  string did                = 1;
  string display_name       = 2;
  // The token used to register, if any. Lets bots route based on
  // which invite link the new user came in through (regional channel
  // onboarding flow, event registration, gatekeeper approval, etc.).
  // Absent for direct registrations.
  optional string invite_token = 3;
  // Server epoch millis at registration.
  int64 joined_at_ms        = 4;
}
```

The `invite_token` field is the data-carried hand-off that lets adminbot route without any bot-to-bot call: adminbot reads the token's issuer + routing tags and maps them to channels (see `24-vetted-onboarding-project.md` and *Coordination is data-carried* above).

A bot reconnecting after downtime fetches the events it missed via a paginated HTTP endpoint:

```
GET /v1/admin/events?since=<event_id>&kind=account_joined
```

Each event carries a server-side monotonic `event_id`. The bot persists the highest id it has processed; on (re)connect it requests the tail. The server retains events for a configurable window (default 30 days); older events are dropped. This catch-up is what makes routing *deferred, not lost*, when adminbot is down.

**Privacy posture.** The server already knows every account that registers (it ran the registration). Disclosing that to a bot the operator has explicitly installed adds no new leak. The bot is a privileged participant of the same trust domain as the server operator.

### Default notifications on accept

A bot inviting a user can express a default notification preference inside the `GroupContext` DM. The hint is end-to-end encrypted (it's inside the DM, not group state) and the server never sees it.

```proto
message GroupContext {
  // …existing fields…
  optional NotificationDefaults notification_defaults = N;
}

message NotificationDefaults {
  uint32 level = 1;                   // 0 = all, 1 = mentions only, 2 = muted
  optional int64 mute_until_ms = 2;   // reverts to "all" after this time
}
```

- Auto-accept path: applied silently.
- Manual accept path: shown in invite-confirmation UI ("Adminbot suggests: muted for 7 days") and applied on accept.

Always a hint, never a command.

### Security-update awareness

Adminbot tracks the homeserver's running version, compares against a known-good manifest, and DMs `#admins` when the server is behind on a security-relevant update.

**Server build endpoint:**

```
GET /v1/server/build                           (any authenticated session)
Response: { "commit": "...", "version": "0.4.1", "built_at_ms": ..., "started_at_ms": ... }
```

Adminbot polls every few hours; server pushes a `ServerBuildEvent` over WS on restart for immediate post-upgrade detection.

**Known-good manifest:**
1. Operator-curated `security_manifest.toml` (simplest).
2. Project-signed feed from an upstream release authority (future — needs a release-authority key concept).

```toml
[[security_release]]
minimum_version = "0.4.1"
severity        = "high"
summary         = "Fix CVE-2026-xxxxx (sender-cert spoofing)"
published_at_ms = 1717200000000
```

**Notification policy:**
- `info`/`low` behind: silent unless queried via `/version`.
- `medium` behind: DM `#admins` once per release; repeat weekly.
- `high`/`critical` behind: DM `#admins` once per release; repeat daily. After 7 days of non-action on critical, DM each `#admins` member individually.

Entirely client-side (in adminbot). The homeserver never makes value judgments about which versions are good.

### Full chat-command surface

```
/install-project <name> <url>          install + start a Project's bot, grant its declared capabilities
/uninstall-project <name>              uninstall (revokes capabilities + tokens)
/grant <bot_did> <capability>          grant a capability to a bot
/revoke <bot_did> <capability>         revoke
/officialize <bot_did>                 set the bot's ✓ flag
/unofficialize <bot_did>               clear the ✓ flag
/list-projects                         list installed Projects
/pause <bot_did>                       freeze that bot's capabilities
/unpause <bot_did>                     reverse /pause
/kick <did> from <group>               adminbot removes <did> from <group>
/add <did> to <group>                  adminbot invites <did> to <group>
/version                               report current server build + pending updates
/seed-into <group_id>                  adminbot joins the group as Admin
```

Destructive operations gate on reaction-based confirmation: adminbot replies "React 👍 within 60s to confirm" and only acts on the reaction. Makes commands legible in group history (auditable) and protects against typos.

The Server Console — adminbot's chat-command interface — is structurally just a Project. Adminbot installs itself as the very first Project at bootstrap.

### Reserved-name protection for `#admins`

The group title `#admins @ {server_hostname}` is a reserved string on each server, but the server has no way to enforce uniqueness because group state is encrypted. Protections:

- The legitimate `#admins` is created by adminbot, which carries the `official` flag and the `invites:auto-accept` scope; the badge and auto-accept apply only to such operator-blessed bots.
- Long-term, an **official-groups** registry: the server marks a group's server-visible id as the canonical `#admins`, surfaced to clients the same way as the bot `official` flag — a plain server record, no signing (same-server only, like the badge). The open part is how the client reliably maps the encrypted group it's in to that record; defer until the threat is concrete.

### Leave/rejoin flow

A human can leave `#admins` like any other group, via `remove_members(self)`. After the leave applies, adminbot:

1. DMs the leaver: "You've left the admins group. If this was a mistake, reply with `/rejoin` and I'll add you back." Includes a deep link.
2. On `/rejoin`, adminbot issues a fresh `invite_members`. Client auto-accepts (adminbot holds `invites:auto-accept`), user is back.

No special handling for "last admin leaves" — same DM is sent. If they ignore it, the server's `#admins` ends up empty until someone acts on the rejoin link. Operator-shell recovery remains the fallback for adminbot being permanently unreachable.

### Configuration (full)

Beyond v1's hardcoded "invite everyone to `#admins`", per-server config (welcome message, channel sets, invite-token routing rules) lives in adminbot's own process — file on disk, or a Project-internal DB. This is where adminbot maps invite-token tags → channels (the declarative routing that replaces any bot-to-bot RPC).

```toml
[adminbot]
welcome_message = "Welcome to Safe Haven! These are our main channels."

[[adminbot.rule]]
# Invite everyone who joins.
match.always = true
groups = ["did:plc:safe-haven-announcements", "did:plc:safe-haven-general"]
notification = { level = 1, mute_until_ms = 604800000 }

[[adminbot.rule]]
# Invite only users whose invite token carried a specific tag.
match.invite_token_tag = "regional-northeast"
groups = ["did:plc:northeast-region"]
notification = { level = 0 }
```

Long-term, chat commands (`/add-rule`, `/list-rules`, `/edit-rule`) expose this through `#admins`.

### Full recovery story

1. **Adminbot down temporarily.** Restart; state persists.
2. **Adminbot identity keys lost / corrupted.** Adminbot regenerates and rotates via the normal device-rotation flow. The server's pin is on the DID, not the key.
3. **Adminbot DID itself unrecoverable.** Operator-shell recovery: re-run bootstrap with a fresh DID, write the new `ADMINBOT_DID`, re-create `#admins`. Existing admin humans need to be re-invited. Once-in-a-lifetime event.
4. **`#admins` group lost.** Adminbot creates a new one and starts the bootstrap re-add flow. Cached `AccountJoinedEvent` DIDs from prior runs can auto-populate.

A `/pause` mechanism (freeze adminbot's capabilities without uninstalling) helps with rollback if adminbot is misbehaving.

### Full API surface

| Method & path | Auth | Purpose |
| --- | --- | --- |
| `POST /v1/admin/projects` | adminbot only | Install a Project (creates bot account, returns its DID). |
| `GET /v1/admin/projects` | adminbot only | List installed Projects. |
| `DELETE /v1/admin/projects/{id}` | adminbot only | Uninstall. |
| `POST /v1/admin/capabilities` | adminbot only | Grant a capability to a bot. |
| `DELETE /v1/admin/capabilities/{account_id}/{capability}` | adminbot only | Revoke. |
| `POST /v1/admin/official/{account_id}` | adminbot only | Set the bot's officialness record. |
| `DELETE /v1/admin/official/{account_id}` | adminbot only | Clear officialness. |
| `GET /v1/server/build` | any authenticated session | Current server commit / version / build time. |
| `GET /v1/admin/events?since=<id>&kind=...` | bot session w/ capability | Paginated catch-up for missed events. |

Officialness is a plain `official` flag set via `POST/DELETE /v1/admin/official/*` — no signing endpoint, no attestation (see `20-project-security.md`).

New WebSocket frame variants (`ws.proto`):

| Variant | Direction | Purpose |
| --- | --- | --- |
| `AccountJoinedEvent` | server → bot | Push for `subscribe.account_joined`. |
| `ServerBuildEvent` | server → bot | Push on server (re)start. |

New DB tables / columns:

| Table / column | Where | Purpose |
| --- | --- | --- |
| `project_capabilities` | homeserver | `(account_id, capability, granted_at, granted_by)`. |
| `accounts.is_official` | homeserver | Server-side officialness flag (the ✓ badge), set via the `#admins` install flow. |
| `server_events` | homeserver | `(id, kind, payload, created_at)` — append-only, drained by `GET /v1/admin/events` and WS push. |

(There is no `adminbot.official_bots` table and no `adminbot.attestation_key`: officialness is a plain `official` flag on the bot's account record — no signing, no attestation. See `20-project-security.md`.)

---

## Open questions

1. **Bot identity-key rotation telemetry.** Adminbot rotating its identity keys is transparent (DID is the pin). But the client-side trust UX should probably surface a safety-number-like change indicator when adminbot's identity key rotates — same as any contact. Confirm this is in scope for the rotation UX work or treat as a follow-up.

2. **`did:local:` resolution.** `docs/02-todos-deferred.md` mentions the concept but doesn't define a DID document shape or resolution path. Probably resolved by "the homeserver's accounts table is the DID document for `did:local:{this-host}:*`" — simplest possible thing. Confirm when `did:local:` lands as a real feature.

3. **Cross-server officialness (deferred).** Officialness is now a same-server `official` flag plus the `invites:auto-accept` scope, with no signing (see `20-project-security.md`). If a future federated/guest scenario ever needs a *cross-server-verifiable* badge or auto-accept, that's where a signed projection would re-enter — deferred with federation, not needed now.

4. **Default-mute discoverability.** A user auto-joined into 8 muted channels may never look at them. Channel discovery + "you have unread mentions" surfaces in the Chats tab will mostly handle this, but worth checking once the UX is in users' hands.

5. **Server-event privacy disclosure.** A compromised bot with `subscribe.account_joined` gets a real-time roster of everyone who joins this server, including timing. The threat model accepts this (the operator authorized the bot; the server already had the data). Worth a one-line acknowledgment in `docs/00-design.md` "Trust model" when this lands.

6. **Recovery from `#admins` ghosting.** If every admin leaves and nobody acts on adminbot's rejoin DMs, the server is frozen. The current answer is "operator shell." Worth thinking about whether a backup recovery DID (dormant, operator-pinned, promotable out-of-band) is worth the complexity. Defer.

7. **Event durability across adminbot restarts.** v1's `AccountJoinedEvent` push is fire-and-forget — if adminbot is disconnected, the event is lost. The future `server_events` table + `GET /v1/admin/events?since=…` catch-up endpoint fixes this, but is deferred. Worth checking how painful the gap is in practice.
</content>
