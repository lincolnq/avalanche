# 01 — Technical overview

> **Status:** Built — the repo layout, crates, stack, and tooling as they exist today.
> **Last verified against code:** 2026-10-03

## Summary

Avalanche is a monorepo: a Rust core (crypto, local store, network client, the shared
`app-core` library, the homeserver, the push relay), three native client UIs (iOS, Android,
Desktop) over that core, Node bots on napi bindings, and a static website. This doc maps what
exists and records why the stack looks the way it does. The roadmap lives in
`02-todos-deferred.md`; the security posture in `09-security-posture.md`.

## Current design

### Two governing principles

- **Don't implement cryptography; compose audited implementations.** libsignal provides the
  protocols; our job is to wire them correctly.
- **Make whole vulnerability classes impossible.** Everything security-critical is Rust, so
  memory-safety bugs cannot exist by construction in that code.

### Repository layout

```
avalanche/
├── core/                  # Rust Cargo workspace (see crate map below)
│   ├── crates/
│   └── proto/             # protobuf: content.proto, ws.proto, provisioning.proto, …
├── mobile/
│   ├── ios/               # Swift/SwiftUI app + NotificationServiceExtension + ShareExtension
│   └── android/           # Kotlin/Jetpack Compose app
├── desktop/               # Tauri 2 shell: Solid/TypeScript frontend + src-tauri Rust bridge
├── node/                  # npm workspace: @theavalanche/app-core (napi) + bots
│   └── packages/          #   app-core, adminbot, testbot
├── infra/
│   ├── migrations/        # PostgreSQL migrations (001–025), applied by `avalanche-server migrate`
│   ├── deploy/bundle/     # the av-deploy bundle: install.sh, update.sh, systemd units (docs/42)
│   └── docker-compose.yml # local Postgres
├── web/                   # Hugo site for theavalanche.net, incl. the server configure page
├── go.theavalanche.net/   # deep-link host (Universal Links / App Links / AASA)
├── design/                # icons, brand assets, Sketch files
├── projects/              # empty placeholder (chatbot/ has no files); real Projects live in node/packages
└── docs/
```

### Crate map (`core/crates/`)

| Crate | Role | State |
|---|---|---|
| `types` | Newtypes (AccountId, DeviceId, MessageId, Timestamp) and prost-generated protobuf types. No logic. | Built |
| `crypto` | libsignal wrappers: identity, prekeys, sessions, sealed sender, sender certificates, Sender Keys, zkgroup group crypto, attachment encryption, ephemeral ECDH. **No I/O**; defines the `Store` trait. | Built |
| `store` | SQLCipher local DB via tokio-rusqlite. Split into `IdentityStore` (durable per-identity state) and `DeviceStore` (per-device crypto state) (`store/src/db.rs`, docs/06). Implements `crypto::Store`. | Built |
| `net` | reqwest HTTP client + WebSocket client for the homeserver API. | Built |
| `app-core` | The shared client library: orchestrates crypto + store + net. UniFFI exports for iOS/Android; used directly by Desktop's Tauri bridge and by `app-core-node`. See docs/07. | Built |
| `app-core-node` | napi-rs bindings over `app-core` for Node bots, published to npm as `@theavalanche/app-core`. | Built |
| `server` | The homeserver: Axum + Tokio + PostgreSQL (sqlx). See docs/10. | Built |
| `relay` | Push relay: pseudonym → device-token map in SQLite; dispatches content-free wakeups to APNs, FCM, UnifiedPush. See docs/15, docs/41. | Built |
| `test-utils` | Test helpers (`TestClient` with identity keys + store). Dev-dependency only. | Built |
| `federation` | One-line placeholder. No federation code exists. | Placeholder |
| `project-sdk` | One-line placeholder. Projects use the npm `@theavalanche/app-core` package and plain HTTP instead. | Placeholder |

