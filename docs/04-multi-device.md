# Multi-device: detailed design

Status: **in-progress design doc.** The cryptographic substrate (per-device
prekeys, sessions, sender keys, message routing) is already built and exercised;
the application-level pieces (linking, sync) are not. Sections marked **OPEN**
are unresolved; sections marked **DECIDED** are committed.

Background reading:

- `docs/01-technical-implementation.md` §"Multi-device" — the short summary this doc supersedes.
- `docs/03-groups.md` — sender-key fan-out, which is per-device.
- `docs/50-identity-auth-recovery.md` — identity key, rotation key, recovery blob (linking reuses this machinery).
- Signal "Sesame" spec (Marlinspike, Perrin) — the account/device/session model this follows.

## 1. The central distinction: static credential vs. stateful machinery

The question that drives the whole design is *what is safe to copy across a
user's devices, and what is not.* The answer splits cleanly:

**The identity key is a static credential and is SHARED across all of a user's
devices.** Its job is to answer "is this really Alice?" — it signs prekey
bundles and authenticates the X3DH handshake. It never changes as messages flow.
Copying a static credential to a second device creates no consistency problem:
both devices prove "I am Alice" the same way, like two copies of one passport.
This is also what real Signal does — its identity key is **identity-scoped** (per
DID in our terms; Signal calls it account-level) and provisioned onto each linked
device, **not** minted per device.

**Sessions, prekeys, and sender keys are stateful machinery and are PER-DEVICE.**
A Double Ratchet session mutates on every message: it advances a ratchet,
derives a fresh message key, and discards the old one (forward secrecy). The two
endpoints must stay in lockstep by counter. If two of Alice's devices shared one
session with Bob, they would:

- advance the same ratchet independently → diverging state;
- derive the **same message key for different plaintexts** → key/nonce reuse,
which breaks the AEAD outright;
- collide on counters so Bob cannot reconcile them;
- be unable to delete a key another device still needs → forward secrecy lost.

So sessions *cannot* be shared — not by policy, but because the construction
breaks. That forces sessions to be per-device, which in turn forces **per-device
prekeys** (each device must be independently reachable to start a session, and
one-time prekeys are consumed) and **per-device registration IDs** (to detect
when a given device's session has gone stale). Group **sender keys** have the
same stateful-ratchet property and are therefore per-device too.

> **One-line version.** You can copy a static credential (the identity key); you
> cannot copy a running ratchet (a session/sender-key). Identity is shared
> because it's static; sessions and sender keys are per-device because they're
> stateful and forward-secret.

**DECIDED:** identity key is identity-scoped (per DID) and shared; sessions,
prekeys, sender keys are per-device. The implementation already matches this
(`store::account` persists one `identity_keypair` row; the server keys prekeys,
registration IDs, and message queues by `(account_id, device_id)`).

## 2. Membership is per-identity; only delivery is per-device

A frequent confusion: if sessions are per-device, is a "member" of a group a
device, or the identity? **Membership is per-identity (per-DID).** Two layers:

1. **Membership / roster — per-DID.** The group state lists DIDs, roles,
  invites. Alice is *one* member whether she has one device or three. Adding a
   device does **not** add a member.
2. **Delivery / encryption — per-device.** At send time each member DID is
  expanded into its devices, and the sealed-sender envelope carries one
   destination (`ProtocolAddress`) per device (`groups.rs`,
   `send_group_message` → `ensure_group_recipient_sessions`). Each device also
   holds its own sender key for the group.

So nothing extra is registered "into membership" per device. Membership stays at
the identity level; the per-device multiplication lives entirely in the
encryption fan-out beneath delivery.

**DECIDED:** group roster is keyed by DID; per-device expansion happens only in
the send/fan-out path.

## 3. Current state of the substrate

What already works per-device, today, and is exercised by tests:

- **Registration** — server `devices` table, `UNIQUE(account_id, device_id)`,
one row per device carrying `identity_key`, `registration_id`, prekeys.
- **Prekey fetch** — `fetch_prekey_bundle(did, device_id)`, per device.
- **1:1 send** — `send_dm` fetches all of the recipient's devices and encrypts
once per device.
- **Group send** — fans out to every device of every member; SKDMs distributed
per device.
- **Receive** — messages are queued per `(account_id, device_id)`; each device
drains its own queue.
- **Stale-session reconciliation** — registration-id comparison forces a session
refresh when a peer device re-registers (the lazy-establishment work in
`ensure_group_recipient_sessions`).

What is **not** built (the rest of this doc):

- Device linking / provisioning (a 2nd device cannot currently join an identity).
- Sent-message and read-state sync across a user's own devices.
- History backfill onto a freshly linked device.
- Device revocation flow.
- Device-set-change detection / UX.

`device_id` is threaded through registration and recovery but hardcoded to `1`
in practice (`RegistrationInfo::device_id`).

## 4. Device linking (provisioning a 2nd device)

**Status: DECIDED (model); provisioning-channel wire details TBD.**

The problem reduces to: *transport the identity's shared identity private key (and
rotation key) onto the new device without it ever touching the server in
plaintext, then let the new device build its own fresh per-device state.*

