# BitChat Mesh Fallback Transport

> **Status: Design document only — optional, opportunistic feature.**
> Implement only once core messaging, groups, and federation milestones are stable.
> See `docs/02-todos-deferred.md` for the milestone checklist.

---

## 1. Overview and Motivation

actnet's threat model names homeserver seizure and disrupted connectivity as primary risks. The current transport — HTTP + WebSocket over TLS to a homeserver — has no fallback if the server is unreachable. This document specifies how the [BitChat](https://github.com/permissionlesstech/bitchat) mesh protocol can serve as a seamless, automatic fallback transport layer.

**What BitChat provides:**
- Bluetooth LE mesh networking with multi-hop relay (up to 7 hops)
- WiFi Direct (MultipeerConnectivity on iOS) for higher-bandwidth local delivery
- Bloom-filter deduplication and TTL-based loop prevention
- Fully offline-capable; no accounts or central servers required
- Public domain (Unlicense) — can be freely adapted
- 98.5% Swift; iOS/macOS native

**What actnet adds on top:**
- actnet uses BitChat purely as a **transport layer**. The existing Signal Double Ratchet ciphertext is wrapped in BitchatPacket envelopes. Relay nodes see only opaque encrypted bytes — the same security guarantee as the homeserver path.
- No new encryption layer: Signal E2E is preserved end-to-end regardless of transport.

**Goal:** zero UX interruption. A non-intrusive banner ("Using mesh network") is the only visible change when fallback activates. Messages appear in the same conversation view, with a small mesh indicator icon next to their timestamp.

---

## 2. Design Decisions

### 2.1 Identity

BitChat identifies nodes by a Curve25519 Noise static key. Rather than generating a separate keypair, actnet derives it deterministically from the existing Ed25519 identity key:

```
HKDF-SHA256(
  ikm  = ed25519_private_key_bytes,
  salt = b"actnet-mesh-v1",
  info = b"noise-static"
) → 32-byte Curve25519 scalar
```

This is implemented as `IdentityKeyPair::derive_mesh_noise_keypair()` in `core/crates/crypto/src/identity.rs`. The resulting scalar is passed to Swift as `Data` and loaded via `CryptoKit.Curve25519.KeyAgreement.PrivateKey(rawRepresentation:)`.

**Threat model:** mesh peers learn your DID fingerprint. This is no worse than the homeserver already knowing your DID — a deliberate design choice consistent with the existing threat model.

### 2.2 Activation

Fallback activates **fully automatically** when the homeserver is unreachable. A 10-second grace period (`Degraded` state) absorbs transient network hiccups before BLE scanning starts; no false-positive banners. When the homeserver becomes reachable again, the app switches back automatically and shows a 2-second "Connected to server" toast.

### 2.3 Message Scope

**In scope (initial implementation):**
- 1:1 DMs (PreKey and Whisper messages)
- Delivery and read receipts
- Prekey bundle exchange (enabling new Signal sessions without the homeserver)

**Deferred:**
- Group messages (depends on Sender Keys, actnet Stage 4)
- Nostr relay network (internet-based third-tier fallback)

---

## 3. Connectivity State Machine

```
Online ──(WS drop or HTTP send failure)──► Degraded
                                               │
                                       10s deadline expires
                                               │
                                               ▼
                                            Offline ◄── mesh transport active
                                               │
                                       homeserver probe succeeds (every 30s)
                                               │
                                               ▼
                                            Online
```

**State definitions:**

| State | Meaning | UI |
|-------|---------|-----|
| `Online` | WS connected, HTTP sends succeeding | no indicator |
| `Degraded` | WS dropped or HTTP failed; within grace period | no indicator (silent) |
| `Offline` | Grace period elapsed; BLE/WiFi Direct scanning active | blue-gray mesh banner |

**Implementation:** `ConnectivityMonitor` in `core/crates/app-core/src/connectivity.rs` (new file). Runs as a background task spawned once in `AppCore`. Shares `Arc<Mutex<ConnectivityState>>` with `TransportDispatcher`.

