# BitChat Mesh Fallback Transport

> **Status: Design document only — optional, opportunistic feature.**
> Implement only once core messaging, groups, and federation milestones are stable.
> See `docs/02-todos-deferred.md` for the milestone checklist.

---

## 1. Overview and Motivation

avalanche's threat model names homeserver seizure and disrupted connectivity as primary risks. The default transport — HTTP + WebSocket over TLS to a homeserver — has no fallback if the server is unreachable. This document specifies how the [BitChat](https://github.com/permissionlesstech/bitchat) mesh protocol can serve as a fallback transport layer for DMs, group messages, and a broadcast channel for local communication.

**What BitChat's protocol designs for:**
- Bluetooth LE mesh networking with multi-hop relay (up to 7 hops)
- Noise e2e encryption protocol handshake between peers for transport-layer encryption
- A reasonably efficient flood-based mesh chat protocol (Bloom-filter deduplication and TTL-based loop prevention)
- Fully offline-capable; no accounts or central servers required
- Public domain (Unlicense) — can be freely adapted
- Swift + iOS native.

**What avalanche adds on top:**
- For DMs and group chats: the existing Signal ciphertext is flooded through the BitChat mesh. Relay nodes see only opaque encrypted bytes — the same security guarantee as the homeserver path. No new encryption layer needed.
- For broadcast: a plaintext local mesh channel, visible to all mesh participants. Useful for open coordination when E2E encryption isn't needed.

**Constraints:**
- Mesh DMs and group messages only work with existing Signal sessions / group memberships established via the homeserver. New session establishment requires prekey exchange, which depends on the homeserver.
- Mesh relay depends on iOS background BLE behavior. See section 6 for details.

---

## 2. Core Design Decisions

### 2.1 Flooding, not routing

BitChat uses a flood protocol: every message is broadcast to all connected peers, who re-broadcast it (decrementing TTL, deduplicating via bloom filter). Recipients check if a message is addressed to them; relay nodes forward blindly.

avalanche adopts this model directly. There's no requirement to know whether a recipient is reachable before sending. Messages are flooded into the mesh and either arrive or don't. This matches BitChat's design and avoids the fragile liveness-tracking problem.

### 2.2 Identity

BitChat identifies nodes by a SHA-256 fingerprint of a Curve25519 public key. avalanche derives this key deterministically from the existing Ed25519 identity key so that no separate keypair needs to be managed:

```
HKDF-SHA256(
  ikm  = ed25519_private_key_bytes,
  salt = b"actnet-mesh-v1",
  info = b"noise-static"
) → 32-byte Curve25519 scalar
```

Note: BitChat uses a Noise_XX handshake for transport-layer encryption between directly connected BLE peers. avalanche does not use this in the initial implementation — DM/Group payloads are already Signal-encrypted end-to-end, and broadcast messages are intentionally plaintext. Noise_XX is listed as a deferred item for metadata protection (hiding sender DID from relay nodes).

### 2.3 Bluetooth mode is user-activated

Mesh mode is **not automatic by default**. It is an explicit feature the user turns on:

1. When the homeserver becomes unreachable, a banner appears: "Disconnected from server. Tap to enable Bluetooth mesh"
2. The user toggles on the mesh (or sets a setting which means it auto-activates when server disconnected). This starts BLE scanning and advertising.
3. Mesh mode stays on until either:
   - The user manually turns it off, or
   - The homeserver is reachable again and 4 hours elapse since the last message sent or received via mesh (auto-off to save battery and stop BLE advertising)

This avoids surprise BLE activity and gives users explicit control over when they're broadcasting their presence over Bluetooth.

### 2.4 Three message types

**DMs:** Signal Double Ratchet ciphertext, flooded through the mesh. Addressed by a short derived recipient tag (see section 2.5). Only works with existing Signal sessions. Same E2E security as the homeserver path.

**Group messages:** Sender Key ciphertext, flooded through the mesh. Addressed by a short derived group tag (see section 2.5). Only works with groups you're already a member of. **This covers both group types** — action-bound (zkgroup) and cross-server casual — because both use Sender Keys for message content; the difference between the two types is their server-side authorization layer (credentials + endorsements vs nothing), which is irrelevant on mesh. For action-bound groups specifically, what does *not* survive mesh is the zkgroup machinery: state mutations (add/remove member, role changes), credential issuance, and send endorsements — all of those require the homeserver. Steady-state messaging is unaffected. See `docs/03-groups.md` §7 for the zkgroup-side framing.

**Broadcast:** Plaintext messages visible to everyone on the mesh. A "local mesh" channel that appears alongside your conversations while in mesh mode. Useful for open coordination when E2E encryption isn't needed or when you need to reach people you don't have Signal sessions with.

### 2.5 Addressing: derived tags

Both DMs and group messages use short **tags** (8 bytes) derived via HMAC so that recipients can quickly identify messages for them without exposing stable identifiers to relay nodes. Tags rotate daily to limit traffic correlation.

**DM recipient tag:**
```
recipient_tag = HMAC-SHA256(recipient_identity_key, "mesh-dm-tag" || epoch)[:8]
```
The sender computes this from the recipient's identity key (already known from the existing Signal session). The recipient precomputes their own tag for the current epoch and checks incoming packets against it.

**Group tag:**
```
group_tag = HMAC-SHA256(sender_key, "mesh-group-tag" || epoch)[:8]
```
Each group member precomputes tags for all their groups and checks incoming mesh packets against a hash table — a fast lookup regardless of group count.

The `epoch` is `unix_days` in both cases.

