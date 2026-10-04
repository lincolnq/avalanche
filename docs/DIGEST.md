# DIGEST — compressed index of docs/

Derived and lossy. Source docs are authoritative; every section is tagged with its source doc
numbers, and `(03 §3.9)` style pointers lead back to detail. Status words follow
`docs/CLAUDE.md`: **Built** (in code), **Partial** (some built), **Planned** (committed, unbuilt),
**Proposed** (contract change agreed in principle; **pending project-owner review, do not
implement until approved**), **Speculative** (no commitment), **Superseded** (kept for rationale).
Todo lists, deploy commands, SQL/proto tables and UI copy are omitted; see `02` for the roadmap.

---

## 1. Premise and governing principles (00, 01)

- **Premise.** A social network acquired through collective action: people install because a
  Project (canvass, strike, rally) needs it, and stay for the social graph formed there.
  Activism = acquisition; social = retention. Building campaign tools is easy; building
  Signal-quality encrypted comms is hard, so: a boring, reliable encrypted **substrate** plus
  many **Projects** on top.
- **App-first, feels like Signal.** One unified inbox across all servers and identities,
  sorted by recency; servers/Projects browsable in their own tab; you never "enter a server"
  to read messages.
- **Front door priority.** The path "campaign sends a link -> inside their Project and
  groups" deserves as much care as chat polish. Today it is not smooth (links don't survive
  App Store install; no `/project/<t>` deep link) (00, 23).
- **Goals.** Projects with deep auth integration; decentralization (orgs run servers, no
  single party holds everyone's data); E2E DMs/groups/channels; bots and agents as
  first-class but always visible; Signal-grade iOS/Android/Desktop apps; **easy for
  non-technical organizers to run a server** via the website's configure tool (one paste; the
  canonical setup path; any config/deploy change must keep it working, secrets generated on
  the box) (00, 42). Speculative: mesh,
  public profiles/feeds as Projects, engagement tooling (with care).
- **Two technical principles.** Don't implement crypto (use libsignal). Make vulnerability
  classes impossible (Rust for all security-critical code). Copy Signal by default; diverge
  only for multi-server identity, Projects, multi-account.
- **Terminology.** *Identity* = cryptographic identity a person controls (today `did:plc`),
  the compartmentalization boundary between unlinkable personas. *Account* = (identity,
  server) pair. *Device* = one install; devices share the identity key but keep their own
  sessions/prekeys/sender keys. Durable user data is identity-scoped, never shared across
  identities.
- **Substrate vs. Project heuristic.** Needed by multiple Projects or touches encrypted comms
  -> substrate. One Project only or purely public data -> Project. The private connections
  graph is never exposed; any public follow graph is Project-level.
- **ATProto stance.** Public-by-default is wrong for organizing; we use ATProto-compatible
  DIDs only. Public-social features belong in Projects (possibly published to Bluesky). Under
  the Proposed identity change, a public DID becomes an opt-in link.
- **Two group kinds rule:** "if a group needs an admin, it needs a homeserver" (00, 03).
- **First-party Projects:** testbot and adminbot Built; others Speculative (invite codes,
  channel directory, Q&A bot, teams, calendar, Action Day map, CRDT docs, engagement).
  Cautions: Project server state is seizable; "most active / where is everyone" lists are
  target lists, so locations must be E2E and rankings client-side.

### Where things stand (00)

| Area | Status |
|---|---|
| 1:1 messaging, receipts, reactions, edit/delete, attachments, link previews | Built |
| Action-bound groups (zkgroup, sealed-sender send, expiry, roles, avatars) | Built, known gaps |
| Identity: did:plc + passkey PRF, recovery blob | Built (no-blob path missing) |
| Multi-device linking + storage sync | Partial |
| Multi-account | Partial |
| Push (relay, APNs/FCM/UnifiedPush, iOS NSE) | Built |
| Contacts, profiles, blocking, requests, reporting | Built, known gaps |
| Projects (manifest install, capabilities, directory, tokens, OAuth, adminbot) | Partial |
| Platforms iOS (reference) / Android / Desktop | Built; parity in `62` |
| Client-side federation | Proposed |
| Threading beyond quote-reply | Planned / Speculative |
| Calls | Speculative (undesigned) |
| Supergroups, mesh | Speculative |

---

## 2. Security posture summary (09)

Living register; read before trusting any privacy claim elsewhere. When a gap is fixed,
update `09` and the subsystem's Known gaps together.

**Threat model.** In scope: **server seizure** (disk+DB should not yield contacts,
memberships, history, real names; users can carry on elsewhere); **surveillance of
membership** (membership lists are targeting data; limit linking across servers and
identities); **hostile participants** (strangers, malicious members, malicious/careless
Projects); **device seizure** (limited: disappearing messages, platform-keyed at-rest
encryption). Out of scope: targeted state surveillance (no onion routing/cover
traffic/mixnets; use Tor/VPN), traffic analysis beyond TLS, compromised OS, coercion.

**What holds today.** Content confidentiality (X3DH/PQXDH + Double Ratchet; Sender Keys);
group tables name no members (encrypted blob; routing keyed by encrypted member IDs;
non-members get 404); anonymous group send (zkgroup group-send token, no session
credential); encrypted profiles; homeservers never see push tokens; bots always visible (no
out-of-band read path); SQLCipher at rest on iOS/Android with hardware-backed keys
(Desktop is not, S-05).

**What each adversary learns today (intended in brackets):**
- *Seized DB:* registered DIDs; encrypted blobs; up to 30 days of undelivered DM rows with
  sender account (co-membership via SKDM/invite bursts); readable unpruned group history
  (pseudonyms, link passwords, exact timestamps); ~1 h of IPs for anonymous sends; raw invite
  tokens. [DIDs and ciphertext only] (S-09..S-12)
- *Live operator:* the above plus presence, IPs, live account<->pseudonym<->group links
  (accepted), the full identified DM graph. [own-org social graph, not group membership] (S-09)
- *Relay:* each device's DM + all group pseudonyms and timing; *relay + homeserver:* full
  group membership. [timing only; nothing more together] (S-13)
- *Public PLC log:* signup server forever, every rotation; recovery GET tests which servers
  hold a DID. (S-06, S-07, S-19, S-20)
- *Stranger with your DID:* can add you to groups; self-declared bots skip requests;
  attachment URLs leak your IP. [nothing until accept] (S-03, S-04, S-08; S-02 fixed)
- *Malicious member:* squat others' delivery/wakeups; removed members keep Sender Keys; no
  sender membership check; extend expiry. (S-14..S-16)
- *Project operator:* its own bot signup key only (setup-code escalation S-01 fixed, not yet deployed);
  audience-free tokens replay across Projects. (By design, every member of a server running
  adminbot is an admin, S-29.) (S-17)
- *Stolen device:* rotation key = permanent DID takeover; no revocation; Desktop constant key;
  plaintext caches of deleted media. (S-05, S-06, S-18, S-25)
- *Page on `*.theavalanche.net`:* can request the root PRF secret (shared RP with Project
  hosting). (S-07)

**Register (ID, severity, gap -> fix doc):**
- **Critical:** S-01 setup codes embedded `REGISTRATION_SHARED_SECRET`; rewriting slug to
  `adminbot` = superuser. **fixed in code (not yet deployed):**
  per-Project reusable bot signup keys; shareable secret can't link Projects; separate
  on-box `SUPERUSER_BOOTSTRAP_SECRET`, claim-once; no raw tokens in events (purged). Rotate
  the shared secret where old setup codes were handed out (22, 24, 51).
- **High:** S-02 (fixed) profile key in delivery receipts to un-accepted requests (52); S-03
  self-declared `is_bot` bypasses request gate (54); S-04 group invites auto-accepted from
  non-blocked strangers, Reported/needs UI check (12, 03); S-05 Desktop constant SQLCipher
  key (61); S-06 rotation key on every device and in link bundle, sole rotation key (50
  Proposed, 04); S-07 passkey RP shared with Project hosting (50, 20); S-08 attachment
  pointers fetched from any host, no size cap, under core lock (35); S-09 identified DM plane
  (SKDMs, invites, `sender_account_id`) undercuts group opacity -> sealed sender (03, 13);
  S-14 WebSocket group subscribe last-writer-wins, drain deletes rows -> secret-backed
  pseudonyms (03); S-15 no sender-membership check on SKDM/group receive, no re-seed on
  removal, `announcement_only` unenforced (03); S-17 Project tokens audience-free, in query
  string (20); S-29 (accepted by design) every new human joins `#admins`, so all members
  are admins on a young server; gap is onboarding on how to close it as the server grows (22).
- **Medium:** S-10 readable never-pruned group history (03); S-11 exact timestamps on group
  routing tables (03); S-12 IPs persisted in Postgres rate-limit table (03); S-13 relay sees
  full pseudonym set, unauthenticated `INSERT OR REPLACE` registration (15, 41); S-16 group
  expiry unclamped (03); S-18 no device list/revocation; `/link` `/replace` don't check
  identity key, non-transactional, PLC fetch without timeout (04, 50); S-19 unauthenticated
  `GET /v1/recovery/{did}` returns device IDs (50); S-20 genesis op publishes signup server
  (50 Proposed); S-21 device linking with no confirmation code (2025 Signal phishing pattern)
  (04 §4.3); S-22 storage records not version-bound -> rollback (05); S-23 iOS Project
  webview allows any navigation; Network-tab/`conversation/` links use first account (20,
  23); S-24 no server WS ping, half-open sockets suppress push (10); S-25 deleted/expired
  messages leave attachment rows/keys and plaintext caches on all platforms (35, 36); S-27
  shared avatar/attachment blob namespace, sequential profile-avatar ids (55); S-30 bots
  choose their own `did:local:` suffix, so `adminbot` can be squatted on a fresh server (22).
