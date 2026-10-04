# 03 — Groups

> **Status:** Partial — action-bound groups are built end to end (zkgroup credentials, encrypted state, sealed-sender sends, invites, link joins, roles, avatars, disappearing messages). The §3.9 membership-opacity property has real gaps (below), the §9 invariant tests do not exist, and cross-server casual groups (§6) and mesh (§7) are not built.
> **Last verified against code:** 2026-10-04

## Summary

A group lives on exactly one homeserver (the *hosting server*). Its state is an
encrypted blob the server stores but can't read; the server keeps only an opaque
routing subset (`encrypted_member_id`s, roles, policy) so it can enforce membership
without learning who the members are. Message content is Sender Keys, delivered in
a multi-recipient sealed-sender envelope on an endpoint that never sees a session
credential. This is Signal's private-group design with the homeserver as the
credential issuer.

The design goal is that **a seized server does not yield group memberships**
(§3.9): membership is protected **at rest**. It is **not** hidden from whoever
operates the server live: every group send lists each recipient's ID, an unsalted
hash of their DID, so the server learns the full membership of any group that has
activity (§3.11). That is accepted by design, as in Signal: the operator is your
org, which the trust model already trusts with your social graph (`00`). The
server-side group tables mostly hold the at-rest property. The data around them
does not yet: invites and Sender Key distribution travel as identified DMs, the DM
queue records sender accounts, group-change history is server-readable and never
pruned, and the relay plus the server together can correlate pseudonyms. See Known
gaps and `09-security-posture.md`.

Background: Chase, Perrin, Zaverucha 2019, *The Signal Private Group System*;
libsignal at the pinned commit (`rust/zkgroup`, `rust/zkcredential`).

## Known gaps

Verified against code on 2026-10-03 unless marked otherwise. Security items are
also tracked in `09-security-posture.md`; fixes are in `02`.

**Membership opacity (§3.9)**
- **A live operator sees membership (accepted).** Each group send carries every
  recipient's service ID (`SHA-256("actnet-did-to-uuid-v1" ‖ did)`, unsalted;
  `crypto/src/groups/group_key.rs` `did_to_uuid`) paired with their EMI
  (`net/src/groups.rs` `GroupSendRecipient`). The server holds every registered DID,
  so it can match them exactly. Not stored today; §3.9 rule 6 and its test keep it
  that way. Hiding recipients from a live operator would need an anonymous send
  (Speculative) and still leaves receive-side IP correlation.
- **Identified DM plane leaks co-membership.** `GroupContext` invites and
  `SenderKeyDistribution` messages go out as ordinary identified DMs
  (`app-core/src/messaging.rs` `send_dm`), and the DM queue stores
  `sender_account_id` (`infra/migrations/001_initial.sql:64`,
  `server/src/routes/messages.rs:137-146`). Every join fans SKDMs from the joiner
  to every member, so a live operator sees the co-membership graph and a seized DB
  holds it in any undelivered rows (up to the 30-day queue TTL).
- **Relay + server correlation.** A device registers its DM pseudonym and all its
  group pseudonyms with the same relay under one device token; the server maps
  group pseudonym → group. Whoever holds both the relay DB and the server DB can
  rebuild memberships. The 7-day per-group rotation offset (§3.7) is not
  implemented and would not help against this.
- **Group-change history is server-readable and unbounded.** `group_state_history.actions`
  is the canonical JSON of the submitted actions (`server/src/routes/groups.rs:589`),
  including affected EMIs, roles, push pseudonyms and the invite-link password,
  though the migration annotates it `-- opaque`. Nothing prunes the table — the
  256-revision ring buffer of §3.4 is not implemented (no `DELETE FROM group_state_history`
  anywhere).
- **Exact timestamps.** `group_state_history.created_at` and
  `group_member_pseudonyms.created_at` are `now()` (`infra/migrations/010_groups.sql`,
  `020_group_device_pseudonyms.sql`, `server/src/db/groups.rs:456`), contrary to
  §3.9 rule 5. The pending tables are correctly day-aligned.
- **IPs are persisted.** Group send, change submission and push-binding are
  rate-limited by IP in the Postgres `ip_rate_limit_counters` table
  (`server/src/routes/groups.rs:528,633,727`, `db/ip_rate_limits.rs`), rows kept up
  to an hour. §3.11 says send IPs are "never persisted".

