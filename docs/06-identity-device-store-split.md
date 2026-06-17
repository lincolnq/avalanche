# Identity store / device store split

Status: **implemented** (client split landed; multi-server backup + event-log
home still future). The store crate now exposes `DeviceStore` (device.db,
`crypto::Store`) and `IdentityStore` (identity.db) via `store::open_split`, with
a one-time file-copy+prune migration from the pre-split single file
(`core/crates/store/src/db.rs`); `AppCoreInner` owns one `IdentityStore` + the
primary account context + a `backup_accounts` vec
(`core/crates/app-core/src/lib.rs`). Captures the storage split that makes
multi-device, multi-account, snapshot, and recovery fall out of one model instead
of being special-cased. Spun out of the stage-4 discussion in `docs/05` §7.
Sections marked **PROPOSED** are recommendations; **OPEN** are unresolved.

Two pragmatic deviations from the literal design below, both noted inline at
their sections: (1) `DeviceStore` `Deref`s to its `IdentityStore` so durable
methods resolve on a device handle without an explicit `.identity` hop (§3/§5
boundary-crosser, made ergonomic — the method-name sets are disjoint, so the
type boundary still holds); (2) because `net::Client` isn't `Clone`, the primary
account context is surfaced as `AppCoreInner` fields rather than `accounts[0]`,
with the vec (`backup_accounts`) holding only the non-primary contexts (§9's
"N=1 thin shim").

Background reading:

- `docs/04-multi-device.md` — per-device session/prekey state; the event channel.
- `docs/05-device-data-sync.md` — the storage service that syncs the identity store; §7's single-authoritative + passive-backup model that motivates this split.
- `docs/50-identity-auth-recovery.md` — the recovery blob that bootstraps the identity keys.
- `docs/53-multi-account-ux.md` — multiple accounts of one identity.
- `core/crates/crypto/src/session.rs:57` — the `crypto::Store` trait the split formalizes.

## 1. Problem

Today a single `store::Store` (one SQLCipher database per `AppCore`, see
`core/crates/store/src/db.rs` and the one-file schema in
`core/crates/store/src/schema.rs`) holds **everything**: libsignal device crypto
*and* durable identity state. `AppCoreInner` carries exactly one `store` handle
(`core/crates/app-core/src/lib.rs:591`).

That conflates two fundamentally different scopes:

- **Per-device transport crypto** — Double Ratchet sessions, this device's prekey
  pools, sender keys. Cannot be shared between devices (sharing ratchet state
  breaks forward secrecy and desyncs); re-established on each device.
- **Per-identity durable state** — contacts, group **master** keys, profiles,
  blocked list, conversation flags, settings. The *same content* on every device
  and every account of the identity (`docs/05` §1; `docs/00` "Durable user data … is scoped to the identity").

Symptoms surfaced while specing stage 4 (`docs/05` §7):

1. **Multi-account has no home for durable state.** With one `AppCore` per
   `(identity, server)` account — each with its own store — per-identity data
   gets trapped in one account's store while the sibling accounts' `contacts`/
   `groups`/… tables sit dead. The unit (`account`) doesn't match the data's
   scope (`identity`).
2. **Snapshot/backup has no clean unit.** §7 wants "snapshot the identity's
   durable state." Aggregating N divergent per-account stores would be
   multi-master reconciliation, which §7/§9 explicitly reject.
3. **Recovery/link special-cases which tables to bootstrap vs. rebuild**, because
   both kinds live in one bag.

## 2. The seam already (half-)exists

`crypto` defines the store contract libsignal needs
(`core/crates/crypto/src/session.rs:57`):

```rust
pub trait Store:
    signal::SessionStore
    + signal::IdentityKeyStore
    + signal::PreKeyStore
    + signal::SignedPreKeyStore
    + signal::KyberPreKeyStore
    + Clone + Send {}
```

`store::Store` implements that **plus** a large set of methods libsignal never
calls — `contacts`, `groups`, `profiles`, `conversation_settings`, the
`storage_sync` sidecar, etc. That "plus" *is* the durable identity state. The
split just formalizes a boundary the codebase already implies: the `crypto::Store`
sub-traits on one side, the durable-state methods on the other.

## 3. The split — two stores (plus the event log)