Dependency graph: `types ← crypto ← store ← net ← app-core`; `server` uses `types`/`crypto`
(no libsignal session code — it relays opaque bytes); `app-core-node` and Desktop's
`src-tauri` depend on `app-core`.

### Cryptographic stack

All protocol crypto comes from **libsignal**, pinned to commit `4c460615` as a git
dependency (not a branch). What we use:

- **X3DH + PQXDH prekeys** (signed, one-time EC, Kyber) for asynchronous session setup.
- **Double Ratchet** for 1:1 forward secrecy.
- **Sender Keys** for group message encryption.
- **Sealed sender** (multi-recipient) for the group send path (docs/03 §3.11). The 1:1 DM
  path does **not** use sealed sender today — see docs/09.
- **zkgroup** (the Chase/Perrin/Zaverucha private-group scheme) for action-bound group
  membership credentials and `GroupSendEndorsement` tokens (docs/03 §2).

Primitives: X25519, Ed25519/XEdDSA, AES-256-GCM, HKDF-SHA-256, Ristretto255 (zkgroup).
**Attachments deliberately use AES-256-CBC + HMAC-SHA256** (Signal-exact, for incremental
verification of large files) — the one divergence from the GCM default (docs/35). DID rotation
keys are P-256 (PLC's requirement; docs/50).

We do not implement any of these schemes ourselves and do not use OpenSSL directly.

### Server stack

Rust (Axum, Tokio, sqlx with compile-time-checked queries and checked-in `.sqlx/` offline
data), PostgreSQL. No Redis, no libsignal session code on the server. Message content columns
are `bytea`. Session tokens are opaque and revocable (not JWT). Detail: docs/10.

**The homeserver is single-instance.** WebSocket connections, group-pseudonym subscriptions,
and admin-event subscribers live in in-process `Arc<RwLock<HashMap<…>>>` maps
(`server/src/state.rs:90-113`), so two instances behind a load balancer would not deliver to
each other's sockets. Horizontal scaling would need a fan-out channel (e.g. Postgres
LISTEN/NOTIFY). At activist-org scale one instance is the intended deployment.

### Client architecture: Rust core + native UI

Every client runs the same `app-core`:

| Platform | UI | Bridge to app-core |
|---|---|---|
| iOS | Swift/SwiftUI | UniFFI-generated Swift, XCFramework |
| Android | Kotlin/Jetpack Compose | UniFFI-generated Kotlin + `libapp_core.so` via JNA (not an AAR) |
| Desktop | Solid/TypeScript in Tauri 2 | Hand-written Tauri commands in `desktop/src-tauri/src/lib.rs`; TypeScript `bindings.ts` generated by tauri-specta |
| Bots | TypeScript on Node | Hand-written napi-rs bindings in `core/crates/app-core-node` |

Crypto, networking, and storage are written once in Rust; the platform layers do presentation,
OS integration (push, keychain, notifications, deep links), and app-level state. Platform
notes: docs/60 (Android), docs/61 (Desktop); parity matrix: docs/62.

**On-device storage.** SQLCipher, one identity DB + one device DB per identity (docs/06).
DB key source: iOS Secure Enclave–wrapped key (`SecureEnclaveKeyManager`), Android Keystore
(`KeystoreKeyManager`), bots from an operator-supplied env var. **Desktop currently opens
SQLCipher with the constant key `"dev-placeholder-key"`** (`desktop/src/state/createAccounts.ts`)
— its databases are effectively unencrypted at rest. See docs/09 and docs/61.

### Message content envelope

The plaintext inside every ciphertext is a protobuf `ContentMessage`
(`core/proto/content.proto`) with a `oneof body` (text, receipt, group context, sender-key
distribution, group message, timer change, reactions, edits, deletes, sync transcripts, and
more) plus cross-cutting envelope fields (`timestamp_ms`, `profile_key`, expiry timer).
The `.proto` file is authoritative; forward compatibility is by reserved field numbers — a new
body variant takes a reserved number and retired numbers are never reused. Attachments ride as
`repeated AttachmentPointer` inside `TextMessage` (docs/35).