- **Low:** S-28 Desktop link-preview SSRF; deep links create rows from unvalidated DIDs;
  S-26 mesh tags keyed on public identity key (design only, 14).

**Hardening order:** (1) critical and stranger-facing fixes (S-01, S-02 done)
S-03..S-05, S-08, S-14, S-17, S-30,
then S-25 (small, no design); (2) move identity root off devices (S-06, S-07, S-19, S-21;
parts Proposed); (3) sealed sender for 1:1 + SKDM with delivery keys (S-09; biggest privacy
win, foundation of federation); (4) server metadata hygiene (S-10..S-12, S-16) and relay
unlinkability (S-13); (5) client group membership enforcement (S-15).

**Audit readiness:** not ready. No `cargo audit`/`deny`, no app-core e2e in CI, no
mobile/relay/bot builds in CI, no `03` §9 invariant tests, no reproducible builds.

---

## 3. Stack, repo, app-core, homeserver (01, 07, 10)

**Repo (Built, 01).** Crates `types`, `crypto` (no I/O; defines `Store`), `store`, `net`,
`app-core`, `app-core-node` (napi), `server`, `relay`; `federation`/`project-sdk` are empty
placeholders. Plus iOS, Android, Desktop (Tauri), Node bots, infra, web.

**Crypto stack (Built).** libsignal pinned to commit `4c460615`: X3DH+PQXDH (incl. Kyber),
Double Ratchet, Sender Keys, multi-recipient sealed sender (group path only; **1:1 DMs do
not use sealed sender**), zkgroup. Primitives X25519, Ed25519/XEdDSA, AES-256-GCM,
HKDF-SHA-256, Ristretto255. **Attachments use AES-256-CBC+HMAC** (Signal-exact, incremental
verification) — the one divergence from GCM. DID rotation keys are P-256 (PLC requirement).

**Envelope.** Plaintext is protobuf `ContentMessage` with a `oneof body` plus envelope fields
(`timestamp_ms`, `profile_key`, expiry). Forward compatibility by reserved field numbers;
retired numbers never reused; clients must silently ignore unknown variants (36; verifying
this is a todo per 08).

**Clients.** Rust core + native UI: iOS (UniFFI), Android (UniFFI + JNA), Desktop (hand-written
Tauri commands), bots (hand-written napi). DB keys: Secure Enclave, Keystore, operator env,
**Desktop constant placeholder** (S-05).

**Server (Built, 10).** Axum/Tokio/sqlx (compile-time checked, `.sqlx/` checked in),
Postgres only, no Redis, no libsignal session code (relays opaque bytes so a server bug
can't touch plaintext). DB functions take `&mut PgConnection` (rollback-isolated tests,
composable transactions). Opaque revocable session tokens, not JWT. One-time prekeys
consumed with `DELETE ... RETURNING`. Migrations only via `migrate`, never on start. Auth =
identity-key-signed challenge -> opaque token; **issuance identity-scoped, membership checked
on use** (401 vs 403, 34). Delivery: WebSocket protobuf frames, drain on connect, ack deletes
row; HTTP fallback; offline relay wakeup; 1:1 queue 30 days. Table-backed rate limits.
- **Single-instance contract:** `ws_connections`, `group_subscriptions`,
  `account_joined_subscribers` are in-memory maps — never persisted (keeps live
  pseudonym<->account links out of a seized DB) but two instances can't share sockets.
  Horizontal scale would need a fan-out channel (Speculative). Gaps: no server WS ping
  (S-24), persisted IPs, PLC calls without timeout, no S3.

**app-core (Built, 07).** One client library for every client; **bots and humans get the
same API**. Owns connection, all crypto state, both stores, background tasks, and events. A
library you drive, not a daemon; never calls back into platform code.
- **Instances:** `AppCoreInner` is structured as one identity with a primary account plus
  `backup_accounts` (always empty, dead scaffolding). Platforms hold one `AppCore` per
  signed-in (identity, server) keyed by DID, merged into one inbox.
- **Storage defaults:** humans (`did:plc:`) persist incoming content **before acking**
  (load-bearing: server deletes on ack; event channel not durable). Bots (`did:local:`) don't
  persist content, sync storage, or upload recovery blobs. Contacts are interaction-driven
  (`touch_contact`).
- **FFI shape (load-bearing):** exports sync by default, blocking on a global
  `OnceLock<Runtime>`, because libsignal store traits return non-`Send` futures. **Exception:**
  long waits (`next_events`, `wait_for_connection_state_change`) are native async exports;
  a sync version pinned a thread per call and exhausted Swift's cooperative pool at three
  accounts. Rule: never add a sync export that can block indefinitely; never wrap long waits in
  `Task.detached`/`Dispatchers.IO`. Interior mutability via `tokio::sync::Mutex<AppCoreInner>`
  with lock-free clones for read-only paths; never hold `inner` across a network await
  (crypto send/group paths are a bounded documented exception). Two error types. Tests use
  `_async` variants. Store = one serialized Arc connection per DB, never a pool (libsignal
  multi-`&mut`).
- **Gaps:** hand-maintained Tauri/napi bindings, unchecked; **Desktop bridge doesn't
  compile** (calls async exports synchronously); parked waits uncancellable (logout leaves
  socket open); one mutex serializes all crypto sends.
- **Planned:** actor model (one thread with `LocalSet` owns libsignal state; async exports
  over channels; retire the global mutex); generate or CI-check Tauri/napi bindings.

**Rationale (01).** Rust server (memory safety, no GC, native libsignal; rejected Go,
C/C++). Rust core + native UI (security code once; cost: three UIs and bindings). Capacity:
messaging cheap; attachments drive storage. Speculative: key transparency; calls (1:1
WebRTC, group LiveKit SFU with insertable-streams E2E); horizontal scaling.

---

## 4. Identity, authentication, recovery (50, 56)

**Status: Partial.** Built: signup and blob-path recovery via passkey or 12-word phrase on
iOS/Android (Desktop phrase-only). Not built: no-blob recovery, second-server join, any
multi-device-aware recovery. Proposed redesign pending owner review.

**Current design (Built).**
- No phone/email (removes strongest real-world identifier from the server).
- Keys: **rotation key** (P-256, in PLC `rotationKeys`, authorizes DID ops and device
  replace/link; HKDF `actnet-rotation-v1` from passkey PRF or phrase seed); **blob key**
  (HKDF `actnet-blob-v1`; encrypts recovery blob; cached on signup/recovered device, not on
  linked devices); **identity key** (libsignal Curve25519, random, shared across devices,
  published as `#avalanche`, used for sessions and server auth); **storage key** (random,
  record-level, in blob and link bundle).
- DID = hash of genesis op `{rotationKeys:[rotation_pub], services:{avalanche_homeserver:
  signup_url}}` that **omits the identity key**, so passkey + signup URL recompute the DID
  without lookup; an update op then adds the identity key.
- Passkey: RP `theavalanche.net`, fixed PRF salt `actnet-recovery-v1`, userHandle = signup
  server URL (no typed input at recovery), discoverable. Phrase: BIP39 12 words; first 32
  bytes of seed replace PRF in the same HKDF; user records server URL. Skipping both ->
  random rotation key, unrecoverable.
- Recovery blob: identity keypair, servers, profile key, name, group keys, storage key; not
  the rotation key. Registration checks the identity key against PLC `#avalanche`.
  Recovery restores the same identity key (same safety number), does a rotation-signed
  `/replace` on the first server, re-seeds group sender keys (old messages lost), then pulls
  storage. Passkeys never used day to day. Identities share no keys or server state.

**Known gaps.** Rotation key persisted on every device and shipped in link bundle, sole
rotation key -> any device compromise = permanent DID takeover (S-06). No-blob recovery
path has no code. Recovery is single-server, single-slot (reuses `min(device_ids)`).
Unauthenticated recovery GET leaks registration, device count, and (via size) group count
(S-19). Signup server published forever (S-20); "home server can be omitted" claim is
unachievable with this genesis. **Identity key mislabelled as Ed25519 multicodec** in the
DID document (actually Curve25519/XEdDSA). Any `*.theavalanche.net` origin can run the
ceremony (S-07). Domain seizure = recovery outage. One vault links all personas (labels
`"<name> @ <server>"`). Server PLC fetches have no timeout.

**Planned.** Build or drop no-blob recovery; whole-device-set revocation on every server;
PLC timeouts and IP rate limit before PLC fetch; fix DID-doc key type; stop hosting
third-party content under the RP domain (dedicated RP domain); neutral passkey labels.

**Proposed (pending owner review; agreed in principle 2026-10-03; needs migration plan):**
- **P1 wrap, don't derive.** Random 32-byte root secret; rotation and blob keys derive from
  it; root stored only wrapped under each factor (primary-RP passkey PRF, **backup-RP-domain
  passkey** with different registrar/jurisdiction, phrase), stored with the blob on every
  server. Why: today root = PRF output, unrotatable; compromised vault is forever; seized
  domain disables recovery. Cost: DID not recomputable from passkey alone; recovery needs a
  wrapped copy (server hint in userHandle).
- **P2 priority-ordered rotation keys.** Top = root-derived recovery key, never stored on a
  device; optional lower-priority device key. PLC lets higher-priority keys nullify lower
  ops within 72 h. Linking ships no rotation key; `/v1/devices/link` authorized by the
  existing device's session + identity-key signature, server checks same identity key.
- **P3 unpublished private identity.** Self-certifying identifier (hash of genesis with root
  public keys), never in a global directory; key document held by own homeservers and passed
  to contacts over sessions; key changes/moves are rotation-signed statements; strangers
  find you via invites/QR only. `did:plc` becomes an opt-in public link via signed
  attestation. Why: PLC is public, permanent, records signup server, timestamps every
  change, and is a third-party dependency. Gives up Bluesky identity by default and bare-DID
  lookup. Pairs with 13.