**New FFI methods on `AppCore`:**
```rust
#[uniffi::export]
pub fn connectivity_state(&self) -> ConnectivityStateFfi

#[uniffi::export]
pub fn set_connectivity_override(&self, forced: Option<ConnectivityStateFfi>)
// ^ dev/QA: force-trigger Offline mode without killing the network

#[uniffi::export]
pub fn inject_mesh_message(
    &self,
    ciphertext: Vec<u8>,
    sender_did: String,
    sender_device_id: u32,
    message_kind: i16,
) -> Result<(), AppErrorFfi>
// ^ called from Swift MeshTransportManager when a mesh DM arrives
//   feeds an mpsc channel merged into receive_messages_ws_async via tokio::select!
```

`ConnectivityStateFfi` is a `#[uniffi::Enum]` with variants `Online`, `Degraded`, `Offline`.

`AppState.swift` polls connectivity state from the existing `messageWsLoop`:
```swift
@Published var connectivityState: ConnectivityStateFfi = .online

// inside messageWsLoop, at each iteration:
let state = core.connectivityState()
await MainActor.run { self.connectivityState = state }
```

---

## 4. Transport Abstraction

New file: `core/crates/app-core/src/transport.rs`

```rust
pub struct MeshOutbound {
    pub recipient_did: String,
    pub recipient_device_id: u32,
    pub ciphertext: Vec<u8>,
    pub message_kind: i16,
}

pub struct TransportDispatcher {
    client: net::Client,
    mesh_tx: Option<mpsc::Sender<MeshOutbound>>,
    connectivity: Arc<Mutex<ConnectivityState>>,
    backlog: Vec<MeshOutbound>,  // drained on Offline→Online
}
```

Routing logic in `TransportDispatcher::send`:
- `Online` → HTTP `client.send_messages(...)`
- `Degraded` → HTTP send; also enqueue to mesh as redundant path (best-effort)
- `Offline` → mesh only; on failure, push to `backlog`

On `Offline → Online` transition, `backlog` is drained via HTTP.

`AppCoreInner::send_dm` calls `self.dispatcher.send(...)` instead of `self.client.send_messages(...)` directly. All other uses of `self.client` (prekey fetch, auth, registration) remain HTTP-only and are not part of this abstraction.

---

## 5. BitchatPacket Wrapping

BitChat's binary packet format (13-byte fixed header + variable payload) is reused as-is. actnet defines four new type bytes within the BitChat type space:

| Type byte | Name | Purpose |
|-----------|------|---------|
| `0x10` | `ACTNET_DM` | Encrypted DM (Signal Double Ratchet ciphertext) |
| `0x11` | `ACTNET_ANNOUNCE` | DID → fingerprint identity advertisement |
| `0x12` | `ACTNET_PREKEY_REQUEST` | Request a peer's prekey bundle |
| `0x13` | `ACTNET_PREKEY_RESPONSE` | Deliver a prekey bundle |

### 5.1 ACTNET_DM payload

```
[recipient_fingerprint: 32B]   SHA-256(recipient Noise static pubkey)
[sender_did_len: 2B]           big-endian u16
[sender_did: variable]         UTF-8 DID string
[device_id: 4B]                big-endian u32
[message_kind: 1B]             0 = PreKey, 1 = Whisper
[nonce: 16B]                   deduplication nonce (OsRng)
[ciphertext_len: 4B]           big-endian u32
[ciphertext: variable]         Signal Double Ratchet ciphertext (opaque)
```

Overhead per message: ~75 bytes + DID length. Signal ciphertexts are typically 200–1500 bytes; total fits comfortably in BLE L2CAP CoC fragmentation.

**TTL values:**
- DMs: TTL = 7 (maximum BitChat hop count)
- Receipts: TTL = 3 (conserve bandwidth)
- Announces: TTL = 3

### 5.2 ACTNET_ANNOUNCE payload

```
[announce_version: 1B]         0x01
[noise_static_pubkey: 32B]     Curve25519 public key
[did_len: 2B]
[did: variable]
[identity_key: 33B]            compressed Signal Ed25519 identity pubkey
[device_id: 4B]
[timestamp_ms: 8B]             big-endian unix millis
[signature: 64B]               Ed25519 sig over all preceding fields
```

The signature is computed with the sender's actnet Ed25519 identity private key. Receivers verify it before trusting the DID → fingerprint binding.

### 5.3 ACTNET_PREKEY_REQUEST payload