### Multi-device

The identity key is shared across an identity's devices; prekeys, sessions, sender keys, and
registration IDs are per-device; the sender fans out one ciphertext per recipient device.
Full design: docs/04.

### Cross-server casual group encryption

Action-bound groups (built) use zkgroup credentials on one homeserver plus Sender Keys for
content. Cross-server casual groups (not built) would use Sender Keys with client fan-out and
no central issuer. MLS remains an option behind the same `crypto::groups` interface. Design:
docs/03 and docs/13.

### Build, test, CI

- Build entry points are `make` targets (root `CLAUDE.md` lists them).
- **CI** (`.github/workflows/ci.yml`) runs: `cargo check` + `cargo clippy -D warnings` on the
  workspace; tests for `crypto`, `store`, `types`; `server` tests against Postgres; a Desktop
  frontend build plus a check that `desktop/src/bindings.ts` matches the Tauri command surface.
- **Not in CI:** `cargo audit` / `cargo deny`, app-core e2e tests (`make test-e2e`), iOS or
  Android builds, relay tests, Node bot builds, and any migration-compatibility check.
- **Release** (`.github/workflows/release.yml`, on `v*` tags): builds `av-server` and
  `av-relay` per target, the adminbot/testbot Node bundles, the arch-independent `av-deploy`
  bundle, and (stable tags) publishes `@theavalanche/app-core` to npm.

## Known gaps

- Desktop SQLCipher key is a constant placeholder (above; docs/09).
- No supply-chain checks (`cargo audit`/`cargo deny`) and no reproducible builds, though both
  are prerequisites for the external audit.
- CI does not build the mobile apps or run the app-core e2e suite, so FFI or behavior
  regressions can merge unnoticed.
- The `federation` and `project-sdk` crates are empty placeholders; `projects/` is an empty
  directory.

## Planned

- **Supply chain and audit readiness:** `cargo audit` + `cargo deny` in CI; reproducible
  builds for server, relay, and mobile; a pre-launch third-party audit of the crypto core, the
  homeserver, and mobile key storage, published in full.
- **CI coverage:** run app-core e2e against a CI-spawned server; build iOS and Android; check
  that every UniFFI export has Tauri and napi counterparts.

## Speculative

- **Key transparency** (a verifiable log of identity keys, as WhatsApp and Google Messages
  deploy) to let users detect server key substitution.
- **Calls.** 1:1 over WebRTC with the homeserver as signaling only (STUN/TURN, DTLS-SRTP);
  group calls via a LiveKit SFU with Insertable-Streams E2E encryption; large one-to-many
  broadcasts as a distinct experience. Would add LiveKit as a second deployable. Nothing is
  built and no client has a Calls tab.
- **Horizontal server scaling** via a cross-instance fan-out channel, if one instance ever
  stops being enough.

## Rationale and rejected alternatives

- **Rust for the server.** Memory safety by construction, no GC pauses, and libsignal is Rust,
  so the server uses it natively. Rejected: Go (GC, no native libsignal) and C/C++ (memory
  safety).
- **Rust core + native UI** (Signal's architecture) rather than a cross-platform UI toolkit:
  the security-critical code is written and reviewed once; native UIs get platform polish.
  The cost is three hand-maintained UIs and three binding layers.
- **Tauri over Electron for Desktop**, Solid over React/Svelte/Dioxus: recorded in docs/61.
- **PostgreSQL only, no Redis:** the homeserver is one binary plus Postgres; rate limiting is
  table-backed.
- **Opaque session tokens, not JWT:** revocable, no signing key to manage.
- **Protobuf envelope** (Signal's wire format) so every client agrees on encoding.
- **Capacity.** Messaging is cheap (ciphertext relay; a 1 GB box handles ~1,000 concurrent
  WebSockets); attachments drive storage (TTL-GC'd, docs/35); a push relay serving many
  homeservers fits on a $4/mo droplet (docs/41).
