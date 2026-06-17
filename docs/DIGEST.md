# Architecture Digest (compressed)

> Dense summary of the whole `docs/` design for fast context-loading. Preserves
> decisions, their rationale, and rejected alternatives; drops TODO lists, step-by-step
> deployment commands, and UI minutiae. When a detail matters, go to the source doc
> (numbering in parens). Status tags: ✅ built · 🚧 partial · 📐 design-only.

## 0. Product premise & goals (`00`, `01`)

Activism social network: **activism is the acquisition vector, social experience is retention.**
Install because a Project (canvass, rally, phonebank) requires it; stay for the friendships.
Thesis: *building Projects is now easy; building Signal-quality encrypted comms is still hard* — so
build a boring, reliable encrypted **substrate** + many **Projects** on top. **App must feel like
Signal, not Slack/Discord:** primary surface is a unified inbox across all servers, sorted by recency.

Two governing technical principles: **don't implement crypto — use libsignal**; **make whole vuln
classes impossible** (Rust memory safety for all security-critical code). All code open-source;
pre-launch third-party audit; reproducible builds; `cargo audit`/`deny` in CI.

Three app tabs (Signal-style): **Calls**, **Chats** (default, unified inbox), **Network** (servers →
their Projects; Projects open full-screen webviews with own nav).

## 1. Threat model (`00`)

Tuned for two threats:
- **Server seizure** — a seized homeserver must not yield contacts, memberships, message history, or
  real names. Users reconstitute identity+connections elsewhere. → E2E everywhere + message expiry +
  encrypted profiles + structural membership opacity.
- **Surveillance** — limit cross-server linkage of persistent identities; membership lists are a
  targeting vector. → per-server push pseudonyms, encrypted profiles, selective federation, optional
  PLC home-server omission.

**Not** hardened against state-actor surveillance of high-risk individuals (no onion routing / cover
traffic / mixnets) — left to user (Tor/VPN). Network traffic analysis beyond TLS is out of scope.
This is the deliberate target, not a limitation to fix now.

## 2. Identity / terminology (`00`, `50`) — keep these distinct

- **Identity** = a **DID** (`did:plc`, same method as Bluesky → portable across both networks). The
  cryptographic identity a person controls; holds the long-term identity key. **Separate identities
  are the compartmentalization boundary** (deliberately unlinkable personas).
- **Account** = an **(identity, server) pair** — one DID registered on one homeserver. Server-side
  rows keyed per account.
- **Device** = one app install of an identity. Shares the identity key; keeps its own per-device
  session/prekey/sender-key state. Registers an account on each server the identity uses.

Durable user data (contacts, group keys, settings) is **identity-scoped**: synced across the
identity's devices, replicated across its accounts, never shared across identities.

**DID design (`50`):** DID = `f(derived_rotation_pub, server_url)` — identity key deliberately
**omitted** from genesis op so the DID is recoverable from passkey + signup-server alone. Two PLC ops
at signup: genesis (rotation key only) then update (adds random per-device identity key). Rotation key
is the root authority (changes signing keys / endpoints / transfers DID); signing keys are day-to-day.

**Recovery authority = the passkey** (or written phrase). Rotation key + recovery-blob key are
**deterministically derived** from the WebAuthn PRF output via HKDF labels `"actnet-rotation-v1"` /
`"actnet-blob-v1"` — never stored on a server. RP is a universal avalanche domain (`theavalanche.net`),
so only official apps can recover. `user.id`(userHandle) = signup server URL → recovering device
reconstructs the genesis op (and thus the DID) with no prompt; `user.displayName` is cosmetic.

**Recovery blob = convenience, not authority.** Server-cached ciphertext (AES-256-GCM under the
PRF-derived blob key), replicated to all the user's homeservers, holds: device identity keypair,
server list, profile key + display name, **and (historically) group master keys**. Losing every copy
costs session continuity (safety-number change), the server list, and per-group sender-key continuity
— but **not** DID control (always recoverable from passkey). Blob key cached in SQLCipher so routine
state changes re-upload silently (passkey only needed at create + recover). **Group keys are moving
out of the blob into the storage service** (see §7), shrinking the blob to a near-constant keyring →
enables a tight `MAX_RECOVERY_BLOB` cap (not yet enforced; sequenced after group-key sync is sole path).
`GET /v1/recovery/{did}` is **unauthenticated** (opaque ciphertext).

Recovery has two paths: **blob path** (common: restore identity key, server list, group keys; no
safety-number change) and **no-blob path** (fresh identity key via rotation-key-signed PLC update;
DID preserved, safety-number changes, server list lost). Passkey alone always reaches the no-blob path.

**Membership privacy levers:** no default DID enumeration; auth-gated existence checks; PLC home-server
URL optional (omit for privacy); encrypted profiles; per-server rotating push pseudonyms; selective
federation. (Open-membership servers still leak existence to anyone who joins.)

## 3. Crypto stack & repo (`01`, `11`)

libsignal pinned at commit `4c460615` (git dep, not branch). Gives Double Ratchet (FS for 1:1 +
sender-key groups), X3DH (async session init via prekeys), sealed sender, zkgroup anonymous credentials.
Primitives: X25519, Ed25519, AES-256-GCM (ChaCha20-Poly1305 fallback), HKDF-SHA-256, Ristretto255
(zkgroup), Argon2id. **Attachments deliberately use AES-256-CBC+HMAC** (Signal-exact, for incremental
verification of large files) — the one divergence from the app's default AEAD.

**Crate graph:** `types ← crypto ← store ← net ← app-core`; `server` and `app-core` both use `store`;
mobile crosses the UniFFI boundary at `app-core`. Also `relay`, `federation` (stub), `project-sdk`,
`test-utils`. Repo is a monorepo: Rust `core/`, `mobile/{ios,android}`, `node/` (napi bindings, bots),
`projects/`, `infra/`, `docs/`.

