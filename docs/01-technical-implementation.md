# Activism Social Network — Technical Implementation

## Philosophy

Two principles govern every technical decision here:

- **Don't implement cryptography; use audited implementations.** The academic literature on secure messaging is rich, and Signal has already done the hard work of turning it into production-quality, open-source code. Our job is to compose those primitives correctly, not to reinvent them.
- **Make whole classes of vulnerabilities impossible.** The right tool for security-critical server software is one where memory safety bugs — buffer overflows, use-after-free, data races — cannot exist by construction, not one where we try hard to avoid them.

---

## Repository structure

The repository is a monorepo. The Rust codebase is the platform core — the cryptography, the homeserver, the mobile shared library, the push relay — but first-party Projects may be written in any language, and the mobile UI layers are Swift and Kotlin. Rust does not own the root.

```
actnet/
├── core/                     # Rust — Cargo workspace root
├── mobile/
│   ├── ios/                  # Swift/SwiftUI
│   └── android/              # Kotlin/Jetpack Compose
├── projects/
│   ├── sdk/                  # HTTP client libraries for Project developers
│   ├── channel-directory/
│   ├── team-assignment/
│   ├── action-day/
│   ├── qa-bot/
│   ├── collab-docs/
│   └── engagement-tracking/
├── infra/
│   ├── docker/
│   └── migrations/           # versioned PostgreSQL migrations
└── docs/
```

### `core/` — the Rust workspace

Everything security-critical is here: cryptography, local storage, networking, the homeserver, the push relay, and the UniFFI boundary that mobile UI layers call into. The workspace is organized into crates with clear boundaries so that security-sensitive code can be audited in isolation.

```
core/
└── crates/
    ├── types/          # shared serializable types: DIDs, AccountId, DeviceId,
    │                   # message envelopes, group identifiers. No logic, no heavy
    │                   # dependencies. Everything else imports this.
    │
    ├── crypto/         # libsignal wrappers. X3DH key generation and session
    │                   # initialization, Double Ratchet encrypt/decrypt, zkgroup
    │                   # anonymous credentials (Stage 4), Ed25519 signing for
    │                   # federation (Stage 9). Pure logic — no I/O. Testable
    │                   # without a database or network.
    │
    ├── store/          # SQLCipher-backed local database. Implements libsignal's
    │                   # ProtocolStore trait. Schema covers sessions, prekeys,
    │                   # message queue, group state, and CRDT operation logs.
    │                   # Database key is a placeholder in Stage 1; wired to the
    │                   # platform secure enclave in Stage 3.
    │
    ├── net/            # Async HTTP/WebSocket client. Knows how to talk to a
    │                   # homeserver: account registration, prekey upload/fetch,
    │                   # message send, WebSocket delivery loop, offline queue
    │                   # drain on reconnect, Project deep link resolution.
    │
    ├── app-core/       # UniFFI boundary crate. Composes crypto + store + net
    │                   # into the interface the mobile UI layers call. Contains
    │                   # the .udl interface definitions; generated Swift and
    │                   # Kotlin bindings live here. Nothing else crosses the
    │                   # FFI boundary.
    │
    ├── server/         # The homeserver binary. Axum + Tokio, PostgreSQL via
    │                   # sqlx. Grows across stages: account registration and
    │                   # message relay (Stage 2), zkgroup credential issuance
    │                   # (Stage 4), Project registration and scope enforcement
    │                   # (Stage 6), WebRTC signaling (Stage 8), federation relay
    │                   # via the federation crate (Stage 9).
    │
    ├── relay/          # The push relay binary. A standalone service with a
    │                   # single responsibility: map pseudonyms to device tokens
    │                   # and fire content-free wakeups via APNs and FCM. Minimal
    │                   # dependencies; holds no message content.
    │
    ├── federation/     # Server-to-server protocol logic. HTTPS with Ed25519
    │                   # request signing and verification, did:plc resolution,
    │                   # cross-server message routing, selective federation
    │                   # allowlist enforcement. Used as a library by server/.
    │                   # Not a separate process.
    │
    ├── project-sdk/    # The Rust SDK for Project developers. Defines the API
    │                   # contract: scope declarations, bot account primitives,
    │                   # deep link registration, group access. First-party
    │                   # Projects are built against this, so third-party
    │                   # developers get the same interface.
    │
    └── test-utils/     # Shared test helpers. In-process homeserver harness,
                        # testcontainers Postgres setup, simulated client
                        # builders, fake prekey bundles and session fixtures.
                        # Dev-dependency only — never compiled into any binary.
```

### `mobile/`

The iOS and Android apps. Each is a native UI project (Xcode / Gradle) that imports `app-core` as a compiled artifact — an XCFramework on iOS, an AAR on Android. All cryptography, networking, and local storage logic lives in the Rust core; the mobile layers handle presentation only.

### `projects/`