- **`DeviceStore`** — implements `crypto::Store` (sessions, prekeys of all kinds,
  identity-key *access*) plus `SenderKeyStore`, device registration, push state,
  and server-bound caches. Scope: **per device**, with the server-bound parts
  per `(device, server)`. **Never synced**; fully rebuildable (register, publish
  prekeys, re-seed sender keys — the `restore_group` dance at
  `core/crates/app-core/src/lib.rs:1136`).
- **`IdentityStore`** — the durable per-identity state, the identity/rotation/
  storage **keys**, and the `storage_sync` sidecar. Scope: **one logical store
  per DID**. Each device holds a local replica, kept in sync by the storage
  service (`docs/05`) and bootstrapped from the recovery blob (`docs/50`).
- **Event log** (`message_history`, `reactions`, `message_revisions`, read/
  delivery marks) — per-identity *content* that roams via the **event channel**
  (`docs/04` §5), a different mechanism from both. Out of scope for this doc;
  listed so the partition below is exhaustive. May become its own store later.

## 4. Table partition

Mapping today's schema (`core/crates/store/src/schema.rs`) onto the split:

| Current table | Goes to | Why |
|---|---|---|
| `sessions` | **Device** | Double Ratchet state; unshareable. |
| `prekeys`, `signed_prekeys`, `kyber_prekeys`, `prekey_counters` | **Device** | This device's published pools. |
| `sender_keys` | **Device** | Re-seeded per device on link/recovery. |
| `push_state` | **Device** | This device's push token + pseudonym. |
| `group_credentials`, `group_server_params` | **Device** | Per-`(account/server)` caches, re-fetchable. |
| `profile_fetch_state` | **Device** | Per-device fetch throttle cache. |
| `message_queue` | **Device** | Outbound pending for this device. |
| `storage_cursor` | **Device** | This device's `seq` position in a server's `/items`. |
| `registration_id` (today bundled in `identity_keypair`) | **Device** | libsignal registration id is per-device. |
| `identity_keypair` (the keypair) | **Identity** | The DID's long-term key (see §5 — boundary-crosser). |
| `rotation_key` | **Identity** | DID rotation key. |
| `storage_key_state` | **Identity** | The identity storage key. |
| `recovery_blob_key` | **Identity** | Cached PRF-derived blob key. |
| `own_profile` | **Identity** | This persona's name + profile key. |
| `contacts`, `contact_profiles` | **Identity** | The social graph + nicknames/keys. |
| `groups` (master_key + durable cols) | **Identity** | Group *master* keys roam (`docs/05` §1). |
| `conversation_settings` | **Identity** | Per-conversation personal flags/timers. |
| `known_identities` (trust store) | **Identity** *(OPEN)* | Trust decisions; arguably should roam (see §11). |
| `account_info_cache` | **Identity** | Cache of server account records. |
| `account` row | **split** | `account_id`/DID → Identity; `server_url`/`device_id` → Device. |
| `storage_sync` sidecar | **Identity** | Per-record dirty/version for the durable records. |
| `message_history`, `reactions`, `message_revisions` | **Event log** | Roams via the event channel (`docs/04` §5). |

Two wrinkles to call out:

- **`identity_keypair` currently bundles a per-identity keypair with a per-device
  `registration_id`** in one row — these must separate (keypair → Identity,
  registration_id → Device).
- **`storage_sync` (per-record dirty/version) stays with Identity**, but
  **`storage_cursor` (per-server pull position) is per-device** and stays with
  Device. They were adjacent in the schema; the split divides them.

## 5. The boundary-crossers: identity keys

The identity keypair and rotation key are **per-identity** yet **consumed by
device crypto** (`IdentityKeyStore` needs the identity key to sign/verify;
sealed-sender and PLC ops need the rotation key). They live with the identity but
must be available on every device.

Resolution — they are **bootstrapped via the recovery blob / provisioning
channel** (`docs/50`, `docs/04` §4), not via the storage service: you cannot
fetch your identity key from a service you authenticate to *with* that key
(chicken/egg). So:

- identity keypair, rotation key, storage key → **IdentityStore** (authoritative
  local copy), seeded from the blob/provisioning at setup, then read by device
  crypto as needed.
- `registration_id` → **DeviceStore** (genuinely per-device).

