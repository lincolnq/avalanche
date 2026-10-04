# 07 — app-core

> **Status:** Built — the shared Rust client library behind every Avalanche client.
> **Last verified against code:** 2026-10-03

## Summary

`app-core` (`core/crates/app-core`) is the one client library that every Avalanche client
runs: the iOS and Android apps (via UniFFI), the Desktop app (via the Tauri bridge in
`desktop/src-tauri`), and Node bots (via the napi crate `app-core-node`). It owns the
homeserver connection, all cryptographic state, and the local encrypted store, and exposes
plaintext-level operations. **Bots and humans get the same API** — there are no second-class
clients.

## Current design

### What app-core owns

- **Connection.** A background reconnect task owns the WebSocket lifecycle, lazy
  challenge-response auth, and HTTP fallback (`connection.rs`).
- **All crypto state.** Identity key, sessions, prekeys (with replenishment), Sender Keys,
  zkgroup credentials, sealed-sender certificates. Client code only sees plaintext.
- **The local store.** One `IdentityStore` (durable per-identity state) and one `DeviceStore`
  (per-device crypto state), both SQLCipher (`store/src/db.rs`; docs/06).
- **Background tasks** per instance: reconnect loop, storage-sync scheduler (docs/05),
  disappearing-message reaper (docs/03 §5).
- **Events.** Decrypted messages, receipts, reactions/edits/deletes, group changes, sync
  notifications, and connection-state changes, surfaced through `next_events()` and
  `wait_for_connection_state_change()`.

### Instances: one `AppCore` per (identity, server) account

`AppCore` (`app-core/src/lib.rs`, `pub struct AppCore`) wraps an `AppCoreInner` that is
structured as **one identity**: an `IdentityStore` plus a primary account context (device
store, `net::Client`, device id) and a `backup_accounts: Vec<AccountContext>` for additional
servers (docs/06 §9). **`backup_accounts` is always empty today** — it is scaffolding marked
`#[allow(dead_code)]`. In practice each platform holds one `AppCore` per signed-in
(identity, server) account, keyed by DID (iOS `AppState.cores`, Android
`AppViewModel`, Desktop `HashMap<String, Arc<AppCore>>` in `src-tauri`), and merges their
conversations into one inbox.

### Storage defaults: humans persist, bots opt in

- **Human accounts (`did:plc:`) durably persist incoming message content before acking the
  server** (`persist_incoming`, set in `AppCore::build`). This is load-bearing: the server
  deletes its copy on ack, and the in-memory event channel is not durable, so persisting first
  is what survives a consumer crash.
- **Bots (`did:local:`) do not** persist incoming content, run storage sync, or upload
  recovery blobs — operator-managed bots keep only crypto and protocol state unless they call
  `save_message` / `touch_contact` themselves.
- Contacts are interaction-driven: the embedder calls `touch_contact(did, curated)` on
  deliberate gestures (docs/52).

### FFI shape

- **Exports are synchronous by default.** UniFFI-exported methods block on a process-global
  Tokio runtime (`OnceLock<Runtime>`, `lib.rs` near the top). This is because libsignal's
  store traits return non-`Send` futures, so UniFFI's async export can't carry the crypto
  paths. Platforms call these off the main thread.
- **Exception: long waits are native async exports.** `next_events()` and
  `wait_for_connection_state_change()` are `pub async fn` exports. Their futures are pure
  channel awaits (`Send`), and platform loops park on them for the life of the process; a
  sync-blocking version pins one thread per call and exhausted Swift's cooperative pool at three
  accounts. **Rule: never add a sync export that can block indefinitely.**
- **Interior mutability.** UniFFI wraps objects in `Arc`, so mutable state sits in a
  `tokio::sync::Mutex<AppCoreInner>`. Cheap lock-free clones of the device store, identity
  store, `net::Client`, and DID live directly on `AppCore` for read-only and idempotent paths.
- **Lock discipline.** Never hold `inner` across a network `.await`; the crypto send/group
  paths are a bounded, documented exception. The full rule and review checkpoint are in
  `core/CLAUDE.md` ("Concurrency").
- **Two error types.** `AppError` (rich, internal) and `AppErrorFfi` (string reasons, exported).
- **Tests use `_async` variants** of exported methods so they don't block the test runtime.

### Lifecycle hooks

- `set_app_active(bool)` — foreground/background hint; gates the foreground keepalive and
  triggers a liveness probe on foreground.