**Delivery**
- **Pseudonym squatting.** WebSocket subscribe has no ownership check and is
  last-writer-wins (`server/src/routes/websocket.rs:418-423`); subscribing drains
  queued rows and an ack deletes them. Members (and pending invitees) can read other
  members' first pseudonyms from the change history. So a member can silently
  swallow another member's group messages. §8 required "deliver to all claimers";
  that was not built.
- **Recipient not in group is silently skipped**, not rejected with 400
  (`routes/groups.rs:790-795`). Harmless (the token still binds the recipient set)
  but contradicts §3.11.

**Client-side checks**
- **No membership check on inbound group traffic.** Inbound SKDMs are installed
  from any DM sender (`messaging.rs:1766`, `groups.rs:1896`) and `GroupMessage`
  bodies are decrypted with no check that the sender is in the cached member list.
- **No Sender Key rotation on removal.** `seed_own_sender_key` is idempotent and
  only called at create/invite/accept/reconcile; nothing re-keys when a member is
  removed. A removed member keeps every member's current Sender Key. *Plausible,
  not tested:* combined with the missing membership check, a removed member (or
  anyone holding the master key) can inject messages over the DM path.
- **`announcement_only` is not enforced by recipients.** The server can't enforce
  it (sealed sender) and app-core never checks it on receive.

**Server checks**
- **No per-group rate limit** (`group_policy.rate_limit_per_minute` does not exist)
  and **no per-recipient send budget** (zkgroup endorsement tokens expire; they do
  not count uses).

**Product gaps**
- No client path to `modify_policy` or `modify_description` (no FFI), so join
  policy, invite-link password and announcement-only can't be changed after
  creation, and there is no UI to create an invite link.
- Group push pseudonyms are rotated during reconcile but not on the 7-day schedule
  of §3.7.

## 1. Scope

Covers action-bound (single-server) groups: credentials, encrypted state, change
authorization, delivery, sends, invites, and the threat checklist. Related docs:
`12` (abuse), `13` (federation), `08` (very large channels), `14` (mesh), `32`
(threading), `33` (reactions), `55` (group avatars).

## 2. zkgroup in our pinned libsignal — Built

### 2.1 What's there

We depend on `libsignal-zkgroup` and `zkcredential` at the pinned libsignal commit
(the workspace patches `curve25519-dalek` to Signal's fork, which zkgroup needs).
The pieces we use:

- **Server params.** One `ServerSecretParams` per homeserver (generated at first
  boot, persisted in `zkgroup_server_params`), public half served at
  `GET /v1/groups/server-params`.
- **Group params.** `GroupMasterKey` (32 bytes, created by the founder, shared E2E)
  → `GroupSecretParams` → group id, member-id encryption, state-blob encryption.
- **Auth credentials.** `AuthCredentialWithPniZkc`: issued daily, presented per
  group; the presentation carries the member's id encrypted under the group key, so
  the server can check validity without learning identity.
- **Group send endorsements.** Per-recipient anonymous tokens proving "the sender may
  send to these recipients in this group", combined into one `GroupSendFullToken`
  per send.

### 2.2 What works directly

Group id derivation, state-blob encryption, member-id encryption and endorsements
are generic crypto over a master key and a server key — nothing Signal-server
specific.

### 2.3 The identity attribute: DID → UUID

zkgroup's identity attribute is a 16-byte UUID (Signal's ACI/PNI); we have
variable-length DIDs. **Decided:** carry `UUID(did) := SHA-256("actnet-did-to-uuid-v1" || did)[..16]`
as `Aci::from(UUID(did))` (and the same bytes as the `Pni`). `encrypted_member_id`
is a stock `UuidCiphertext`, deterministic in `(did, group key)`, opaque to the
server. Clients map EMI → DID through the cleartext member list inside the encrypted
state. Identity and session stores are keyed on
`Aci::from(did_to_uuid(did)).service_id_string()` because sealed sender parses
`ProtocolAddress.name()` as a ServiceId (`crypto::groups::did_to_service_id_string`).

Rationale and the rejected alternatives are in §2.4.

### 2.4 Why stock zkgroup with a derived UUID