This is the existing §11 bootstrap, now with a named home for each key.

## 6. Scope & sharing semantics (recap)

From the `docs/05` enumeration:

- **Per-device (A):** transport crypto — DeviceStore, never synced.
- **Per-identity (B):** durable state + identity keys — IdentityStore, synced via
  the storage service, snapshotted to backups, bootstrapped from the blob.
- **Per-person cross-identity (C):** UI/accessibility prefs that apply to the
  human regardless of persona — **deliberately not served here.** Identities are
  isolated (separate storage keys, no cross-identity link, to prevent persona
  correlation), so C has no shared encryption home. Keep device-local, or a
  separate person-scoped keyring outside this system.

Note the earlier nuance: a group **master key** is per-identity (B, roams), while
its **sender key / pseudonym** is per-device (A, re-seeded). The split puts each
on the correct side.

## 7. Encryption at rest

- **DeviceStore:** SQLCipher key from the device secure enclave, as today —
  device-bound, useless if the file is exfiltrated without the device.
- **IdentityStore:** **PROPOSED — also encrypt at rest with the device/enclave
  key, *not* the storage key.** The storage key remains the *record-level* key
  (sealed records + snapshot blob, `docs/05` §4). Encrypting the IdentityStore
  file itself with the storage key is tempting (portable file) but creates a
  chicken/egg: `storage_key_state` lives *inside* the IdentityStore, so the file
  couldn't be opened without already holding the key. Keeping at-rest = device
  key and record-level = storage key avoids that and matches today's model.

## 8. How the split resolves the open questions

- **Snapshot (`docs/05` stage 4):** a snapshot is "serialize the **IdentityStore**"
  — one well-defined unit. DeviceStores never enter it.
- **Multi-server:** the IdentityStore is **server-agnostic** (per DID). Per-server
  crypto lives in per-`(device, server)` DeviceStores. The authoritative server
  hosts the live `/items`; backups hold the snapshot.
- **`AppCore` unit (the A-vs-B question in `docs/05`):** **DECIDED — `AppCore` =
  one identity.** It owns **one IdentityStore** plus **1..N account/device
  contexts** (one DeviceStore + server client each). Full rationale in §9.

## 9. The AppCore boundary: one per identity

**DECIDED: `AppCore` = one identity (one persona).** It owns one IdentityStore +
1..N account contexts (each = a DeviceStore + server client + per-server
registration). Single-account is the trivial **N=1** default; multi-server
(human backups) expands N internally; multiple personas = multiple `AppCore`s.
This makes `AppCore == identity == persona == storage-key boundary == one
IdentityStore` — every boundary lines up.

The decision was driven by two lenses:

**Bots.** A bot creates its handle in one call and uses it directly
(`AppCore.loginOrCreateBot(serverUrl, dbPath, dbKey, displayName)` →
`createGroup`/`sendDm`/`events`; see `node/packages/app-core`). A bot is one
identity, one server, one device, and never syncs, links, recovers, or
multi-accounts. So the handle a bot creates must be a self-contained single
identity with **zero multi-account ceremony**, and the two-file split must be
**hidden behind the constructor** (derive `identity.db` + `device.db` from one
location — bots still pass one path). `AppCore = identity` with N=1 is exactly
that; the existing bot method surface is unchanged.

**Cross-AppCore aggregation is real but lands above this boundary, not below
it.** A unified view (e.g. contact autocomplete across everything) must span
**identities** — that layer exists regardless. But it operates at *IdentityStore
granularity*: durable state is per-identity, so there is exactly **one contact
list per identity**, and the app aggregates one-list-per-identity no matter how
many server-accounts an identity has. There is no user-facing per-account data to
aggregate (per-account = device crypto + caches). So the app-level layer's job is
**read-only persona aggregation** — stateless and simple — while the **write-side
sync/snapshot coordination stays inside the identity-`AppCore`**, next to the
IdentityStore it owns. The alternative (`AppCore` = account) would push that
stateful authoritative-election/CAS/snapshot dance *up* into the app, tangled
with read-aggregation — strictly worse.

Adjacent UX note (not a boundary concern): cross-persona autocomplete is a mild
privacy footgun — selecting persona-B's contact while composing as persona-A
risks a cross-persona send. The aggregated picker must bind each contact to its
owning identity (and switch the sending identity), or scope autocomplete to the
active persona.