```
[requester_fingerprint: 32B]
[requester_did_len: 2B]
[requester_did: variable]
[target_did_len: 2B]
[target_did: variable]
[nonce: 16B]                   echoed in response for correlation
```

### 5.4 ACTNET_PREKEY_RESPONSE payload

```
[nonce: 16B]                   echo of request nonce
[bundle_version: 1B]           0x01
[identity_key: 33B]            Signal identity pubkey (compressed)
[registration_id: 4B]
[device_id: 4B]
[signed_prekey_id: 4B]
[signed_prekey_public: 33B]
[signed_prekey_signature: 64B]
[has_one_time_prekey: 1B]
[one_time_prekey_id: 4B]       present if has_one_time_prekey = 1
[one_time_prekey_public: 33B]  present if has_one_time_prekey = 1
[kyber_prekey_id: 4B]
[kyber_prekey_public: 1568B]   ML-KEM-1024 public key
[kyber_prekey_signature: 64B]
[outer_signature: 64B]         Ed25519 sig over all preceding fields
```

Total response: ~1900 bytes. BLE L2CAP CoC handles fragmentation transparently.

---

## 6. BLE / WiFi Direct Integration (iOS)

### 6.1 New Swift files

All files under `mobile/ios/Actnet/Sources/Mesh/`:

| File | Responsibility |
|------|----------------|
| `MeshTransportManager.swift` | Top-level coordinator. Owns `BLEMeshTransport` and `WiFiDirectTransport`. Routes incoming `ACTNET_DM` packets to `AppState.handleMeshInbound()`. Exposes `send(...)` for outbound messages. |
| `BLEMeshTransport.swift` | CoreBluetooth `CBCentralManager` + `CBPeripheralManager`. L2CAP CoC channels for large payloads. Multi-hop relay loop with bloom filter. Adapted from BitChat's `BluetoothMeshManager` (public domain). |
| `WiFiDirectTransport.swift` | `MultipeerConnectivity` `MCSession`. Higher bandwidth, useful when both devices are on the same WiFi or in direct proximity. Adapted from BitChat's `MultipeerConnectivityManager`. |
| `BitchatPacket.swift` | Binary serialization/deserialization for the BitchatPacket format and all actnet payload types. |
| `MeshBloomFilter.swift` | Per-node bloom filter for deduplication. Prevents relay loops. Adapted from BitChat. |
| `MeshFingerprintStore.swift` | DID → 32-byte fingerprint mapping. In-memory with UserDefaults persistence across app restarts. |
| `MeshAnnounceManager.swift` | Broadcasts `ACTNET_ANNOUNCE` on scan start and every 5 minutes. Processes incoming announces. Handles `ACTNET_PREKEY_REQUEST/RESPONSE`. |

### 6.2 Inbound routing (Swift → Rust)

```swift
// AppState.swift
func handleMeshInbound(_ packet: BitchatPacket) {
    guard let payload = packet.actnetDmPayload() else { return }
    guard let core = cores[localAccountId] else { return }
    Task.detached {
        try? core.injectMeshMessage(
            ciphertext: payload.ciphertext,
            senderDid: payload.senderDid,
            senderDeviceId: payload.deviceId,
            messageKind: payload.messageKind
        )
    }
}
```

### 6.3 Outbound routing (Swift → mesh)

```swift
// AppState.sendMessage, when connectivityState == .offline:
guard let fingerprint = meshFingerprintStore.fingerprint(forDid: recipientDid) else {
    // surface "contact not reachable on mesh" error
    return
}
meshTransportManager.send(
    recipientFingerprint: fingerprint,
    ciphertext: encryptedBytes,
    senderDid: myDid,
    senderDeviceId: myDeviceId,
    messageKind: kind
)
```

### 6.4 Store table for fingerprint persistence

Append to `MIGRATIONS` in `core/crates/store/src/schema.rs`:

```sql
CREATE TABLE IF NOT EXISTS mesh_fingerprints (
    did          TEXT    NOT NULL PRIMARY KEY,
    fingerprint  BLOB    NOT NULL,
    identity_key BLOB    NOT NULL,
    last_seen_ms INTEGER NOT NULL
)
```

---

## 7. Contact Discovery Over Mesh

### Problem