- **Rejected: a DID-shaped credential on `zkcredential` ("option 2").** This was
  the first choice and shipped. It was reverted when the same DID-vs-UUID mismatch
  appeared in `GroupSendEndorsement`, and would have recurred for every zkgroup
  primitive, each needing a parallel ~500-line security-sensitive reimplementation.
  Its claimed advantages didn't hold up: server opacity and cross-group
  unlinkability come from the per-group encryption key in both schemes; the
  128-bit collision space matches zkgroup's own ACI, and a collision would have to
  be found among DIDs already in the system and would be caught by the client's
  cleartext member-list cross-check.
- **Rejected: blind-signature or rotated bearer tokens ("option 3").** Strictly
  weaker anonymity.
- The `app-core` API stays scheme-agnostic (`encrypt_member_id(&str)`), so an MLS
  swap remains possible later.

### 2.5 API surface

`crypto::groups` is a thin wrapper over zkgroup: `GroupKey` (generate, group_id,
encrypt/decrypt state, `encrypt_member_id(did)`), `did_to_uuid`,
`ServerSecretParams`/`ServerPublicParams` newtypes, and re-exported
`AuthCredentialWithPniZkc{,Response,Presentation}`. Sealed-sender support lives in
`crypto::sender_cert` (single-key trust-root chain, `issue_sender_cert`,
`validate_sender_cert`), `crypto::sealed_sender` (`encrypt_group_envelope`,
`parse_sent_message`, `decrypt_envelope_to_usmc`) and
`crypto::groups::endorsements` (issue/receive/`token_for_recipients`/
`verify_token_for_service_ids`). Read the source for signatures.

## 3. Encrypted group state

### 3.1 What group state is — Built

**Encrypted state blob** (opaque to the server; the source of truth for clients):
group identity, members `(did, encrypted_member_id, role, joined_at,
profile_key_ciphertext)`, metadata (title, description, avatar reference + key),
policy (expiry timer, announcement-only, join policy), and a monotonic `u64`
revision. The plaintext is `proto::groups::GroupState` (`core/proto/groups.proto`).
The DID is in cleartext *inside* the blob so members can render names.

**Server-visible routing subset** (the minimum to enforce membership and route):
- `member_credentials(group_id, encrypted_member_id, role)` — full members.
- `group_member_pseudonyms(group_id, encrypted_member_id, group_push_pseudonym)` —
  one row per member *device* (migration 020). Accepted leak: the server learns
  device count per member, as Signal does.