First-party Projects, each in its own subdirectory with its own language and build system. Projects are applications built on top of the substrate — they interact with the homeserver through the Project API, not by linking against `core/`. The `sdk/` subdirectory contains HTTP client libraries (TypeScript, Python, and others as needed) for Projects that don't use Rust.

Projects that happen to be written in Rust can depend directly on `core/crates/project-sdk/` as a Cargo path dependency, but this is not required.

### `infra/`

Docker images and Compose stacks for local development and CI, plus versioned PostgreSQL migrations managed by `sqlx migrate`. The homeserver binary expects migrations to have been applied; the migration files are the authoritative schema definition.

### `docs/`

Design documents, threat model, audit reports, and operational guides. The two documents currently at the repo root (`activism-network-design.md`, `technical-implementation.md`) will move here.

---

## Core cryptographic stack

### libsignal

The cryptographic foundation is **[libsignal](https://github.com/signalapp/libsignal)**, Signal's open-source cryptographic library. It is primarily written in Rust and provides bindings for Swift, Kotlin, and TypeScript. We use it directly rather than reimplementing any of the schemes it provides.

What libsignal gives us:

- **Double Ratchet Algorithm** — forward secrecy for 1:1 and small-group chats. Each message uses a fresh key derived from a ratcheting key chain; compromising one message key doesn't compromise past or future messages.
- **X3DH (Extended Triple Diffie-Hellman)** — asynchronous key establishment. Alice can initiate an encrypted session with Bob before Bob is online, using Bob's prekeys published to the server.
- **Sealed sender** — the server cannot determine who sent a message to whom, only that some authorized group member sent it.
- **zkgroup / anonymous credentials** — the scheme from the Chase/Perrin/Zaverucha paper ("The Signal Private Group System"), which provides Signal-style group membership guarantees. Members prove they belong to a group without revealing which member they are. This is the basis for action-bound groups.

### Primitive choices

All of these are provided by libsignal or the RustCrypto / dalek-cryptography ecosystem:

| Primitive | Algorithm | Notes |
|---|---|---|
| Key agreement | X25519 | ECDH on Curve25519 |
| Signatures | Ed25519 | Fast, small signatures |
| Symmetric encryption | AES-256-GCM | AEAD; ChaCha20-Poly1305 where AES hardware acceleration is unavailable |
| Key derivation | HKDF-SHA-256 | |
| Group credentials | Ristretto255 | Prime-order group for anonymous credential scheme |
| Password hashing | Argon2id | For any user-facing secrets |

### What we are not doing

We are not implementing any of these schemes ourselves. We are not using OpenSSL directly. We are not using any crypto primitives that haven't been through independent academic and implementation review.

---

## Server implementation

### Language: Rust

The homeserver is written in **Rust**. The reasons are security-first:

- **Memory safety by construction.** The entire class of vulnerabilities that plague C/C++ servers — buffer overflows, use-after-free, dangling pointers, data races — cannot occur in safe Rust. This is not "we try hard to avoid them"; it is a compile-time guarantee. For a server handling encrypted communications for activists, this matters enormously.
- **No garbage collector.** Consistent, predictable latency without GC pauses. Important for real-time messaging.
- **libsignal is Rust.** We use the library natively without an FFI boundary on the server.
- **Increasingly the industry standard for security-critical infrastructure.** Cloudflare, AWS, Mozilla, and the Linux kernel have all moved security-sensitive components to Rust for these reasons.

### Framework and runtime

- **Tokio** — async runtime. The de facto standard for high-performance async Rust; handles tens of thousands of concurrent connections efficiently.
- **Axum** — HTTP framework built on Tokio and Tower. Type-safe, composable, well-maintained. Handles the HTTP/WebSocket layer.
- **WebSockets** for real-time message delivery to connected clients. HTTP/2 for server-to-server federation transport, with mutual TLS for authentication.
- **sqlx** — async PostgreSQL client with compile-time query checking. Queries are verified against the actual schema at compile time, eliminating a class of runtime errors.

### Database

- **PostgreSQL** on the server. Stores encrypted message blobs, routing metadata, group credential state, DID registrations, push pseudonyms, push queuing, and session tokens (with `expires_at` columns and a background cleanup task). The server stores ciphertext it cannot read; the schema reflects this — message content columns are `bytea`, never text.
- **Rate limiting** is handled in-process with Tokio. For multi-instance deployments, PostgreSQL advisory locks or a lightweight counter table are sufficient at the scales activist org servers will realistically reach.
- No Redis. The homeserver dependency is a single binary plus PostgreSQL.

### Server-to-server federation

Homeservers authenticate to each other using their own DID-based keys. The transport is HTTPS with request signing (similar to ActivityPub's HTTP Signatures, but using Ed25519 keys from the server's DID document). A receiving server verifies the signature against the sending server's DID before processing any federated request.

---

## Mobile implementation

### Architecture: Rust core + native UI

The mobile apps share a **Rust core** that handles all cryptography, networking, local database access, and business logic. The UI layer is native per platform. This is the same architecture Signal uses internally.

- **iOS:** Swift/SwiftUI UI layer. The Rust core is compiled to a static library and exposed to Swift via **UniFFI** (Mozilla's tool for generating Swift/Kotlin/Python bindings from Rust).
- **Android:** Kotlin/Jetpack Compose UI layer. Same Rust core, same UniFFI bindings.

This means the security-critical code — crypto operations, key storage, message processing — is written once, in Rust, and reviewed once. UI differences between platforms are confined to the presentation layer.

### On-device storage

- **SQLCipher** — SQLite with AES-256 transparent encryption, keyed from the user's device credentials. All local message history, group state, and keys are encrypted at rest.
- Keys are stored in the platform secure enclave (iOS Secure Enclave / Android Keystore) where hardware support is available. The SQLCipher database key is derived from a secret held in the secure enclave, so extracting the database without the device's hardware key is not useful.

### Calls

Calls are substrate-level, not a Project. They surface in the app's Calls tab and are available in any DM or group chat.

**1:1 calls** use WebRTC peer-to-peer. The homeserver acts as a signaling channel only — it brokers the initial WebRTC handshake over the existing WebSocket connection, then steps aside. Media flows directly between devices. STUN/TURN servers handle NAT traversal; the TURN server relays media when a direct connection isn't possible but learns nothing about content. DTLS-SRTP provides media encryption.

**Group calls** require a media server. Pure peer-to-peer doesn't scale past 3–4 participants — each participant would need to send their video stream to every other participant simultaneously. Instead, participants send one stream each to a **Selective Forwarding Unit (SFU)**, which routes streams to recipients without decoding them. We use **LiveKit**, an open-source SFU written in Go that is self-hostable and actively maintained.

E2E encryption for group calls is provided by **WebRTC Insertable Streams** (a W3C API): clients encrypt media frames before they leave the device using keys derived from the group's key material, and decrypt on receipt. The SFU forwards encrypted frames it cannot read. This is the same approach Signal uses for group calls.

**Large broadcasts** (hundreds or thousands of listeners, e.g. a movement-wide address) are not calls — they are one-to-many streams. LiveKit supports this mode alongside its SFU mode. The distinction matters: a call is bidirectional and has a practical size limit of tens of participants; a broadcast is unidirectional and scales to any audience size. The app should expose these as distinct experiences rather than trying to make one UI cover both.

The LiveKit SFU is a separate deployable component from the homeserver. Small orgs can run it on the same machine; larger deployments will want it separate. It is the one infrastructure dependency besides PostgreSQL that self-hosters need to run for full functionality.

### Push

The mobile client registers a per-(user, server) pseudonym with the push relay at account creation on each homeserver. The relay is a simple Rust service; its only job is the pseudonym → push token mapping described in the design document. It holds no message content and no cross-server linkage.

---

## Security practices

### Open source and auditable

All code — homeserver, mobile apps, push relay, cryptographic core — is open source. For this user base, "trust us" is not a valid security argument. The code is the security argument.

### Third-party audits

Before launch, independent security audits of the cryptographic implementation and the server software. This is standard practice for Signal, Matrix/Element, and other serious encrypted messaging projects, and it is non-negotiable here. Audit reports are published in full.

### Reproducible builds

Builds are reproducible: the same source produces the same binary, verifiably. Users and auditors can confirm that the app they're running matches the published source code. This is a defense against a compromised build pipeline inserting malicious code.

### Dependency management and supply chain

- **`cargo audit`** runs in CI and blocks on known vulnerabilities in any dependency.
- Dependencies are pinned in `Cargo.lock` and reviewed on update.
- The dependency tree is kept as small as practical. Every dependency is a supply-chain risk.
- The cryptographic dependencies (libsignal, RustCrypto) are treated as especially sensitive and audited separately.

### Threat modeling as a living document

The threat model in the design document is reviewed and updated as the implementation evolves. New features go through a threat-model pass before landing.

---

## Performance

### Compilation

Rust compile times are slower than Go or interpreted languages, but manageable:

- **Incremental compilation** in development — only changed crates recompile.
- **sccache** for shared build caching in CI.
- **Workspace structure** organized to minimize recompilation: the cryptographic core, the server logic, and the federation layer are separate crates. A change to the federation layer doesn't recompile the crypto core.
- **cargo-nextest** for faster parallel test execution.

### Runtime

- Near-C execution performance. No GC pauses. Rust's zero-cost abstractions mean high-level code compiles to the same machine code as hand-optimized C.
- Tokio's async runtime handles high connection counts efficiently with minimal thread overhead.
- Message encryption/decryption is fast: AES-256-GCM with hardware acceleration (AES-NI on x86, ARMv8 Crypto Extensions on mobile) runs at multiple GB/s.
- The server is designed to be horizontally scalable: multiple homeserver instances can run behind a load balancer with shared PostgreSQL.

### Self-hostability

The homeserver ships as a single statically-linked binary plus PostgreSQL. Docker images are provided for convenience. There is no requirement to run our infrastructure.

### Capacity planning

Resource requirements vary significantly by use case. All figures below assume a Rust homeserver with PostgreSQL on the same machine unless noted. The dominant costs are concurrent WebSocket connections (messaging), storage (media attachments and document snapshots), and bandwidth (calls).

| Deployment | Active users | Suggested spec | Approx. cost |
|---|---|---|---|
| Small org / single action | ~100 | 1 vCPU, 1 GB RAM, 20 GB SSD | ~$6/mo |
| Medium org | ~1,000 | 2 vCPU, 4 GB RAM, 100 GB SSD | ~$20/mo |
| Large org | ~10,000 | 4+ vCPU, 16 GB RAM, separate PostgreSQL | ~$80–150/mo |
| Push relay | Serves many homeservers | 1 vCPU, 512 MB RAM | ~$4/mo |

A few notes on what drives these numbers:

- **Messaging** is cheap. Each concurrent WebSocket connection uses roughly 50–100 KB of RAM in a well-optimized Rust server. CPU cost is minimal — the server handles encrypted blobs, not plaintext. 1,000 concurrent connections is well within a 1 GB RAM machine.
- **Media attachments** are the main storage driver. Text messages are tiny; a server with active photo/file sharing needs storage budgeted accordingly. Object storage (S3-compatible) is the right answer at any meaningful scale — keep the homeserver stateless with respect to files.
- **Calls are the most resource-intensive component** and are handled by LiveKit, not the homeserver. A 10-person video call at 720p uses roughly 15–20 Mbps of SFU bandwidth. A small org running occasional calls can share a machine with the homeserver; an org with frequent concurrent calls should run LiveKit on dedicated hardware or a separate VPS. Audio-only calls are roughly 10× cheaper on bandwidth.
- **Collaborative documents** add modest storage overhead (encrypted operation logs + snapshots) but negligible CPU or RAM.
- **Large announcement groups** (thousands of members) are fine for messaging but generate a burst of push relay traffic when a message is sent. The relay fans out pushes asynchronously so this doesn't block the sender, but the relay needs enough outbound bandwidth to handle the burst.

---

## Multi-device

A user may register multiple devices (phone, tablet, desktop). Each device has its own identity key pair, its own prekey bundles, and its own Double Ratchet sessions. When Alice sends a message to Bob, her device encrypts separately for each of Bob's registered devices — the server knows which devices belong to an account and fans out the ciphertext to each one. This is the same model Signal uses.

Implications:

- **Prekey bundles are per-device.** The server stores and serves bundles keyed by `(AccountId, DeviceId)`. When initiating a session, the sender fetches bundles for all of the recipient's devices and establishes a separate session with each one.
- **Message fan-out is server-side.** The sender submits one encrypted payload per recipient device; the server queues each independently. This is acceptable because the server only sees ciphertext and recipient device identifiers — no plaintext, no aggregation across accounts.
- **Device linking** uses a secure channel between the existing device and the new one (e.g., scanning a QR code containing a one-time secret). The new device generates its own keys, the existing device signs an attestation, and both are uploaded to the server. The existing device optionally transfers encrypted message history to the new device over a direct connection.
- **Device revocation** removes the revoked device's prekey bundles from the server and notifies all active sessions that the device is no longer valid. Senders stop encrypting for it on the next message.

Multi-device support ships in **Stage 3** alongside the mobile apps. The server schema in Stage 2 already models devices as a first-class concept (`device_id` on session tokens, prekey bundles stored per-device).

---

## Message content envelope

The `EncryptedMessage.ciphertext` field wraps a structured plaintext envelope. The envelope is serialized with Protocol Buffers (the same wire format Signal uses) so that all clients and the eventual UniFFI boundary agree on encoding without ad-hoc parsing.

```protobuf
// proto/content.proto

syntax = "proto3";
package actnet;

message ContentMessage {
  oneof body {
    TextMessage    text       = 1;
    MediaMessage   media      = 2;
    ReceiptMessage receipt    = 3;
    TypingMessage  typing     = 4;
    ExpiryUpdate   expiry     = 5;
    // Future: reaction, reply, profile key update, group state change, etc.
  }

  uint64 timestamp_ms  = 15;  // sender's wall clock, unix millis
  uint32 expiry_timer  = 16;  // seconds; 0 = use group/conversation default
}

message TextMessage {
  string body = 1;
}

message MediaMessage {
  string content_type = 1;  // MIME type
  bytes  key          = 2;  // AES-256-GCM key for the attachment blob
  bytes  digest       = 3;  // SHA-256 of the encrypted blob
  string upload_url   = 4;  // URL where the encrypted blob was uploaded
  uint64 size_bytes   = 5;  // plaintext size (for UI pre-allocation)
  bytes  thumbnail    = 6;  // optional encrypted thumbnail, inline
}

message ReceiptMessage {
  enum Type {
    DELIVERY = 0;
    READ     = 1;
  }
  Type             type       = 1;
  repeated uint64  timestamps = 2;  // timestamps of the messages being receipted
}

message TypingMessage {
  bool started = 1;
}

message ExpiryUpdate {
  uint32 expiry_timer = 1;  // new timer in seconds
}
```

The protobuf definition lives in a new `proto/` directory at the workspace root. The `types` crate generates Rust structs from it via `prost-build`; mobile layers use the same `.proto` files to generate Swift and Kotlin types. The envelope is defined in Stage 1 alongside the crypto core, even though most message types won't be fully exercised until later stages.

---

## Media and attachments

Attachments follow Signal's proven encrypt-then-upload model:

1. **Sender encrypts the file locally** with a random AES-256-GCM key.
2. **Sender uploads the encrypted blob** to the homeserver's attachment endpoint (or an S3-compatible object store that the homeserver proxies). The server stores opaque ciphertext and returns a URL.
3. **Sender includes the decryption key, digest, and URL in the message envelope** (the `MediaMessage` field above). This metadata is itself E2E encrypted as part of the normal message.
4. **Recipient downloads the blob from the URL**, verifies the digest, and decrypts locally.

The homeserver never sees plaintext file content. Attachment URLs are scoped to authenticated users (the server checks a session token before serving the blob), so the URLs are not publicly accessible.

Storage strategy:

- **Small deployments:** attachments are stored on the homeserver's local filesystem and served directly by the Axum server.
- **Larger deployments:** the homeserver is configured with an S3-compatible endpoint (MinIO, Backblaze B2, AWS S3). The homeserver generates presigned upload/download URLs; clients transfer directly to/from object storage, keeping the homeserver out of the data path.

Attachment expiry follows message expiry: when a message is deleted (by timer or manually), the server deletes the corresponding blob. A background garbage-collection task catches orphaned blobs.

Media handling ships in **Stage 3** alongside the first mobile DM experience. The server attachment endpoint is added in Stage 2.

---

## Cross-server casual group encryption

Action-bound groups use libsignal's zkgroup on a single server, but cross-server casual groups (Stage 9) have no single credential issuer. The encryption approach for these groups is **Sender Keys with fan-out**, the same scheme Signal uses for its non-anonymous group chats:

1. Each group member generates a **Sender Key** — a symmetric ratcheting key — and distributes it to every other member via their existing pairwise Double Ratchet sessions.
2. When sending a group message, the sender encrypts once with their Sender Key (which ratchets forward). All recipients who hold that Sender Key can decrypt.
3. When a member is added, existing members send their current Sender Keys to the new member. When a member is removed, all remaining members rotate their Sender Keys and redistribute.

This scheme is efficient (encrypt once, not once per recipient) and does not require a central server to manage group state. It is well-suited for the small (< 50 member), ad-hoc groups the design envisions.

The tradeoff vs. MLS (RFC 9420): MLS provides stronger forward secrecy guarantees for large groups and more efficient member add/remove at scale, but it is significantly more complex to implement and its benefit is marginal for groups under 50 members. If real usage shows demand for larger cross-server groups, MLS can be adopted later without changing the substrate's external API — it would be a change inside the `crypto` crate's `groups` module.

The `crypto` crate's `groups` module (currently stubbed for Stage 4 zkgroup) will grow a `sender_keys` sub-module in Stage 9. The interface is designed now so that `app-core` doesn't need to know which encryption scheme a group uses — it calls `groups::encrypt` / `groups::decrypt` and the module dispatches based on group type.

---

## Open questions

1. **Federation transport details.** HTTP/2 + request signing is the plan, but the exact signing scheme and key rotation story for server-to-server auth needs to be specced out before implementing federation.

2. **Key transparency.** For high-assurance users, a key transparency log (similar to what Google's E2E and WhatsApp have deployed) would let users verify that the server isn't silently substituting keys. Significant implementation work; worth revisiting after the core ships.

3. **Rust core / UniFFI maturity.** UniFFI is solid but the async story across the FFI boundary is still evolving. Monitor and adopt improvements as they land.

---

## Staged build plan

Each stage produces a testable, shippable increment. Later stages depend on earlier ones; within a stage, components can be built in parallel. The order is chosen to get encrypted 1:1 messaging working as early as possible — that is the load-bearing core everything else rests on.

---

### Stage 1 — Rust cryptographic core

**What gets built:**

- Cargo workspace skeleton: separate crates for `crypto`, `store`, `net`, `server`, `relay`, and `app-core` (the UniFFI boundary crate)
- libsignal integration: X3DH prekey generation and key-bundle construction; Double Ratchet session initialization and message encrypt/decrypt
- SQLCipher-backed local store: schema for sessions, prekey material, and message queue; key derived from a placeholder secret (real secure-enclave integration comes in Stage 3)
- UniFFI interface definitions for the functions mobile UI will need; stub bindings generated but not yet wired to a real UI

**Why first:** Everything else in the system is downstream of correct crypto. Getting this isolated, tested, and reviewed before connecting it to a server or UI eliminates an entire class of integration bugs.

**Testing:**
- Unit tests in `crypto` crate covering encrypt → decrypt round-trips, ratchet advancement, and prekey consumption
- Property-based tests (using `proptest`) on session state: any sequence of sends and receives should leave the session in a consistent state
- All tests run in CI with `cargo-nextest`; `cargo audit` blocks on any advisory

---

### Stage 2 — Homeserver MVP

**What gets built:**

- PostgreSQL schema: accounts, DID registrations, prekey bundles, encrypted message queue, device sessions, push pseudonyms (stub only), rate-limit counters
- Axum HTTP server: account registration, device auth (session token issuance), prekey upload and fetch, message send (store-and-forward), WebSocket endpoint for real-time delivery
- Background task: expire queued messages and session tokens; vacuum prekeys below refill threshold
- `did:plc` stub: local DID creation and document storage (no PLC directory interaction yet — full DID portability is a federation-stage concern)
- Docker Compose file: homeserver + PostgreSQL for local development

**Why second:** The homeserver is the counterpart the crypto core needs to be useful. Having both lets us test a full end-to-end message path — encrypt on one device, relay through the server, decrypt on another — before writing any UI.

**Testing:**
- Integration tests: spin up a real Postgres instance (via `testcontainers-rs`), run account registration, prekey exchange, and a message round-trip
- sqlx compile-time query checks catch schema/query mismatches at build time
- HTTP endpoint fuzz testing with `cargo-fuzz` on the message ingestion path
- Load test: simulate 1,000 concurrent WebSocket connections, verify no memory growth or dropped messages

---

### Stage 3 — Mobile apps: 1:1 encrypted DMs

**What gets built:**

- iOS (Swift/SwiftUI) and Android (Kotlin/Jetpack Compose) app shells wired to the Rust core via UniFFI
- Secure key storage: SQLCipher database key held in iOS Secure Enclave / Android Keystore
- Account creation and onboarding: generate DID, generate prekeys, register with homeserver, display recovery key
- **Chats tab:** unified conversation list sorted by recency with unread indicators; 1:1 DM conversation view (text, images, files); message send/receive over WebSocket with offline queue drain on reconnect
- Placeholder **Calls** and **Network** tabs (visible but empty)
- Basic push notification wakeup: app wakes on ping and fetches new messages (push relay not yet live; development uses polling as a stand-in)

**Why third:** This is the first thing a real user can interact with. Getting Signal-quality 1:1 DMs on both platforms is the acceptance criterion for the first user-facing milestone.

**Testing:**
- XCTest (iOS) and Espresso (Android) UI tests covering the account creation flow and message send/receive
- Cross-platform interop test: iOS device sends an encrypted message, Android device decrypts it correctly, and vice versa — run against a real test homeserver in CI
- Manual: dog-food the app internally for day-to-day team communication starting here

---

### Stage 4 — Action-bound groups

**What gets built:**

- libsignal zkgroup / anonymous credentials on the homeserver: group creation, member credential issuance, membership proofs, sealed sender for group messages
- Group messaging in the Rust core and on the server; groups appear in the Chats tab alongside DMs
- Group admin surface in the app: create group, invite members, assign roles (admin / member), approve join requests
- **Message expiry:** timer stored in encrypted group state; clients delete on schedule; homeserver deletes its copy on the same schedule; timer cannot be extended by the server
- Announcement-only mode: enforced at the protocol level so non-admin members cannot post

**Why fourth:** Action-bound groups are the primary organizing primitive. They need the crypto foundation from Stage 1 and the server/mobile layers from Stages 2–3. Message expiry ships here, not later — retrofitting it is much harder than building it in from the start.

**Testing:**
- Multi-client integration test: 20 simulated clients join a group, exchange messages, verify each client decrypts correctly, and that no client can impersonate another (sealed sender)
- Expiry test: set a 10-second timer, verify server and clients both delete on schedule; verify the server cannot re-serve a deleted message
- Credential verification test: a client with a tampered credential is rejected by the server
- Announcement-only test: non-admin send attempt is rejected at the protocol level

---

### Stage 5 — Push notifications

**What gets built:**

- Push relay: standalone Rust service with a single table mapping `(pseudonym) → (device_token, platform)`; exposes two endpoints — one for clients to register a pseudonym, one for homeservers to send a wakeup
- APNs and FCM integration in the relay: sends a content-free wakeup (no subject, no body) on receipt of a homeserver ping
- Pseudonym rotation: clients rotate their pseudonym periodically (default: weekly); old pseudonym is valid for a grace period then deleted
- Homeserver integration: on message delivery, fire a push ping to the relay for each recipient device not currently on a live WebSocket

**Why fifth:** Push is required for the app to be practically usable (no one polls for messages). It's decoupled enough from groups and crypto that it can wait until basic messaging is stable, but it should land before federation so the relay design can be validated at single-server scale first.

**Testing:**
- Unit tests: relay correctly maps pseudonyms, rejects unknown pseudonyms, and rotates gracefully
- Integration test with APNs/FCM sandbox: send a wakeup, verify the device receives it and the payload contains no user-identifiable content
- Privacy test: confirm the relay's access log contains only pseudonyms and timestamps — no homeserver identity, no content
- Rotation test: old pseudonym stops receiving after the grace period; new pseudonym receives correctly

---

### Stage 6 — Project framework

**What gets built:**

- Project registration API on the homeserver: a Project declares its scopes (e.g., "read availability for users in this group," "send push to RSVP'd attendees") and receives a Project identity
- User-facing permission grant flow: when a user is added to a Project-managed group or opens a Project for the first time, the app presents the requested scopes for approval
- Bot first-class support: bots are created as Project-owned accounts with their own keys; they join groups as normal members; their presence is visible to all group members
- Project deep links: `actnet://project/<server>/<project-id>/<path>` scheme registered on iOS and Android; links open the correct Project view or navigate into a chat
- **Network tab** in the app: hierarchical list of servers → Projects; tap a Project to open its full-screen view
- Project host SDK (Rust crate + documentation): the interface Project developers use to interact with the substrate

**Why sixth:** The Project framework is what turns the substrate into a platform. It needs stable groups and push underneath it, but it does not need federation — Projects are single-server by default and the cross-server guest model can be layered on later. Building the SDK here, before the first-party Projects, means the first-party Projects are built the same way third-party developers will build.

**Testing:**
- Scope enforcement test: a Project attempts an operation outside its granted scopes; the server rejects it
- Bot visibility test: all group members can enumerate bots in their group; a bot cannot hide its presence
- Deep link test: tapping an `actnet://` link from outside the app opens the correct view on both platforms
- SDK smoke test: a minimal "hello world" Project (a bot that echoes messages) is built against the SDK and run against a test homeserver

---

### Stage 7 — First-party Projects

Built in this sub-order, since each one exercises more of the framework:

**7a — Channel Directory**

A server's browsable listing of open and semi-open groups. Simplest possible Project: reads group metadata, displays it, handles join and join-request flows. Validates that the Project framework's read scopes and the Network tab UX work end-to-end.

*Tests:* Open join completes in one tap and lands user in the correct group in the Chats tab. Application-required join triggers an admin notification. Unlisted groups do not appear.

**7b — Team Assignment**

Sign-up flow → team placement → encrypted team group. Exercises: scoped write to user profile, roster encryption under user keys, team-lead role with scoped permission to read roster. Swap request matching is a straightforward two-sided queue.

*Tests:* Sign-up creates account, user lands in correct team group. Team lead can read roster; members cannot. Bidirectional swap request matches correctly; unmatched request stays queued.

**7c — Action Day**

Map with admin-set markers + ephemeral location upload + announcement-only encrypted group. Key constraint: location records are deleted on a rolling window and purged completely when the action ends. The announcement group is a standard action-bound group in announcement-only mode.

*Tests:* Admin pushes a marker; all participants receive it and it appears on the map. Participant location is visible to others within the rolling window. After the rolling window, location is no longer returned by the server. After action end, all location records are gone. Non-admin send to the announcement group is rejected.

**7d — Q&A Bot**

A bot that answers questions by grounding responses in an admin-provided document corpus. Exercises the bot framework end-to-end: bot joins a group, receives messages, calls the LLM API with retrieved context, replies. The "don't speculate" constraint is enforced by the prompt; if the retrieval step returns nothing above a confidence threshold, the bot says so.

*Tests:* Bot answers a question that is clearly in the corpus. Bot declines to answer a question outside the corpus. Bot correctly cites its source. Bot appears in the group member list.

**7e — Collaborative Documents**

CRDT operations broadcast through an action-bound group channel; server stores encrypted operation blobs; periodic snapshots. New members sync from the latest snapshot.

*Tests:* Two clients concurrently edit the same document; both converge to the same state. New member joins after 100 operations, syncs from snapshot, and sees the correct document. Server cannot reconstruct the document from the blobs it stores.

**7f — Engagement Tracking**

Observer bots in action-bound groups surface high-engagement moments to an organizer dashboard. The bots are visible group members. The dashboard is accessible only to organizers with explicit access. Data is never aggregated across servers.

*Tests:* Bot is visible in group member list. Organizer sees the dashboard; non-organizer gets a 403. A flagged message links back to the correct conversation. No engagement data is transmitted to any external server.

---

### Stage 8 — Calls

**What gets built:**

- **1:1 calls:** WebRTC signaling over the existing WebSocket connection; STUN/TURN for NAT traversal; DTLS-SRTP media encryption; Calls tab on iOS and Android
- **Group calls:** LiveKit SFU integration; E2E encryption via WebRTC Insertable Streams (clients encrypt frames before they leave the device; SFU forwards ciphertext); group call UI (grid/speaker layout)
- **Large broadcasts:** LiveKit's broadcast mode exposed as a distinct UX from calls — unidirectional, scales to thousands of listeners

**Why eighth:** Calls are infrastructure-intensive and not required for the core organizing use case. They're also the one component that requires a second deployable service (LiveKit). Getting the substrate and all first-party Projects right first means this stage doesn't destabilize an already-working system.

**Testing:**
- 1:1 call: two clients connect, verify audio reaches both sides; simulate NAT by forcing TURN relay and verify call still works
- Group call encryption test: capture SFU traffic and verify frames are encrypted (no decodable video/audio) at the SFU — the SFU should be blind to content
- Large broadcast: spin up a LiveKit instance, simulate 500 listener connections, verify the homeserver's CPU and memory are not affected (all load is on LiveKit)
- Calls tab UI tests: incoming call notification, accept/decline, in-call controls

---

### Stage 9 — Federation

**What gets built:**

- Server-to-server transport: HTTPS with Ed25519 request signing; receiving server verifies the signature against the sending server's DID document before processing
- Full `did:plc` integration: DID creation, update, and deactivation synced with the PLC directory; DID resolution for remote users
- Cross-server DM delivery: Alice on server A can DM Bob on server B; A looks up B's DID, finds its homeserver, and relays the encrypted message
- Cross-server casual groups: ad-hoc encrypted group chats spanning homeservers (peer-managed, no Project required)
- Guest participation in action-bound groups: homeserver A issues a guest credential for a user, homeserver B's Project accepts it and grants scoped access
- Selective federation: homeserver admins configure an allowlist of servers they federate with

**Why ninth:** Federation is a meaningful differentiator for resilience and multi-org organizing, but the system is fully usable without it. Every first-party Project works on a single homeserver. Deferring federation means the complexity of cross-server delivery, DID portability, and guest credentials doesn't slow down the path to a working, deployable product. It also means the federation design can be informed by real usage patterns from the single-server deployment rather than guessed at upfront.

**Testing:**
- Two-homeserver integration harness in CI (two Docker Compose stacks, networked together): Alice on server A DMs Bob on server B; verify end-to-end encryption is preserved across the relay boundary
- Guest credential test: user on server A joins an action-bound group on server B as a guest; verify scoped access (can post, cannot see full member list)
- Federation fault injection: server B is unreachable; server A queues the message and retries; verify no message loss and no timeout leak to the user
- Selective federation test: server A rejects a federation request from a server not on its allowlist

---

### Stage 10 — Security hardening and launch readiness

This stage runs in parallel with Stages 7–9 and extends after them.

**What gets built / done:**

- **Reproducible builds:** iOS, Android, and server builds are made reproducible; documented verification steps published alongside each release
- **CI hardening:** `cargo audit` already runs; add `cargo deny` for license and duplicate-dependency checks; add scheduled supply-chain diff alerts for dependency updates
- **Third-party security audit:** engage an external firm to audit the cryptographic core (`crypto` crate + libsignal integration), the homeserver (auth, message handling), and the mobile key storage implementation; publish the full report
- **Threat model review:** walk through every stage's output against the threat model in the design document; document any new attack surfaces introduced and the mitigations in place
- **Operational security for the relay:** the push relay is a metadata target; audit its logging, ensure no cross-server linkage is retained, and document its operational trust assumptions

**Testing / acceptance criteria:**

- Audit report published with no unmitigated critical or high findings
- Reproducible build verification passes for a fresh build from source on a clean machine
- All CI checks pass on a dependency update PR within 24 hours of a new advisory
- A "red team" exercise (internal or external) attempts to: extract message content from a seized homeserver, link a user's activity across servers via push metadata, impersonate a group member. Each attempt should fail in a documented way.

---

### Summary table

| Stage | Deliverable | Key acceptance criterion |
|---|---|---|
| 1 | Rust crypto core | Encrypt → decrypt round-trip passes; ratchet property tests pass |
| 2 | Homeserver MVP | 1,000 concurrent WebSocket connections; message round-trip under load |
| 3 | Mobile: 1:1 DMs | iOS ↔ Android interop test passes; dog-food begins |
| 4 | Action-bound groups | 20-client group test; expiry verified server- and client-side |
| 5 | Push notifications | Content-free wakeup confirmed; relay log contains no user identity |
| 6 | Project framework | Scope enforcement; bot visibility; deep links |
| 7 | First-party Projects | Each Project's acceptance tests (see above) |
| 8 | Calls | SFU blindness verified; group call E2E encryption confirmed |
| 9 | Federation | Cross-server DM round-trip; fault injection passes |
| 10 | Hardening + audit | Audit report published; reproducible builds verified; red team exercises pass |