**Load-bearing patterns (also in root CLAUDE.md):**
1. `crypto` has **no I/O** — defines a `Store` trait; `store` implements it (SQLCipher via
   tokio-rusqlite); `app-core` wires them.
2. `store::Store` is **Clone, Arc-backed single connection** serialized on one blocking thread —
   load-bearing for libsignal's multi-`&mut` sub-trait API. **Do NOT replace with a pool.**
3. Server DB fns take `&mut PgConnection` (callers `acquire()` or `begin()`) → transaction-rollback tests.
4. UniFFI exports are **sync**, blocking on a global `OnceLock<Runtime>` (libsignal futures aren't Send);
   tests use `_async` variants.
5. `AppCore` uses `Mutex<AppCoreInner>` (UniFFI wraps in Arc).
6. Two error types: `AppError` (rich) and `AppErrorFfi` (string reasons).
7. **Default to Signal's approach** for crypto/protocol/UX; diverge only where needs require (DIDs,
   federation, Projects, multi-account).

**Message envelope** = protobuf `ContentMessage` in `core/proto/content.proto`. Body `oneof`: text,
receipt, group_context, sender_key_distribution, group_message, timer_change (more reserved). Cross-cutting
fields: `timestamp_ms`(15), `profile_key`(17), `profile_version`. Forward-compat by reserved field numbers.
`types` generates Rust via prost; mobile generates Swift/Kotlin from same `.proto`.

**Calls (`01`, Stage 8, not built):** substrate-level. 1:1 = WebRTC P2P (server is signaling only over
WS), STUN/TURN, DTLS-SRTP. Group = LiveKit SFU + WebRTC Insertable Streams (E2E; SFU forwards ciphertext).
Broadcasts = LiveKit one-to-many, distinct UX from calls. LiveKit is the one extra deployable besides PG.

## 4. Federation model (📐 `13`, federation crate is a stub)

People register on a homeserver (their org/campaign/community). Servers federate so users can find each
other, DM, form ties across servers. Posture aims higher than Matrix: your home server knows your social
graph (you trust it), other servers learn as little as possible cross-boundary; E2E everywhere; **selective
federation** (operator allowlist). Origin auth via Ed25519 server keys published at
`.well-known/actnet-server` — **not** a peering handshake; semi-open by default, abuse-gated by attestations.

**Routing (multi-homing):** every DID has exactly **one discovery server** (in PLC, "home") + zero-or-more
**member servers**. Each member server holds the user's per-server prekeys, queue, WS. **Key consequence:
same-community conversations never federate** — if all members of a group are on `safe-haven.org`, all
traffic stays there. Federation only enters at account-creation and for DMs crossing boundaries.
- **Default route:** send via sender's discovery server S → S checks if recipient R is local (deliver, no
  federation) else resolve R's DID → R's discovery server → federate.
- **Learned route:** when you receive from C via member server X, record "for C route via X"; converges to
  no-federation after one round trip each way. Only unhandled case: A&B share a non-discovery server
  neither routes through (federate forever) — accepted as rare.
- Prekeys per-server (OTPKs partition naturally; only the identity key is shared, signing every bundle).
- Migration (discovery-server change) is **Settings-only**, rare; memberships persist across it; DID +
  identity key + sessions + local history unaffected. The user's *signed* migration record is authoritative,
  not the old server's say-so (old server can't block it).

**QR / link types** (URL path is the discriminator, opaque base64url token resolved server-side):
`/contact/<t>` (add contact, open chat), `/invite/<t>` (server-join trust-delta screen then join),
`/project/<t>` (join server if needed, then Project). Deliberately no "also join their server?" prompt on
contact-add, no migrate option in invite flows.

PLC is a centralization point (Bluesky-operated) — assumed working. DID-resolution caching is dangerous
(migration staleness) → short TTLs + signed move records.

## 5. Groups (`03`, the deepest doc) — ✅ much built, Stage 5/9

**Two group types (one of the most important product choices):**
- **Action-bound** (single-server, rich): tied to a Project; full roles/vetting/moderation; Signal-private-
  group crypto guarantees (zkgroup anonymous credentials), but issuer is the Project's homeserver. Can be
  announcement-only. Federated users join as **guests** (deferred). **This is what's built.**
- **Cross-server casual** (small <~50, peer-managed, Stage 9): ad-hoc E2E groups via **Sender Keys with
  fan-out** (no central issuer). Basic chat only, no rich state/moderation. **Rule: if a group needs an
  admin, it needs a homeserver.** MLS deferred — could later swap inside `crypto::groups::groups` behind
  the same `encrypt`/`decrypt` interface, no API change.

**Message expiry** is substrate-level (not a Project option): timer in encrypted group state; clients
delete on schedule; server deletes its copy on same schedule; **server can't extend retention** (its own
backstop is 30-day undelivered-slot TTL, matching Signal). Action-bound default ~30d, casual ~7d.

### zkgroup identity-attribute decision (`03` §2.3–2.4) — important rejected-design history
zkgroup's `AuthCredentialWithPniZkc` is hardcoded to `(Aci, Pni)` = 16-byte UUIDs; we have variable-length
DIDs. **DECIDED option 1:** `UUID(did) := SHA-256("actnet-did-to-uuid-v1" || did)[..16]`, carried as
`Aci::from(UUID(did))`, so **stock zkgroup primitives work as-is**.
- **Originally chose option 2** (build a DID-shaped credential on `zkcredential`, ~500 LOC, shipped in
  step 5), then **switched to option 1** when the same DID↔UUID mismatch began repeating for
  `GroupSendEndorsement` (and would for every future zkgroup primitive). Re-analysis showed option 2's
  claimed advantages (tighter DID binding, collision resistance) were illusory: server opacity, cross-group
  unlinkability come from the *per-group encryption key*, identical in both; 128-bit collision matches
  zkgroup's own Aci and is restricted to DIDs already in the system; clients cross-check via cleartext
  members list. Switching deleted `crypto::groups::credentials`, `DidStruct`, `DidEncryptionDomain`.