- **P4 authenticated blob fetch** with a per-factor fetch key (`HKDF(PRF,"actnet-fetch-v1")`);
  drop `device_ids`.

**Speculative.** Bluesky-linked identities (ATProto OAuth proves existing `did:plc`; lossy
recovery); other OAuth providers as recovery authorities.

**Rationale.** Passkey recovery = best UX (platform-synced); identity key kept random
(passkey controls DID, blob restores key); genesis without identity key made DID
recomputable (cost: publishes signup server; reason for P3); universal RP domain so one
passkey works across servers and only official apps run recovery (cost: concentrated risk).
Rejected: homeserver-held recovery keys (seized server takes identities); consumer-cloud
backup (subpoenable).

**Desktop passkeys via external browser (56) — Proposed, spec only, pending owner
review and re-check against 50 P1.** Run WebAuthn in the default browser on a bridge page at
the RP domain; seal PRF to an app ephemeral X25519 key; POST back to a `127.0.0.1` loopback
(RFC 8252). Must produce PRF bit-identical to mobile (same RP, salt, userHandle) or it is a
blocker. Rejected: in-app WebView WebAuthn (webkit2gtk lacks it, WKWebView entitlement);
custom URL scheme (any app can register; leaks into history). Deferred: native OS ceremony
(three integrations, no app<->domain binding on Windows, Linux can't reach 1Password).
Risks: local process racing the signup POST could plant an attacker-known root (single POST,
short timeout, user confirmation); bridge must be on an origin with no third-party content.
Phase 0 spike gates everything.

---

## 5. Multi-device (04)

**Status: Partial.** Built: per-device crypto, linking on all three platforms, group
fan-out, sent-transcript sync, storage service. Not built: sending `SyncRead`, device list
UI/revocation, whole-identity recovery reset, link confirmation.

- **Central distinction (load-bearing):** the identity key is a static credential and is
  shared (like Signal); sessions, prekeys, sender keys are running ratchets and are
  per-device (sharing would reuse keys/nonces and collide counters). Server keys prekeys,
  registration IDs, queues by `(account_id, device_id)`.
- **Membership is per-identity; delivery per-device.** Adding a device doesn't add a member;
  send fans out per device.
- **Linking (Built, §4).** Short-lived ciphertext-only mailbox on a homeserver; either device
  shows or scans (QR or paste); ECDH over an out-of-band key, so a hostile mailbox can only
  abort; existing device seals identity, rotation, storage keys. Polling (joining device has
  no account), cancellable from UI. `/v1/devices/link` is additive and rotation-key-authorized.
  All devices co-equal; linked devices lack the blob key.
- **§4.3 Planned:** existing device shows new device name + code derived from `K`; paste
  accepted only on the new device; "New device linked" notice on all devices (S-21).
- **Sync channels (§5, decided):** *Conversation* (anything recipients see) syncs free
  via a **Sent transcript** wrapping the `ContentMessage`; *Durable* current values go to the
  storage service (05); *Device-local* never syncs. Only *local events* add sync types; cap
  target set `{Sent, Read, Viewed, LocalDelete}`. Decision rule: recipients need it ->
  ContentMessage; only my devices, current value -> storage record; action -> thin event.
  Rejected: one SyncMessage per feature (Signal accreted ~20). Sync messages are pairwise DMs
  to yourself, no sealed sender. Live `SyncSent`/`SyncRead` emit scoped
  `ConversationUpdated`. Gaps: `SyncRead` receive-only; `SyncViewed`/`SyncLocalDelete`
  undefined.
- **Group fan-out (§6, Built):** sealed-sender keys derive from the shared identity key so any
  device decrypts; per-device pseudonyms; linked devices reconcile groups after storage pull.
  Accepted: server learns device count.
- **Recovery vs. linking (§7):** linking additive (device alive); recovery should be total
  (revoke entire device set on every server). As built: one-slot `/replace` on primary only
  (Partial). Planned whole-identity reset endpoint.
- **§8 Planned:** shared identity key means a peer's new device doesn't change safety
  number (accepted weakness); mitigate with "Bob added a device" notice.
- **§9 Planned revocation:** delete device row/prekeys, kill session tokens, remove group
  pseudonyms; doesn't rotate identity key.
- **§10 History backfill:** explicit non-goal (Signal parity).
- **Gaps:** no device list/revocation; rotation key on all devices; phishable linking; server
  doesn't check linked identity key, `/replace` non-transactional, no IP limit, PLC fetch no
  timeout (S-18); no e2e test (needs live PLC).
- **Rejected:** per-device identity keys; Signal's desktop-always-secondary; provisioning
  WebSocket. Deferred: spoken PAKE codes.

---

## 6. Device data sync / storage service (05)

**Status: Partial.** Built: server `/v1/storage/items`, engine, four types (tag 1 group keys,
2 contacts, 3 conversation settings, 4 contact profiles), trigger dirty tracking,
commit-hook scheduler, WS nudge. Parked: passive backup snapshots. Data-loss bugs.

- **Model:** domain tables stay the source of truth; a payload-free sidecar
  (`storage_sync`: type, logical key, version, dirty, deleted) plus one adapter per type
  (`SyncedType` trait). Adding a type = table + adapter + registration.
- **Triggers** (generated from registry) mark dirty in the same transaction; one rusqlite
  `commit_hook` wakes a debounced scheduler; 60 s safety poll; sync on reconnect. Rejected:
  write-path helpers (discipline), inline push (blocks, fails offline, loses crash
  durability), payload in sidecar (drift).
- **Opacity:** storage key (identity-level, never to server; **its presence is the opt-in**:
  bots have none). Record id = HMAC(storage_key, tag||key)[..16]; AES-GCM envelope binds tag
  and key inside ciphertext (checked against record id) but **not version** (S-22).
- **Server:** per-record CAS versions, per-account cursor, quotas; WS nudge to other devices.
- **One authoritative account** on the discovery server (`servers[0]`); not multi-master;
  never consumer cloud. Passive backups parked because snapshots lack seq/CAS so promotion
  needs restore-then-reseed. Cost accepted: lose discovery server -> lose non-blob durable
  state.
- **Conflict model:** intended per-record LWW (single user, low contention). **As built: on
  conflict the next pull overwrites the dirty local edit** ("first write to reach server
  wins"); unknown-type records discarded permanently; rollback possible.
- **Bootstrap:** link and recovery both = "get storage key, pull since=0". Blob still inlines
  group keys; no `MAX_RECOVERY_BLOB` (2 MB axum default).
- **Planned:** keep dirty edits and re-push; keep unknown payloads; bind version; trust-store
  adapter; drop group keys from blob and cap 32-64 KB.
- **Speculative:** client-assigned HLC versions sealed inside ciphertext -> true LWW,
  rollback detection, every server a dumb mirror, no snapshots.
- **Rejected:** multi-master/CRDT/vector clocks; consumer cloud (re-centralizes, leaks
  DID<->platform).

---

## 7. Identity/device store split (06)

**Status: Built** (multi-account contexts are scaffolding only).
- Two SQLCipher files: **device.db** (libsignal state, sender keys, push state, caches;
  never synced, rebuildable) and **identity.db** (keys, profile, contacts, groups, settings,
  trust store, sidecar, and for now the event log).
- Identity keys are bootstrapped via blob or link bundle, not the storage service (can't
  fetch the key you authenticate with).
- Group master key per-identity (roams); sender key and pseudonym per-device.
- Both files keyed by platform key, not the storage key (which lives inside identity.db).
- **AppCore = one identity = persona = storage-key boundary.** Cross-identity aggregation
  (e.g. autocomplete) lives above, read-only; picker must bind contacts to their identity
  (cross-persona send footgun). Rejected: AppCore = account (pushes sync coordination into
  the app). Per-person cross-identity prefs deliberately unsupported (device-local).
- **Decided:** two files; trust store per-identity and **should sync** (not built; cost: a
  rolled-back store could poison trust everywhere); full replica per device. **Open:** event
  log placement; one DID across servers.
- Gaps: `backup_accounts` always empty; "add server" never registers; trust store doesn't
  roam; event log doesn't sync via storage.

---

## 8. Groups (03)

**Status: Partial.** Action-bound groups built end to end; §3.9 opacity has real gaps; §9
invariant tests don't exist; cross-server casual groups (§6) and mesh (§7) not built.

- **Shape.** A group lives on one hosting server. State is an encrypted blob (source of
  truth: members with DID in cleartext inside the blob, metadata, policy, revision); server
  keeps an opaque routing subset (`member_credentials`, per-device
  `group_member_pseudonyms`, `members_pending`, `members_pending_approval`, policy columns).
  **No group table has a DID or account column.** Clients trust the blob, not the subset.
- **zkgroup via `UUID(did)` (§2, Built).** `SHA-256("actnet-did-to-uuid-v1"||did)[..16]` as
  Aci (and Pni). EMI = stock `UuidCiphertext`. Rejected: a DID-shaped `zkcredential`
  credential ("option 2"; shipped then reverted because the mismatch recurred in every
  zkgroup primitive, each needing a ~500-line security-sensitive reimplementation, and its
  claimed advantages didn't hold); blind/rotated bearer tokens ("option 3"; weaker). API
  scheme-agnostic so MLS remains possible.
- **Updates (§3.3).** Presentation-authorized changes; admin-class (batchable) and self-class
  (sole action); revision+1 else 409; `modify_policy` and `modify_member_role` are
  protocol-fixed Admin (else members could grant themselves anything); one transaction.
  Actions are the diff; titles, descriptions, expiry, profile keys sub-encrypted.
- **Fetch (§3.4):** non-members get **404 not 403** (no existence probing). History kept for
  catch-up, timeline backfill, and **tamper detection** (client response to tampering
  undesigned). 256-revision ring Planned, not implemented.
- **Concurrency (§3.5):** monotonic revisions, declarative idempotent actions + 409 retry.
  Rejected CRDT/OT.
- **Layered roles (§3.6):** server enforces what it sees; clients re-verify against blob;
  each applied change becomes a `kind > 0` system row + `GroupMetadataChanged`.
- **Delivery (§3.7):** per-device group pseudonym, separate from DM pseudonym; in-memory
  pseudonym->socket map; offline relay wakeup; live-memory account<->pseudonym<->group link
  accepted (never persisted; Signal accepts same). 7-day per-group-offset rotation Planned
  (endpoint exists, unscheduled).
- **Expiry (§3.8, §5):** timer in encrypted state; clients stamp each message; countdown
  starts on read; app-core reaper hard-deletes and emits `MessagesExpired` (substrate, not
  UI). Server deletes undelivered rows (30-day default). Server should clamp; **doesn't**.
- **§3.9 schema discipline:** no did->groups table/cache; no EMI->did map; no credential ids
  logged; counts-only logging; day-aligned timestamps. Rules 1-4 hold; rule 5 violated. Honest
  claim: **group tables don't name members, but DM-plane metadata, readable history, relay
  correlation and IP tables let a live operator reconstruct much of the graph.** Change `09`
  first before relaxing a rule.
- **Invites/links (§3.10):** two-step (pending then self-promote) following Signal; invitee
  supplies own profile key and pseudonym. Invite = admin action then identified DM with
  `GroupContext` (master key). Links carry master key + password. Master key alone grants
  nothing (fetch gated, Sender Keys pairwise) — **assuming clients reject non-member SKDMs,
  which they don't yet**.
- **Sender opacity (§3.11):** Sender Key ciphertext in multi-recipient sealed sender with a
  homeserver-signed sender certificate (2-day); `POST /send` takes no Authorization header,
  authorizes a `GroupSendFullToken` over recipient ServiceIds; logs nothing identifying.
  Credential refresh is identified (session) — anonymity at send, not refresh (Signal's
  trade). State changes stay identified by EMI. Group abuse reporting needs selective
  sender disclosure (undesigned).
- **§3.12:** groups never federate, under current and Proposed models.
- **Gaps:** S-09..S-12, S-14..S-16 (see section 2); no per-group rate limit; no policy or
  description FFI, no invite-link UI.
- **Planned:** sealed sender for 1:1/SKDM; secret-backed pseudonyms (server stores H(secret),
  subscribe presents preimage, no account link); client membership rules; server metadata
  fixes; policy FFI/UI; scheduled rotation; three cheap invariant tests (others dropped).
- **§6 Cross-server casual groups — Planned:** <~50, peer-managed, Sender Keys with client
  fan-out to each member's server; needs churn/re-key design. **§7 mesh — Speculative:** group
  mesh tags per sender from Sender Key, never from master key.
- **§8 threat checklist** is the PR review gate for any group code change; walk it, verify
  in code.
- **Rationale/superseded:** two group types (most designs refuse the split and pay in UX or
  guarantees); invite links carry master key (Signal); skipping sealed sender for invite DM
  because admin is identified — **Superseded in effect** (the DM reveals *who was invited*);
  claim-squatting defense "deliver to all claimers" chosen over "reject non-owned
  subscribes" (needs account->group link) — neither built; replaced by secret-backed
  pseudonyms.
- Open: account deletion under opacity is client-driven leave cascade (53); tamper response.

### Supergroups (08) — Speculative

Normal groups to ~200. Costs linear/worse in N: per-recipient envelope slots, per-device
storage, O(N^2) SKDMs; plus spam megaphone. Insight: **cost is delivery, not readability** —
push nothing but wakeups; pull content, count reactions server-side. Sketch: promotion (not
creation-time; explicit, visible, one-way); UX-transparent (feels like announcement-only);
admin posts encrypted once under a channel read key, pull-gated; admin sends **pseudonymous
among admins** via zkgroup presentation (server can link one admin's posts, not to a DID,
and can enforce admin-only); replies as pull-based threads; reactions as server-counted
opaque tokens. Gives up: admin-post linkability, weaker FS. Evaluate MLS first. Open: key
schedule, reaction dedup (nullifiers), old clients. Rejected: announcement-only at any size;
identified admin sends (organizer roster for seizers); fully unlinkable admin sends (too much
machinery); separate opt-in discussion group (pull is not push); author-mediated tallies;
create-as-supergroup.

---

## 9. Federation (13) and mesh (14)

**Current (Built):** no federation. Single-server messaging; multi-account (53) is the
cross-server mechanism; group traffic local to hosting server.

**Proposed — client-side federation (pending owner review; changes wire protocol and
endpoints).** Servers never talk to each other. To reach Bob on `b.org`, Alice's client
learns his server from how it learned of him (invite/QR, contact card, group data, move
notice), fetches prekeys from `b.org`, and delivers **sealed-sender** directly; `a.org`
learns nothing.
- **Delivery keys:** Bob derives `HKDF(profile_key,"delivery")`, registers its hash; only
  contacts holding his profile key can deliver sealed or fetch prekeys. Strangers make an
  **identified, signed, rate-limited first contact** that lands as a message request. A server
  may refuse first contact (closed community). **Consequence: profile keys must go only to
  accepted contacts** (S-02, now fixed).
- Sender certificate trust root comes with card/invite or is fetched and pinned (open:
  server-issued vs identity-key-signed certs).
- **Move notice** `{did, new_servers, issued_at}` signed by identity key to contacts; old
  server can't block.
- Learns: sender's server nothing; recipient's server a sealed delivery and an IP (stranger
  first contact reveals sender); relay unchanged. Costs: sender IP to foreign server;
  routing needs current server; cross-server policy can't rest on another server's vouching;
  abuse-report forwarding loses its channel (open conflict with 12).
- Contract: new endpoints (sealed delivery, identified first contact, delivery-key
  registration, key-authorized prekey fetch), delivery key, move-notice type, stop profile
  keys on request receipts. Casual groups become client fan-out on the same path.

**Superseded — server-to-server multi-homing (2026 drafts):** discovery server per DID; the
sender's server federated ciphertext; server keys, learned routes, trust scoring, proxied
prekeys. Why superseded: sender's server saw every cross-server recipient (seizable social
graph); lots of leaky machinery against the threat model; activist operators can't evaluate
server-to-server trust. Kept: same-community traffic stays local; per-server prekeys;
migration authority is the user's signed record; join flows show what the new server sees;
adding a contact never requires joining their server.

**Speculative:** Project-to-Project federation is a Project concern over plain HTTPS
(optionally Sign in with Avalanche); substrate doesn't provide pub/sub/RPC (`00` once
committed to it). Rejected: full ATProto federation; Matrix-style room replication;
multiple discovery servers per DID.

**Mesh fallback (14) — Speculative.** Fork BitChat BLE flooding mesh; carry existing Signal
ciphertext; user-activated when the server is unreachable; DMs need existing sessions; plus a
labelled plaintext Local Mesh channel. **DM tag design is broken** (keyed on the public
identity key, so anyone can track a device; must use shared secrets, S-26). Group tags per
sender, never from the master key. Threat: forced activation by jamming to observe
co-membership.

---

## 10. Push (15, 16, 41)

**Push relay (15, 41) — Built**, at `relay.theavalanche.net`, deployed by hand. Homeservers
POST content-free wakeups to pseudonyms; the relay (small losable SQLite) maps pseudonym ->
token and dispatches APNs, FCM (data-only), or UnifiedPush (relay POSTs to the client
endpoint, SSRF-guarded). Every external transport goes through the relay. Client picks FCM,
else UnifiedPush, else foreground WS. Desktop never registers. Multiple relays allowed; push
opt-out possible.
- **Learns:** device token joins DM pseudonym and all group pseudonyms across servers;
  rotation doesn't help (S-13). Registration unauthenticated `INSERT OR REPLACE` -> anyone
  knowing a pseudonym can redirect wakeups; group members can see each other's pseudonyms.
- Scheduled weekly pseudonym rotation is Planned (not built); the relay keeps a rotated
  pseudonym for a 7-day grace period.
- **Planned:** secret-backed pseudonyms with authenticated registration; distributor picker;
  Android foreground keepalive; relay into deploy bundle; separate dev/prod relays.
- **Rejected:** homeserver holds tokens (it and Apple/Google learn identity->device);
  homeserver-direct UnifiedPush; Web Push payload encryption (no payload).

**iOS Notification Service Extension (16) — Partial.** Silent `content-available` pushes are
unreliable for rarely opened/force-quit apps, so (like Signal) send an alert push with
`mutable-content` and no content; the NSE runs **full app-core** `fetch_notifications` for
every account (no per-account hint; payload stays content-free), through the same
`process_decrypted` path, acking after durable write. Relay `APNS_PUSH_MODE` (default
`silent`; production value not recorded in repo).
- Stages 1-4 and 6 Built (App Group storage, alert payload, NSE + FFI, WAL + busy timeout;
  app/NSE fetch race resolved by the ratchet: second decrypt fails and is skipped). Stage 5
  Signal-parity presentation (each message its own local notification, trigger completes
  empty) **Built but gated off** pending Apple's filtering entitlement; interim rewrite
  model can leave stray generic banners.
- **Measured:** NSE footprint ~2-3 MB vs ~24 MB cap -> decision full core, fetch-based;
  slim-decrypt and ciphertext-in-push rejected (cost privacy or duplicate crypto to dodge a
  non-binding limit).
- **Decision: fail silent, not generic banner** (generic is a false signal on every burst);
  requires lost-push detection (Planned, not built) before enabling.
- **Background lifecycle (0xDEAD10CC):** suspended process holding shared-container SQLite
  locks is killed (nine field kills in a day), and a socket left open suppresses push. On
  expiration: `prepare_for_background` per core on GCD (never `Task.detached`): set flag,
  clean WS close, park reconnect, bounded wait, then **close** store connections via
  `GatedConnection` (drain-only failed: idle WAL connections hold locks). Resume reopens.
  iOS only. Divergences from Signal: no cross-process connection lock (NSE uses HTTP);
  store-layer gate instead of task cancellation. Residual: busy-timeout overrun; unannounced
  suspension; crashed sockets need server WS idle timeout (missing).
- Privacy: Apple sees token + generic alert timing; decrypted banner text lands in iOS
  notification store.
- Deferred: per-account targeting (would expose a pseudonym to Apple). Rejected: silent push
  primary; generic-text alert; hybrid fallback (for now); drain-only quiesce.

---

## 11. Abuse handling (12)

**Status: Partial.** Built: message requests, block, local spam reports. Not built:
forwarding, enforcement ladder, profile reports, group abuse. Follows Signal (passes App
Store 1.2). **Reports never contain content.**
- **Requests (Built):** un-accepted sender -> read-only thread with Accept/Delete/Report;
  no read receipts or typing until accepted; delivery receipts still sent; sender can't tell
  outcome. Known sender = curated or `is_bot` (should require server vouching).
- **Block (Built):** local, synced via contact record; blocked inbound decrypted (advance
  ratchet) then dropped silently; UI replaces composer (app-core doesn't refuse sends).
  **Server-side block rejected for v1** (leaks cut-off list, diverges from Signal); revisit
  only on observed queue flooding (per-pair rate limits or opt-in list).
- **Report (Partial):** only on requests (highest-value signal; blunts weaponized
  reporting); DID + reason enum to own server, stored for operator review, then block.
  **Planned forwarding:** reporter's server signs `{reported_did, reporter_homeserver, time,
  reason}` without reporter DID. Rejected: client->reportee server (leaks reporter);
  unsigned anonymous (forgeable). **Open conflict with 13** (no server-to-server channel):
  keep one narrow endpoint, or a signed reporter-anonymous token the client submits.
- **Enforcement ladder (Planned):** throttle at 5 distinct reporters/24h; suspend at 20/7d or
  50 total; ban at operator review or 100. Ban = this server only (DID is the user's).
  Forwarded reports count distinct servers.
- **Federation trust scoring (Speculative, kept as record):** assumes server-to-server
  federation; attestation store is itself seizable membership metadata; no signal with few
  servers. If ever needed: opt-in signed third-party blocklists as local inputs.
- **Profile abuse (Planned):** client name filter, profile reports, forced reset.
- **Never build:** content reporting/hashes, report button in accepted conversations, global
  ban list, client ML moderation, on-device scanning (legal conflict), "who reported me".
- Gaps: self-declared bot bypass (S-03); stranger group
  invites ungated (S-04); reports don't leave reporter's server. Open: group abuse, Project
  abuse, appeals, cross-server aggregation.

---

## 12. Projects (20, 21, 22, 23, 24, 25)

### Security model (20) — Partial

- A Project = web UI in an app webview + optional bot accounts (ordinary E2E participants).
  Anything touching content or membership goes through a **visible bot**; **no silent
  observer mode** (invariant). Trust model is Slack-workspace: user trusts homeserver admin,
  admin vets Projects. Server rows: `projects`, `project_bots` (one Project per bot),
  `project_capabilities`.
- **Project tokens (Built):** `POST /v1/project-token` (session) mints 32 random bytes,
  1 h, multi-use, stored with caller-supplied `project_url`; Project calls unauthenticated
  `verify` -> `{did, project_url}`. OAuth access tokens are Project tokens. Always discloses
  the real DID. Server learns which accounts asked for which Project; not webview traffic.
- **Webview (Built):** no JS bridge; input = URL params (`?token=`); output = navigation to
  `https://go.theavalanche.net/<action>/<arg>` intercepted by host match (works without
  Universal Links); routes `conversation/<did>`, `i/<token>`, `authorize`; any intercept
  dismisses. Chrome always names the Project. No custom schemes from webviews.
