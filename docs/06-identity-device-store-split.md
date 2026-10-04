# 06 — Identity store / device store split

> **Status:** Built — the client store is split into `DeviceStore` (device.db) and `IdentityStore` (identity.db). `AppCore` is one identity with one primary account context. Multi-account contexts (`backup_accounts`) are scaffolding only and never populated.
> **Last verified against code:** 2026-10-03

## Summary

The local store holds two kinds of state with different scopes: per-device transport crypto (sessions, prekeys, sender keys) and per-identity durable state (contacts, group master keys, settings, identity keys). They live in two SQLCipher files. The device file is never synced and fully rebuildable; the identity file is replicated to the identity's other devices by the storage service (`05`) and bootstrapped from the recovery blob or the link bundle.

Section numbers below are cited from code and are stable.

## Known gaps

- **One identity = one server today.** `AppCoreInner` has a primary account context plus `backup_accounts: Vec<AccountContext>`, but every constructor sets `backup_accounts: Vec::new()` (`app-core/src/lib.rs`). "Add a server to an existing identity" in the apps only appends a `ServerInfo` to the local list and never registers on the new server (`53` Known gaps).
- **The trust store does not roam.** `known_identities` lives in identity.db but has no storage-sync adapter, contrary to the §12 decision.
- **The event log lives in identity.db but does not sync.** `message_history`, reactions, and revisions are in `IDENTITY_TABLES` (`store/src/schema.rs`); they roam only via the event channel (`04` §5).

## 1. Problem

Before the split, one SQLCipher store per `AppCore` held both libsignal device crypto and durable identity state. That conflated two scopes: transport crypto that cannot be shared between devices, and durable state that should be identical on every device of the identity. The symptoms: no clean home for per-identity data when an identity has several accounts, no clean unit to snapshot, and recovery/linking had to special-case which tables to restore versus rebuild.

## 2. The seam

`crypto` defines the store contract libsignal needs (`crypto/src/session.rs`, `trait Store: SessionStore + IdentityKeyStore + PreKeyStore + SignedPreKeyStore + KyberPreKeyStore + Clone + Send`). Everything else the old store did (contacts, groups, profiles, settings, the sync sidecar) is durable identity state. The split formalizes that boundary.

## 3. The split

**Built** (`store::open_split`, `store/src/db.rs`, `store/src/schema.rs` `DEVICE_MIGRATIONS` / `IDENTITY_MIGRATIONS`). A one-time file-copy-and-prune migration converted pre-split single files.

- **`DeviceStore`** (device.db) — implements `crypto::Store` plus `SenderKeyStore`, device registration, push state, and server-bound caches. Per device (and per `(device, server)` for server-bound parts). **Never synced; fully rebuildable.** The single-`Arc`-connection invariant (root `CLAUDE.md` pattern 2) applies here.
- **`IdentityStore`** (identity.db) — durable per-identity state, the identity/rotation/storage keys, the trust store, the `storage_sync` sidecar, and (for now) the event log.

Deviation from the original sketch: `DeviceStore` derefs to its `IdentityStore`, so durable methods resolve on a device handle without an explicit hop. The method sets are disjoint, so the type boundary still holds.

## 4. Table partition

| Table | Store | Why |
|---|---|---|
| `sessions` | Device | Double Ratchet state; unshareable |
| `prekeys`, `signed_prekeys`, `kyber_prekeys`, `prekey_counters` | Device | This device's published pools |
| `sender_keys` | Device | Re-seeded per device on link/recovery |
| `push_state` | Device | This device's push token + pseudonym |
| `group_credentials`, `group_server_params` | Device | Per-server caches, re-fetchable |
| `profile_fetch_state` | Device | Per-device fetch throttle |
| `message_queue` | Device | Outbound pending (store API exists; unused by app-core, see `34`) |
| `storage_cursor` | Device | This device's position in the server's `seq` space |
| `device_account` (incl. `registration_id`) | Device | Per-device registration |
| `identity_keypair` | Identity | The DID's long-term key (§5) |
| `rotation_key` | Identity | DID rotation key (§5; see `50` Known gaps) |
| `storage_key_state` | Identity | The identity storage key |
| `recovery_blob_key` | Identity | Cached passkey-derived blob key |
| `own_profile` | Identity | This persona's name + profile key |
| `contacts`, `contact_profiles` | Identity | Social graph; synced |
| `groups` | Identity | Group master keys; synced |
| `conversation_settings` | Identity | Per-conversation flags/timers; synced |
| `known_identities` | Identity | Trust store; **not synced** (Known gaps) |
| `account_info_cache` | Identity | Cache of server account records |
| `storage_sync` | Identity | Per-record dirty/version for synced records |
| `message_history`, `reactions`, `message_revisions` | Identity | Event log; roams via the event channel only |