The chosen flow, following Signal's provisioning model:

1. New device generates an ephemeral keypair, displays a QR encoding its
  ephemeral public key + a provisioning address.
2. An existing (already-trusted) device scans it, encrypts the shared identity
  key + rotation key to the ephemeral public key, and sends it over the
   server-relayed-but-E2E-encrypted provisioning channel (reuse the relay; the
   server sees only ciphertext).
3. New device registers its **own** `device_id`, prekeys, and registration ID on
  the server; generates its own sessions and sender keys going forward.

Notes / sharp edges:

- **Linking reuses recovery-blob crypto.** "Restore from recovery" and "link a
new device" produce nearly the same end state — a device holding the shared
identity key. The difference is the source (a live device vs. a server-stored
blob) and the *aliveness assumption* (see §7).
- **All devices are co-equal; there is no "primary."** Any already-trusted device
can authorize a link. This is Signal's model and is accepted here, but it means
possession of *any* one device is sufficient to add another — write it down as
a conscious choice.
- **Linking is additive: new `device_id`, fresh per-device state.** The new device
takes a `device_id` not currently in use and generates its own registration ID,
prekeys, and sessions. The shared identity key is the only thing transported in.

## 5. Cross-device sync: what roams, and how

**Status: DECIDED (model). The event channel (§5.4) is the near-term build; the
Durable channel — the storage service in `docs/05-device-data-sync.md` — is
staged later.**

Without this, multi-device is cryptographically correct but *feels* broken:
Alice sends from her phone and her tablet never sees it; she reads on the phone
and the tablet badge doesn't clear. Messages go to the *recipient's* devices, not
to the sender's own other devices.

The trap to avoid is **"one new `SyncMessage` variant per UX feature."** Signal
accreted ~20 such variants (read, viewed, blocked, configuration, contacts,
groups, fetchLatest, …) before moving most durable state into a generic Storage
Service. We commit up front to a model that *caps* the sync-message-type count
rather than growing it per feature.

### 5.1 Three channels, sorted by the nature of the data

Almost everything that must stay consistent across your devices sorts into one of
three buckets by its nature, and each has **one** generic mechanism — so new
features slot into an existing channel instead of minting a new message type.

- **Conversation — things recipients also see.** Text, media,
reactions, edits, deletes, disappearing-timer changes. These are all
`ContentMessage`s already sent to recipients, so they sync to your own devices
*for free* by riding inside a **Sent transcript** (the transcript wraps
"whatever content message I just emitted"; the other device applies it exactly
as a recipient would). New content types later (polls, stickers, voice) sync
with **zero new sync plumbing**.
- **Durable — current values only your devices need.**
Mute / archive / pin / marked-unread per conversation, contact list +
nicknames, blocked list, settings (receipts on/off, typing indicators, default
timer), profile. These have a *current value*, not an event history — a
newly-linked device needs the **snapshot**, not the deltas. They live in a
generic **versioned encrypted record store** synced via a dedicated **storage
service** (designed in full in `docs/05-device-data-sync.md`; per-record
last-writer-wins). Adding a feature here = **a domain table + a small adapter**,
not a new sync message and not new transport (see §5.6).
- **Device-local — never synced.** Theme, notification sound, biometric-lock
toggle, which-device-am-I. Named explicitly so we don't reflexively sync what
shouldn't roam.