Before routing a ciphertext to a peer over mesh, actnet needs the peer's 32-byte BitChat fingerprint (SHA-256 of their Noise static pubkey). This mapping isn't stored on the homeserver.

### Protocol

`MeshAnnounceManager` broadcasts an `ACTNET_ANNOUNCE` packet:
- On BLE/WiFi scan start
- Every 5 minutes while mesh is active
- Immediately on receiving a peer's announce (ensures mutual discovery)

**On receipt of an announce:**
1. Verify Ed25519 signature against the claimed `identity_key`.
2. Verify `identity_key` matches the DID if already known locally; otherwise accept on first-seen.
3. Compute `fingerprint = SHA-256(noise_static_pubkey)` and store in `MeshFingerprintStore`.

**Privacy:** announces only fire when `connectivityState == .offline`. When the homeserver is reachable, no BLE identity broadcast occurs.

**Unknown contacts:** if a contact's fingerprint has never been seen, `send` fails with "Contact not reachable on mesh" — the message is held until the contact appears on the mesh.

---

## 8. Prekey Distribution Over Mesh

New Signal sessions (PreKey messages) require a prekey bundle normally fetched from the homeserver. When offline, this exchange moves to the mesh.

### Flow

1. `AppCoreInner::send_dm` detects no session exists for the recipient → `TransportDispatcher` broadcasts `ACTNET_PREKEY_REQUEST` (target DID, requester fingerprint, 16-byte nonce).
2. The outbound message is held in a `HashMap<nonce, PendingMessage>` with a 30-second timeout.
3. Target peer receives request → `MeshAnnounceManager` calls `core.build_prekey_bundle_for_mesh()` → broadcasts `ACTNET_PREKEY_RESPONSE`.
4. Requester receives response → Rust core calls existing X3DH session init with the bundle (same path as homeserver prekey fetch in `AppCoreInner::send_dm`).
5. Pending message is retried via mesh.
6. On timeout: error surfaced as "Contact not reachable on mesh".

### New FFI method

```rust
#[uniffi::export]
pub fn build_prekey_bundle_for_mesh(&self) -> Result<MeshPrekeyBundleFfi, AppErrorFfi>
```

Reads local store: signed prekey, pops one one-time prekey (marks consumed), packs Kyber prekey. Returns as a `#[uniffi::Record]`. Lives in `core/crates/app-core/src/lib.rs` alongside existing FFI methods.

---

## 9. UX

### 9.1 MeshModeBanner

New file: `mobile/ios/Actnet/Sources/Views/Common/MeshModeBanner.swift`

- Subdued blue-gray strip: `.systemBlue.opacity(0.12)` background
- System icon: `antenna.radiowaves.left.and.right`
- Text: "Using mesh network" (`.subheadline`)
- No close button — auto-dismisses when connectivity returns to `Online`
- Shown as `.overlay(alignment: .top)` in both `ChatsView` and `ConversationView`

In `ChatsView.swift`, the existing recovery key banner overlay is extended:
```swift
.overlay(alignment: .top) {
    VStack(spacing: 0) {
        if appState.connectivityState == .offline {
            MeshModeBanner()
        } else if !appState.hasRecoveryKey {
            RecoveryKeyBanner()
        }
    }
}
```

`Degraded` state: no UI change. The 10-second grace period is invisible.

### 9.2 Per-message transport indicator

Extend `Message.swift`:
```swift
var transport: MessageTransport?

enum MessageTransport {
    case homeserver
    case mesh
}
```

In `MessageBubble.swift`, for `transport == .mesh` messages, add a small icon next to the timestamp:
- System image: `dot.radiowaves.up.forward`
- Font: `.caption2`
- Color: `.secondary`

This is intentionally subtle — it informs without alarming.

### 9.3 Reconnect toast

In `ChatsView.swift`:
```swift
@State private var showReconnectedToast = false
```

Fires on `Offline → Online` transition (observed via `onChange(of: appState.connectivityState)`). Auto-dismisses after 2 seconds. Text: "Connected to server".

---

## 10. iOS Entitlements and Info.plist

### Switch to explicit Info.plist

`UIBackgroundModes` is an array and cannot be set via `GENERATE_INFOPLIST_FILE` build settings. Switch in `project.yml`:

```yaml
# Remove:
GENERATE_INFOPLIST_FILE: YES
# Add:
INFOPLIST_FILE: Sources/Info.plist
```

Create `mobile/ios/Actnet/Sources/Info.plist` with all existing auto-generated keys plus:

```xml
<key>NSBluetoothAlwaysUsageDescription</key>
<string>actnet uses Bluetooth to send and receive messages when the server is unavailable.</string>
<key>NSLocalNetworkUsageDescription</key>
<string>actnet uses the local network to send messages when the server is unavailable.</string>
<key>UIBackgroundModes</key>
<array>
    <string>bluetooth-central</string>
    <string>bluetooth-peripheral</string>
</array>
```

**No special entitlements required.** CoreBluetooth peripheral mode and `MultipeerConnectivity` both work with a standard App Store provisioning profile.

Note: `MultipeerConnectivity` connections are suspended when the app is backgrounded (no background mode for it). This is acceptable — MC is supplemental to BLE and only active while the app is foregrounded.

---

## 11. Staged Milestones

| Stage | Deliverables | Est. effort |
|-------|-------------|------------|
| **M1** | Rust: `ConnectivityMonitor`, `TransportDispatcher`, new FFI methods (`connectivity_state`, `inject_mesh_message`, `build_prekey_bundle_for_mesh`), `mesh_fingerprints` schema migration | 1–2 weeks |
| **M2** | Swift: `BitchatPacket` serialization, `MeshBloomFilter`, `MeshFingerprintStore`, `MeshAnnounceManager` announce signing/verification (no BLE yet — tested in-process) | 1 week |
| **M3** | Swift: `BLEMeshTransport` (CoreBluetooth), `MeshTransportManager` coordinator, `AppState` wiring, `project.yml` + `Info.plist` updates. Two-device E2E test: DM delivered over BLE with homeserver down. | 2–3 weeks |
| **M4** | Swift: `WiFiDirectTransport` (MultipeerConnectivity). Test: DM via MC with homeserver down. | 1 week |
| **M5** | UX: `MeshModeBanner`, per-message transport icon, reconnect toast. Prekey request/response over mesh (`ACTNET_PREKEY_REQUEST/RESPONSE`) — enables new sessions without homeserver. | 1–2 weeks |
| **M6** | Reliability: backlog drain on `Offline → Online`, nonce deduplication in `inject_mesh_message`, delivery/read receipts via `TransportDispatcher`, 72-hour TTL for mesh messages (background cleanup in `AppCore`). | 1 week |

---

## 12. Deferred Items

### Nostr relay (M7+)

When both homeserver AND local BLE/WiFi Direct mesh are unavailable (internet exists but homeserver is seized; contacts are geographically dispersed), Nostr relay network provides a third transport tier.

Requires:
- Nostr NIP-17 (sealed DM) client in the Rust core
- A `NostrFallback` state in `ConnectivityMonitor`
- Nostr relay address(es) configurable per-account

Deferred because: the primary use case (server seizure during a local action) is fully served by BLE mesh; Nostr adds complexity without benefiting nearby users.

### Group messages over mesh

Once Sender Keys (actnet Stage 4) are implemented, group ciphertexts can be wrapped in `ACTNET_GROUP_DM` (type byte `0x14`) using the same scheme as DMs. Deferred until groups ship.

### Android

The Rust core changes (M1) are platform-agnostic and will work on Android without modification. The BLE/WiFi Direct transport will need a separate Kotlin implementation using Android's `BluetoothLeScanner`, `BluetoothLeAdvertiser`, and `WifiP2pManager` APIs. Deferred.

### Multi-hop prekey rate-limiting

Prekey responses over mesh can be consumed by malicious relay nodes (repeated request injection depletes one-time prekey pools). Rate-limiting logic in `MeshAnnounceManager` (max N responses per requester fingerprint per time window) is deferred to M7+.

### Noise_XX channel encryption for relay nodes

BitChat uses `Noise_XX_25519_ChaChaPoly_SHA256` for node-to-node BLE channel encryption, preventing relay nodes from reading packet headers. In actnet's integration, Signal E2E encryption already protects message content end-to-end. Channel-level Noise handshake is a defence-in-depth improvement (protects metadata like sender DID from relay nodes) but is deferred given the complexity.
