# 04 — Multi-device

> **Status:** Partial — per-device crypto, device linking (iOS, Android, Desktop), group fan-out, sent-transcript sync, and the storage service are built. Read-state sync is receive-only, and there is no device list, revocation, or whole-identity recovery reset.
> **Last verified against code:** 2026-10-03

## Summary

An identity (DID) can run on several devices. The identity key is shared; sessions, prekeys, and sender keys are per-device. A second device joins over a short-lived, ciphertext-only mailbox on a homeserver. Cross-device consistency comes from three channels: conversation content rides a **Sent transcript**, durable state rides the **storage service** (`05`), and a tiny set of local events (read marks) ride thin sync messages.

Section numbers below are cited from code (`§4`, `§4.2`, `§5.4`, `§5.5`, ...) and are stable.

## Known gaps

- **Read state does not sync between your own devices.** `SyncRead` is applied on receive (`app-core/src/messaging.rs` `apply_sync_read`) but never sent: `mark_messages_read` (`app-core/src/lib.rs`) only writes the local store. Reading on the phone does not clear the tablet's badge.
- **`SyncViewed` and `SyncLocalDelete` do not exist** in `core/proto/content.proto`. Only `SyncSent` (field 10) and `SyncRead` (field 11) are defined.
- **No device list, no revocation.** The server exposes `GET /v1/accounts/{did}/devices` (`server/src/routes/accounts.rs`) but there is no client UI to see your devices and no endpoint or FFI to revoke one. A lost device keeps working until someone does a recovery that happens to replace its slot.
- **Every device holds the DID's rotation key.** Linking ships the rotation private key in the bundle (`core/proto/provisioning.proto` field 2) and it is persisted (`store/src/account.rs` `save_rotation_key`). Stealing any device's unlocked database gives permanent control of the DID. See `50` §Proposed and `09`.
- **Linking is phishable.** The existing device accepts a pasted or scanned pairing code and sends the full key bundle without showing the new device's name or a matching confirmation code. This is the pattern used in 2025 attacks on Signal's linked devices. See §4.3 (Planned).
- **The server does not check that a linked device's identity key matches the identity's.** `/v1/devices/link` and `/v1/devices/replace` accept any `new_identity_key` (`server/src/routes/devices.rs`). Both run on `acquire()`, not a transaction; `/replace` deletes the old device row before creating the new one, so a failure in between leaves the account with no device. `/replace` has no IP rate limit, and it fetches PLC (`server/src/plc.rs`, `reqwest::get`, no timeout) for any well-formed request signed by any key.
- **Recovery is a single-slot swap, not a whole-identity reset** (§7). It reuses `min(existing device_ids)` and only replaces on the primary server (`app-core/src/lib.rs` `recover_from_blob`).
- **Device-link and recovery registration are not e2e-tested**, because both need a `did:plc:` rotation key resolvable in the live PLC directory. The mailbox, handshake, and bundle round-trip is tested in both directions; server-side `link` validation is HTTP-tested.

## 1. The central distinction: static credential vs. stateful machinery

**Built.** The question that drives the design is *what is safe to copy across a user's devices, and what is not.*

**The identity key is a static credential and is shared across all of an identity's devices.** It answers "is this really Alice?": it signs prekey bundles and authenticates the X3DH handshake, and it never changes as messages flow. Copying it creates no consistency problem, like two copies of one passport. Signal does the same: its identity key is account-scoped and provisioned onto each linked device.

**Sessions, prekeys, and sender keys are stateful machinery and are per-device.** A Double Ratchet session mutates on every message. If two devices shared one session they would advance the same ratchet independently, derive the same message key for different plaintexts (key/nonce reuse, which breaks the AEAD), collide on counters, and be unable to delete a key the other device still needs. So sessions cannot be shared, which forces per-device prekeys and per-device registration IDs. Group sender keys have the same property.

> **One-line version.** You can copy a static credential (the identity key); you cannot copy a running ratchet (a session or sender key).

Implementation: `store` persists one `identity_keypair` per identity (identity.db, `06`); the server keys prekeys, registration IDs, and message queues by `(account_id, device_id)`.

## 2. Membership is per-identity; only delivery is per-device

**Built.** Membership is per-DID: the group roster lists DIDs, and adding a device does not add a member. Delivery and encryption are per-device: at send time each member DID expands into its devices, and the sealed-sender envelope carries one destination per device (`app-core/src/groups.rs`, `send_group_message` → `ensure_group_recipient_sessions`). Each device holds its own sender key per group.

## 3. Current state of the substrate

**Built**, and exercised by tests:

- **Registration** — server `devices` table, `UNIQUE(account_id, device_id)`, one row per device carrying `identity_key`, `registration_id`, prekeys.
- **Prekey fetch** — per device.
- **1:1 send** — `send_dm` fetches all of the recipient's devices and encrypts once per device.
- **Group send** — fans out to every device of every member; SKDMs distributed per device (§6).
- **Receive** — each device drains its own `(account_id, device_id)` queue.
- **Stale-session reconciliation** — registration-id comparison forces a session refresh when a peer device re-registers.
- **Device linking** — §4.

`device_id` is `1` at account creation. Linking allocates `max(existing) + 1`. Recovery reuses the smallest existing `device_id` (§7).

## 4. Device linking (provisioning a 2nd device)

**Built** on iOS (`LinkDeviceView`, `LinkNewDeviceView`), Android, and Desktop (`desktop/src/state/createDeviceLink.ts`, `views/settings/LinkDeviceView.tsx`). Core: `app-core/src/provisioning.rs`, `crypto/src/ephemeral.rs`, `server/src/routes/provisioning.rs` + `devices.rs`, migration `019`.

The problem: move the identity private key (plus rotation key and storage key) onto the new device without it touching the server in plaintext, then let the new device build its own per-device state.

### 4.1 The provisioning channel

The transport is a short-lived, ciphertext-only **mailbox on a homeserver** (not the push relay). Three unauthenticated, per-IP-rate-limited endpoints, ~5-minute sessions: `POST /v1/provisioning/sessions`, and `PUT`/`GET /v1/provisioning/{id}/{slot}` over two slots (`handshake`, `bundle`).

The handshake is **role- and rendering-flexible**, a deliberate divergence from Signal's "desktop is always secondary":

1. Either device generates an ephemeral Curve25519 key pair and renders a pairing string (`av1.<b64url>…`) as a QR code and/or copy-paste code, carrying `{mailbox_url, session_id, ephemeral_pub}`. A server-less new device defaults the mailbox to `DEFAULT_MAILBOX_SERVER` (`av.theavalanche.net`).
2. The other device ingests the string, generates its own ephemeral key pair, and `PUT`s the public half to `handshake`.
3. Both derive `K = HKDF(X25519(...))`.
4. The **existing** device seals the bundle under `K` (AES-256-GCM, recovery-blob envelope shape) and `PUT`s it to `bundle`. The new device polls, opens it, and registers (§4.2).

Because `K` depends on the out-of-band `ephemeral_pub`, a hostile mailbox can only cause a clean abort, never read the bundle.

**Polling, not a WebSocket:** the joining device has no account or WS session, and the flow is rare and short. The FFI exposes single-step poll methods (`link_send_bundle_step`, `await_link_step`) that the UI loops with its own cancellable delay, so polling stops the instant the user leaves the screen. Blocking wrappers (`link_send_bundle`, `await_link`) remain for bots and e2e tests.

### 4.2 Additive registration