- **Permissions:** declared in manifest, **granted by admin at install, default-deny, no
  per-user runtime prompts** (would re-litigate admin choice, train reflexive Allow; identity
  prompts are theatre). Login consent screen is legibility, not scope approval. One
  dot-separated `namespace.action` space (earlier split separators rejected).
- **Server-enforced capabilities (Built):** only `accounts.read` (roster + join feed, live
  and 30-day catch-up) and `registration.gatekeeper` (pins Ed25519 key). `adminbot` Project
  bots implicitly hold all. Roster to an operator-installed bot adds no new leak; no group
  linkage.
- **Manifest (Built):** slug (`adminbot` reserved), permissions, `webEntries` (always
  non-official), unique `clientId`, exact-match `redirectUris`. Untrusted input: sanitize and
  attribute to (server, Project).
- **Identity follows interaction model:** bot-bearing Projects are always real-DID;
  pseudonymous per-Project identity only coherent for webview-only Projects (Speculative).
- **Officialness:** plain operator-set flag (signed attestation rejected — decomposes into a
  flag plus scope). Today only on `directory_entries`, never settable -> no checkmark anywhere.
- **Isolation:** separate processes/accounts; origin isolation only if each Project has its
  own origin — deploy bundle serves under `/p/<slug>/` sharing the homeserver origin.