The only residue is a small, **near-closed** set of **local events** — read
marks, viewed / view-once-opened, delete-for-*me* — which are event-shaped
(ordered, high-frequency, "up to timestamp") and must NOT reach recipients. They
ride the event stream as thin types. This is the *only* category that ever adds a
sync message type, and the set barely grows.

**DECIDED:** durable preferences sync via the generic record store (Durable), **not**
by accreting `SyncMessage` variants. This caps the sync-message-type count at
roughly `{Sent, Read, Viewed, LocalDelete}`.

### 5.2 The decision rule for any future feature

1. **Do recipients need to know?** → it's a `ContentMessage`; syncs free via the
  Sent transcript. *(adds a content type, not a sync type)*
2. **Only your own devices need it — current value or action?**
  - **current value** (preference / relationship / setting) → a record in the
   storage service (§5.6). *(adds a domain table + adapter, not a sync type)*
  - **action at a time** → a thin event type. *(the only path that adds a sync
  message type, and the set is near-closed)*

### 5.3 Catalog


| What                                       | Category     | Mechanism                                                      | New type per feature? |
| ------------------------------------------ | ------------ | -------------------------------------------------------------- | --------------------- |
| Text / media                               | Conversation | Sent transcript                                                | no                    |
| Reactions, edits, delete-for-everyone      | Conversation | Sent transcript (content msg)                                  | no                    |
| Disappearing-timer change                  | Conversation | Sent transcript (content msg)                                  | no                    |
| Future content (polls, stickers, voice…)   | Conversation | Sent transcript                                                | no                    |
| Read marks                                 | event        | `SyncRead`                                                     | one-time              |
| Viewed / view-once opened                  | event        | `SyncViewed`                                                   | one-time              |
| Delete-for-me (local)                      | event        | `SyncLocalDelete`                                              | one-time              |
| Mute / archive / pin / marked-unread       | Durable      | record store                                                   | no — a field          |
| Contact list, nicknames, blocked list      | Durable      | record store                                                   | no — a field          |
| Group master keys (membership re-entry)    | Durable      | record store (today: recovery blob)                            | no — a record         |
| Settings (receipts, typing, default timer) | Durable      | record store                                                   | no — a field          |
| Profile (name / avatar / bio)              | Durable      | already an encrypted blob on the discovery server — reuse that | no                    |
| Theme, notif sound, biometric lock         | Device-local | —                                                              | never synced          |
| Message history backfill                   | —            | §10, deferred                                                  | —                     |


### 5.4 Wire shapes (event channel)

The Sent transcript subsumes the entire Conversation channel because it wraps the content
message verbatim; only the local events need their own thin types.

```protobuf
message SyncSent {
  int64 timestamp = 1;          // send-time; all your devices agree on ordering/identity
  oneof target {                // for attribution on the receiving device
    string recipient_did = 2;   //   a DM
    bytes  group_id      = 3;   //   a group
  }
  ContentMessage content = 4;   // the SAME payload sent to recipients:
                                //   text / reaction / edit / delete / timer-change
  // optional: per-recipient delivery state so other devices can show ticks
}

message SyncRead {              // clears unread state on your other devices
  repeated ReadMark marks = 1;
}
message ReadMark {
  oneof conversation { string peer_did = 1; bytes group_id = 2; }
  int64 up_to_timestamp = 3;    // everything at/below this is read
}
```

### 5.5 Transport

