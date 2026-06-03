# Adminbot

This document describes **adminbot**, the canonical first-party Project that runs on every homeserver to handle server administration through chat. The two foundational pieces are:

1. **One superuser identity** — adminbot's DID — pinned in server config. All privileged `/v1/admin/*` endpoints check that the caller is adminbot; nothing else.
2. **An `#admins` group** — a regular action-bound group, encrypted like any other, whose membership *is* the set of human administrators. The server can't read this membership (group state is E2E encrypted), so it has no opinion on which humans have admin authority. Adminbot mediates: humans post commands in `#admins`; adminbot verifies the sender is a group member and executes the command by calling its own superuser endpoints.

The big property this gives us: **the homeserver database doesn't reveal who has administrative authority.** Privilege sits in the encrypted member list of `#admins`, which adminbot can decrypt because it's a member but the server itself can't.

The doc is split into a **v1** section describing what's being built today and a **Future** section sketching the design directions that motivated the v1 shape but are not yet implemented.

---

## v1 scope (what's being built today)

A demo where the operator runs `make dev-all`, the homeserver and a separate adminbot process come up together, the first human registers, adminbot invites them to `#admins`, and the human types `/whoami` in `#admins` and gets a reply. Nothing more.

Concretely:

1. **Adminbot is a separate process** — a Node/TypeScript package at `node/packages/adminbot/`, talking to the server over HTTP+WS via `@actnet/app-core`. The server and adminbot are coupled only by the well-known DID `did:local:adminbot`.
2. **Reserved DID, no two-phase bootstrap.** The server defaults `ADMINBOT_DID` to `did:local:adminbot` and validates registration requests for this DID. The adminbot registers under it on first run (passing `did_suffix: "adminbot"`); the server's accounts table TOFU-claims the identity key for that DID forever after. Subsequent adminbot runs re-login against the local SQLCipher store.
3. **Single privileged endpoint.** `GET /v1/admin/ping` accepts only the caller whose authed DID equals the pinned `ADMINBOT_DID`. That's the entire "superuser" surface for v1 — proof that the pin works.
4. **`#admins` group.** Adminbot creates the group titled `#admins @ {hostname}` at bootstrap, joins as the sole member with Admin role.
5. **New-user detection via WS event push.** When a new account registers, the server pushes an `AccountJoinedEvent` frame over the WebSocket of whichever session is currently authenticated as `ADMINBOT_DID`. Adminbot receives it and calls `invite_member(#admins, that_did)`. If adminbot is disconnected at the moment of registration, the event is lost — v1 accepts that limitation. A catch-up HTTP endpoint is future work.
6. **Manual invite acceptance.** No auto-accept; the human accepts the `#admins` invite from the normal invite UI like any other group.
7. **Two chat commands.** `/whoami` (echoes caller's DID) and `/help` (lists commands). Authority check: any current member of `#admins` is allowed to issue commands. Adminbot itself knows the member list because it's in the group.

Anything not on this list is **not in v1**. See `## Future` below for what's deferred and why the v1 shape was chosen with that future in mind.

## Adminbot's DID (v1)

Adminbot's DID is fixed at `did:local:adminbot`. The server defaults `ADMINBOT_DID` to this value and treats `adminbot` as a permitted suffix on the bot-account registration path (otherwise the suffix is a random 24-char base32). `did:local:` is server-scoped — `did:local:adminbot` on different homeservers refers to different identities, so there's no collision concern.

The operator overrides `ADMINBOT_DID` only in the rare case they want a non-default identity (e.g. a parallel staging adminbot on the same DB). The default just works.

Race window caveat: between server boot and the moment adminbot first registers, an external attacker who already knows the server URL could theoretically claim `did:local:adminbot` themselves. In practice the window is sub-second when both processes are launched together (`make dev-all`, or a systemd unit ordered after the homeserver in production), and the operator notices immediately on the next adminbot run (DID mismatch, registration fails). Documented and accepted; revisit if a real threat surfaces.

## Bootstrap (v1)

1. Operator runs `make dev-all` (or, in prod, starts the homeserver + adminbot systemd units). The server is up with `ADMINBOT_DID=did:local:adminbot`.
2. Adminbot has no local SQLCipher store → first-run path: register the reserved DID on the homeserver (the wrapper retries until the server is reachable; ordering is not load-bearing). Persist identity keys + session to `ADMINBOT_STATE_DIR/store.db`.
3. Adminbot creates the `#admins @ {hostname}` group via the normal action-bound group creation flow, becoming its sole Admin member. State sidecar at `ADMINBOT_STATE_DIR/state.json` records the group id and (if set) the `ADMINBOT_INITIAL_ADMINS` DIDs already invited.
4. Adminbot opens its WebSocket and waits for `AccountJoinedEvent` pushes.
5. First human registers. Server pushes `AccountJoinedEvent` to adminbot's WS. Adminbot calls `invite_member(#admins, that_did)`. Human accepts via normal invite UI, lands in `#admins`.
6. Human types `/whoami` → adminbot replies with their DID.

Restart behavior: on every subsequent boot, adminbot loads its store and resumes its WebSocket. Events that arrived while adminbot was down are missed in v1 (future: a catch-up endpoint).

## Retrofit onto an existing server

For a server that already has users when adminbot is first installed, the new-user-join hook doesn't fire for them. The `ADMINBOT_INITIAL_ADMINS` env var (comma-separated DIDs) seeds the initial admin set: on bootstrap, after creating `#admins`, adminbot issues an `invite_member` for each listed DID. Those humans accept via their normal invite UI and they're in.

End-to-end for an existing deployment:

1. Operator deploys adminbot pointing at the live server. (Server already defaults `ADMINBOT_DID` to `did:local:adminbot`; no env change needed unless overriding.)
2. Operator sets `ADMINBOT_INITIAL_ADMINS=<their own DID>` on the adminbot process.
3. Adminbot registers under `did:local:adminbot`, creates `#admins`, invites the listed DIDs.
4. Operator accepts the invite in their existing client, types `/whoami`, demo works.

Existing user sessions and data are undisturbed by the install — no server restart required, no user-facing endpoints change behavior.

Edge cases (flagged, not solved in v1):
- If `#admins` already exists from a prior aborted install, adminbot should detect and reuse it rather than create a duplicate. A `state.db` flag is enough.
- If a DID in `ADMINBOT_INITIAL_ADMINS` belongs to no real account, the invite sits pending — operator notices in logs and fixes.
- The list is read once at bootstrap; subsequent additions go through the normal `#admins` member flow rather than restarting with a new list.

## Authorization model

Two distinct authorities, even in v1:

- **Superuser (server-level)** — only adminbot's DID. Checked at the HTTP boundary. The `/v1/admin/*` endpoints reject any caller whose DID is not the pinned `ADMINBOT_DID`. Nothing else can speak to those endpoints. For v1 the only such endpoint is `GET /v1/admin/ping`, but the middleware is the lasting shape.
- **Admin (human-level)** — "the DID is a member of `#admins`." Determined entirely by adminbot, not the server. When a human posts a command in `#admins`, adminbot:
  1. Verifies the sender's DID is in the group's current member list (which adminbot decrypts because it's a member).
  2. Executes the action.

The server has no opinion on which humans are admins. It only knows the pinned adminbot DID. This is the load-bearing invariant the whole design is built around, and it's true from v1 onward.

## Group membership management (v1)

For v1 there's exactly one rule: **every new account gets invited to `#admins`.** This is the simplest thing that exercises the join-detection → invite path; subsequent v's add proper rules (channel sets, regional groups, invite-token-driven routing). See `## Future` for the planned config shape.

Adminbot adds users only via the standard `invite_members` group action; the bot's account must be a member of the group with sufficient role. Adminbot can only act on groups it's in; the bot has no special server-side ability to bypass group join policies.

## Chat-command surface (v1)

Adminbot watches `#admins` for messages matching a simple command grammar. Each command is a regular group message; adminbot replies in-thread.

```
/whoami        echo the caller's DID
/help          list commands
```

Authorization: adminbot checks the sender's DID is in the `#admins` member list it just decrypted, ignores everyone else. (For v1 there's no destructive action a command can take, so no confirmation flow yet — that's needed before the first real command lands.)

## API surface (v1)

| Method & path | Auth | Purpose |
| --- | --- | --- |
| `GET /v1/admin/ping` | adminbot only | Returns 200 `{ok: true}` for the pinned DID, 401 for anyone else. Proves the superuser middleware works. |

All `/v1/admin/*` endpoints check `caller_did == ADMINBOT_DID` at the middleware layer. There's no `accounts.is_server_admin` column; the check is a single equality test against the pinned config value.

WebSocket additions:

| Frame | Direction | Auth | Purpose |
| --- | --- | --- | --- |
| `AccountJoinedEvent { did, joined_at_ms }` | server → bot | session DID == `ADMINBOT_DID` | Pushed after each successful account registration to whichever session is currently connected as adminbot. If no adminbot session is connected, the event is dropped (no queue in v1). |

The auth check is the same equality test against `ADMINBOT_DID` — no capability table in v1, no `subscribe.account_joined` grant. Future generalization (per-bot capabilities + an event log + a catch-up endpoint) is in `## Future`.

## Configuration (v1)

| Key | Where | Purpose |
| --- | --- | --- |
| `ADMINBOT_DID` | server env / config | Pinned superuser identity. Defaults to `did:local:adminbot`; override only for non-default deployments. Read by `/v1/admin/*` middleware. |
| `ADMINBOT_STATE_DIR` | adminbot env (default `./adminbot-state`) | Directory holding the SQLCipher store and the `state.json` sidecar (group id, invited initial admins). |
| `ADMINBOT_SERVER_URL` | adminbot env | The homeserver adminbot connects to. |
| `ADMINBOT_DB_KEY` | adminbot env | Passphrase for the SQLCipher store. |
| `ADMINBOT_INITIAL_ADMINS` | adminbot env (optional) | Comma-separated DIDs to invite to `#admins` at bootstrap. Used when retrofitting onto a server that already has users; unused for fresh deploys (the first registrant gets invited automatically). |

No TOML rule file in v1 — the "invite everyone to `#admins`" rule is hardcoded.

## Recovery (v1)

1. **Adminbot down temporarily.** Restart the process. State loads from `ADMINBOT_STATE_DIR`; bot resumes its loop.
2. **Adminbot state lost.** The reserved DID is already TOFU-claimed by the original identity key, so re-registration fails. Recovery requires releasing the claim server-side — for v1, delete the `did:local:adminbot` row from the accounts table (and its cascaded rows) before restarting adminbot. A `make adminbot-reset` target wrapping the SQL is good follow-up. Old `#admins` group is orphaned; easiest path is to delete it.
3. **Server restart.** Adminbot reconnects via its existing WS reconnect logic (same as any client).

## Non-goals

- **Not a general bot framework.** Adminbot is one Project among many. The cross-cutting concepts in this doc — superuser pinning, official-bot attestation, the join-event API — are the framework hooks. Other Projects use them too. None of that framework exists in v1.
- **Not a free pass to invite anyone to anything.** Adminbot can only add users to groups *it is itself a member of*. The group's existing invite policy still applies; the bot acts as a regular admin, just on a hair trigger.
- **Not federation-aware.** Adminbot runs against the homeserver it's installed on, learns about local joins, and acts on local groups. Federated joins / multi-homed users follow `docs/13-federation.md` separately.

---

## Future / not yet built

Everything below is design sketch, not built. The v1 shape was deliberately chosen so each of these can be layered on without rework: adminbot is a regular client account, the server's only opinion is the DID pin, all policy lives in adminbot's process.

### Human-readable `did:local:` scheme

`did:local:` is already implemented as a random-suffix format. A future iteration could move to a human-readable, server-scoped shape like `did:local:{server_hostname}:adminbot` so the DID is recognizable at a glance. Trade-off is just operator UX vs. the current random suffix; both verify identically and use the same accounts-table resolution path. Defer until there's a concrete reason to break the random-DID invariant.

### Project installation and capabilities

A Project is installed by registering a bot account with the homeserver and granting the bot's account some set of **capabilities**. Capabilities are explicit, named permissions that gate access to server-side facilities most accounts cannot touch.

Initial capability set (extensible as Projects need more):

| Capability | What it grants |
| --- | --- |
| `subscribe.account_joined` | Receive a notification each time a new account registers on this server (see "Join event API"). |
| `subscribe.account_left` | Receive a notification when an account is deleted from this server. |

(Officialness is not a server-side capability — it's a row in adminbot's local `official_bots` table, plus an attestation adminbot signs and ships to the bot. The server has no say.)

Capabilities are **per-bot**, stored server-side in `project_capabilities (account_id, capability, granted_at, granted_by)`. The `granted_by` is always adminbot's DID — these endpoints only accept adminbot as caller — but the record is kept so we can later distinguish "which human admin's chat command authorized this" by cross-referencing the `#admins` thread.

A bot without `subscribe.account_joined` cannot learn about new users by any server-mediated mechanism.

Adminbot would then be granted `subscribe.account_joined` (so it can act on new users) plus the implicit `superuser` pin. No other bot ever gets superuser.

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
  // onboarding flow, event registration, etc.). Absent for direct
  // registrations.
  optional string invite_token = 3;
  // Server epoch millis at registration.
  int64 joined_at_ms        = 4;
}
```

A bot reconnecting after downtime fetches the events it missed via a paginated HTTP endpoint:

```
GET /v1/admin/events?since=<event_id>&kind=account_joined
```

Each event carries a server-side monotonic `event_id`. The bot persists the highest id it has processed; on (re)connect it requests the tail. The server retains events for a configurable window (default 30 days); older events are dropped.

**Privacy posture.** The server already knows every account that registers (it ran the registration). Disclosing that to a bot the operator has explicitly installed adds no new leak. The bot is a privileged participant of the same trust domain as the server operator.

Two guardrails:

1. **Bots subscribed to join events should be marked official.** Users should be able to see why a bot keeps appearing in their channels right after they join. The `/grant` command should refuse `subscribe.account_joined` for a non-official bot, with a hard override for exotic cases.
2. **Invite tokens carrying personally-identifying context** (a personal welcome message, a specific event registration) shouldn't be made available to bots whose role is org-wide channel autoresizing. Capabilities can be scoped to *which* invite tokens trigger their events — but that's a follow-up. Stage 1 grants are coarse.

### Officialness via delegation

The server's only role in officialness is **delegating signing authority to adminbot once**, via a delegation cert. After that, adminbot signs attestations on its own. The trust path:

```
server's pinned trust-root key  (same key used for sender certs, docs/03-groups §3.11)
  └─ signs ServerDelegationCert { server_hostname, adminbot_did, adminbot_pubkey,
                                  issued_at, expires_at }
       │
       └─ adminbot_pubkey signs OfficialBotAttestation { subject_did, display_name,
                                                        purpose, issued_at, expires_at }
                            signs OfficialGroupAttestation { ... }
                            signs any other "official X" payload
              ▲
              │ delivered via DM by adminbot to the bot
              │
            bot embeds (delegation_cert, attestation) pair in its substrate profile blob
              ▲
              │ fetched + decrypted by
              │
            client (normal contact-row population)
              │
            1. Verify delegation_cert signature against pinned trust root, check not expired,
               check server_hostname matches the hosting server.
            2. Verify attestation signature against delegation_cert's adminbot_pubkey,
               check not expired, check subject_did matches.
              → shows ✓ badge
```

Why this shape: the server's long-lived identity key (the trust root, also used for sender certs) signs almost nothing — just sender certs daily and a delegation cert on rare rotation. Adminbot's key is the "hot" attestation signer, but it has well-defined scope and is itself signed by the server. Compromising adminbot lets an attacker mint *attestations*, but not sender certs; compromising the server identity key is still the catastrophic case but it's used so rarely that defense-in-depth (HSM, offline storage) becomes easier to justify later.

**The delegation cert.** `ServerDelegationCert` is issued by the server at adminbot bootstrap and on adminbot key rotation, and re-issued daily for freshness (same daily refresh cadence as sender certs — see `docs/03-groups.md` §3.11). Shape:

```protobuf
message ServerDelegationCert {
  string server_hostname     = 1;
  string adminbot_did        = 2;
  bytes  adminbot_pubkey     = 3;  // public half of adminbot's attestation-signing key
  int64  issued_at_ms        = 4;
  int64  expires_at_ms       = 5;  // typically issued_at + 7 days
}
```

Endpoint:

```
POST /v1/admin/delegation-cert                 (adminbot only)
Body: { "adminbot_pubkey": base64 }
Response: {
  "delegation_cert": base64,                   // serialized ServerDelegationCert
  "signature":       base64                    // trust-root signature over the cert
}
```

Server logic: verify caller is adminbot; verify proof-of-possession of `adminbot_pubkey`; build the cert with `server_hostname` from config; sign and return. The server has no policy say in what adminbot then chooses to attest to.

Adminbot's **attestation-signing key** is separate from its messaging identity keys — distinct keypair, dedicated purpose. Rotating the attestation key triggers re-fetching the delegation cert; rotating the messaging identity key (e.g., on device re-register) doesn't.

**Adminbot's official-bot list.** A Project-local table:

```
adminbot.official_bots:
  bot_did       TEXT PRIMARY KEY
  display_name  TEXT
  purpose       TEXT
  expires_at    TIMESTAMP    -- the most recent attestation's expiry
```

Modified by chat commands in `#admins`: `/officialize <bot_did> [--purpose ...]` adds; `/unofficialize <bot_did>` drops.

**Attestation issuance.** Daily, adminbot loops through its `official_bots` table, builds the payload, signs it with its attestation key (no server round-trip), and delivers `(delegation_cert, delegation_cert_sig, attestation, attestation_sig)` to the bot. The bot stores it locally and updates its profile:

```protobuf
message OfficialAttestationBundle {
  bytes delegation_cert     = 1;
  bytes delegation_cert_sig = 2;
  bytes attestation         = 3;
  bytes attestation_sig     = 4;
}

message OfficialBotAttestation {
  string subject_did   = 1;
  string display_name  = 2;
  string purpose       = 3;
  int64  issued_at_ms  = 4;
  int64  expires_at_ms = 5;
}
```

Profile is re-uploaded after each refresh; per `docs/35-contacts-and-profiles.md`, profile_version is bumped and contacts pick up the new copy on their next fetch.

Adminbot self-officializes at bootstrap (same code path — adds itself to its own `official_bots` table, runs one issuance cycle). No special-cased "implicitly official" logic.

**Revocation.**
1. `/unofficialize <bot_did>` — drop the row; existing attestations expire within a day.
2. `/pause <bot_did>` — freeze that bot's capabilities and invalidate its sessions server-side.
3. **Rotate adminbot's attestation key** — fresh delegation cert + re-issue. Old-key attestations stop verifying. Use this if adminbot's attestation key is compromised.
4. **Force-rotate the server's trust root** — invalidates everything. Heavy operator-shell action.

**Delivery method.** Push (adminbot DMs `OfficialAttestationDelivery` on a daily timer) vs pull (bot DMs `/refresh-attestation` near expiry). Both work; choose one when the bot SDK lands.

**Cross-server.** Not attempted. A federated guest seeing a remote server's delegation cert gets nothing useful — they have no reason to trust an unrelated server's trust root.

### Auto-accept invites from official bots

If the inviter's profile carries a valid `OfficialAttestationBundle` whose delegation cert is signed by the hosting server's trust root and whose attestation is not expired, the client auto-accepts the invite. There is no preference toggle and no per-inviter override.

1. Resolve the sender's profile (`docs/35-contacts-and-profiles.md`).
2. If `official_attestation` is present and the chain verifies: call `accept_invite_async` immediately and surface the new group in the Chats tab.
3. Otherwise: regular accept-or-decline UX.

Cross-server safety falls out: a federated guest whose pinned trust root differs never validates the attestation.

A subtle one-line indicator on the group thread for ~24h after auto-accept ("Added by Adminbot · Leave group") makes the auto-add legible.

**Invite-trust grants (further future).** The current code treats "is this an official bot?" as both the identity claim and the auto-accept decision. These are conceptually different. When a real product need shows up (Project onboarding, per-contact "always accept Carol's invites"), a generalized `invite_trust_grants(inviter_did, scope, permissions, source)` table earns its existence. Don't design speculatively; record the split.

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
/install-project <name> <url>          install + start a Project's bot
/uninstall-project <name>              uninstall (revokes capabilities + tokens)
/grant <bot_did> <capability>          grant a capability to a bot
/revoke <bot_did> <capability>         revoke
/officialize <bot_did> [--purpose ...] add to official-bots registry
/unofficialize <bot_did>               remove from registry
/list-projects                         list installed Projects
/list-officials                        echo the local officials list
/pause <bot_did>                       freeze that bot's capabilities
/unpause <bot_did>                     reverse /pause
/kick <did> from <group>               adminbot removes <did> from <group>
/add <did> to <group>                  adminbot invites <did> to <group>
/version                               report current server build + pending updates
/rotate-attestation-key                regenerate adminbot's signing key + fetch new delegation cert
/seed-into <group_id>                  adminbot joins the group as Admin
```

Destructive operations gate on reaction-based confirmation: adminbot replies "React 👍 within 60s to confirm" and only acts on the reaction. Makes commands legible in group history (auditable) and protects against typos.

The Server Console — adminbot's chat-command interface — is structurally just a Project. Adminbot installs itself as the very first Project at bootstrap.

### Reserved-name protection for `#admins`

The group title `#admins @ {server_hostname}` is a reserved string on each server, but the server has no way to enforce uniqueness because group state is encrypted. Protections:

- The legitimate `#admins` is created by adminbot, who is an official bot. Auto-accept and the verified-badge UX kick in only for invites from official bots.
- Long-term, an **official-groups** registry: adminbot signs `OfficialGroupAttestation { group_id, display_name, purpose, expires_at }` and bundles it with the current delegation cert. The signing primitive already exists; the open part is the publication path (embed in the encrypted group state? maintain a server-published-but-adminbot-signed index?). Defer until the threat is concrete.

### Leave/rejoin flow

A human can leave `#admins` like any other group, via `remove_members(self)`. After the leave applies, adminbot:

1. DMs the leaver: "You've left the admins group. If this was a mistake, reply with `rejoin` and I'll add you back." Includes a deep link.
2. On `rejoin`, adminbot issues a fresh `invite_members`. Client auto-accepts (official-bot rule), user is back.

No special handling for "last admin leaves" — same DM is sent. If they ignore it, the server's `#admins` ends up empty until someone acts on the rejoin link. Operator-shell recovery remains the fallback for adminbot being permanently unreachable.

### Configuration (full)

Beyond v1's hardcoded "invite everyone to `#admins`", per-server config (welcome message, channel sets, invite-token routing rules) lives in adminbot's own process — file on disk, or a Project-internal DB.

```toml
[adminbot]
welcome_message = "Welcome to Safe Haven! These are our main channels."

[[adminbot.rule]]
# Invite everyone who joins.
match.always = true
groups = ["did:plc:safe-haven-announcements", "did:plc:safe-haven-general"]
notification = { level = 1, mute_until_ms = 604800000 }

[[adminbot.rule]]
# Invite only users whose invite token came from a specific tag.
match.invite_token_prefix = "regional-northeast-"
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
| `POST /v1/admin/projects` | adminbot only | Install a Project (creates bot account, grants capabilities). |
| `GET /v1/admin/projects` | adminbot only | List installed Projects. |
| `DELETE /v1/admin/projects/{id}` | adminbot only | Uninstall. |
| `POST /v1/admin/delegation-cert` | adminbot only | Issue a `ServerDelegationCert`. Daily refresh + on key rotation. |
| `GET /v1/server/build` | any authenticated session | Current server commit / version / build time. |
| `POST /v1/admin/capabilities` | adminbot only | Grant a capability. |
| `DELETE /v1/admin/capabilities/{account_id}/{capability}` | adminbot only | Revoke. |
| `GET /v1/admin/events?since=<id>&kind=...` | bot session w/ capability | Paginated catch-up for missed events. |

New WebSocket frame variants (`ws.proto`):

| Variant | Direction | Purpose |
| --- | --- | --- |
| `AccountJoinedEvent` | server → bot | Push for `subscribe.account_joined`. |
| `ServerBuildEvent` | server → bot | Push on server (re)start. |

New DB tables:

| Table | Where | Purpose |
| --- | --- | --- |
| `project_capabilities` | homeserver | `(account_id, capability, granted_at, granted_by)`. |
| `server_events` | homeserver | `(id, kind, payload, created_at)` — append-only, drained by `GET /v1/admin/events` and WS push. |
| `adminbot.official_bots` | adminbot's DB | `(bot_did, display_name, purpose, expires_at)`. |
| `adminbot.attestation_key` | adminbot's DB | Adminbot's attestation keypair + most recent delegation cert. |

---

## Open questions

1. **Bot identity-key rotation telemetry.** Adminbot rotating its identity keys is transparent (DID is the pin). But the client-side trust UX should probably surface a safety-number-like change indicator when adminbot's identity key rotates — same as any contact. Confirm this is in scope for the rotation UX work or treat as a follow-up.

2. **`did:local:` resolution.** `docs/02-todos-deferred.md` mentions the concept but doesn't define a DID document shape or resolution path. Probably resolved by "the homeserver's accounts table is the DID document for `did:local:{this-host}:*`" — simplest possible thing. Confirm when `did:local:` lands as a real feature.

3. **Default-mute discoverability.** A user auto-joined into 8 muted channels may never look at them. Channel discovery + "you have unread mentions" surfaces in the Chats tab will mostly handle this, but worth checking once the UX is in users' hands.

4. **Server-event privacy disclosure.** A compromised bot with `subscribe.account_joined` gets a real-time roster of everyone who joins this server, including timing. The threat model accepts this (the operator authorized the bot; the server already had the data). Worth a one-line acknowledgment in `docs/00-design.md` "Trust model" when this lands.

5. **Cross-server official bots.** Stage 5 doesn't handle federated guests treating a remote server's official-bot list as trustworthy. Probably resolved by *not* trusting it cross-server. Confirm once federation lands.

6. **Recovery from `#admins` ghosting.** If every admin leaves and nobody acts on adminbot's rejoin DMs, the server is frozen. The current answer is "operator shell." Worth thinking about whether a backup recovery DID (dormant, operator-pinned, promotable out-of-band) is worth the complexity. Defer.

7. **Push vs pull attestation delivery.** Adminbot ships fresh `(payload, signature)` pairs to each official bot daily. The mechanism (push, pull, or both) is left open until the bot SDK lands.

8. **Event durability across adminbot restarts.** v1's `AccountJoinedEvent` push is fire-and-forget — if adminbot is disconnected, the event is lost. The future `server_events` table + `GET /v1/admin/events?since=…` catch-up endpoint fixes this, but is deferred. Worth checking how painful the gap is in practice; if registrations happen while adminbot is restarting during the demo, we may want catch-up sooner.