**Metadata properties:**
- Relay nodes see tags but cannot link them to identities or groups without the key material.
- Within one epoch (one day), relay nodes can correlate messages to the same recipient or group and observe traffic volume and timing.
- Across epochs, tags change, limiting long-term traffic analysis.

**Known threat: forced mesh activation.** An adversary with physical proximity to suspected group members could degrade network connectivity (e.g. cell jammer) to force mesh activation, then passively observe mesh traffic. Within a single epoch, the adversary can confirm that devices are exchanging messages in the same group by correlating which group tags appear around which devices. The adversary cannot read message content (Signal E2E), but can confirm group co-membership.

This is a real risk in the adversarial scenarios mesh is designed for. Mitigations to prioritize in future work:
- **Per-recipient tag derivation** — derive a unique tag per group member, so no two members share the same observable tag. Increases packet count (one per member, like DMs) but eliminates correlation.
- **Noise_XX channel encryption** — encrypting packet headers between BLE peers would hide tags from relay nodes, though a direct BLE neighbor could still observe.
- **Dummy traffic** — periodic fake packets with random tags to obscure real group activity patterns.

---

## 3. Connectivity State Machine

```
Online ──(WS drop or HTTP send failure)──► Disconnected
                                               │
                                     user enables mesh
                                               │
                                               ▼
                                           MeshActive ◄── BLE scanning, can send/receive
                                               │
                                     homeserver probe succeeds (every 30s)
                                               │
                                               ▼
                                            Online (mesh stays on until user disables or 4h timeout)
```

| State | Meaning | UI |
|-------|---------|-----|
| `Online` | WS connected, HTTP sends succeeding | no indicator |
| `Disconnected` | Server unreachable, mesh not enabled | banner with "Enable Bluetooth mesh?" prompt |
| `MeshActive` | BLE scanning/advertising active | blue-gray mesh banner: "Bluetooth mesh active" |

When the server comes back while mesh is active, DMs and group messages resume going through the server. Mesh stays on (BLE keeps scanning) so the broadcast channel remains available.

---

## 4. Implementation Approach: Fork BitChat

BitChat is Unlicense (public domain). Rather than reimplementing BLE mesh networking from scratch, avalanche forks the relevant BitChat source files directly and modifies them:

**Copied from BitChat** (stripped of BitChat UI, Nostr, and Tor code):
- `BluetoothMeshManager` — BLE scanning, advertising, L2CAP CoC connection management, peer tracking
- `BitchatPacket` — packet serialization, header format, TTL handling
- Bloom filter — deduplication logic
- Relay loop — receive, check bloom, decrement TTL, rebroadcast

**Added by avalanche:**
- `ACTNET_DM`, `ACTNET_GROUP`, and `ACTNET_BROADCAST` payload types (three new type bytes in BitChat's packet format)
- `MeshTransportManager` — coordinator that routes inbound DMs and group messages to the Rust core for decryption, manages broadcast channel, exposes `send(...)` for outbound

---

## 5. UX

### 5.1 Disconnected banner

When the server is unreachable and mesh is not enabled:
- Yellow/amber banner at the top: "Disconnected from server"
- Button: "Enable Bluetooth mesh"

### 5.2 Mesh active banner

When mesh is active:
- Blue-gray banner: "Bluetooth mesh active"
- Antenna icon: `antenna.radiowaves.left.and.right`
- Tap to access mesh settings (disable, see connected peer count)
- If server is also connected: "Bluetooth mesh active — server connected"

### 5.3 Local Mesh broadcast channel

- Appears in the chat list as "Local Mesh" when mesh is active
- Disappears from the chat list when mesh is disabled
- Messages are plaintext, show sender display name (unauthenticated — anyone can claim any name)
- No read receipts, no delivery confirmation
- Warning at the top of the channel: "Messages in this channel are not encrypted and visible to anyone on the mesh"

### 5.4 Per-message transport indicator

We can integrate info about whether a message is going out via mesh using a small modification of the Signal style double checkmark system: if the message has gone out to at least one peer on the mesh it will show a # icon instead of a single checkmark; if the recipient has acknowledged / read then it will show double checkmark/double blue as normal.

For DMs and group messages received via mesh, a subtle # icon next to the timestamp.

---

## 6. Background Behavior and Limitations

- **Existing p2p BLE connections** are maintained when the phone is locked. Relay continues to work through locked phones that already have connections, at least for a little while. **iOS can suspend the app** under memory pressure, which will cause it to stop relaying.
- **New peer discovery** is degraded in the background. iOS throttles BLE scanning frequency, making it slower to find new peers.

---

## 8. Deferred Items

### Prekey exchange over mesh
Enabling new Signal sessions without the homeserver. Would allow messaging contacts you haven't previously communicated with. Deferred until core mesh flow is proven.

### WiFi Direct (MultipeerConnectivity)
Higher-bandwidth local transport, but suspended when the app is backgrounded, limiting its utility as a relay layer. Deferred.

### Group tag metadata hardening
See threat analysis in section 2.5. Per-recipient tag derivation, Noise_XX channel encryption, and dummy traffic are all potential mitigations for the forced-mesh-activation attack. Should be prioritized once the core mesh flow is proven.

### Nostr relay fallback
Third transport tier for when both homeserver and local BLE mesh are unavailable (internet exists but homeserver is seized; contacts are geographically dispersed). Deferred.

### Android
Rust core changes are platform-agnostic. BLE transport needs a separate Kotlin implementation. Deferred.

### Noise_XX channel encryption
Transport-layer encryption between directly connected BLE peers, protecting packet headers (sender DID, recipient/group tags) from relay nodes. Defence-in-depth; also a mitigation for the forced-mesh-activation attack (section 2.5). Deferred given complexity.