- Rejected option 3 (blind-sig bearer tokens) = strictly worse anonymity.
- `app-core` API stays scheme-agnostic (`encrypt_member_id(&str)`) so MLS swap stays possible.
- ServiceId migration: identity + session stores keyed on `Aci::from(did_to_uuid(did)).service_id_string()`
  (SSv2 parses `ProtocolAddress.name()` as a ServiceId).

### Encrypted group state (`03` §3)
- **Encrypted state blob** (opaque to server, source of truth for clients): group identity, members
  `(did, encrypted_member_id, role, joined_at, profile_key_ciphertext)` with **did in cleartext inside the
  blob** so clients render names, metadata, policy, monotonic revision `u64`.
- **Server-visible routing subset** (the minimum to enforce membership/route): `member_credentials`,
  `members_pending`, `members_pending_approval`, `group_policy` — following Signal's
  MemberPendingProfileKey / MemberPendingAdminApproval / access-control model.
- Server stores blob + 256-revision **history ring buffer** (for catch-up bandwidth, historical UI, and
  **tamper detection** — clients walk `(state_N, change, state_{N+1})` verifying each is the legit
  continuation; the load-bearing reason to retain history).
- **CRITICAL §3.9 schema discipline (membership opacity is structural, no at-rest encryption needed):**
  the property holds *iff* the server keeps no auxiliary DID↔group links. Rules: no `(did→groups)` table/
  persisted cache ever; no `(encrypted_member_id→did)` map; credential issuance not logged with credential
  id (per-DID-per-day rate counters OK); presentation verification logs counts only; `member_credentials`
  timestamps jittered/omitted. **Tables carry NO did/account_id column.** Operational "remove DID from all
  groups" is *not available server-side* — must be client-driven. These are **test-enforced** (`03` §9:
  migration schema-annotation audit, forbidden-column/forbidden-join lints, logging AST audit, send-endpoint-
  unauthenticated test, transactional-writes audit). `03` §8 is a standing PR-review threat checklist.