A sync message is just a normal pairwise DM **to yourself** — encrypted under your
device→device Double Ratchet session and fanned out to
`self.devices \ {sending_device}`. No sealed sender (sender and recipient are both
you, nothing to hide) and no new envelope crypto; it reuses the same per-device
session machinery as any other DM. The fan-out path already iterates devices per
DID, so the new work is the proto variants plus including your own other devices
as recipients.

The Durable channel uses a **separate** transport entirely — the storage service
(`docs/05-device-data-sync.md`), not the message queue. §5.5 here covers only the
event channel.

### 5.6 Durable state → the storage service (separate doc)

The Durable channel — contacts, group keys, settings, per-conversation flags,
profile — does **not** ride the message queue. It is a versioned encrypted
**record store** synced via a dedicated **storage service**, designed in full in
`docs/05-device-data-sync.md`. It outgrew this doc; here is only the shape:

- **One local source of truth.** Durable state stays in its normal SQLCipher
  domain tables (group master keys in `groups`, contacts in the contacts table,
  …); the sync layer **never duplicates** the payload. Adding a new synced type
  is a domain table + a small adapter + a one-line registration.
- **Single authoritative server + passive backups.** One authoritative copy on
  your discovery homeserver; your other homeservers hold one-way encrypted
  snapshots. Deliberately **not** multi-master — per-record last-writer-wins with
  a server-assigned version and a sync cursor.
- **Storage key chaining.** The store is encrypted under an identity-level
  **storage key** (per DID), shared across the identity's devices (provisioned at link time, §4) and
  carried in the recovery blob. The recovery blob shrinks to that keyring; group
  master keys move *out* of the blob and *into* the store, so the snapshot path
  becomes load-bearing for total-loss recovery. Linking and recovery both reduce
  to "get the storage key, then pull the store" (see `05` §11).
- **Fast sync** reuses the WebSocket: a "storage changed" nudge triggers a delta
  pull; coalescing is client-side, the server does no debounce.

See `docs/05-device-data-sync.md` for the storage API, the adapter model that
makes new types easy, replication, conflict handling, limits, and the
bootstrap/recovery flows.

## 6. Group fan-out & new-device-in-existing-group

**Status: mostly DECIDED (substrate), one OPEN test gap.**

Per-device fan-out and per-device sender keys are already built (§3). The cost is
performance: N members × M devices = N·M sealed-sender slices and a separate
ratchet per device for SKDM delivery. This is inherent to the per-device model
and the architectural price is already paid.

The OPEN gap: when an *existing* member links a *new* device mid-group, that
device needs a session **and** a fresh SKDM. The lazy-establishment /
registration-id path should fire for "existing member, new device" the same as
for a late joiner. Add an explicit test: member links device → next group send
establishes the new device's session and delivers it a fresh sender key →
new device decrypts.

## 7. Recovery interaction

**Status: DECIDED in shape, details in `docs/50-...`.**

The recovery blob + rotation key already assume the identity key is
identity-scoped (per DID) and restorable — consistent with shared-identity multi-device. A
restored device and a linked device converge on the same state (a device holding
the shared identity key + its own fresh per-device state).

**What actually changes on recovery is the registration ID, not the device ID.**
The recovery blob holds the identity key (and the bootstrap essentials — see
`docs/05-device-data-sync.md` §11 for the storage-key chaining; note it does
*not* carry the rotation key, which is re-derived from the passkey, nor
contacts) — but **not** per-session ratchet
state (that lived only on the lost device and is gone). So a restored device has
the *same* identity key (no safety-number break, no re-verification) but a
*fresh* registration ID, fresh prekeys, and zero sessions.
When a peer next sends, its old session is dead; the registration-id
reconciliation in §3 detects the changed reg-id, discards the stale session, and
re-establishes from a fresh prekey bundle. The `device_id` carries no key
material — it is only a routing label — so it plays no part in this reset.

**The load-bearing difference between linking and recovery is the aliveness
assumption, not `device_id` allocation:**

