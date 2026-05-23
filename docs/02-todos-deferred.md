# Deferred TODOs

## Dev Infra
- Make it super easy to launch Postgres, the main server & relevant Projects all at once in dev

## Chatbot Project (finishing touches)
- ~~Bot display name: conversations currently show raw DID instead of a friendly name~~ — implemented
- ~~Bot account marking: flag someplace to distinguish bot accounts in member lists~~ — implemented

## Mobile app
- Recovery key UI: setup and backup flows (banner currently always shows, hardcoded false)
- Scroll-position-based read marking (see docs/31-read-tracking.md, Stage B)
- Delivery receipts — auto-send on message receive (see docs/31-read-tracking.md, Stage D)
- Read receipt user preference toggle (send_read_receipts setting)
- Scroll position: remove invisible "bottom" anchor hack in ConversationView (Color.clear spacer) when scroll position saving is implemented

## Auth
- ~~Identity key signature verification on `POST /v1/auth/token`~~ — implemented: two-step challenge-response flow via `POST /v1/auth/challenge` + `POST /v1/auth/token` with Ed25519 nonce signature.

## Crypto / protocol
- Kyber prekey pool: upload one-time Kyber prekeys with server-side atomic consumption (like EC one-time prekeys), keep one last-resort key. Currently only a single last-resort key is used.
- Protobuf message envelope: plaintext is raw bytes, design calls for ContentMessage protobuf (proto/content.proto)
- DB encryption key from Secure Enclave instead of hardcoded "dev-placeholder-key"

## Server
- WebSocket request/response framing: tunnel HTTP-style request/response pairs over the WebSocket (like Signal does), with request IDs and correlated responses. Move message sends and acks onto the WS transport, replacing the current split of HTTP sends + WS acks. This gives persistent-connection benefits while keeping clear success/failure semantics per operation.
- Message expiry: background task to delete expired messages, configurable per-group/DM
- DID document resolution endpoint (GET /.well-known/did/:did)

## Big milestones (not yet started)
- Groups: action-bound (zkgroup) and cross-server casual (Sender Keys)
- Invite links & onboarding: QR codes, deep links, auto-enrollment into groups/Projects
- Push notifications (see Push Notifications section below)
- Projects framework: SDK, scoped bot permissions, JS bridge for webviews
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Calls: voice and video (VoIP)
- Public profiles: client-owned profile blobs (display name, avatar, bio) pushed to servers
- Multi-account support in mobile app

## Mesh Fallback / BitChat protocol (optional — implement only after core features are stable)

See `docs/32-bitchat-fallback.md` for the full design. BLE + WiFi Direct mesh transport as a
seamless automatic fallback when the homeserver is unreachable. Signal E2E encryption is
preserved; BitChat is used as a transport layer only.

### M1 — Rust transport abstraction
- [ ] `ConnectivityMonitor` state machine (`core/crates/app-core/src/connectivity.rs`)
- [ ] `TransportDispatcher` with homeserver/mesh routing (`core/crates/app-core/src/transport.rs`)
- [ ] FFI: `connectivity_state()`, `inject_mesh_message()`, `build_prekey_bundle_for_mesh()`
- [ ] Store: `mesh_fingerprints` table migration

### M2 — Swift packet layer
- [ ] `BitchatPacket` serialization (`ACTNET_DM`, `ACTNET_ANNOUNCE`, `ACTNET_PREKEY_*`)
- [ ] `MeshBloomFilter`, `MeshFingerprintStore`
- [ ] `MeshAnnounceManager` with Ed25519 announce signing and verification

### M3 — BLE transport
- [ ] `BLEMeshTransport` (CoreBluetooth; adapted from BitChat open-source)
- [ ] `MeshTransportManager` top-level coordinator
- [ ] `AppState`: `connectivityState` `@Published`, mesh wiring in `messageWsLoop`
- [ ] Switch to explicit `Info.plist`; add Bluetooth usage strings and `UIBackgroundModes`

### M4 — WiFi Direct transport
- [ ] `WiFiDirectTransport` (MultipeerConnectivity)

### M5 — UX and prekey over mesh
- [ ] `MeshModeBanner`, per-message transport indicator, reconnect toast
- [ ] Prekey request/response over mesh (enables new sessions without homeserver)

### M6 — Reliability
- [ ] Backlog drain on `Offline → Online` transition
- [ ] Nonce deduplication in `inject_mesh_message`
- [ ] Delivery/read receipts routed through `TransportDispatcher`
- [ ] 72-hour TTL for mesh-only messages; background cleanup task

### Deferred mesh items
- Nostr relay as third-tier fallback (M7+; for dispersed users when internet exists but homeserver is seized)
- Group messages over mesh (after Sender Keys land in Stage 4)
- Android BLE/WiFi Direct transport
- Multi-hop prekey rate-limiting (prevent one-time prekey pool depletion by relay attackers)
- Noise_XX channel encryption for relay node authentication (defence-in-depth)

## Push Notifications

### 1. Push relay service (`core/crates/relay/`)
- [ ] DB table: `(pseudonym) → (device_token, platform, registered_at)`
- [ ] Client endpoint: register/update/delete pseudonym-to-token mapping
- [ ] Homeserver endpoint: accept wakeup-by-pseudonym, fire content-free push to APNs/FCM
- [ ] Pseudonym rotation: grace period (~1 week) where old pseudonym still works
- [ ] APNs integration (content-free wakeup payload)
- [ ] FCM integration (content-free wakeup payload)

### 2. Server integration
- [ ] On message delivery to offline device, look up push pseudonym and ping relay
- [ ] Hook into existing WebSocket connection tracking to determine online/offline
- [ ] Server config: relay URL

### 3. Mobile client (iOS first, then Android)
- [ ] Request push permission during signup
- [ ] Register device token with APNs/FCM
- [ ] Register per-(user, server) pseudonym with relay on account creation
- [ ] On wakeup: connect WebSocket, fetch queued messages
- [ ] Periodic pseudonym rotation (default weekly)
- [ ] Opt-out setting for high-risk users (poll-only mode)

### 4. Testing & privacy
- [ ] Verify relay payloads contain zero user-identifiable content
- [ ] Verify relay logs contain only pseudonyms + timestamps
- [ ] Pseudonym rotation grace period test
- [ ] APNs/FCM sandbox integration test