- **Multi-server rule:** a conversation lives on one homeserver via one account; any Project
  affordance in it comes only from that server's Projects; nothing crosses accounts.
- **Known gaps:** no audience enforcement (S-17); tokens in query string; wrong
  identity for Network tab and `conversation/` links (first account); default unhardened
  WKWebView (S-23); officialness unsettable; self-declared bot bypass; group invites
  auto-accepted (asserted as fact here; 09 marks it Reported).
- **Planned:** mandatory `audience` on verify and mint only for
  installed origins (additive); tokens out of query string (fragment; Proposed, owner review); identity-correct minting/routing; webview hardening (per-Project data
  store, origin lock, per-Project subdomains); checkmark from `project_bots` linkage to an
  official Project exposed on account info; manifest from well-known URL.
- **Proposed:** OIDC "Sign in with Avalanche" (25).
- **Speculative:** client-honored scope vocabulary (identity, DM reach, surfaces); profile
  sharing via token minting; guest access (superseded in spirit by 13); a JS bridge only with
  its own permission system.
- **Rejected:** JWT (no key distribution, trivial revocation); reverse-proxying Projects
  through the homeserver with `X-User-DID` (plaintext through server, general proxy); runtime
  prompts; in-feed widgets.

### Testbot (21) — Built (dev/demo only)

Node `node:http` service: "Text Me" spawns a new ephemeral bot per tap (temp SQLCipher store,
Claude Haiku replies or echo fallback; read receipt + reaction exercises 33) and hosts the
OAuth demo. Each bot registers with testbot's bot signup key, read per spawn from
`TESTBOT_BOT_SIGNUP_KEY_FILE` (written by adminbot's manifest install; dev falls back to the
shared secret). Gaps: ignores `project_url`; unbounded account creation; shared `/p/` origin.
Planned: check audience, split OAuth demo. Rationale: ported from Rust to
TS to prove "any language on app-core" (and Node's single thread avoids the non-`Send`
workaround); ephemeral because it's a dev tool.

### Adminbot (22) — Partial

- **Superuser = link to reserved `adminbot` Project** (`AuthAdminbot`), not a DID. Admin API
  refuses to link/unlink/install/uninstall it or mint it a signup key; **only entry is a
  one-time claim** with `SUPERUSER_BOOTSTRAP_SECRET` (generated on the box, never shared;
  claim-once while the Project has no linked account; independent of gatekeepers). Recovery
  after state loss: `avalanche-reset-adminbot` (deletes old account, clears claim; fresh
  `#admins`).
- **`#admins` group membership is the admin roster** — E2E, so the server DB doesn't reveal
  who administers. Adminbot is the bridge between E2E authority and server privilege;
  concentration risk accepted, kept minimal, privileged commands legible in `#admins`.
  Principle: **"the threat decides the home"** — server resources -> server capability;
  seizure-sensitive social authority -> E2E state; offline/cross-server trust -> signature
  rooted in a cold key.
- **Coordination is data-carried, not bot-to-bot** (events, signed tokens, catch-up).
- No inbound surface; off-box operation supported and attractive (keys off the seized box),
  but the deploy bundle runs it on the server box.
- **Does today:** creates `#admins`; auto-invites new humans into every group where it is
  admin (promoting adminbot makes a group an onboarding target); announces bots; clamps
  timers to 4 weeks; release check; installs manifests at startup (writing each Project's bot
  signup key to `$SHARED/bot-signup-keys/<slug>.key` if absent); a few commands
  (`/install-project` mints/rotates and DMs the bot signup key; `/list-projects`; both gated
  on fresh `#admins` membership at any role).
- **Join events (server Built):** `AccountJoined {did, joined_at_ms}` (raw token no longer
  carried; S-01) pushed to
  `accounts.read` holders, logged 30 days, catch-up endpoint. Adminbot uses only live push.
- **`did:local:` decision (not built):** random per-server DIDs, no well-known literal, since
  clients key by DID and `did:local:adminbot` merges adminbots across servers (block one,
  block both). Rejected: `did:local:{hostname}:adminbot` (couples identity to hostname);
  client conversation-key rewriting.
- **Open admin by design (S-29):** every new human joins `#admins` and admin commands check
  membership, so everyone is an admin on a young server; intended for new orgs.