- `members_pending(group_id, encrypted_member_id, role, day_aligned_invited_at)` —
  invited, not yet accepted (Signal's `MemberPendingProfileKey`).
- `members_pending_approval(group_id, encrypted_member_id, group_push_pseudonym,
  day_aligned_requested_at)` — link-join requests awaiting an admin (Signal's
  `MemberPendingAdminApproval`).
- Policy columns on `groups`: per-action minimum role, join policy, invite-link
  password, announcement-only.

Every change updates both views in one transaction (§3.3). Clients trust the
encrypted blob, never the server's subset.

### 3.2 Where it lives — Built

The server stores, per group: public `group_id`, `group_public_params`,
`current_revision`, the latest `encrypted_state`, the policy columns, and
`group_state_history` (revision, encrypted state, actions). Schema:
`infra/migrations/010_groups.sql`, `020_group_device_pseudonyms.sql`.

**None of the group routing tables has a DID or account column.** The server
can't answer "which groups is DID X in?" or "which DID is EMI E?" from its group
tables. (It can partly answer them from DM-plane metadata — see Known gaps.)
`encrypted_member_id` is a 32-byte `UuidCiphertext`.

### 3.3 How updates are authorized — Built

A change is `{revision, new_encrypted_state, actions}` posted to
`POST /v1/groups/{id}/changes` with an `X-Group-Auth` presentation. Action types
(`ActionsWire`, `server/src/routes/groups.rs:432`):

- Admin-class, batchable: `invite_members`, `remove_members`, `modify_member_role`,
  `approve_join_request`, `deny_join_request`, `modify_policy`, and the
  sub-encrypted `modify_title`, `modify_description`, `modify_expiry`,
  `modify_avatar` (`55`; gated by the title role).
- Self-class, must be the sole action: `promote_pending_members`, `decline_invite`,
  `join_via_link`, `cancel_join_request`, `leave` (`53`; the named EMI must be the
  actor's).

Server checks, in order:
1. Presentation verifies; extract the actor's EMI.
2. Actor eligibility by class: admin-class → actor in `member_credentials`;
   promote/decline → in `members_pending`; cancel → in `members_pending_approval`;
   `join_via_link` → no membership needed, but the link password must match
   (constant-time) and the join policy must not be `Closed`.
3. `revision == current_revision + 1`, else 409 (§3.5).
4. Role check for admin-class actions against the actor's role and the group
   policy. `modify_policy` and `modify_member_role` are **protocol-fixed Admin** —
   if members could change them they could grant themselves anything.
5. Apply all structural changes, store the new blob, bump the revision, append
   history — in one transaction.

`join_via_link` branches on the join policy: `OpenLink` → member immediately
(`200 {member}`), `RequestToJoin` → pending approval (`202 {pending}`), `Closed` →
rejected.

The actions *are* the diff; there is no separate cleartext diff. The server sees
which operations happened, which EMIs and roles they touched, push pseudonyms, and
policy values; it does not see titles, descriptions, expiry values or profile keys
(sub-encrypted under the group key).

### 3.4 Fetching state — Built

`GET /v1/groups/{id}` and `GET /v1/groups/{id}/changes?from_revision=N`
(presentation-auth) return the current state or the change list, 256 per page.
**Fetch is membership-gated and non-members get 404, not 403**
(`not_found_or_forbidden`, `routes/groups.rs:1502`), so a valid credential can't be
used to probe for group existence. Full members and pending invitees may fetch
(invitees need the state to build their promotion); pending link-requesters may not.

**Why keep change history:** catch-up bandwidth, backfilling "Alice added Bob"
timeline entries for offline periods, and — the load-bearing reason — tamper
detection: a client walking `(state_N, change, state_{N+1})` can check each state
is the legitimate continuation. What clients do on detected tampering is not
designed.

**Sizing (Planned):** keep the last 256 revisions per group. Not implemented —
history is currently never pruned (Known gaps).

### 3.5 Concurrent updates — Built

Monotonic revisions: exactly one concurrent update wins, the loser gets
`409 {current_revision}`, fetches the missing changes, reconciles, and resubmits.
Actions are **declarative** ("ensure these members are absent", "set title to T"),
so a retry after 409 is idempotent. CRDT/OT was rejected as overkill for rare admin
changes.

### 3.6 Role enforcement is layered — Built

The server enforces what it can see (membership, role, policy) at submission. Clients
re-verify each change against the authoritative encrypted blob when applying it,
which catches a compromised or buggy server and races between policy and role
changes. app-core persists each applied change as a `message_history` system row
(`kind > 0`) and emits `IncomingEvent::GroupMetadataChanged`; every UI renders these
as centered system lines (root `CLAUDE.md`, "Group/admin actions surface a system
message").

### 3.7 Fan-out, delivery, and notification — Built (rotation schedule Planned)

Each member device has a `group_push_pseudonym`, distinct from its DM pseudonym and
registered directly with the relay. The server holds `(group, EMI, pseudonym)`; only
the relay holds `(pseudonym → device token)`.

- **Online:** on WebSocket connect the client sends `SubscribeGroupPseudonyms` with
  its full pseudonym set (`app-core/src/connection.rs:126`). The server keeps an
  **in-memory** `pseudonym → socket` map (`AppState::group_subscriptions`), drains
  queued rows for newly subscribed pseudonyms, and pushes `GroupDeliverRequest`;
  the client acks and the row is deleted.
- **Offline:** the server sends a content-free wakeup to the relay by pseudonym; the
  device wakes and drains via WebSocket or `GET/DELETE /v1/groups/{id}/messages`
  (presentation-auth; the pseudonym is resolved server-side from the presentation).
- **Live-memory caveat (accepted):** while a socket is connected the server's memory
  links account ↔ pseudonym ↔ group. Never persisted; cold seizure doesn't yield
  it. Signal accepts the same tradeoff.
- **Rotation (Planned):** 7-day rotation with a per-group offset
  (`registered_at + 7d + hash(group_id) mod 7d`) via `POST /v1/groups/{id}/push_binding`.
  The endpoint and `rotate_group_pseudonym` exist; nothing schedules them yet.

Squatting is a known gap (above).

### 3.8 Message expiry — Built

The disappearing-messages timer lives in the encrypted group state; the server never
learns it. Clients stamp it on each outgoing group message and the local reaper
deletes on schedule (§5). Independently, the server deletes each undelivered queue
row after its expiry (default 30 days, Signal's number) via the
`group_message_expiry` task. The server can't be made to extend retention past the
backstop: the group send endpoint clamps the sender-supplied expiry to the same
bounds as DMs (`clamp_group_expiry`, fixed 2026-10; S-16).

### 3.9 Schema discipline for membership opacity — Partial

Scope: this is an **at-rest** property — a seized server, or its logs. A live
operator can see membership (§3.11, Known gaps); that is accepted.

The property "a seized server does not yield group memberships" holds *structurally*
— because the server never holds the group key — **only if** the server keeps no
auxiliary data linking DIDs to groups. Rules:

1. No `(did → groups)` table or persisted cache, ever.
2. No `(encrypted_member_id → did)` map.
3. Credential issuance is not logged with the credential identifier (per-DID-per-day
   rate counters are fine).
4. Presentation verification logs counts only, never identifiers or EMIs.
5. Timestamps on group routing rows are day-aligned or omitted.
6. Recipient service IDs seen during a group send, and their pairing with EMIs, are
   never logged or persisted. They identify every member (§3.11). Enforced by the
   `group_send_handler_logs_nothing` test (`server/src/routes/groups.rs`): the send
   handler contains no logging at all.

Server-side group-management operations that need DID ↔ group lookup ("remove DID X
from all groups") are not available; they must be client-driven.

**Status.** Rules 1–4 and 6 hold in the current schema and code. Rule 5 is violated by
`group_state_history.created_at` and `group_member_pseudonyms.created_at`. More
importantly, these rules only cover the group tables. The property as stated is
undercut by:

- identified DM traffic for invites and SKDMs, plus `sender_account_id` on the DM
  queue (a timing-and-recipient-set signature of group joins);
- server-readable change history that is kept forever;
- relay + server correlation of pseudonyms;
- IPs in the rate-limit table.

So today the honest claim is: **a seized server's group tables don't name members,
but the server's other tables and a live operator can reconstruct much of the
membership graph.** `09-security-posture.md` states the overall posture; the Planned
section below lists the fixes. If a change would relax a rule, update this section
and `09` first.

### 3.10 Invite and link-based join flows — Built

Joining is two-step, following Signal: an action puts the user in a pending state,
then a self-action (or an admin approval) promotes them. The invitee supplies their
own profile key and fresh pseudonym at acceptance, may decline, and first contact
needs no extra round trip.

**Invite (admin-initiated):** admin submits `invite_members` → the admin's client
DMs the invitee a `GroupContext {group_id, master_key, hosting_server_url,
inviter_did}` (X3DH PreKey message if no session) → invitee stores the key, fetches
state, and submits `promote_pending_members` (or `decline_invite`). Clients submit
the action first and the DM second; a pending row is a recoverable receipt if the DM
fails.

**Link join (user-initiated):** the link carries the master key and the invite-link
password. The requester submits `join_via_link`; the server decides immediate member
vs pending approval vs rejected, so the client shows a neutral "Join" and renders the
outcome. Admins approve or deny pending requests.

**What the master key grants.** Without a membership-gated server fetch, a master-key
holder can derive group params and compute their own EMI, nothing more: no state
(fetch is gated), no content (Sender Keys are distributed pairwise), no forged
credentials. That is why invite links can safely carry it, as Signal's do. The
invite-link password is rotatable via `modify_policy`; the master key is not
realistically rotatable. **Caveat:** the "no content" claim assumes clients reject
SKDMs and group messages from non-members, which they currently don't (Known gaps).

**Invitees without an account yet:** an extended invite token (`51`) may carry
`group_invitations: [{master_key, invite_link_password}]`; after registration the
client submits `join_via_link` for each (Planned — see `51`).

### 3.11 Sender opacity for group message sends — Built

Goal: the server validates a legitimate group send without learning which member
sent it. Strong against DB snapshots, logs and "who sent message X" subpoenas; weak
against live network correlation (source IP), which is out of scope (Tor/VPN).

**Layer 1, envelope.** Encrypt once with the sender's Sender Key; wrap with
`sealed_sender_multi_recipient_encrypt`, one slot per recipient device, carrying a
`SenderCertificate` signed by the homeserver's sender-cert chain (trust root pinned
by clients from `GET /v1/groups/server-params`; certs bind `(did, device_id,
identity_key, expiry)`, valid 2 days). Recipients validate the cert against the
pinned root, then Sender-Key decrypt.

**Layer 2, endpoint.** `POST /v1/groups/{id}/send` takes **no Authorization header**;
it authenticates with one `GroupSendFullToken` over the recipient ServiceId set.
The server parses the envelope, resolves each recipient EMI to its device pseudonyms,
verifies the token against the ServiceIds, enqueues one `group_message_queue` row per
device, live-pushes or relay-wakes, and logs nothing (§3.9 rule 6). Rate limiting is
per IP (stored — Known gaps).

**What the server learns.** The *sender* is hidden. The *recipients* are not: each
ServiceId is `UUID(did)`, an unsalted hash the server can compute for every DID it
has registered, so the server learns the group's full membership, paired with EMIs,
from any send. Signal makes the same trade (sender hidden, recipients known). The
protection that remains is at rest: queue and routing rows are keyed by pseudonym and
EMI, never by account, and nothing stored maps them to DIDs.

**Layer 3, network:** out of scope.

**Daily credential refresh** is split: `POST /v1/groups/credentials` (session-auth,
identified: auth credential + per-device sender cert) and
`GET /v1/groups/{id}/endorsements` (presentation-auth, per group). Anonymity applies
at send time, not refresh time — the same trade Signal makes. Endorsements are
fetched per send (no caching yet).

**State changes stay identified by EMI** (§3.3): they're infrequent and role
enforcement needs the actor.

**Abuse reporting** for sealed-sender group sends needs selective sender disclosure
(the recipient reveals one message's sender certificate). Not designed; see `12` §10.

### 3.12 Federation interaction

A group lives on one hosting server. Under the current model a user participates by
having an account on that server (multi-account, `53`); all group traffic is then
local to it, and the design above applies unchanged. Under the **Proposed**
client-side federation model (`13`) this stays true: groups never federate. Guest
access without an account is Speculative and
undesigned.

## 4. Open questions

1. **Account deletion under opacity.** The server can't walk a deleting user's
   groups. `53` specifies a client-driven leave cascade before deletion; anything
   the client misses lingers as opaque rows.
2. **What clients do on detected tampering** (§3.4): warn, freeze sends, fork the
   group, or migrate. Undesigned.
3. **Group abuse reporting** (§3.11, `12` §10).

## 5. Implementation status

What's built, by area. Code comments cite "03 §5" for disappearing messages.

- **Crypto and server (Built).** zkgroup params, credentials, endorsements,
  sender-cert chain; endpoints `server-params`, `POST /v1/groups`, `credentials`,
  `GET {id}`, `changes` (GET/POST), `push_binding`, `endorsements`, `send`,
  `messages` (GET/DELETE); WebSocket `SubscribeGroupPseudonyms` /
  `GroupDeliverRequest` / `GroupDeliverAck`; `group_message_expiry` sweeper. Group
  endpoints use URL-safe unpadded base64 everywhere.
- **app-core (Built).** `create_group`, `invite_member`, `accept_invite`,
  `decline_invite`, `join_via_link`, `cancel_join_request`, `approve_join_request`,
  `deny_join_request`, `remove_member`, `leave_group`, `change_member_role`,
  `set_group_title`, `set_group_expiry`, `set_group_avatar`/`clear_group_avatar`,
  `fetch_group_state`, `apply_pending_group_changes`, `send_group_message`,
  `rotate_group_pseudonym` (`app-core/src/groups.rs`). WebSocket group receive runs
  through the same `process_decrypted` pipeline as DMs. Undecryptable group
  ciphertext is buffered (7 days) until the sender's SKDM arrives.
- **Disappearing messages (Built).** The sender stamps `expire_timer_secs` on each
  message (group: from cached group state, `groups::group_expiry_seconds`; DM: the
  conversation timer). The countdown starts when the message is marked read
  (`store/src/messages.rs`, mark-read sets `expire_at`). A background reaper in
  app-core (`expire_reaper_loop`) hard-deletes due rows and emits
  `IncomingEvent::MessagesExpired`, so expiry is enforced in the substrate, not per
  UI.
- **Clients (Built on iOS; see `62` for parity).** Group creation from compose,
  group detail with members, roles, remove, leave, title, avatar, expiry,
  approve/deny join requests, system-event rows.
- **Not built:** `modify_policy`/`modify_description` from clients, invite-link
  creation UI, scheduled pseudonym rotation, history pruning, endorsement caching,
  negative-path e2e tests (expired cert, bad token), the 20-client integration test.

## 6. Cross-server casual groups — Planned

Small (under ~50), peer-managed groups across servers: Sender Keys with client
fan-out, no server-side state or moderation. **Rule: if a group needs an admin, it
needs a homeserver.** Under client-side federation (`13`), a casual group is client
fan-out of Sender-Key messages delivered to each member's own server. Needs a design
for membership churn (who re-keys when, how stragglers recover) before it is built.
`crypto::groups` should expose the same `encrypt`/`decrypt`/`add`/`remove` shape for
both group types so MLS can later replace either behind it.

## 7. Action-bound groups over mesh — Speculative

If mesh (`14`) is ever built: steady-state messaging works unchanged (Sender-Key
ciphertext doesn't need the server; SKDMs are signed). State changes, credential
refresh, endorsements and sealed sender need the server and are queued until
reconnect. Group tags must be derived **per sender** (from the sender's Sender Key),
never from the group master key, or a leaked invite link would let an observer tag a
group's mesh traffic.

## 8. Threat checklist (PR review gate)

Walk this for any PR touching `crypto/src/groups*`, `server/src/{db,routes}/group*`,
group FFI methods, group migrations, or group proto types. Verify the code realizes
each defense; don't take it on faith.

**Authorization at submission**
- [ ] Presentation verified before anything else; failures give one generic error.
- [ ] Actor eligibility checked per action class (§3.3 step 2).
- [ ] Self-class actions are the sole action in their change.
- [ ] Revision freshness enforced (409 on conflict).
- [ ] Role check uses actor role + policy; `modify_policy` and `modify_member_role`
      are always Admin.
- [ ] `join_via_link` compares the password in constant time and rejects `Closed`.

**Authorization at fetch and delivery**
- [ ] Group fetches and change fetches return 404 to non-members.
- [ ] `POST /v1/groups/{id}/send` never accepts or reads a session credential.
- [ ] Pseudonym subscription cannot let one client drain another's queue (currently
      fails — Known gaps). The fix must not add a `(account → groups)` link (rule 1).

**§3.9 discipline**
- [ ] No DID/account column on group tables; no `(did → groups)` or `(EMI → did)`
      map, persisted or cached.
- [ ] No credential ids or EMIs in logs.
- [ ] Group-table timestamps day-aligned or omitted.
- [ ] Every new column on a group table carries `-- public | -- opaque | -- ephemeral | -- exempt`,
      and the annotation is true (a server-readable JSON column is not `opaque`).
- [ ] No new identified (non-sealed) traffic whose timing or recipients reveal
      group membership.

**Sub-encryption boundaries**
- [ ] Title, description, expiry, avatar and profile keys are sub-encrypted under the
      group key and never persisted server-side in readable form.

**Atomicity**
- [ ] State bump, history append and table changes happen in one transaction.

**Information-flow leaks**
- [ ] Error responses and timing don't reveal group existence or policy to
      non-members.

**Rate limiting**
- [ ] Every group write endpoint is rate-limited; no limiter persists IPs longer
      than needed (target: in-memory).

## 9. Invariant tests — Planned

**These tests don't exist yet** (`core/crates/server/tests/` has only `db_tests.rs`,
`group_tests.rs`, `http_tests.rs`), except the no-logging check for the send handler
(§3.9 rule 6), which is a unit test in `server/src/routes/groups.rs`. The earlier plan listed eight AST-
and regex-based audits; three of them catch most drift for little cost:

1. **Migration schema audit.** Every column on a group table has an annotation, no
   group table has a DID/account column or FK to `accounts`, and server-readable
   columns aren't annotated `opaque`.
2. **Forbidden joins.** No SQL in `server/src/db/group*.rs` joins group tables with
   `accounts` or the DM `push_pseudonyms` table.
3. **Unauthenticated send endpoint.** `send_group_message` takes no auth extractor
   (compile-time), and a request carrying a session bearer behaves identically to one
   without.

Dropped as not worth their cost: the `syn`-based logging audit, the transactional-
writes AST walk, the "discipline coverage" meta-test. Logging and transactions stay
on the §8 review checklist.

## Planned

- **Sealed sender for 1:1 and SKDM traffic** (Signal parity), with delivery keys
  derived from the profile key (`13`, `52`). The biggest single fix for §3.9:
  removes the identified invite/SKDM signature and `sender_account_id`.
- **Pseudonym with a secret** — now written up under Proposed.
- **Client-side membership rules:** accept SKDMs and group messages only from DIDs in
  the cached member list (re-fetch state on an unknown sender); re-seed your own
  Sender Key when a member is removed; drop non-admin posts in announcement-only
  groups.
- **Server fixes:** prune history to 256
  revisions; day-align or drop the exact timestamps; move IP rate limits in-memory;
  make history actions opaque to the server where it doesn't need them (pseudonyms,
  link password).
- `modify_policy` / `modify_description` FFI and UI (invite links, join policy,
  announcement-only).
- Scheduled pseudonym rotation (§3.7).
- The three invariant tests (§9).

## Proposed

Pending project-owner review; not to be implemented until approved.

### Secret-backed group pseudonyms (fixes S-14)

**Problem.** Anyone may subscribe to any pseudonym, and members can read each other's
pseudonyms from the change history, so a member can silently take over another
member's group delivery and, if the victim is offline, drain and delete their queued
messages (Known gaps, `09` S-14).

**Design.**
- When a device creates a group pseudonym, it also generates a random 32-byte
  `pseudonym_secret`. It registers `(pseudonym, H(pseudonym_secret))` with the server
  (in the `promote_pending_members` / `push_binding` action, in place of the bare
  pseudonym). The server stores only the hash on the `group_member_pseudonyms` row.
- Every operation that acts on a pseudonym presents the secret: `SubscribeGroupPseudonyms`
  carries `(pseudonym, secret)` pairs, offline pickup and ack likewise, and relay
  registration too (so the relay can't be pointed elsewhere, `09` S-13). The server
  hashes and compares, and ignores entries that don't match.
- Change history and member-visible actions no longer carry pseudonyms at all (a
  pseudonym is server routing data, not something members need), which also fixes
  part of S-10.
- The secret is device-local: each device of an account has its own pseudonym and
  secret, which also enables per-device group delivery (`04`).

**What it keeps.** The server still never links a pseudonym to an account at rest:
the secret is random, and its hash says nothing about who holds it.

**Contract changes.** New fields on the group-change and WebSocket subscribe frames;
a schema column; relay registration changes. Old clients send bare pseudonyms: for a
transition, the server accepts a bare subscribe only for pseudonyms registered
without a secret, and refuses it for pseudonyms that have one, so upgraded members are
protected as soon as they re-register (every member re-registers on its next
reconcile or rotation).

**Rejected alternatives.**
- *Route group delivery by account, like Signal.* Removes pseudonyms entirely, but
  undelivered group messages would then be stored per account, so a seized database
  would show which accounts have pending messages in which group.
- *Refuse a subscribe held by another account (interim).* Blocks only the live hijack;
  an attacker subscribing while the victim is offline still drains their queue.

## Speculative

- **Anonymous group send.** The sender proves group membership with an anonymous
  credential presentation instead of listing recipients, and the server fans out to
  the group's stored pseudonyms itself, so a live operator never learns recipients.
  Receive-side IP and timing correlation would still let a determined operator link
  members, so it only pays off alongside an anonymizing transport. Revisit only if
  serving users who can't trust their own server operator becomes a goal.
- Guest access for users without an account on the hosting server.
- MLS in place of Sender Keys, behind the scheme-agnostic interface.
- Very large channels — see `08`.

## Rationale and rejected alternatives

- **Two group types.** Rich single-server groups with an admin and a credential
  issuer, versus small peer-managed cross-server groups. Most designs refuse this
  split and pay for it in either UX or guarantees.
- **zkgroup via `UUID(did)`** over a custom DID credential or bearer tokens (§2.4).
- **Declarative actions + 409 retry** over CRDT/OT (§3.5).
- **404, not 403, for non-members** so credentials can't probe for existence (§3.4).
- **Invite links carry the master key** (Signal does this; the key alone grants
  nothing without membership) rather than admin-only DM invites for open groups
  (§3.10).
- **Sealed sender is skipped for the invite `GroupContext` DM** on the grounds that
  the admin is already identified by submitting `invite_members`. Superseded in
  effect: the identified DM still tells the server *who was invited*, which the EMI
  in the action does not. Planned to move to sealed sender with the rest of the DM
  plane.
- **Claim-squatting defense: "deliver to all claimers, rely on decryption failure"**
  was chosen over "reject subscribe for pseudonyms the account doesn't own" because
  the latter needs an account → group link (rule 1). Neither was built; the
  secret-backed pseudonym proposal above replaces both.