- `prepare_for_background()` / `resume_from_background()` — iOS suspension handshake: close
  the WS (so the server sees the device offline and push fires) and release SQLite locks on
  App Group files before suspension (docs/16).
- `reconnect_now()` — wake the reconnect loop for an immediate attempt.

### Connection state

`ConnectionState` is `Disconnected | Connecting | Connected | Reconnecting { next_attempt_at_ms }`
(`lib.rs`, `pub enum ConnectionState`). The richer model in docs/34 (outage-duration tiers,
`Unauthorized`, opportunistic reconnect while a server is down) is not built.

### API at a glance

Not exhaustive; the exported methods in `app-core/src/lib.rs` are the reference.

| Area | Representative methods |
|---|---|
| Account lifecycle | `create_account`, `PreparedAccount::new` + `finalize_account` (two-stage passkey signup), `login`, `login_or_create_bot`, `recover_from_blob`, device linking (`link_create_pairing`, `link_send_bundle`, …) |
| Connection | `start_reconnect_task`, `wait_for_connection_state_change`, `reconnect_now`, `set_app_active`, `prepare_for_background` |
| Receive | `next_events` |
| Send | `send_message(target: MessageTarget, …)` (unified DM/group), reactions, edits, deletes, attachments, timers |
| History | `save_message`, `load_conversations`, `load_messages`, `mark_messages_read` |
| Contacts / profiles | `list_contacts`, `touch_contact`, `block_contact`, `accept_request`, `report_and_block`, `set_display_name`, avatars, `get_account_info` |
| Groups | create, invite, accept/decline, join via link, roles, leave, fetch state, group avatar |
| Push / Projects | `register_push_token`, `fetch_projects`, `request_project_token`, OAuth approve calls (docs/25) |
| Bots only | `admin_request` passthrough, admin-event stream (`next_admin_events_async`) |

## Known gaps

- **Binding layers are hand-maintained.** UniFFI covers iOS/Android, but Desktop's ~90 Tauri
  commands and the ~140 napi attributes in `app-core-node` are written by hand, so a new
  export takes about six edits across platforms. There is no automated check that every
  export is bound everywhere. **Desktop's Tauri bridge currently fails to compile** because
  `next_events` / `wait_for_connection_state_change` became async exports and
  `desktop/src-tauri/src/lib.rs` still calls them synchronously.
- **No cancellation for parked waits.** A parked `next_events()` keeps the `AppCore` (and its
  authenticated WebSocket) alive after logout until the next event arrives. Fix: a shutdown
  signal the parked `recv()`/`changed()` selects on, invoked by each client on logout.
- **Single `Mutex<AppCoreInner>` serializes all crypto sends,** and a few crypto paths still
  fetch prekeys or POST under it — one slow prekey fetch stalls every send on that account.
- `backup_accounts` (multi-server identity) is dead scaffolding; adding a server to an existing
  identity does not register anything on the new server (docs/06, docs/53).

## Planned

- **Actor model for crypto state.** A dedicated thread running a Tokio `LocalSet` owns the
  non-`Send` libsignal state; exported `async fn`s send requests over a channel and await a
  `oneshot` reply. This retires the sync-export pattern and the global `Mutex<AppCoreInner>`
  as the serialization mechanism, allows per-conversation serialization, and gives a natural
  place for cancellation. Until then, migrate `Send`-safe methods (store reads, pure network
  calls) to async exports incrementally.
- **Generate the Tauri and napi binding layers** from the exported surface (or add a CI check
  that every UniFFI export has a Tauri and napi counterpart).

## Rationale and rejected alternatives

- **One library, every client.** Crypto, protocol, and storage logic is written and reviewed
  once; a bot is a full participant with the same guarantees as a phone.
- **A library you drive, not a daemon.** The embedder pulls events and calls methods; app-core
  runs its own background tasks but never calls back into platform code.
- **Store is one serialized connection per DB, not a pool.** libsignal's multi-`&mut` store
  API is satisfied by cloning an `Arc`-backed handle whose operations serialize on one
  blocking thread. Do not replace it with a pool (root `CLAUDE.md`, pattern 2).
- **Rejected: sync export for long waits.** It deadlocked the Swift cooperative pool (above).
- **Rejected: platform-side `Task.detached` / `Dispatchers.IO` wrappers around indefinite
  waits.** They only move the parked thread somewhere else.