- **Gaps:** no onboarding explaining open admin or how to close it; reserved `did:local:` names squattable
  (S-30); `/audit` ungated; no catch-up; fixed DID; stale `ADMINBOT_DIDS` comment.
- **Planned:** open-admin onboarding plus a way to stop auto-admin (S-29); parsed claims in events; gate `/audit`; catch-up;
  random DID; officialness; uninstall/revoke.
- **Speculative:** routing rules (token tags -> channels); notification hints in invites;
  fuller commands; recovery ladder; backup recovery identity.
- **Rejected:** bot-to-bot RPC/service mesh (uptime coupling; use explicit HTTP trust edges
  if ever needed); gatekeeper commanding adminbot (token carries routing tags instead);
  signed officialness; per-admin server-verified credentials (server would accumulate the
  roster `#admins` protects); single-use bot tokens (testbot spawns a bot per user, so keys
  are per-Project and reusable). Two secrets because the configure tool's first-members
  invite is shared by design. Non-goals: general bot framework, RPC hub, federation-aware.

### Messaging extensions: core vs Project (23) — Partial

- **Thesis:** keep the in-conversation surface boring (native, auditable, E2E); push real
  interactivity into an explicitly opened Project webview.
- **Three rules:** (1) explicit handoff only — no Project reads the composer, no
  Project-rendered UI inside a conversation; (2) **1:1-DM litmus test** — must work in a DM
  with no bot -> core; (3) mechanism vs content — core owns mechanism/surface/privacy,
  Project supplies content at a seam.
- Placement: reactions, receipts, previews, contact cards core Built; replies, mentions core
  Planned; polls, live location core Speculative; rich text, emoji packs, slash autocomplete
  split Speculative; Giphy, surveys, task/flag actions, maps Project Speculative.
- **Never:** inline interactive cards or in-feed forms; lightweight actions are
  reactions/replies a member bot observes. Webview costs: not reproducible-build code,
  metadata hit, phishing surface.
- **Gaps:** no launch context; no Project deep link (invite redirect only opens a DM).
- **Proposed (pending owner review; changes Project contract and deep-link behavior):**
  `/project/<t>` links that survive install (installed+account -> open with fresh token;
  no account -> invite onboarding then Project; not installed -> landing page + paste-prompt
  "continue where you left off", no attribution SDK); **opaque launch context**
  `ctx = HMAC(K_launch(group master key), project_origin)` — only a Project whose bot is a
  member can map it; server and other Projects learn nothing.