## 5. The boundary-crossers: identity keys

The identity keypair and rotation key are per-identity but consumed by device crypto (signing prekeys, sealed sender, PLC and device-replace signatures). They live in the IdentityStore and are **bootstrapped via the recovery blob or the link bundle**, not the storage service: you cannot fetch your identity key from a service you authenticate to with it. `registration_id` is genuinely per-device and lives in device.db.

(`50` §Proposed changes what the rotation key is and where it lives.)

## 6. Scope & sharing semantics

- **Per-device:** transport crypto — DeviceStore, never synced.
- **Per-identity:** durable state + identity keys — IdentityStore, synced via the storage service, bootstrapped from the blob.
- **Per-person across identities** (e.g. accessibility prefs): deliberately not served. Identities are isolated to prevent persona correlation, so there is no shared encryption home. Keep these device-local.

A group's **master key** is per-identity (roams); its **sender key and delivery pseudonym** are per-device (re-seeded).

## 7. Encryption at rest

**Built.** Both files are SQLCipher-encrypted with the device's platform-protected key (iOS Secure Enclave/Keychain, Android Keystore), not the storage key. The storage key is record-level only (`05` §4); it lives inside identity.db, so it could not also gate opening that file.

## 8. How the split resolves the open questions

- **Snapshot unit:** "serialize the IdentityStore" (if snapshots return; `05` §7 parks them).
- **Multi-server:** the IdentityStore is server-agnostic; per-server crypto lives in per-`(device, server)` DeviceStores.
- **`AppCore` unit:** one identity (§9).

## 9. The AppCore boundary: one per identity

**Decided; partially built.** `AppCore` = one identity (one persona). It owns one IdentityStore plus account contexts (each a DeviceStore + server client). Single-server is the N=1 case; multiple personas are multiple `AppCore`s. `AppCore == identity == persona == storage-key boundary == one IdentityStore`.

As built, the primary context is held as `AppCoreInner` fields (because `net::Client` isn't `Clone`), with non-primary contexts in `backup_accounts`, which is always empty. Platform apps still keep one core per account ID.

Why one per identity:

- **Bots** are one identity, one server, one device, and never sync, link, recover, or multi-account. The two-file split is hidden behind the bot constructor (one path in, two files derived).
- **Cross-identity aggregation** (for example contact autocomplete across personas) lives *above* this boundary, read-only, at IdentityStore granularity. Write-side sync stays inside the identity's `AppCore`. The alternative (`AppCore` = account) would push authoritative-election and sync coordination up into the app.

UX note: cross-persona autocomplete is a footgun — picking persona B's contact while composing as persona A risks a cross-persona send. The picker must bind each contact to its owning identity or be scoped to the active persona.

## 10. Recovery & device-link, mapped to the split

- **Link a new device:** fresh DeviceStore (register, publish prekeys) + hydrate the IdentityStore (bundle seeds the keys → storage pull fills contacts, groups, settings → sender keys re-seed lazily).
- **Total-loss recovery:** same, with the keys from the recovery blob.

Each step touches exactly one store.

## 11. Migration plan (from the single store)

**Built.** API split over one connection, then a physical split into two files with a one-time copy-and-prune migration, then `AppCore` rewired to hold both handles. Kept for its section number.

## 12. Decisions & open questions

**Decided:**

- **Two database files**, not one with two schemas.
- **The trust store is per-identity and should sync**, so a safety-number change is flagged consistently on every device. Cost: a compromised or rolled-back synced store could poison trust on every device at once (see `05` rollback gap). **Not yet built** (Known gaps).
- **Every device keeps a full IdentityStore replica**, which enables offline use.
- **`AppCore` = one identity** (§9).

**Open:**

- **Event log placement** — keep it in identity.db (today), move it to the DeviceStore, or a third store. Settle alongside `04` §5.
- **One DID across servers** — the working assumption for multi-server identities. Firm once multi-server registration is built; see also the client-side federation proposal in `13`, which changes how a second server is used.

## 13. Sequencing

Remaining work that depends on this split:

1. Multi-server registration so `backup_accounts` (or its replacement) holds real contexts.
2. Trust-store adapter (`05` Planned).
3. Decide event-log placement (§12).
