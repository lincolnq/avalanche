# 14 — BitChat mesh fallback

> **Status:** Speculative — an optional, opportunistic transport for when the homeserver is unreachable. Nothing is built. Revisit only after core messaging, groups and federation are stable.
> **Last verified against code:** 2026-10-03

## Summary

Fork the public-domain [BitChat](https://github.com/permissionlesstech/bitchat)
Bluetooth LE mesh (multi-hop flooding, up to 7 hops) and carry our existing Signal
ciphertext over it when the server is down — for example at an action where the
network is jammed or overloaded. Relay nodes see only opaque bytes, the same
guarantee as the server path, so no new encryption layer is needed. A plaintext
"Local Mesh" channel covers open coordination with people you have no session with.

## Design sketch

- **Flooding, not routing.** Every node rebroadcasts (TTL, Bloom-filter dedup);
  recipients check whether a packet is for them. No liveness tracking.
- **User-activated.** When the server is unreachable, a banner offers "Enable
  Bluetooth mesh". It stays on until turned off, or until the server has been back for
  4 hours with no mesh traffic. No surprise BLE advertising.
- **Three payload types:**
  - **DM:** Double Ratchet ciphertext. Only works with sessions already established
    through the server (new sessions need prekeys).
  - **Group:** Sender-Key ciphertext, for groups you're already in. Server-side group
    operations (state changes, credentials, endorsements) queue until reconnect
    (`03` §7).
  - **Broadcast:** plaintext Local Mesh channel, clearly labeled unencrypted; sender
    names are unauthenticated.
- **Transport identity.** BitChat identifies nodes by a Curve25519 key; derive it from
  the identity key with HKDF so there's no extra keypair.
- **UI.** Disconnected and mesh-active banners; a per-message mesh indicator in place
  of the single checkmark.
- **iOS limits.** Existing BLE connections survive screen lock for a while; iOS
  throttles background scanning and may suspend the app.
- **Implementation.** Copy BitChat's BLE manager, packet format, Bloom filter and
  relay loop (stripped of its UI, Nostr and Tor code); add three payload types and a
  `MeshTransportManager` that hands inbound packets to the Rust core.

## Addressing — needs redesign before building

Packets carry short (8-byte) tags so recipients can recognize their traffic. The
earlier draft derived them as:

- DM: `HMAC(recipient_identity_key, "mesh-dm-tag" || day)[:8]`
- Group: `HMAC(sender_key, "mesh-group-tag" || day)[:8]`

**The DM tag is broken.** The identity key is a *public* key: it's in every server's
`devices` table and in the DID document. Anyone holding it (a seized server, anyone
who resolves the DID) can compute a person's daily tag and physically track their
device through mesh traffic. Tags must come from secrets the two parties share — for
example derived from the Double Ratchet session or a per-contact secret exchanged over
an existing session.

The group tag is keyed per sender from that sender's Sender Key, never from the group
master key (which travels in invite links).

## Threats

- **Forced mesh activation.** An adversary near suspected members jams connectivity to
  push them onto mesh, then observes which tags appear around which devices and
  confirms group co-membership within a day's epoch. Mitigations: per-recipient tags,
  Noise_XX link encryption between BLE neighbors to hide tags from relays, dummy
  traffic.
- **Local Mesh sender identity.** Decide what a broadcast packet exposes (a stable
  transport key would be a tracking beacon). Use per-session ephemeral keys.
- **Running mesh and server paths together** needs dedup of messages that arrive both
  ways.

## Deferred

Prekey exchange over mesh (new sessions without the server); WiFi Direct; a Nostr
relay tier for when the internet works but the homeserver is gone; Android BLE; Noise_XX
link encryption.