The new device registers via `POST /v1/devices/link`, the additive sibling of `/v1/devices/replace`. It is rotation-key authorized (signature over `linkdevice:{did}:{new_device_id}:{nonce}`, verified against the DID's PLC `rotationKeys`) and **inserts** a device row, leaving existing devices intact. It then pulls durable state from the storage service (`05` §11).

The joining device has no account yet, so the existing device resolves the two prerequisites while sealing the bundle: the free `new_device_id` (from its authenticated device-list fetch) and the anti-replay `link_nonce` (a `POST /v1/auth/challenge` for one of its own devices, valid 5 minutes). `/v1/devices/link` itself is unauthenticated; the rotation signature is the authorization.

Notes:

- **The bundle carries the rotation key.** Recovery omits it (re-deriving from the passkey); a linked device has no passkey, so today it must be transported. This is the gap called out above; `50` §Proposed replaces it.
- **All devices are co-equal; there is no primary.** Possession of any one device is enough to add another.
- **A linked device does not cache the recovery `blob_key`**, so it cannot refresh the server recovery blob. Group keys roam via storage sync instead.

### 4.3 Link confirmation and new-device notice

**Planned.** Closes the phishing gap above. Before sending the bundle, the existing device shows the new device's self-reported name and a short code derived from `K` that must match the code shown on the new device; pasted codes are accepted only on the *new* device's side. After any link, every existing device of the identity shows a "New device linked: <name>" system notice (derived from the device list it already fetches). Matches Signal's post-2025 hardening.

## 5. Cross-device sync: what roams, and how

**Partial.** The model is decided. The Sent transcript is built and sent; `SyncRead` is receive-only (see Known gaps); the Durable channel is the storage service (`05`).

Without sync, multi-device is cryptographically correct but feels broken: Alice sends from her phone and her tablet never sees it. The trap to avoid is "one new `SyncMessage` variant per UX feature" (Signal accreted ~20 before moving durable state into a Storage Service). We cap the sync-message-type count up front.

### 5.1 Three channels, sorted by the nature of the data

- **Conversation — things recipients also see.** Text, media, reactions, edits, deletes, timer changes. These are already `ContentMessage`s sent to recipients, so they sync to your other devices **for free** inside a **Sent transcript** that wraps the content verbatim. New content types sync with zero new plumbing.
- **Durable — current values only your devices need.** Mute/archive/pin, contacts and nicknames, blocked list, group master keys, settings. These need a snapshot, not deltas, and live in the **storage service** (`05`, per-record versioned records). Adding one = a domain table + a small adapter.
- **Device-local — never synced.** Theme, notification sound, biometric lock.

The residue is a near-closed set of **local events** (read marks, viewed, delete-for-me) that must not reach recipients and ride thin sync types. This is the only category that ever adds a sync type; the target set is `{Sent, Read, Viewed, LocalDelete}`.

### 5.2 The decision rule for any future feature

1. **Do recipients need to know?** → a `ContentMessage`; syncs free via the Sent transcript.
2. **Only your devices need it — current value or action?**
   - **current value** → a storage-service record (§5.6).
   - **action at a time** → a thin event type (the only path that adds a sync type).

### 5.3 Catalog

| What | Category | Mechanism | Status |
|---|---|---|---|
| Text / media / reactions / edits / delete-for-everyone / timer | Conversation | Sent transcript | Built |
| Read marks | event | `SyncRead` | Partial (receive-only) |
| Viewed / view-once opened | event | `SyncViewed` | Not defined |
| Delete-for-me | event | `SyncLocalDelete` | Not defined |
| Disappearing-message timer per conversation | Durable | storage record (`ConvSettingsAdapter`) | Built |
| Mute / archive / pin per conversation | Durable | storage record (extend `ConvSettingsAdapter`) | Planned (features not built; `37`) |
| Contacts, curation, block flag, nicknames | Durable | storage records (`ContactAdapter`, `ContactProfileAdapter`) | Built |
| Group master keys | Durable | storage record (`GroupKeyAdapter`) + recovery blob | Built |
| Trust store (`known_identities`) | Durable | none | Not synced (`06` §12) |
| Profile (name / avatar) | Durable | encrypted blob on the discovery server | Built |
| Theme, notif sound, biometric lock | Device-local | — | never synced |
| Message history backfill | — | §10 | Non-goal |

### 5.4 Wire shapes (event channel)

**Built** (`core/proto/content.proto`, `SyncSent` / `SyncRead`). The Sent transcript wraps the content message verbatim; only local events need their own types.

```protobuf
message SyncSent {
  int64 timestamp = 1;          // send-time; all your devices agree on ordering/identity
  oneof target {
    string recipient_did = 2;   // a DM
    bytes  group_id      = 3;   // a group
  }
  ContentMessage content = 4;   // the same payload sent to recipients
}

message SyncRead {
  repeated ReadMark marks = 1;
}
message ReadMark {
  oneof conversation { string peer_did = 1; bytes group_id = 2; }
  int64 up_to_timestamp = 3;
}
```

On a live-WS `SyncSent`/`SyncRead`, the receiving device applies it to its store and emits a **scoped** `ConversationUpdated { conversation_id }` event so the UI re-reads just that conversation (deliberately not the coarse `StorageSynced` signal). The explicit poll path applies transcripts silently.

Senders: `sync_sent_to_own_devices` (`app-core/src/messaging.rs`) is called from the DM and group send paths. No sender exists for `SyncRead`.

### 5.5 Transport

**Built.** A sync message is a normal pairwise DM **to yourself**, encrypted under your device-to-device Double Ratchet sessions and fanned out to your other devices. No sealed sender. The Durable channel uses the storage service, not the message queue.

### 5.6 Durable state → the storage service

**Built** (see `05`). Shape:

- **One local source of truth.** Durable state stays in its SQLCipher domain tables; the sync layer never duplicates the payload.
- **Single authoritative server.** One copy on your discovery homeserver; per-record versioned records with a sync cursor. (The passive-backup half of `05` §7 is parked.)
- **Storage key chaining.** Records are encrypted under an identity-level **storage key**, provisioned at link time (§4) and carried in the recovery blob. Linking and recovery both reduce to "get the storage key, then pull the store" (`05` §11).
- **Fast sync** reuses the WebSocket: a `StorageChanged` nudge triggers a delta pull.

## 6. Group fan-out & new-device-in-existing-group

**Built.** Every device of a member independently receives and decrypts group messages.

The group envelope is SSv2 multi-recipient, and its per-recipient key material is derived from the recipient **account's identity key** (libsignal `sealed_sender.rs`), not per device. Every linked device shares that key, so the same delivered blob is decryptable by any device of the member.

What makes it work:

1. **Per-device delivery pseudonyms.** Routing pseudonyms live in `group_member_pseudonyms` (N per `(group, encrypted_member_id)`, one per device). Send fan-out resolves a recipient's EMI to all its pseudonyms and enqueues the identical blob to each. `POST …/push_binding` is additive; offline pickup names the device's own pseudonym, ownership-checked against the member's set.
2. **Linked-device reconciler.** A linked device gets group master keys via storage sync with no pseudonym. After every pull, `reconcile_synced_groups` fetches state, registers a per-device pseudonym, subscribes, and seeds and distributes its sender key.
3. **Per-device sender-key distribution.** `sender_key_shared` is keyed per `(group, recipient_did, device)`. Before each send, distribution discovers co-members' new devices and ships SKDMs to any unshared device.

Costs accepted: the server learns device-count per member (N pseudonyms per opaque EMI, same as Signal); an extra device-registration fetch per member per send; messages sent before a sender redistributes to a new device are not decryptable there (history backfill is a non-goal, §10).

## 7. Recovery interaction

**Partial.** Details of the recovery flow live in `50`.

A restored device has the *same* identity key (from the recovery blob) but a fresh registration ID, fresh prekeys, and zero sessions. Peers' registration-id reconciliation (§3) discards stale sessions and re-establishes. `device_id` is only a routing label and carries no key material.

**The difference between linking and recovery is the aliveness assumption:**

- **Linking is additive** and assumes an existing device is alive (§4).
- **Recovery is total** and should assume no device survives: revoke the identity's *entire* prior device set, on every server it uses, and register one fresh device. Revoking everything is hygiene (no dead fan-out targets) and security (a lost or stolen device is cut off).

**As built**, recovery calls `POST /v1/devices/replace` once, on the primary server only, naming `min(existing device_ids)` as the old device and reusing that same `device_id` for the new one (`app-core/src/lib.rs` `recover_from_blob`; the code comment there says this is wrong for multi-device). Other device rows, and other servers, are untouched. The hazard: "recovering" while another device is still alive leaves that device fully working and silently replaces one slot.

**Planned:** a whole-identity reset endpoint (same rotation-key authorization as `/replace`) that revokes every device row for the DID and kills their session tokens, called on every server in the blob's server list.

## 8. Trust on device-set change

**Planned (UI notice only).** Because the identity key is shared, adding or removing a peer's device does not change the safety number. This is the accepted weakness of the shared-identity model: a silently linked device is encrypted-to with no safety-number alarm. Mitigation: surface "Bob added a new device" as an info event by diffing the device list and registration IDs we already fetch, plus the own-device notice in §4.3.

## 9. Device revocation

**Planned.** Not built: no endpoint, no FFI, no UI.

For a lost device:

1. Delete its prekeys and `devices` row server-side, so peers stop encrypting to it on their next device fetch.
2. Immediately kill its session tokens so it cannot drain already-queued messages.
3. Remove its group delivery pseudonyms.

Revoking a device does not rotate the shared identity key, and with today's design does not revoke the rotation key that device holds (Known gaps). A compromised identity key is a worse event: full re-registration with a safety-number change for every contact.

## 10. History backfill

**Decided: explicit non-goal for v1.** A newly linked device starts blank and syncs forward, matching Signal. Optional later: direct device-to-device transfer over the provisioning channel.

## 11. Implementation order (remaining)

1. Send `SyncRead` from `mark_messages_read` (and the read-receipt path) to own devices.
2. Link confirmation code + new-device notice (§4.3).
3. Device list and revocation (§9).
4. Stop shipping the rotation key in the link bundle; authorize `/v1/devices/link` with an existing device's signature (`50` §Proposed).
5. Whole-identity recovery reset across all servers (§7).
6. Peer device-set-change notice (§8).

## Rationale and rejected alternatives

- **Per-device identity keys (rejected).** Would make every device a separate cryptographic identity, force safety-number changes on every link, and diverge from Signal's Sesame model for no gain. The shared static credential is safe precisely because it is static.
- **Signal's fixed "desktop is always secondary" linking (rejected).** We let either device show or scan, and allow copy-paste codes, so phone-to-phone and desktop-first setups work. Cost: the paste path needs the confirmation step in §4.3.
- **Short human-spoken PAKE codes (deferred).** Would need SPAKE2; a scanned or pasted high-entropy code is secure under plain ECDH.
- **A provisioning WebSocket (rejected).** Signal uses one; we chose stateless polling because the joining device has no account and the flow is rare and short.
- **One `SyncMessage` per feature (rejected).** Signal's experience; we cap event types and put current-value state in the storage service.
- **History backfill (non-goal).** Matches Signal; removes the false sense that transfer must be solved before linking ships.