- **Speculative:** entry points ("+", message long-press disclosing one message,
  participant long-press only where the Project's bot is a member); credential-free magic
  links; return content (fetched sender-side, capped, never auto-sent) or bot posts result;
  rich text as BodyRanges; slash commands as plain text; client-tallied polls; messenger-bot
  HTTP API Project; scaffold; offline webviews.
- **Rejected:** inline cards/forms; Project access to compose buffer; Project-hosted simple
  polls (wouldn't work in DM/offline); slash commands as invocations.

### Vetted onboarding / gatekeeper (24) — Partial

- **Built server side:** **closed registration default** (anything unrecognized = closed;
  fail-closed); admitted by a gatekeeper-signed invite (verified locally against pinned
  Ed25519 key; **server never calls the Project**), a bootstrap token (shareable
  `REGISTRATION_SHARED_SECRET`: plain accounts only, until a gatekeeper exists, never links a
  Project; `SUPERUSER_BOOTSTRAP_SECRET`: claims superuser once), or a Project's **bot signup
  key** (`{s, b}`, server-minted, hash-stored, bots only, links to its Project, reusable,
  re-mint revokes, survives gatekeeper install). Many gatekeepers allowed. Token `base64url(JSON)` short keys; signature over
  the exact claims string (no canonicalization hazard); `jti` redeemed before account creation
  (spent even if registration fails). **Token is the hand-off:** admission separate from
  routing; join events will carry parsed routing claims (Planned; raw tokens removed).
- **Gaps:** `GET /v1/invites` doesn't
  validate gatekeeper tokens (user creates passkey/DID before rejection); adminbot can't
  install gatekeepers; no gatekeeper Project exists.
- **Planned:** parsed claims; full validation in invite GET; gatekeeper
  install via adminbot; **the vetting Project**: anonymous form (the abuse surface),
  `#approvals` modeled on `#admins` with low-PII summaries, signed single-use invite with
  routing, delivered out of band. Notes: bearer credential over a non-E2E channel; PII
  residency; single-approver trust; **fail-closed is load-bearing**.
- **Speculative:** quorum approval; handle binding; external form adapters; existing
  identities go through the same vetting.
- **Rejected:** calling the Project to validate (uptime coupling); external forms like Google
  Forms (unvetted PII processor); gatekeeper -> adminbot command; single gatekeeper.

### Sign in with Avalanche (25) — Built on iOS/Android; OIDC Proposed

- Proves "controls this DID and has an authenticated account on this homeserver". **App is
  the authorization endpoint** (`go.theavalanche.net/authorize` Universal Link + `server_url`),
  **homeserver is the token endpoint**, **access token is a Project token** (verified with the
  existing `verify`). No JWT, no new introspection.
- Flows: same-device auth code + PKCE; cross-device RFC 8628 device grant ("another device"
  warning). No client secret. Login is a point-in-time bootstrap; no refresh tokens.
- Gaps: checkmark inert; first account on server used (no identity picker); name resolution
  needs a directory entry; **Desktop as authorizer not built (noted parity exception)**;
  audience unenforced.
- Planned: identity picker; checkmark; audience.
- **Proposed (pending owner review; reverses earlier non-goal):** OIDC conformance as main
  developer story — discovery doc, `id_token` JWT signed by a per-homeserver key (introduces
  the signing key 20 avoided), `userinfo`, scope `openid`; additive. Motivation: campaign
  tools can add an OIDC provider by configuration.
- Threat: cross-device consent phishing (bounded by copy, checkmark, short TTLs, rate limits,
  scoped token). Rejected: server-rendered login page (attack surface on server); reusing
  linking mailbox.

---

## 13. Messaging UX (30-37)

### Mobile app (30) — Partial

- No account without an invitation. Server cannot auto-enroll new users into E2E groups
  (needs the master key; must be a client or bot).
- Existing user: identity picker; **"join as existing identity" is a stub** (P1). Separate
  identities keep personas apart; one name per identity, no per-server overrides.
- Each conversation belongs to exactly one identity by construction; no per-message identity
  choice; account tabs when >1 identity (37).
- Tabs: Chats, Network (servers -> Projects), Settings, Search; tab bar hidden in
  conversations.
- Compose: **DM / New Group / Note to Self** actions; From starts empty and is fixed by the
  first contact's most recent identity (also prevents cross-server groups). Decided: 2+
  recipients always create a new group (Messages, not Signal dedup).
- Group detail Partial. Planned: add/remove members, invite-link controls, per-conversation
  mute (more important than threading for big groups).
- Rejected: recipient-count-decides with no buttons; separate New Group menu (Signal);
  per-row identity badges/switcher; importing OS contacts.

### Read tracking (31) — Built, with gaps

Per-message `read_at` (also starts disappearing timers); unread derived (can't drift);
scroll-visibility marking. Delivery receipts auto-sent on every inbound DM (including
requesters, but carrying the profile key only to accepted contacts, S-02); read receipts only
to curated contacts.
Gaps: no setting (recommend Signal default on with per-identity toggle — **decision
needed**); no debounce; group read receipts mostly suppressed (co-members uncurated; decide
whether groups get them, gate on membership); `SyncRead` never sent; receipt send under core
lock. Planned: VIEWED/PLAYED. Rejected: stored counter; watermark (`lastReadAt`); timer
polling; read receipts to un-accepted senders.

### Replies and threads (32) — quote-reply Proposed; full model Speculative

- **Planned:** `ReplyTo{author_did, sent_at, unsurfaced (always false), quote_text}` as
  `TextMessage` field 5; target identity `(author, sent_at)`; inline everywhere; DMs and
  groups together. Quote-reply is an additive proto field pending owner review.
- **One-primitive property:** every reply is a thread message; turning on threads later flips
  a default, not data.
- **Speculative full model:** per-reply "surface to channel" flag whose default depends on
  conversation shape; one-way promotion; following quiet by default; each message unread in
  exactly one place; announcement groups allow thread replies but gate surfacing (needs
  recipient `announcement_only` enforcement, S-15).
- Rejected: two reply primitives by conversation shape (bakes chat/channel line into data);
  building full threading now; per-thread inbox rows; private/subset threads. Superseded: 32's
  icon "shelf" (by 37's single row).

### Reactions (33) — Built

`(emoji, reactor, target)` encrypted message in the target's conversation via
`send_to_target`; target `(author, sent_at)`; **one reaction per person per message**
(replace/remove; idempotent, order-independent; can precede target); visible to all; cluster
on bubble; never enter feed/unread/badge; delete-for-everyone drops them. Gaps: reactions
outlive expired targets; no notifications; no who-reacted sheet. Planned: low-priority
notifications only for your own messages (coalesced), who-reacted sheet. Never: reactions
feed, badges, private reactions, super/paid reactions. Rejected: many per person (Slack);
reactions as feed messages.

### Connection state (34) — Partial

- AppCore is the single source of truth; UIs render with no client timers; everything per
  (identity, server) membership.
- **Layer 1 Built:** four-state `ConnectionState`; one reconnect task; jittered backoff
  1-30 s forever; offline-safe `login`; lazy auth with 401 re-auth once. Banner shows whenever
  any account is disconnected (the failure this doc targets).
- **Layer 2 Planned:** tiers Online / Retrying (<2 min, global banner) / ServerDown (2 min-7
  days, a property of that server) / Abandoned (>7 days, offer removal), computed in core,
  outage clock persisted; while ServerDown stop timers and probe opportunistically
  (foreground, network path, user action) with a probe floor.
- **401 vs 403 contract (Planned):** 401 transient; 403 = membership revoked, terminal
  (`Unauthorized`, stop reconnect; non-home removed with notice; home -> change home server).
  Server returns no membership 403 yet.
- Layer 3 (Speculative): mesh composes into one model. Send queue (Planned): pending state
  draining on reconnect; unused store `message_queue` — wire or delete. Device-offline
  detection (Planned).
- **Constraints:** removing a server preserves crypto (de-routing, not de-provisioning);
  dead home server -> migrate, don't drop; don't mark DMs unreachable before per-peer
  routing.
- Gaps: dead server pins banner and is probed forever (P1); no send queue, 403 handling, or
  path awareness; no server ping (S-24).
- Rejected: any-offline banner; fixed long timers (don't fire suspended, burst on wake); 403
  as unreachability; wiping crypto on removal; early DM unreachable markers.

### Attachments, link previews, contact cards (35) — Partial

- **Encrypt-then-upload (Built, Signal model):** pad to ~5% buckets; fresh 64-byte
  CBC+HMAC key; authenticated allocate; backend-blind upload; repeated pointer in
  `TextMessage` (albums are one message) with URL, key, digest, size, inline thumbnail;
  **digest verified before decrypting**.
- **Download unauthenticated by design** (unguessable server-minted id is the capability;
  server can't enforce recipients under sealed sender; enables cross-server fetch and
  presigned URLs). LocalFs backend; S3 Planned with no client change.
- **Pointer carries full URL (Decision 7)** — consequence: client must constrain hosts.
- Outgoing images always re-encoded: orientation baked, **EXIF stripped (privacy
  requirement)**, 2048 px JPEG. Receive is format-agnostic, so WebP send later is compatible
  (not HEIC/AVIF).
- **Link previews (Built):** generated by native client layer, not app-core (keeps SSRF and
  HTML parser out of the core bots run). **Load-bearing invariant: sender generates at
  compose; recipient never fetches the URL** (IP harvesting). Render only if `preview.url`
  occurs in body (anti-spoof). No opt-out setting yet.
- **Contact cards:** `SharedContact{did, name}` inline, **no profile key** (third party
  shouldn't decrypt subject's profile).
- **Lifecycle:** server can't refcount; blobs TTL 45 days (> 30-day queue, so offline/new
  devices can pull; delivery buffer, not backup); orphans fine; forwarding re-uploads with
  fresh key (avoid correlation, expiry coupling, original-sender deletion).
- Limits: 100 MB/attachment, ~500 MB/hour/account; scanning impossible by construction.
- **Known gaps:** arbitrary-URL auto-download incl. requests (IP harvest, memory DoS, LAN
  probe; S-08); download holds core lock; deletes/expiry leave attachment rows/keys and
  plaintext caches on all platforms (S-25); cleartext caches; server buffers whole blobs.
- Planned: fixes; S3; auto-download and local storage controls (deleting media keeps the
  message with a placeholder); preview opt-out; upload throttling; Desktop preview hardening.
  Offload Speculative (needs an encrypted backup substrate).
- Never: server transcoding, streaming in-progress uploads, cross-attachment dedup (leaks
  equality).
- Rejected: GCM for attachments; authenticated download; bare id resolved on own server;
  app-core fetching previews; recipient-fetched previews; refcounting; blob reuse on forward;
  deleting whole message when media removed (Signal).

### Editing and deletion (36) — Built, with gaps

- Two operations on one substrate targeting `(author, sent_at)`. **Load-bearing rule:
  recipients apply an edit or FOR_EVERYONE delete only if the cryptographically
  authenticated sender is the target's author** (session or sealed-sender certificate); server
  can't and needn't enforce. FOR_ME honored only from own devices.
- LWW by operation timestamp; **delete is the absorbing top of the lattice** (rejected:
  delete as plain LWW — a delayed edit could un-delete). Edits keep position, record revision;
  deletes tombstone, drop reactions/revisions. No notifications, unread, or bump; edit doesn't
  reset timer. Multi-device via Sent transcript.
- Limits intended: humans 24 h / ~10 edits; bots no cap, 30 days, no revision history
  (update-in-place pattern; "bot" must be server-vouched per 54). As built: only mobile UI
  hides after 24 h; no caps; recipients enforce nothing; bot exemption unwired.
- Gaps: delete leaves attachment keys/plaintext (S-25); out-of-order ops dropped not held;
  edits replace only body (stale preview); bot revisions accumulate. Version skew: unknown
  variants silently ignored (conservative failure).
- Rejected: separate global message ids; server-enforced authorship; editing attachments;
  edit-to-empty as delete.

### Chat organization (37) — Partial

- Default inbox: plain, one row per conversation, no tab row. **Account tabs Built (iOS,
  Android; not Desktop)** only with >1 identity: avatar + unread badge, filter.
- **Decided: no per-row identity/server marking**; the conversation is the context; if you
  want separation, make it a tab. Supersedes 30's per-row indicator and switcher.
- Per-conversation mute Planned (synced via conversation settings; muted excluded from badge;
  P1).
- **Speculative full model:** tabs are homes (exhaustive partition, no "All"); structural
  rules plus on-device classifier proposals; one evolving configuration; **automatic behavior
  never stomps the user's setup**; Threads catch-up row.
- Rejected: per-row marking; filter bar with All; threads as a tab; discrete switch from
  server to topic tabs; the shelf.

---

## 14. Contacts, profiles, avatars, bots, invites, accounts (51-55)

### Contacts and profiles (52) — Partial

- Principles: interaction-driven (no "Add contact"); Signal technical model; **server never
  sees plaintext profile data** (seized server yields blobs and DIDs, not a named roster);
  contacts local-only; one book across identities merged at query time (per-identity tables;
  caveat: unlocking exposes the merged book); per-DID; substrate profile separate from
  Project profiles.
- Contact row: `is_curated` (sticky, set only by deliberate gestures: send DM, accept, save
  shared card), `is_blocked` (orthogonal), `has_pending_request`, `nickname` (local, not yet
  synced), `last_interaction_at`. Curation drives request gate, read receipts, People/Other
  sectioning. Group co-members not curated.
- Profile: name + avatar ref, AES-GCM under a profile key (no rotation on edit) that rides
  outgoing DMs, group messages, and receipts.
- **Fetch throttle decided in core and persisted** by last outcome (improves on Signal's
  in-memory LRU). Profile GET returns identical 404s (no existence leak). Nickname > profile
  name > truncated DID; nickname never erases the real name.
- **Gaps:** accepting a request doesn't share your profile (requester learns your name on
  your first message; S-02 itself fixed); self-declared bot bypass (S-03); no `profile_version` liveness; profile fetch
  under core lock.
- Planned: `profile_version` envelope field (additive contract); favorites/notes/nickname UI;
  `preferred_identity`; cross-identity contact backup under a recovery-derived key;
  profile-key rotation; safety-number UI.
- **Proposed:** delivery keys derived from the profile key (13) — makes the S-02 fix
  structural.
- Speculative: contact merging; Project-introduced bulk save.
- Rejected: per-member-server profile replication; separate `blocked_dids` table; daily
  background sweep; per-row identity marking.

### Avatars (55) — Partial

Small JPEG, client-encrypted (AES-GCM), **overwrite-in-place** blob, one per account and per
group; pointer (version + digest) lives inside already-encrypted state (profile blob under
profile key; group state with key derived from master key). Whoever can read the name can
read the picture; no new key distribution. Digest verified before decrypt (server can blank,
not spoof). Group avatar object id derived from master key (no server group link). Limits:
512 px, <=48 KiB encoded, 60 KiB plaintext, 64 KiB ciphertext, 60 uploads/hour. Device-local
`avatar_cache`. iOS sets/displays both; Android displays only; Desktop initials only.
Gaps: **shared blob namespace** lets any account delete/overwrite attachments and enumerate
profile-avatar ids from sequential account ids (S-27); group blob replaceable by any
old-key holder (fix: per-version object id); linked device doesn't fetch own avatar; upload
under core lock; parity. Rejected: avatar as attachment pointer (45-day TTL, per-send);
separate avatar key; server-visible group->avatar mapping; presigned upload (too small to
matter).

### Bot presentation (54) — Partial

Two independent properties: **provenance** (official bot my server vouches for — verifiable)
and **automation** (is it a bot — always self-declared). Client-applied chrome avatar bytes
can't override: **hexagon avatars and chamfered bubbles** for `is_bot` (Built all
platforms). Gaps: `is_bot` is self-declared yet bypasses requests (S-03) and is presented as
if vouched; no verified tier. Planned: exempt only bots linked to an installed Project on
your server (server-vouched field); checkmark from official installation (same-server only);
hedged "Automated (not verified)" tier. Target tiers: verified / self-identified / person
(absence of signal is not a claim of humanity). Speculative: `account_kind` in profile; server
policy requiring declaration; synthetic default avatars. Rejected: mandatory constrained bot
avatar palette (only constrains honest bots; redundant with badge; strips branding).

### Invite tokens (51) — Built

`base64url(JSON)` short keys in `https://go.theavalanche.net/i/<token>` (legacy `/invite/`).
**Personal invite** `{s, d?}` unsigned, client-generated, doubles as contact link; validation
returns server name and a redirect to the inviter DM; does not admit on closed servers.
**Gatekeeper token** signed (24). **Bootstrap token** `{s, k: secret, p?}`: shareable secret
admits signups only; superuser secret claims `adminbot` once. **Bot signup key** `{s, b}` (24).
Gaps: no server step; no group auto-enrollment; tokens in URL path land in landing-page logs —
any secret-bearing future token must use the fragment; invite GET only understands personal
tokens. Planned: `server_step_url` onboarding webview; group
auto-enrollment via `group_invitations` in the fragment; in-app invite creation. 51 lists
deferred deep links through install as Speculative (see contradictions). Rationale: personal
tokens are discovery, not access control; Projects sign, server pins and verifies locally;
short keys for scannable QR.

### Multi-account UX (53) — Partial

Identities each with server memberships; one **discovery (home) server** per identity. Built:
Accounts screen (identity groups, `home` tag), identity detail (contact QR = personal invite,
DID, public-explainer), **delete identity** (load-bearing order: leave groups and delete
accounts best-effort -> **PLC tombstone must succeed** -> only then wipe local; tombstone is
the authoritative "gone" and wiping first would make failure unretryable), server detail,
leave server (graceful; leaves hosted groups, deletes account; home server can't be left in
place). Gaps: add-server stub; no activity/reachability rows; no remove-from-device; no change
home server. Planned: real second-server registration (requires 06 contexts; under 13 a
second server is for community, not reachability); reachability rows tied to 34 tiers;
remove-from-device as local de-routing preserving crypto and keeping groups listed; change
home server (today PLC update; under 50 P3 a signed move notice). Rejected: remove wipes
crypto.

---

## 15. Deployment (41, 42)

**Server deploy/upgrade (42) — Partial.** Zulip model: each release in an immutable
`deployments/<tag>/`, atomic `current` symlink flip; updater and units ship inside each
release (self-updating); operator `.env` never rewritten; one git tag for all first-party
artifacts; update halts on reconcile mismatch rather than auto-fixing; bots handled
uniformly; Projects get Caddy `/p/<slug>/` routes. Gaps: no rollback, no pre-upgrade dump, no
N-1 migration check, relay outside bundle. **Setup starts at the configure tool** (website
"Set up your homeserver": cloud-init + first-members invite QR; must stay one-paste for
non-technical organizers). `migrate_env_files` (install and update) appends missing env lines
and retires secrets: generates `SUPERUSER_BOOTSTRAP_SECRET` on the box (never in the browser),
drops the shared secret from bot envs, adds bot signup key paths. Operator commands include
`avalanche-reset-adminbot`.
Planned: dumps + rollback, generalized `ensure-secret`, `/upgrade` from `#admins`. Rejected: per-file
binary swap; updater baked into cloud-init (froze boxes at provision-time logic);
`self-update` server; per-Project upgrade logic; independent component versions;
auto-update.

**Relay (41) — Built**, manual droplet deploy (legacy `actnet-relay` names), Caddy TLS,
hardened systemd, state is one losable SQLite file. Planned: fold into `av-deploy`, separate
dev/prod, authenticated registration.

---

## 16. Platforms (60, 61, 62)

- **Android (60) — Built**, near file-for-file port of iOS (`AppViewModel` mirrors
  `AppState`); all FFI via `ActnetService`; sync exports in `Dispatchers.IO`, long waits
  awaited directly. Passkeys via Credential Manager PRF; Digital Asset Links on
  `theavalanche.net` (Cloudflare must not cache `/.well-known/*`). Gaps: recovery-key
  banner hardcoded off; no avatar setting; killed-process push defers to next launch (no NSE
  equivalent); identity list in SharedPreferences. Rationale: native Compose; JNA + generated
  sources over AAR.
- **Desktop (61) — Partial.** Tauri 2 + Solid; Rust commands over `app-core`; TS owns the event
  loops; left sidebar instead of tabs; Project pages in IPC-isolated `WebviewWindow`. **Phrase
  is the credential (sanctioned divergence)**; 56 is the passkey path. Gaps: **constant DB key
  (S-05)**; **bridge doesn't compile**; no Project login, avatars, account tabs, search, QR
  scanning; plain JSON metadata. **Rationale:** Tauri over Electron (OS-patched webview vs
  bundled Chromium; Rust links app-core like mobile; small footprint on cheap, replaceable
  hardware). Solid for the only privileged webview (small dependency tree); rejected
  Dioxus/Leptos (immature WASM integration), React/Vue (huge dep trees), Svelte, others.
- **Parity matrix (62) — Built**, the only parity tracker (60/61 deliberately have none).
  Desktop column describes code, not a shippable binary.

---

## 17. Proposed changes awaiting project-owner review (index)

Do not implement until approved. Each alters a contract.

1. **Identity root wrapping, backup-domain passkey, priority rotation keys, root never on
   devices, device-signed linking** (50 P1/P2; S-06).
2. **Unpublished self-certifying private identity; `did:plc` as opt-in public link** (50 P3;
   S-20).
3. **Authenticated recovery-blob fetch via per-factor fetch key; drop `device_ids`** (50 P4;
   S-19).
4. **Sealed sender for 1:1 and SKDM traffic with profile-key-derived delivery keys** (03,
   13, 52; S-09).
5. **Client-side federation** with identified first contact and move notices (13); resolve
   abuse-report forwarding without server-to-server calls (12).
6. **Acquisition path:** `/project/<t>` links surviving install, opaque per-conversation
   launch handles (23).
7. **OIDC-conformant Sign in with Avalanche** (25, 20).
8. **Project tokens out of the URL query string** (20).
9. **Quote-reply `reply_to`** additive proto field (32).
10. **Group/DM flag on `ConversationSummaryFfi`** so clients stop parsing id prefixes (02, 07).
11. **Validate `conversation/<did>` deep links before creating rows** (02; S-28).
12. Desktop passkey bridge via external browser (56).

---

## 18. Rejected and superseded designs (consolidated index)

**Superseded**
- Server-to-server multi-homing federation (discovery server in PLC, server keys, learned
  routes, trust scoring) -> client-side federation Proposed. Sender's server saw all
  cross-server recipients; leaky machinery; untrustable peers (13).
- Federation trust/attestation scoring (12 §5): presumes S2S; attestation store is seizable
  metadata; no signal yet.
- Skipping sealed sender for invite `GroupContext` DM: superseded in effect — reveals who was
  invited (03).
- "Deliver to all claimers" squatting defense and "reject non-owned subscribe": both replaced
  by secret-backed pseudonyms (03).
- DID-shaped zkcredential credential ("option 2"): shipped then reverted (03 §2.4).
- Per-row identity indicator and in-conversation identity switcher (30) -> account tabs (37).
- Threads "shelf" of icons (32) -> single pinned catch-up row (37).
- Recipient-count-decides compose with no buttons (30) -> explicit DM / New Group.
- Avatar as attachment pointer inside profile (55) -> overwrite-in-place blobs.
- Cloud-init-baked updater (42) -> in-release deploy bundle.
- Drain-only store quiesce (16) -> close-on-suspend.
- Silent `content-available` push as primary (16) -> alert + NSE.
- Rust testbot (21) -> TypeScript on napi.
- `00`'s substrate pub/sub/RPC between Project instances (13) -> Project concern.
- Guest access to remote Projects (20) -> superseded in spirit by client-side federation.
- Passive backup snapshots (05 §7): parked, not superseded; don't re-enable without a
  replacement.

**Rejected** (reasons inline in the sections above)
- Crypto/server: own crypto; Go/C++ server (01); per-device identity keys (04); bearer group
  tokens (03); CRDT/OT group changes (03); GCM attachments (35); JWT tokens (01, 20); Redis;
  client-store connection pool (07); migrate on start (42); server block lists (12);
  server-enforced authorship (36); blob refcounting; authenticated download (35).
- Sync/identity: multi-master/CRDTs; consumer cloud (05, 50); write-path helpers; inline push;
  sidecar payload (05); per-feature SyncMessages; provisioning WebSocket; desktop-always-secondary
  (04); homeserver-held recovery keys; per-server RPs (50); WebView WebAuthn; custom URL
  schemes (56); sync FFI for long waits; `Task.detached` wrappers (07).
- Push: server-held tokens; server-direct UnifiedPush; Web Push encryption; slim NSE or
  ciphertext-in-push; generic-banner fallback (15, 16).
- Groups/federation: announcement-only at scale; identified or fully unlinkable admin sends;
  opt-in discussion group; author tallies; create-as-supergroup (08); Matrix replication;
  ATProto federation; multiple discovery servers (13).
- Projects: `X-User-DID` reverse proxy; runtime prompts; split separators; signed
  officialness (20); bot RPC mesh; gatekeeper->adminbot commands; per-admin credentials;
  literal/hostname `did:local:`; key rewriting (22); calling the gatekeeper; external forms
  (24); server login page; mailbox reuse (25); in-feed cards/forms; compose-buffer access;
  Project polls; slash invocations (23).
- Abuse: direct-to-reportee or unsigned reports (12).
- Messaging UX: multi-reactions; reactions as messages (33); two reply primitives; thread rows;
  private threads (32); delete as plain LWW; editing attachments; edit-to-empty; global ids
  (36); stored counters; watermarks; polling; receipts to requesters (31); recipient or
  app-core preview fetch; blob reuse; whole-message media delete; bare ids (35); any-offline
  banner; long timers; 403 as outage; crypto wipe on removal (34, 53); per-row marking; All
  filter; threads tab; regime switch (37); New Group menu; OS contacts (30).
- Contacts/bots: per-server profile replication; `blocked_dids`; daily sweep; in-memory
  throttle (52); bot avatar palette (54); avatar key; server group->avatar map (55).
- Platforms/ops: Electron, React/Vue, Svelte, Dioxus/Leptos, Elm/Mithril/Lit (61); file swap;
  self-update; per-Project upgrades; per-component versions; auto-update (42).

---

Generated from docs/ as of 2026-10-03. Source docs are authoritative.