### Group changes (`03` §3.3–3.6)
A `GroupChange` = `{revision, actions, presentation}`. Actions are **partly server-visible** (which ops,
which encrypted_member_ids, roles, policy values) **partly sub-encrypted** (title/description/expiry/profile-
keys). Self-class actions (promote/decline/join_via_link/cancel) must be the sole action; admin-class
batch. Server checks: presentation valid → actor-eligibility by class → revision freshness
(`==current+1`, else 409) → role check vs `group_policy` (`modify_policy` & `modify_member_role` are
**protocol-fixed Admin** to prevent privilege escalation) → transactional apply + revision bump + history.
**Actions are declarative** (`remove_members` = "ensure absent", idempotent) so 409-retry is clean.
**Layered enforcement:** server enforces what it sees (membership/role/policy); clients re-verify on apply
against the authoritative blob (catches server compromise/bugs). Fetch is **membership-gated → 404 (not
403) for non-members** to hide group existence. The master key alone grants almost nothing without a
membership-gated server fetch (can't read state, content, or forge credentials).

### Join flows (`03` §3.10): invite (admin→members_pending→invitee promotes with profile_key+pseudonym),
request-to-join, open-link self-join — unified `join_via_link` action, **server picks** immediate-add vs
pending-approval vs reject based on `join_policy` (client shows neutral "Join", renders the outcome).
Invite delivered as a normal substrate DM carrying `GroupContext {group_id, master_key, hosting_server_url,
inviter_did}`; first-contact works via X3DH PreKey message, no new infra. Invite links safely carry the
master key (Signal does this; leaked link gives a passive observer nothing). `invite_link_password` is
rotatable via `modify_policy`; master key isn't realistically rotatable.

### Delivery & push (`03` §3.7): per-`member_credentials` row carries a `group_push_pseudonym` **distinct
from any DM pseudonym, no shared join key**. Online = client sends `subscribe{pseudonyms}` WS frame →
server holds **in-memory** `pseudonym→ws` map (never persisted; rebuilt per reconnect). Offline = relay
wakeup by pseudonym. **Live-memory caveat:** while connected, server transiently knows account↔pseudonym↔
group; never persisted (cold seizure yields nothing) — same tradeoff Signal accepts. Claim-squatting
defense = allow concurrent subscribers, rely on at-recipient decryption failure (option (b); option (a)
would violate §3.9). Pseudonyms rotate 7d with per-group hash offset to avoid correlated bursts.

### Sender opacity for sends (`03` §3.11, ✅ built): three layers.
1. **Envelope** = `sealed_sender_multi_recipient_encrypt`, one slot/recipient-device, wrapping a Sender-Key
   ciphertext + a homeserver-minted `SenderCertificate` (trust-root chain in v3 `GroupCryptoBundle`, pinned
   by clients on first contact).
2. **Dedicated endpoint** `POST /v1/groups/{id}/send` that **rejects session credentials** — auth is a
   single `GroupSendFullToken` (combined endorsements) over the recipient ServiceId set. Server resolves
   each `encrypted_member_id→pseudonym`, verifies token against ServiceIds (never sees DIDs), enqueues to
   `group_message_queue`, discards all connection metadata (IP-rate-limited only, never logged). Test-enforced
   (`03` §9 invariants 4 & 6).
3. **Network layer** (source-IP correlation) explicitly out of scope; mitigation = user Tor/VPN. Matches
   Signal's sealed-sender threat model.
   Daily credential refresh split: `POST /v1/groups/credentials` (session-auth, identity-scoped: auth
   credential + per-device sender cert) and `GET /v1/groups/{id}/endorsements` (presentation-auth, group-
   scoped). Anonymity is at *send* time, not *refresh* time. Rate limiting under anon auth: per-group +
   per-recipient endorsement budget + per-IP. Abuse reporting needs *selective* sender disclosure (recipient
   reveals one message's sender cert) — full design deferred to `12`.

Mesh (§7): action-bound groups work on bitchat in steady state (Sender-Key content flows unchanged; sender
auth via signed SKDMs); state mutations / credential refresh / endorsements / sealed-sender don't (need
server) — queued for reconnect. Use **per-sender** tag derivation, not per-group master key (avoids leak).

## 6. Multi-device (`04`) — substrate ✅, app-level 🚧

**Central distinction:** identity key is a **static credential → SHARED** across devices (provisioned at
link, not minted per-device — matches Signal). Sessions/prekeys/sender-keys are **stateful ratchets →
PER-DEVICE** (sharing a running ratchet breaks FS, desyncs counters, reuses keys). One-line version: copy a
static credential, never a running ratchet. **Membership is per-identity (per-DID); only delivery/encryption
fans out per-device** in the send path.

Built per-device today: registration, prekey fetch, 1:1 + group send fan-out, per-device queues, stale-
session reconciliation via registration-id comparison. `device_id` is a routing label only, carries no key
material. Not built: device linking, sync, history backfill, revocation, device-set-change UX.

**Device linking** (📐, model decided): new device shows QR (ephemeral pubkey + provisioning addr); existing
device encrypts shared identity key + storage key to it over the relay-mediated-but-E2E provisioning channel
(server sees only ciphertext); new device registers its own device_id/prekeys/sessions. **All devices co-
equal, no "primary"** (Signal model — possession of any one device authorizes a link; conscious choice).
Linking reuses recovery-blob crypto.

**Three sync channels (`04` §5), deliberately cap the sync-message-type count** (Signal accreted ~20
SyncMessage variants before moving durable state to a Storage Service — we commit to the capped model up
front):
- **Conversation** (recipients also see it — text, reactions, edits, deletes, timer): rides a **Sent
  transcript** wrapping the ContentMessage verbatim → new content types sync for free, zero new plumbing.
- **Durable** (current-value, only your devices: mute/pin/archive, contacts+nicknames, blocked list, group
  master keys, settings, profile): the **storage service** (§7), per-record LWW snapshot, not deltas.
- **Device-local** (theme, notif sound, biometric lock): never synced.
- Residual thin **event types** (`SyncRead`, `SyncViewed`, `SyncLocalDelete`) — the only category that ever
  adds a sync type, near-closed. Transport = a normal pairwise DM **to yourself** (no sealed sender).

**Recovery vs linking** differ by the **aliveness assumption**, not device_id: linking is **additive**
(existing device alive, both coexist); recovery is **total** (no device survives → revoke the identity's
*entire* prior device set across *all* its accounts and register one fresh device). Current
`POST /v1/devices/replace` is only a single-slot swap (OPEN: whole-identity, cross-account reset). History
backfill = explicit **non-goal** for v1 (matches Signal). Device-set-change doesn't break safety number
(shared identity key) — accepted weakness; optional "Bob added a device" info event from diffing reg-ids.

## 7. Storage service / device-data-sync (`05`) + identity/device store split (`06`)

**Storage service (`05`, server stages ✅, client snapshot/fast-sync 🚧):** how durable identity state stays
consistent across devices and survives total loss. Model = **domain tables + sync sidecar + adapters** (goal:
adding a synced type = one domain table + a small `SyncedType` adapter + one-line registry add; CAS/cursor/
conflict/encryption/backup/recovery all handled). **No payload duplication** — sidecar holds only version/
dirty/tombstone; payload read from domain table on demand. **Dirty-tracking via SQLite triggers** (can't be
forgotten/bypassed), generated from the registry; **scheduling via a single rusqlite `commit_hook`** that
pokes the sync task; periodic poll is the safety net — **zero per-write-path code**. `record_id = HMAC(
storage_key, TYPE_TAG||logical_key)` (opaque, deterministic). **Server sees only opaque ciphertext**, enforces
byte/count quotas only (~4–8MB/account, ~8KB/record, ~10–25k records).

**Conflict = per-record last-writer-wins** (no CRDT/OT/vector clocks) — DECIDED, safe because the data
cooperates (single-user, low-contention, records independent, mostly immutable/monotonic).

**Placement = ONE authoritative account + passive backups (DECIDED: explicitly NOT multi-master).** Live
reads/writes go to the discovery server's account; other accounts hold one-way encrypted snapshots
(`PUT/GET /v1/storage/snapshot`, LWW on snapshot_version). Cost accepted: seizure of authoritative server
pauses live sync until you promote a backup. Backing storage may be S3/R2/GCS; a *consumer* cloud
(iCloud/Drive) is explicitly NOT the substrate (re-centralizes on a subpoenable party, leaks DID↔platform).

**Storage key** = 32-byte identity-level key, provisioned at link, carried in recovery blob. **Its presence
is the single opt-in signal** — bots have none, so they no-op all sync. **Group master keys move out of the
recovery blob into the store**, making the snapshot path load-bearing for total-loss recovery.

**Store split (`06`, client split ✅):** one SQLCipher store conflated per-device crypto with per-identity
durable state. Split into:
- **`DeviceStore`** (device.db) — `crypto::Store` impl: sessions, all prekeys, sender keys, push state,
  per-server credential caches, registration_id, storage_cursor. **Never synced; fully rebuildable.**
- **`IdentityStore`** (identity.db) — durable per-identity state + identity/rotation/storage keys + the
  `storage_sync` sidecar + trust store (`known_identities`, synced). **Synced via storage service,
  snapshotted, bootstrapped from recovery blob.**
- **Boundary-crossers** (identity keypair, rotation key) live in IdentityStore but are consumed by device
  crypto; bootstrapped via blob/provisioning, not the storage service (chicken/egg: can't fetch your
  identity key from a service you authenticate to with it).
- **`AppCore` = one identity (DECIDED).** Owns one IdentityStore + 1..N account contexts (DeviceStore +
  server client + role authoritative|backup). Bots are N=1 with the two-file split hidden behind the
  constructor. Cross-identity aggregation (e.g. contact autocomplete) lives **above** AppCore, read-only,
  at IdentityStore granularity. Both DBs encrypted at rest with the device/enclave key (NOT the storage key
  — storage key is record-level only; it lives *inside* IdentityStore so can't gate the file). Event log
  (`message_history`, reactions, revisions, read marks) is a third concern, roams via the event channel —
  store placement deferred.

## 8. Server implementation (`10`, `11`) — ✅ Stage 2

Axum + Tokio + PostgreSQL via sqlx (compile-time-checked queries, offline `.sqlx/` checked in). **No Redis,
no libsignal on the server** — it stores/relays opaque `bytea`. Internal bigint PKs, external API uses
DIDs + device_id. Schema: `accounts`(did, profile_blob), `did_documents`, `devices`(per
(account,device_id): identity_key, registration_id), `session_tokens`(opaque, not JWT — revocable),
signed/one-time/kyber prekeys, `message_queue`(bytea, 30d TTL, message_kind), `push_pseudonyms`,
`rate_limit_counters`(per-account sliding window, in-process; PG advisory locks for multi-instance).

Auth = two-step challenge-response: `POST /v1/auth/challenge` (nonce) → `/auth/token` (Ed25519-sign nonce).
**Token issuance is identity-scoped; membership check is on token *use***, so a 401 (re-auth) is
distinguishable from a 403 (kicked) — see §11 connection state. WS at `GET /v1/ws?token=` (query param,
browsers can't set WS headers); binary `WsFrame` protobuf, either side originates, `frame.id` correlation;
variants Send/Deliver/Ack/Keepalive/PrekeyLow (+ group + admin frames). HTTP message endpoints remain as
fallback. Background tasks: message+token expiry, prekey vacuum, rate-limit cleanup. One-time prekeys
consumed via atomic `DELETE...RETURNING`. did:plc stub locally (full PLC sync = Stage 9).

## 9. Push relay (`00`, `01`, `41`) — ✅

iOS/Android: only APNs/FCM can wake a backgrounded app. App developer runs a **push relay**: homeservers
send content-free wakeups to per-(user,server) **pseudonyms**; relay maps pseudonym→device token, fires an
**empty** payload; app wakes and fetches itself. Apple/Google see only "app pinged"; relay sees pseudonym
timing but no identity/content/cross-server linkage; homeservers never see the device token. Pseudonyms
rotate. Protocol supports multiple relays from day one (swappable, not a privileged singleton). High-risk
users can opt out → manual fetch. Avalanche relay = `https://relay.theavalanche.net`; servers point via
`RELAY_URL`. Relay state = tiny SQLite (pseudonym→token, 7d TTL); ~$4/mo droplet, losing the DB just forces
re-registration. One relay serves sandbox+production APNs, routed by client-supplied `environment`.

## 10. Projects framework (`20`–`24`, `23`) — testbot+adminbot ✅, framework 📐 Stage 6

**A Project** = standalone service that (1) serves a web UI opened in an app webview, (2) owns **bot
accounts** that are full Signal participants. Because everything is E2E, any Project touching message
content/membership **must** use bots — the server can't mediate (no keys). **Bot visibility is a critical
invariant: a bot's presence in a group is always visible to all members; no silent observer mode.**

**Trust chain:** user trusts homeserver admin → admin installs/configures Project → user implicitly trusts
it (like a Slack workspace admin installing apps). **Scopes are admin-granted at install** (not per-user
runtime prompts — that would re-litigate the admin's decision and train reflexive "Allow"; and is partly
theatre since the admin-run server already knows the DID). Default-deny, least-privilege.

**Auth = homeserver-issued opaque Project tokens** (NOT JWT — no signing key to distribute, trivial
revocation). `POST /v1/project-token` (session-auth) → 1h opaque token; app opens `project_url/?token=`;
Project verifies via `GET /v1/project-token/verify?token=` → returns DID. One HTTP call, no crypto on the
Project side. **Rejected reverse-proxy design** (server forwards with `X-User-DID`): would expose all
Project traffic to the server (metadata/plaintext leak), widen blast radius, make the server a general
proxy. The three-legged flow keeps the server small.

**Webview is bridgeless (through Stage 6):** `WKWebView`/`WebView` sandbox; **no JS bridge** — I/O is URL
params in, intercepted deeplinks out (`theavalanche://compose/attach?url=`, `.../close`). Can't reach the
SQLCipher DB/keys/conversations. Origin-isolated per Project. A JS bridge would need a scoped-permission
system if ever added.

**Scopes (`20`):** identity (`pseudonymous` default / `real-did` / `magic-links` / `profile:read`),
messaging reach (`dm:initiate`, `dm:bypass-request`, `invites:auto-accept` — same-server only), client
surfaces (`surface:compose`/`slash-commands`/`emoji`, `message:context-on-action`). **Identity tier is
derived from the interaction model, not freely chosen:** any bot-bearing Project learns the real DID through
the messaging channel, so `pseudonymous` is only coherent for webview-only Projects. **"Officialness" is NOT
a trust primitive** — it decomposes into a same-server `official` flag (the ✓ badge, on the bot's account
record) + the `invites:auto-accept` scope; no signing, no attestation (earlier signed-attestation drafts
all rejected as overengineered).

**Multi-account compounds Project trust (`20`):** client renders Project surfaces from multiple trust domains
side-by-side. **Per-(account, server, conversation) scoping** is needed now (Stage 6 / multi-account, ahead
of federation): every client-visible surface is tagged with origin and shown only there; manifests are
**untrusted input** (sanitize, length-limit, homoglyph-guard, size-cap assets, attribute to (server,Project)).
A conversation lives on one homeserver/account; its affordances come only from that homeserver's Projects.

### Messaging-extensions boundary (`23`) — core vs Project
Three rules: (1) **explicit handoff only** — no ambient/compose-time Project access; (2) **1:1-DM litmus** —
must it work in a bot-free DM? → core; (3) **mechanism vs content** — core owns mechanism/surface/privacy,
Project contributes content/webview at a seam. Keep the in-conversation surface boring (native/auditable/
E2E); push real interactivity into an explicit full-screen webview.
- **Core:** reactions (`33`), replies/threading (`32`), read receipts/typing (`31`), `@`-mentions
  (on-device, body-range), generic link preview (sender-fetches/recipient-never), live location, **simple
  polls** (reaction-shaped PEER votes, client-tallied, E2E, DM-capable, no bot).
- **Split:** rich text (BodyRanges — bot-authored first), custom emoji packs, slash commands (a slash
  command is just a `TextMessage` a member-bot interprets — no wire change, autocomplete is a reminder).
- **Project (webview):** Giphy/stickers (return content via `attach` deeplink, sender-fetched, never auto-
  sent), cardstack survey (magic link → webview → bot posts result), message actions.
- **Explicitly NOT built:** inline interactive cards / in-feed form controls (redundant phishing-prone middle
  layer once a webview exists). Lightweight in-feed actions = reactions/replies a bot observes.
- **Magic links:** Project-issued self-authenticating links; the *clicking* device mints a scoped token at
  tap time, only for Projects on the clicker's vetted allowlist (no open-redirect). Carry no credential, so
  anyone can share them; beware the who-clicked beacon.

### First-party Projects
- **Testbot** (✅, `21`) — `node/packages/testbot/` TS bot on `@actnet/app-core`: web "Text Me" button →
  ephemeral bot DMs you, relays to Claude Haiku. Proof-of-concept for the Project model.
- **Adminbot** (✅ minimal, `22`) — server admin via chat. Two foundations: (1) **one superuser DID**
  (`did:local:adminbot`) pinned in server config; all `/v1/admin/*` check `caller==ADMINBOT_DID`. (2) An
  **`#admins` group** (regular E2E group) whose encrypted membership *is* the admin set — **the server DB
  doesn't reveal who has admin authority**. Adminbot is the bridge: it can read `#admins` (it's a member)
  AND is trusted by the server (the pin). **Two authorities:** operator authority (install Projects, grant
  caps, officialness — not seizure-sensitive, lives in server DB) vs social-admin authority (who can
  moderate — seizure-sensitive, lives in encrypted `#admins`). Rule: *the threat decides the home.*
  **Headless, outbound-only** (no web UI, no inbound surface) → location-independent, hard to seize (no
  public routing pointer; only holds off-box). **Coordination is data-carried, never bot-to-bot RPC**
  (`AccountJoinedEvent` push + catch-up + invite-token routing tags). Future caps: `subscribe.account_joined/
  left`, `registration.gatekeeper`. **Rejected:** bot-to-bot RPC/service mesh (liveness fragility), per-admin
  server-verified credentials (would leak admin roster, eroding the property `#admins` protects).
- **Vetted onboarding / gatekeeper** (📐, `24`) — gates account creation behind human vetting. Shaped oddly
  because the applicant has **no DID until the end**: front half runs out-of-band (email/SMS invite). Needs
  **closed registration** (`POST /v1/accounts` refused without a token validating against an installed
  gatekeeper, **fail-closed**). New cap `registration.gatekeeper` (any number of Projects hold it; each names
  its issuer; server pins issuer→signing-key). `#approvals` group modeled on `#admins`. Post-join routing
  rides the token's issuer+tags via the join event — central (adminbot maps tags→channels) or self-routing.
  **Rejected:** gatekeeper→adminbot imperative RPC (couples bots, splits authority); external form
  (Google/Typeform routes PII through an unvetted processor).

## 11. Messaging UX features

- **Read tracking (`31`):** per-message `read_at` timestamp (NULL=unread; future-proofs disappearing-msg
  timer start), unread count **derived not stored**. Scroll-position marking (SwiftUI ScrollPosition, no
  timer). `ReceiptMessage{DELIVERY|READ}` as encrypted DM to sender, 3s debounce. Delivery status
  sending→sent→delivered→read (Signal checkmarks). Read receipts opt-in per identity.
- **Reactions (`33`):** `(emoji, reactor, target)` tuple, small encrypted PEER message, never enters feed/
  creates a row. **One reaction per person per message** (Signal-style — replace/remove; rejected Slack's
  many-per as bloating clusters). Visible to all members; client-tallied; key on `(emoji,reactor)` →
  idempotent. Don't touch badge/unread. No reactions feed, no custom emoji (first cut), no private reactions.
- **Threading (`32`):** **every reply is a thread message; one knob = "surface to channel."** Default flips
  by shape: chat-shaped (DMs/casual) surfaced-on with **latent** thread structure (no thread UI unbidden);
  broadcast-shaped (channels/announcements) surfaced-off. Per-channel admin default deferred. **Why one
  primitive:** WhatsApp's confusion came from the same gesture behaving structurally differently across an
  invisible line; here only a *default* changes along a visible line — guessing wrong changes a default, not
  a mechanism. **Threads browser** lives in a **shelf** (chrome above the inbox, scrolls away) — protects the
  one-conversation-one-row Signal inbox; thread-only activity shows only on the Threads icon count, never
  bolds the channel row. **No double-counting by construction:** each message in exactly one unread bucket
  (surfaced→channel bucket, non-surfaced→thread bucket). Following = quiet by default (notify only if
  following+mentioned or surfaced). Promotion posts a **surface message** (new channel message referencing
  the original, own send-time/read-state) — keeps buckets clean, reading it cascades read to referent.
  Announcement groups gate top-level posting but not in-thread replies (surfacing = the post-to-channel cap).
  **Rejected:** two reply primitives (inline quote vs thread) — bakes the channel/chat line into the data
  model, lossy to reorganize.
- **Edit/delete (`36`):** two ops on one substrate, both target by `(author, sent_at)`, LWW with **delete
  absorbing** (tombstone beats any edit regardless of timestamp). `EditMessage`/`DeleteMessage` as new
  oneof variants. **Load-bearing rule: a recipient applies an edit / FOR_EVERYONE delete only if the
  authenticated sender == target's author** (FOR_ME exempt — local-only). Server can't enforce (sealed
  sender). Human limits: 24h window, ~10 edits (client-honored). **Bots: no cap, 30-day window, no retained
  history** (canonical update-in-place pattern: live tally/countdown/status). Edits don't notify/bump/reset
  expiry. Delete drops reactions+attachments; tombstone keeps position. Out-of-order ops held pending keyed
  on target.
- **Attachments (`35`, 📐 not built):** Signal encrypt-then-upload. Blob ≠ message path. **AES-256-CBC+HMAC**
  (Signal-exact, incremental verification) — the 64-byte `key` field, divergent from the app's GCM default
  (open decision to confirm). `digest = SHA-256(ciphertext‖tag)` verified before decrypt. Pad to geometric
  buckets. `AttachmentPointer` inside `TextMessage` (field 2, `repeated`) — captioned photo = one TextMessage,
  no separate media type. Server: `attachments` table + `POST/GET /v1/attachments`, `LocalFs` or `S3`
  presigned backends (client does plain HTTP, only server is provider-aware). Download is **authenticated-
  by-id** (unguessable id is the capability; server can't ACL under sealed sender). **Server can't reference-
  count** (pointer is encrypted) → **TTL-based GC (~45d, longer than message queue** so offline/newly-linked
  recipients still pull). Forwarding **re-encrypts+re-uploads** (avoids correlation, original may have
  expired). Link previews: **sender generates at compose, recipient NEVER fetches** (else sender harvests
  IPs); render only if `preview.url` ∈ body (anti-spoof). On-device storage Tier-1 controls work against the
  delivery buffer; "offload + re-hydrate" (Tier-2) needs a durable backup substrate that doesn't exist yet.
- **Compose (`30`, 🚧):** one flow for DM+group (iMessage-style, recipient *count* decides at send; no "New
  Group" menu). Chip field, autocomplete from local contacts (People/Other sections), direct `did:` entry.
  1 recipient → DM (appends to existing thread); 2+ → new group every time (matches Messages, not Signal's
  membership-dedup). **Server pinning:** first chip pins the server; incompatible recipients become yellow
  chips, never a silent server flip (active-server choice always intentional). From-pill `From: Alice (at
  safe-haven.org)`. Group name optional (auto-default = comma-joined names); auto-mosaic icon.

## 12. Connection state (`34`) — Layer 1 ✅, Layers 2-3 📐

`AppCore` is the single source of truth (iOS/bots render directly, no client timers). **Three dimensions:**
(1) instantaneous `ConnectionState` (Disconnected/Connecting/Connected/Reconnecting{next_attempt,
unreachable_since}/Unauthorized); (2) outage **duration** → tiers Online/Retrying(<2min)/ServerDown(2min–7d)/
Abandoned(>7d); (3) **transport** (server WS today; mesh/Nostr later — reachability is "any transport for
this," not "server up"). Per **(identity,server)** membership, aggregated for display.

Earlier design modeled only dim 1 with "any not-Connected → banner" → a dead server pinned a banner forever
and hammered reconnect — fixed by making duration+transport first-class. Reconnect: **timed backoff while
Retrying, opportunistic (parked, woken by `reconnect_now`) while ServerDown** — a fixed long timer is wrong
on iOS (suspended apps don't fire it; just bursts on wake + drains radio). `unreachable_since` persisted so
cold-launch lands directly in the silent tier (no banner flash). **Offline-safe `login` does no network
call** (renders local DB instantly); lazy auth + transparent 401→re-auth→retry.

**401 vs 403 contract:** 401 = unauthenticated (transparent re-auth, never terminal); **403 = membership
revoked (kicked), terminal → `Unauthorized`.** Requires server to keep token issuance identity-scoped, 403 at
WS-connect/membership-scoped request. Non-discovery 403 → auto-remove locally; discovery 403 → route to
migration. **Send semantics: queue+retry, not fail-fast** — outbound to an unreachable server is pending/
persisted, drains on reconnect, transport-agnostic. **Removal preserves crypto** (de-routing, not de-
provisioning — keeps Signal/sender-key state so mesh can still carry it); groups stay in the list as
unreachable rows. NWPathMonitor distinguishes device-offline ("No internet" banner) from server-down.

## 13. Contacts & profiles (`52`) — 🚧 minimal slice built

Goal: user **owns their contact book** (roll, nicknames, notes), surviving any identity/server loss
(Gmail mental model). Principles: interaction-driven (no "Add to contacts"; surfaces from DMing/nicknaming —
Signal/iMessage); server never sees plaintext profile; contacts local-only; **one unified table across all
identities** (the book exists beyond any identity), but each row has **`preferred_identity`** (the de-anon
guard — "message Alice" sends from her preferred identity, not the foregrounded one); per-DID not per-conversation.

**One row per DID** (`is_curated` = "user knows this person" — drives People list, message-request gate,
backup, search "primary"; sticky, set by any deliberate gesture). Other flags: `is_favorite`, `is_blocked`,
`has_pending_request`, `nickname`/`notes`/`photo_override` (private, never leak), `learned_route_server`,
`profile_key`, `cached_profile_version`. Nickname doesn't erase display name (you may introduce someone by
their real name).

**Substrate profile** = JSON (display_name required; avatar/bio future) encrypted with a 32-byte **profile
key** (rotates only on revocation, not on edit), uploaded as opaque bytes. **Profile key + `profile_version`
ride the outer envelope on every message** to recipients you share with. **Liveness via version:** inbound
version mismatch → refetch (primary path, bypasses rate limit); dormant contacts → conversation-open
opportunistic fetch; **no daily background sweep.** Client-side fetch throttle **keyed on last outcome**
(success 5min, not-found 6h, etc.), **persisted in `profile_fetch_state`** (improvement over Signal's
in-memory LRU). Two endpoints: `get_profile` (humans, blob) vs `get_account_info` (bots, public record →
`account_info_cache` for offline bot names/`is_bot`). **Discovery server is authoritative** for the blob;
fetch satisfiable on any server (proxies to discovery; a 200 leaks nothing about local membership).
**Rejected per-member-server replication** (duplicates prekey complexity for rarely-changing state).

**Bot presentation (`54`):** two independent axes — **provenance** (is it *official*? server-vouched
`official` flag, same-server only, the ✓ badge) vs **automation** (is it a *bot*? `account_kind` in profile
blob, **only self-declared, unenforceable**). UI must never present self-declared as proven. **Three tiers:**
verified bot (hexagon + ✓), self-identified bot (hexagon, no ✓, "Automated (not verified)"), person (circle,
default — absence of a bot signal is NOT a "is human" claim). **Signal lives in client-applied chrome the
avatar bytes can't override** (hexagon frame + octagon-ish bubbles for bots, circle/rounded for people) —
unspoofable, doesn't fight branding, glanceable. **Rejected** mandatory constrained-avatar-palette as a
*security* mechanism (parasitic on the badge; harms legit bot branding; only constrains the honest case).

## 14. Multi-account UX (`53`) + invites (`51`)

**Accounts screen** lists every (identity, server) pair grouped by identity. Server rows show home tag,
activity recency, reachability tier (ServerDown → "Unreachable since X", Abandoned → Remove-from-device,
403 → auto-removed/migration). **Delete identity** = leave-cascade each server + PLC tombstone (rotation-key
signed) + wipe local. **Leave server** (graceful, non-discovery only) = courtesy leave events then membership
delete. **Remove from device** (unreachable/403) = local de-routing, **preserves crypto**, groups stay as
unreachable rows. Discovery server has **no removal path** — only Change-home-server (migration, PLC-signed,
completes even when old home is dead) or Delete-identity.

**Invite tokens (`51`):** `base64url(json)` with at least `server_url`; arbitrary extra fields passed to the
server (typically a Project). Flow: decode → `GET <server>/v1/invites/<token>` (server validates: signature/
expiry/usage — all server/Project concerns) → returns `server_name`, optional `server_step_url` (onboarding
webview), `post_onboarding_redirect`. **Currently unsigned + open registration** (token is discovery
convenience, not access control). Signing/expiry/closed-registration are Project concerns (see gatekeeper
§10). Substrate owns decode/extract/validate-call/UI; Projects own signing/onboarding/auto-enroll
(`group_invitations` array of `{master_key, link_password}`)/redirect.

## 15. Build stages & status (`01`)

✅ **Stage 1** crypto core · ✅ **2** homeserver MVP · ✅ **3** mobile app + identity (iOS only; Android not
started) · ✅ **4** invites/notifications/deployment · then (largely built ahead of plan in places):
**5** action-bound groups (zkgroup, expiry, announcement-only — mostly ✅, see `03` §5) · **6** Project
framework (📐) · **7** first-party Projects (Channel Directory, Team Assignment, Action Day, Q&A Bot, Collab
Docs, Engagement Tracking — all 📐) · **8** Calls (📐) · **9** Federation + cross-server casual groups + guest
credentials (📐) · **10** hardening + audit. Within-stage components parallelizable; order chosen to get
encrypted 1:1 working earliest. Platform parity: iOS leads; Android = UniFFI Kotlin glue exists, no UI;
Desktop = napi bindings exist, no UI; Bots/Node = account/DM/group create+invite/admin events.

## 16. bitchat mesh fallback (📐 `14`, optional/opportunistic)

BLE multi-hop flood (fork of public-domain BitChat) as a fallback transport when the homeserver is
unreachable. **Flooding not routing** (no liveness tracking). Existing Signal/Sender-Key ciphertext flows
unchanged — relay nodes see opaque bytes (same guarantee as the server path); **no new encryption layer**.
Three message types: DMs (Double Ratchet), group (Sender Keys — both group types, since the difference is
only the server-side auth layer), broadcast (plaintext "Local Mesh" channel). **User-activated, not
automatic** (avoids surprise BLE / presence broadcast). **Only works with sessions/memberships already
established via the server** (new sessions need prekeys). Mesh identity = Curve25519 derived from the Ed25519
identity key (HKDF). Addressing = 8-byte HMAC tags rotating daily. **Known threat: forced mesh activation**
(jam connectivity → observe mesh → confirm group co-membership within an epoch); mitigations deferred
(per-recipient tags, Noise_XX header encryption, dummy traffic). Deferred: prekey exchange over mesh, WiFi
Direct, Nostr tier, Android.

## 17. Cross-cutting rejected designs (index)

- zkgroup: DID-shaped credential on zkcredential (option 2, shipped then reverted); blind-sig bearer tokens
  (option 3) — both worse than `UUID(did)` option 1 (`03` §2.4).
- Server-side block-list enforcement / pushing block list to server (`12`) — metadata leak, diverges from
  Signal; revisit only if queue-flooding seen in the wild.
- Direct/anonymous spam reports (`12`) — leak reporter identity / trivially forgeable; chose homeserver-
  mediated signed reports.
- Project reverse-proxy with `X-User-DID` (`20`) — metadata/plaintext exposure, blast radius.
- Signed/attestation-based "officialness" (`20`,`22`) — decomposed to a plain `official` flag + scope.
- Bot-to-bot RPC / service mesh / per-admin server-verified credentials (`22`); gatekeeper→adminbot RPC;
  external onboarding forms (`24`).
- Multi-master storage replication / CRDTs / vector clocks (`05`) — chose single-authoritative + LWW.
- Consumer-cloud (iCloud/Drive) as storage substrate (`05`).
- Two reply primitives (`32`); many-reactions-per-message (`33`); constrained-avatar-palette as security
  (`54`); inline interactive cards / in-feed forms (`23`); per-member-server profile replication (`52`).
- Full ATProto stack (`00`) — public-by-default, no E2E; we take only DIDs, build the private substrate
  ourselves, and leave public-social to ATProto-backed Projects sharing the same DID.

---
*Generated from docs/ as of 2026-06. Source docs remain authoritative; this is a lossy index of decisions,
whys, and rejected alternatives. `signal-research/` (background, not decisions) intentionally omitted.*