- **Linking is additive** and assumes the existing device is alive — new
`device_id`, both coexist, fan-out reaches both (§4).
- **Recovery is total** and assumes *no* device survives — it should revoke the
identity's **entire prior device set** (every device row for the DID, each with
its own registration ID, tokens, prekeys, and queue) and register a single
fresh device. Recovery is the "I have no working device" path; if a device were
alive you would *link* from it instead. Revoking everything is both hygiene
(no orphaned rows left as dead fan-out targets) and security (a lost or stolen
device is cut off). It does **not** preserve sessions — reg-id reconciliation
resets those either way.
  **Implementation gap:** the current `POST /v1/devices/replace` (`devices.rs`)
  is only a *single-slot swap* — it names one `old_device_id`, deletes that one
  row (cascading its tokens/prekeys/messages), and creates the new device,
  leaving all other device rows untouched. That is correct for today's
  single-device case but is **not** a whole-identity reset. A true multi-device
  recovery needs to enumerate and revoke the full device set under the same
  rotation-key authorization (which already proves control of the DID).
  **OPEN — multi-homing:** a device registers an account on *each* server the
  identity uses (per `00` terminology), so an identity's device set spans its
  accounts. Total-loss recovery must therefore revoke across **all** of the
  identity's accounts, not just one server's — the rotation key authorizes this
  on every server, but the flow that fans the revocation out across the
  identity's `servers` is unspecified. Tracks with the storage-store replication
  question in `docs/05-device-data-sync.md` §7.

The hazard is doing the wrong one: "recovering" (replacing the slot) while the
old device is *actually still alive* produces two physical devices claiming the
same `device_id` with different registration IDs — split-brain, each clobbering
the other's reg-id server-side. Hence link and recover must be distinct,
explicit user intents gated on "is the old device gone?".

## 8. Trust on device-set change

**Status: OPEN — security UX, does not block shipping.**

Because the identity key is shared, **adding or removing a peer's device does not
change the safety number** — the safety number is a function of the two
identities' identity keys. This is the known, accepted weakness of the
shared-identity model: a silently-linked (e.g., coerced or attacker-controlled)
device is automatically encrypted-to with no safety-number alarm.

The direction-aware trust work (trust-on-change for Sending, strict for
Receiving) handles a *different* event — peer **re-registration**, which is a
genuinely new identity key — not device linking.

Optional improvement over Signal: surface "Bob added a new device" as an info
event by diffing the device list / registration IDs we already fetch. Cheap, and
a real UX win. Does not block linking.

## 9. Device revocation

**Status: OPEN — design sketch only.**

For a lost device:

1. Delete its prekeys + `devices` row server-side → peers stop encrypting to it
  on their next `fetch_device_registrations` (the destination simply disappears).
2. **Immediately kill its server-side session token** — otherwise the revoked
  device can still drain already-queued messages until the token expires.

Note: revoking a device does **not** rotate the shared identity key. A truly
compromised *identity key* is a different, worse event — full re-registration,
which breaks the safety number for every contact.

## 10. History backfill

**Status: DECIDED — explicit non-goal for v1.**

A newly linked device starts effectively blank and syncs forward. This matches
Signal, which does not backfill full transcript to linked devices. Writing this
down as a non-goal removes the (false) sense that device transfer must be solved
before linking can ship. Optional later: direct device-to-device transfer over
the provisioning channel.

## 11. Suggested implementation order

1. **(this doc)** reconcile the identity-key model — zero code, dissolves the
  scariest worries.
2. **Device linking / provisioning channel** — unblocks everything; reuse
  recovery-blob crypto, provision the shared identity key + storage key (§4, §5.6,
  `docs/05-device-data-sync.md`).
3. **Event-channel sync** — Sent transcript (covers all of the Conversation
  channel) + `Read` at minimum. Makes multi-device usable, not just correct (§5.4).
4. **New-device-in-existing-group SKDM redistribution test** (§6).
5. **Storage service** — the Durable channel; build per `docs/05-device-data-sync.md`.
  After it, durable features cost a domain table + adapter, not a sync type (§5.6).
6. Device-set-change UI notice (§8) + revocation token-kill + whole-set recovery (§9, §7) — security polish.
7. History backfill — explicitly deferred (§10).