**Cost accepted:** `AppCoreInner` (today a single `did`/`client`/`device_id`/
`store`) becomes one IdentityStore + identity key + a `Vec<AccountContext>`
(`server_url`, `device_id`, DeviceStore, client, `role: authoritative | backup`).
For bots/MVP the vec has one entry, so the single-account path stays a thin shim;
the multi-context machinery is incurred only for human multi-server backup.

**Working assumption (see §12):** one DID, registered on multiple servers — the
identity's accounts share the DID, while `device_id`/prekeys are per-(server,
device). This matches `docs/05` §7 and the recovery blob's `servers` list;
multi-server registration is the existing TODO at `core/.../lib.rs:840`.

## 10. Recovery & device-link, mapped to the split

- **Link a new device** (`docs/05` §12): create a fresh **DeviceStore** (register
  on the server, publish prekeys) + hydrate the **IdentityStore** (blob seeds
  identity/rotation/storage keys → storage-service `since=0` pull fills
  contacts/groups/settings → sender keys/sessions re-seed lazily).
- **Total-loss recovery** (`docs/05` §11): same, plus promote a backup server's
  snapshot into a fresh IdentityStore if the authoritative account is gone
  (needs the stage-4 snapshot path).

Each step touches exactly one store, which is the point.

## 11. Migration plan (from today's single store)

1. **API split, one file (de-risk):** introduce `DeviceStore` and `IdentityStore`
   as two Rust handles over the *same* connection initially, partitioning the
   method surface (`crypto::Store` impl → `DeviceStore`; durable-state methods →
   `IdentityStore`). No data move yet; behavior identical.
2. **Physical split:** move tables into two databases; a one-time migration copies
   rows. Split the `identity_keypair` row (keypair vs `registration_id`) and the
   `storage_sync`/`storage_cursor` pair.
3. **`AppCore` rewire:** `AppCoreInner` holds both handles (or an identity-level
   struct owning the IdentityStore + a set of account contexts). The `crypto`
   single-`Arc`-connection invariant (`CLAUDE.md` pattern 2) applies to the
   **DeviceStore** (libsignal's multiple `&mut dyn` sub-traits); the IdentityStore
   has no such constraint.
4. **Sequencing with `docs/05`:** stage-4 *server* endpoints and the
   scope-agnostic `build_snapshot(store)`/`restore_snapshot` core can land
   **before** this split (they don't care which store they read). This split is
   the load-bearing step that then makes snapshot **orchestration** clean
   (internal-automatic, since there's one IdentityStore to snapshot).

## 12. Decisions & open questions

**Decided:**

- **Two database files**, not one with two schemas — matches "the IdentityStore is
  one replicated logical thing" and makes a snapshot literally "serialize the
  identity DB."
- **Trust store (`known_identities`) is per-identity and synced** — verified keys
  roam across the identity's devices (a safety-number change is then flagged
  consistently everywhere), so it lives in the IdentityStore. (Cost: a
  compromise/rollback of the synced store could poison trust on all devices at
  once — acceptable given the alternative of per-device re-TOFU missing pre-link
  key changes.)
- **Every device keeps a full IdentityStore replica** — enables offline use and
  makes the snapshot a cheap whole-store serialize.
- **`AppCore` = one identity** — see §9.

**Still open:**

- **Event log** (`message_history`, `reactions`, `message_revisions`, read/
  delivery marks) — fold into the IdentityStore, keep in the DeviceStore, or a
  third store? **Deferred** for now; to be settled in its own doc alongside the
  `docs/04` §5 event-channel design. It does not block the device/identity split
  or stage 4.
- **One-DID-across-servers model** (§9 working assumption) — firm once multi-server
  registration is actually built (the `lib.rs:840` TODO).

## 13. Relationship to stage 4 & sequencing

This doc does not block stage 4's server side. Recommended order:

1. **Now:** stage-4 server endpoints + the scope-agnostic snapshot build/restore
   core (`docs/05` stage 4).
2. **Next:** implement this split (phases in §11).
3. **Then:** wire snapshot orchestration — which, post-split, is "periodically
   serialize the IdentityStore and push it to each backup server," with no
   cross-store aggregation and no app-level authoritative-routing hack.
