# Identity, Authentication, and Recovery

This document describes the user-facing flows for creating accounts, logging in, and recovering access after device loss.

## Background

Avalanche has no phone numbers or emails. Identity is a DID (`did:plc`), a cryptographic identifier hosted in the public PLC directory. Each DID has two kinds of keys:

- **Rotation keys** — control the DID itself. Can change signing keys, service endpoints, or transfer the identity. These are the "root authority."
- **Signing keys (verification methods)** — used for day-to-day operations (encrypting messages, authenticating to servers). Don't control the DID unless also listed as rotation keys.

This separation is what makes recovery possible: you can lose your signing key and use a rotation key to issue a new one.

### Recovery authority: the passkey owns the rotation key

The user's passkey (or written-down recovery phrase) is the sole authority over the DID. The **rotation key is deterministically derived from the passkey** via the WebAuthn PRF extension — it is not stored on any server. As long as the passkey survives, the user retains full control of the DID, regardless of what happens to any server.

Concretely, the PRF output is treated as seed material and run through HKDF with two distinct labels:

- `"actnet-rotation-v1"` → DID rotation keypair (P-256, deterministic from passkey).
- `"actnet-blob-v1"` → symmetric key for the recovery blob (see below).

A written-down recovery phrase produces the same two outputs via the same KDF, just with the phrase replacing the PRF as the seed.

**The device identity key is *not* derived from the passkey.** It is generated randomly per device at signup time, consistent with libsignal's per-device identity model. This means the DID is `f(derived_rotation_pub, random_identity_pub, server_url)` — not purely a function of the passkey. See "How the DID stays recoverable" below for how we still reconstruct it from the passkey alone.

### How the DID stays recoverable from the passkey alone

To make the DID derivable from just the passkey + the original signup server URL (no need to remember the DID itself), the genesis op intentionally **omits the identity key**. The DID is committed as a function of `(derived_rotation_pub, server_url)` only, and the identity key is added in an immediately-following PLC update operation signed by the same rotation key.

Signup writes two PLC ops back-to-back:

1. **Genesis op** — `rotation_keys = [derived_rotation_pub]`, `services = {homeserver: server_url}`, `verification_methods = {}` (empty). Signed by rotation key. **The DID is fixed by the hash of this op.**
2. **Update op** — adds the random per-device identity key as a verification method. Signed by rotation key, with `prev` pointing at the genesis op.

This separation means a recovering device with only the passkey + the original signup server URL can deterministically recompute the genesis op, derive the same DID, and from there issue a new identity key via another PLC update.

**Where the original signup server URL is stored.** The passkey credential's WebAuthn `user.id` (userHandle) is set to the original signup server URL at create time. The userHandle is returned to the relying party during any future assertion ceremony, so the recovering device gets the server URL back automatically — the user never has to remember or type it. The userHandle does not change if the user later migrates discovery servers; it always reflects the *original* genesis server, because that's what's baked into the DID.

The passkey's `user.displayName` is set to a human-readable label (e.g. `"Sam @ safe-haven.org"`) so the OS passkey picker can disambiguate between multiple identities. That field is cosmetic only.

### Recovery blob: convenience, not authority

The recovery blob is a server-side cache that lets a recovering device skip re-registration friction. It contains:

- Device identity keypair (so Signal sessions continue without a safety-number change).
- Full list of homeservers the user is a member of.
- Profile key and display name (so the user's profile is restored without prompting).
- The group 'master key' for all groups you're a member of (so the user can continue reading messages in that group).

**Wire format.** The plaintext is a versioned protobuf (`actnet.recovery.RecoveryBlob`, currently v4). Homeserver URLs are interned in a top-level `servers` table and referenced by index from each group entry — N groups on the same homeserver pay the URL string once, not N times. The protobuf is encrypted with AES-256-GCM under the `"actnet-blob-v1"` HKDF output; the on-the-wire envelope is `version(1) || nonce(12) || ciphertext+tag`. The intent is to replicate the ciphertext to every homeserver the user belongs to so any of them can anchor recovery.

**Auto-updates.** The PRF-derived blob key is cached in the local SQLCipher database at signup (and again after recovery). Every event that changes blob-relevant state — joining or creating a group, accepting an invite, adding a server — re-encrypts and uploads the blob silently using the cached key. The passkey is only required at account creation and at recovery; routine state changes never re-prompt.

**It does not contain the rotation key.** Losing every copy of the blob costs session continuity (safety-number change), the server list (user re-enters one server URL manually), and per-group sender-key continuity (peers must re-DM their SKDMs to the recovered device) — but not DID control. The rotation key is always recoverable from the passkey, and the DID is always recoverable from `(rotation_key, signup_server_url)`.

### Privacy: DID document and server discovery

The DID document is public. To avoid leaking all of a user's organizational affiliations, the DID document lists only a single "home" homeserver as its service endpoint — whichever server the user signed up on first (changeable in settings via discovery-server migration; see `13-federation.md`). Cross-server message routing uses contact exchange: when two users connect, they learn each other's homeserver addresses and store them locally. The PLC directory is not used for ongoing message routing, only for initial DID verification and recovery.

During recovery, the app resolves the DID via the PLC directory → finds the current home server → downloads the recovery blob → discovers all other servers. If the home server is unreachable or has no blob, recovery still proceeds via the passkey path described above; the user just loses the server-list convenience and takes a safety-number change.

---

## User Stories

### 1. First signup: new identity at a rally

**Story:** Sam scans a QR code at a rally. They've never used avalanche before. They type a name, create a passkey with Face ID, and they're in — with recovery already set up.

**Flow:**

1. Scan QR / tap invite link.
2. App validates invite token with the server.
3. "What's your name?" screen — display name required, photo optional.
4. "Create a passkey to protect your identity" — brief explanation. Sam authenticates with Face ID. The WebAuthn `create()` ceremony is configured with `user.id = <signup_server_url_bytes>`, `user.displayName = "<name> @ <server>"`, and the PRF extension is requested. 1Password (or iCloud Keychain) creates and stores the passkey. The authenticator returns the credential plus the PRF output. (Skipping this step is possible, but discouraged — see below.)
5. App runs the PRF output through HKDF with labels `"actnet-rotation-v1"` and `"actnet-blob-v1"` to derive (a) the P-256 DID rotation keypair and (b) the 32-byte blob-encryption symmetric key.
6. App generates the remaining keys randomly: device identity key (Ed25519 keypair) and Signal protocol prekeys (signed, one-time, Kyber).
7. App builds the **genesis PLC operation** with `rotation_keys = [derived_rotation_pub]`, `services = {homeserver: signup_server_url}`, `verification_methods = {}`. Signs with the rotation key. The DID is now determined by the hash of this op.
8. App builds the **identity-key update PLC operation** with `prev` pointing at the genesis op, adding the random device identity key as a verification method. Signs with the rotation key.
9. App submits both ops to the PLC directory in order. The DID now exists publicly with a registered identity key.
10. App encrypts `{identity_keypair, [signup_server_url], profile_key, display_name}` into the recovery blob using the blob symmetric key from step 5.
11. App registers with the homeserver: `POST /v1/accounts` with identity key, registration_id, device_id, prekeys, DID, and the encrypted recovery blob.
12. Server auto-enrolls Sam into the rally's groups per the invite token.
13. Push notification permission prompt.
14. Sam lands in Chats with groups populated. Recovery is already active.

**Technical details:**
- Passkey relying party: a universal avalanche domain (e.g. `theavalanche.net`), not the homeserver's domain. This means recovery of a passkey identity can only be done by our official mobile apps and/or web application on our domain.
- `user.id` (WebAuthn userHandle): set to the signup server URL bytes. This is what gets returned during any future assertion, letting a recovering device reconstruct the genesis op without prompting the user. It never changes, even after discovery-server migration.
- PRF extension: a fixed app-wide salt (e.g. `"actnet-recovery-v1"`) is provided during the ceremony. The authenticator returns 32 deterministic bytes from `HMAC-SHA256(passkey_secret, salt)`. HKDF-Expand with two labels then derives the rotation keypair and the blob-encryption key. Both are recoverable from the passkey alone.
- Why two PLC ops: the genesis op must be signable before the identity key exists (because we want the DID to be derivable from just the passkey + signup server URL, with no dependency on the random per-device identity key). A second op adds the identity key as a verification method.
- DID genesis + update operations submitted to `plc.directory` (or configured PLC directory).
- Server registration: `POST /v1/accounts` with identity key, registration_id, device_id, prekeys, and recovery blob (stored opaque ciphertext).
- Server stores the DID document with the device's public key as a verification method and the homeserver as a service endpoint.
- **What if the user skips recovery?** No passkey is created, so the rotation key is generated randomly on-device and no recovery blob is written. If Sam loses their phone they cannot recover this identity at all. The server knows this and can nag the user. This is the only case where DID control is not deterministically derivable from a user-held secret.
- **Written-down recovery phrase:** instead of a passkey, generate a high-entropy memorable phrase and tell the user to write it down. The phrase is run through the same HKDF (same labels) to produce the rotation keypair and the blob-encryption key. Since there's no WebAuthn ceremony, the signup server URL is stored alongside the phrase (e.g. printed on the recovery card as "Server: safe-haven.org"). To avoid making the user retype the phrase every time the blob needs re-encrypting, the derived symmetric key is cached in the Secure Enclave after first entry.

---

### 2. Joining a second server with the same identity

**Story:** Sam's org is on a different server. Sam taps an invite link, and the app asks which identity to use. Sam picks their existing name and is in immediately.

**Flow:**

1. Tap invite link for server 2.
2. App shows identity picker: "Join as Sam" (existing DID) or "Create a fresh identity."
3. Sam taps "Join as Sam."
4. App registers the existing DID on server 2: uploads identity public key, prekeys, and recovery blob.
5. Server 2 verifies the DID against the PLC directory — confirms the signing key matches.
6. Auto-enrollment into groups per the invite token.
7. Sam lands in Chats with new groups visible alongside existing ones.

No new keys generated (except fresh prekeys for this server). The DID document already has this device's signing key; the server just verifies it.

No passkey prompt either: the blob symmetric key is already cached locally from the original signup, so the app silently re-encrypts the blob with the updated server list and uploads. Tapping "Join as Sam" is the only user-visible step.

**Technical details:**
- Server resolves the DID via the PLC directory and checks that the presented identity key matches a verification method in the DID document.
- `POST /v1/accounts` with the existing DID, identity key, registration_id, device_id, prekeys.
- The recovery blob now needs to include server 2 in its server list. The app reads the cached blob symmetric key from SQLCipher (saved at signup), re-encrypts, and uploads the updated blob. Replicating to both servers ensures recovery from either one discovers all servers.
- **Written-down recovery key case:** Same story — the derived symmetric key is cached locally after first entry, so subsequent server joins don't make the user dig out the recovery phrase.

---

### 3. Creating a second identity (pseudonymous)

**Story:** Sam wants to organize with a different group under a pseudonym. They create a second identity with a different name, unlinked to the first.

**Flow:**

1. From an invite link, choose "Create a fresh identity."
2. "What's your name?" — Sam enters a pseudonym.
3. "Create a passkey to protect your identity" — Sam authenticates with Face ID. 1Password creates a second passkey.
4. App generates a completely new set of keys: new rotation key, new identity key, new prekeys. Encrypts recovery blob with the new passkey's PRF-derived key.
5. New DID genesis operation submitted to PLC directory. This is a separate DID with no connection to the first.
6. Register with the server, upload recovery blob.

Sam now has two identities. Both appear in the app. Chats from both identities appear in the unified inbox with subtle identity indicators.

**Technical details:**
- Completely independent key material. The PLC directory has two unrelated DIDs.
- Second passkey in 1Password, registered against the same relying party; 1Password distinguishes them by label.
- The two identities share no keys, no server-side state, and no PLC directory linkage. The server cannot tell they belong to the same person.

---

### 4. Recovering an identity after device loss

**Story:** Sam loses their phone. They get a new one, install avalanche, and recover their activist identity using the passkey synced through 1Password.

**Flow:**

1. Install avalanche on new phone. Tap "Recover existing identity."
2. App initiates a WebAuthn assertion ceremony (no `allowCredentials`, discoverable mode) with the PRF extension and the same salt as signup.
3. 1Password syncs to the new phone and presents Sam's passkey(s). Sam selects the one for their activist identity and authenticates with Face ID.
4. Authenticator returns `{credentialId, userHandle = signup_server_url_bytes, prfOutput}`.
5. App runs the PRF output through HKDF with the same two labels, producing the rotation keypair and the blob-encryption symmetric key. **At this point, regardless of any server state, Sam has full DID control.**
6. App **recomputes the genesis op deterministically** with `(derived_rotation_pub, signup_server_url)` and `verification_methods = {}`. Hashing this op yields the original DID. No PLC lookup required to know Sam's DID.
7. App resolves the DID via PLC to find the *current* home server (it may have been migrated since signup), and attempts to download the recovery blob.

8. **Blob path (common case):** App decrypts the blob with the derived blob symmetric key. The plaintext yields the original identity keypair, the full list of homeservers, and the master key for every group Sam was in.
   - App restores the identity keypair — same identity key as before, so contacts see no safety-number change.
   - App generates a new device_id and signs a device replacement request with the rotation key. On each homeserver in the list, the server verifies the signature against the rotation key listed in PLC, revokes the old device, and registers the new device_id with fresh prekeys.
   - App caches the blob symmetric key in SQLCipher so future state changes can refresh the blob silently.
   - For each group master key in the blob: app persists a minimal group row, calls `fetch_group_state` on the host homeserver, rotates the per-device push pseudonym (the old one died with the lost device), re-seeds its own Sender Key, and DMs the new SKDM to every other member so they can decrypt Sam's future group messages. Sam can immediately send into every group. Old group messages that other members sent under Sender Keys this device previously held are undecryptable until those peers re-distribute their keys.
   - Sam is back. Existing 1:1 Signal sessions continue seamlessly.

9. **No-blob path (every blob copy is gone):** App generates a fresh identity keypair on-device. Signs a PLC update with the rotation key replacing the old identity verification method with the new one. Submits to PLC.
   - App proceeds with the original signup server URL (from `userHandle`) and tries to register there. If that server no longer accepts the user, prompts Sam to enter a server URL manually.
   - Re-registration on the chosen server uses the rotation-key-signed proof of DID ownership.
   - Result: Sam's DID is preserved, but contacts see a safety-number change and per-server state (group memberships, queued messages, the full server list) is lost. Sam can re-add other servers later as they remember them.

10. If Sam has additional identities: Settings → Add an account → "Recover a different identity" → repeat steps 2–9 with the next passkey. Each recovery is independent — one passkey per identity, one Face ID prompt each.

**Technical details:**
- WebAuthn assertion uses the same PRF salt as signup and the same HKDF labels, so the resulting rotation key, identity key (where derived), and blob key are bit-identical to those derived at signup. The rotation key is never transmitted or stored on any server.
- `userHandle` returned by the assertion is the original signup server URL bytes; the client uses it to reconstruct the genesis op without prompting.
- Recovery blob downloaded via `GET /v1/recovery/{did}` (unauthenticated — the blob is opaque ciphertext, safe to serve publicly).
- **Device replacement:** The rotation key (re-derived from the passkey, never on disk during normal operation) serves as proof of authority to replace the device. The app signs a replacement request with the rotation key, the server revokes the old device_id (invalidating its session tokens so it can no longer authenticate), and registers the new device_id. This is a server endpoint `POST /v1/devices/replace`, authenticated by rotation key signature rather than session token.
- **Blob path** preserves session continuity and the full server list. No safety-number change.
- **No-blob path** preserves the DID and the rotation key authority. Identity key changes (safety-number change) and server list is lost. This is the fallback path; the passkey alone is always sufficient to reach it.
- After recovery, the app re-authenticates to each homeserver via challenge-response with the restored (or freshly generated) identity key.

---

### 5. Day-to-day app usage (no passkey involved)

**Story:** Sam opens the app, reads messages, sends replies. No authentication prompts.

**Flow:**

1. App launches. SQLCipher database unlocked via Secure Enclave-derived key.
2. Identity key loaded from local database.
3. WebSocket connection established to each homeserver using existing session tokens.
4. If session token is expired: automatic challenge-response re-authentication using the device identity key. No user interaction.
5. Messages encrypt/decrypt using existing Double Ratchet sessions.

Passkeys are never touched during normal use. They exist solely for recovery.

**Technical details:**
- Session tokens have a configurable lifetime. When expired, the client automatically performs the challenge-response flow: `POST /v1/auth/challenge` (get nonce) → `POST /v1/auth/token` (sign nonce with identity key, get new token).
- No user interaction required for re-authentication. The identity key is always available in the local encrypted database.

---

## Summary: What Lives Where

| Secret | Where it lives | What it's for |
|---|---|---|
| Passkey (or written-down recovery phrase) | 1Password / iCloud Keychain / hardware key / paper | Sole authority over the DID. Via HKDF labels `"actnet-rotation-v1"` and `"actnet-blob-v1"`, deterministically produces the rotation key and the blob-encryption key. |
| Passkey `user.id` (userHandle) | Inside the passkey credential | Stores the original signup server URL. Returned during recovery assertion so the client can reconstruct the genesis op and derive the DID without prompting. |
| DID rotation key (P-256) | Re-derived from passkey on demand; cached in device SQLCipher during a session | Signing DID operations (genesis, identity-key updates, device replacement). **Never stored on any server.** |
| Device identity key (Ed25519) | Device (SQLCipher) + encrypted in recovery blob | Signal protocol: session establishment, message encryption, server auth. Generated randomly per device. |
| Prekeys (signed, one-time, Kyber) | Public halves on server; private halves on device (SQLCipher) | Signal protocol: X3DH session initiation |
| Recovery blob | Homeserver(s), encrypted | Contains identity key + server list + profile data + group master keys. Convenience-only: losing every copy costs session continuity, the server list, and silent group re-join — not DID control. |
| Blob symmetric key (cached) | Device (SQLCipher) | PRF-derived AES key for the recovery blob. Cached at signup/recovery so routine state changes (group join, etc.) can re-encrypt and upload silently without a passkey prompt. |
| Session token | Device (memory/keychain) | Authenticating API requests to homeserver |


---

## Notes

### Bluesky-linked identities (future)

The flows above cover standalone avalanche identities. A future extension could allow users to connect an existing Bluesky identity via ATProto OAuth — authenticating with Bluesky to prove ownership of a `did:plc` that already exists, then registering that DID on an avalanche homeserver. This would let public organizers use the same identity across both networks.

Key differences from standalone identities:

- No passkey needed — Bluesky is the identity authority, recovery is "log in with Bluesky again"
- No PLC directory writes — Bluesky manages the DID document
- No recovery blob — device loss means new keys, sessions reset, contacts see a safety number change (same tradeoff Signal makes with phone numbers)
- No automatic server list recovery — the user must remember which servers they were on and re-authenticate with Bluesky on each one individually
- Could extend to other OAuth providers (Google, Apple, etc.) that create a new avalanche DID on the user's behalf with the OAuth provider as the recovery authority

Privacy tradeoff: connecting a Bluesky account means Bluesky can verify that identity on avalanche. For sensitive organizing, users should create a separate standalone identity instead.

### Other OAuth providers

Any OAuth provider could serve as an identity authority using the same pattern as Bluesky. The difference is that non-Bluesky providers don't use `did:plc`, so the avalanche homeserver would create a new DID on the user's behalf and link it to the OAuth identity. Recovery = re-authenticate with the provider. Same lossy recovery (new keys, safety number change) as the Bluesky case.


## Screen Flow

```
 ┌────────────────┐
 │    Landing     │
 │                │
 │ [Scan QR]      │
 │ Enter code     │
 │ Recover        │
 └──┬──────────┬──┘
    │ invite   │ recover
    │ link     │
    ▼          ▼
 ┌────────┐  ┌────────────────┐
 │Choose  │  │   Recovery     │
 │ID page │  │   Explainer    │
 │(if ≥1  │  │                │
 │ID on   │  │ [Passkey]      │
 │device) │  │ Phrase ->      │
 │        │  └───────┬────────┘
 │ Alice  │          │
 │[+] New │          │ WebAuthn sheet
 │[⚷] Re- │          │ (system UI: pick
 │  cover │          │  passkey, Face ID)
 └┬──┬──┬─┘          │
  │  │  │            ▼
  │  │  │ recover  ┌────────────────┐
  │  │  └─────────►│   Recovery     │
  │  │             │   Console      │
  │  │             └───────┬────────┘
  │  │ new                 │
  │  ▼                     │
  │ ┌────────────────┐     │
  │ │    New ID      │     │
  │ │                │     │
  │ │ [photo]        │     │
  │ │ Display Name   │     │
  │ │ [Next]         │     │
  │ │ recover ->     │     │
  │ └───────┬────────┘     │
  │         │              │
  │         ▼              │
  │ ┌────────────────┐     │
  │ │  Server Step   │     │
  │ │  (optional)    │     │
  │ └───────┬────────┘     │
  │         │              │
  │         ▼              │
  │ ┌────────────────┐     │
  │ │    Passkey     │     │
  │ │   Explainer    │     │
  │ │                │     │
  │ │ [photo+name]   │     │
  │ │ [Create]       │     │
  │ │ phrase ->      │     │
  │ │ skip ->        │     │
  │ └───────┬────────┘     │
  │         │              │
  │   WebAuthn sheet       |
  │         │              │
  │         ▼              │
  │ ┌────────────────┐     │
  │ │    Signup      │     │
  │ │   Console      │     │
  │ └───────┬────────┘     │
  │         │              │
  │ existing│              │
  │ identity│              │
  │         ▼              ▼
  │ ┌──────────────────────────┐
  └►│       SIGNED IN!         │
    └──────────────────────────┘
```

## Screen Details

### Landing Page
- **Scan invitation** — primary action, opens camera
- **Enter invite code** — secondary, text entry
- **Recover account** — secondary, navigates to Recovery Explainer

### Choose ID Page
Shown when the user already has one or more identities on this device and scans/taps an invite link. First-time users skip straight to New ID.

- "Log into [server name] with:"
- List of existing identities on this device (tap to join this server with that identity)
- [green] **+ New identity** — navigates to New ID page
- [yellow] **⚷ Recover** — navigates to Recovery Explainer

### New ID Page
- "Create a new identity for [server name]:"
- Profile picture field (optional)
- Display Name field (required)
- (future) Icon field for disambiguation
- **Next** button — primary action, disabled until display name is entered
- Small "recover an existing identity instead →" at bottom (hidden if navigated from Choose ID page, since Recover is already an option there)

### Server Step (deferred)
A webview provided by the homeserver for any pre-account-creation requirements: terms of service agreement, additional signup info, org-specific onboarding, etc. The invite token tells the app whether a server step exists and what URL to load. If the server doesn't specify one, this screen is skipped entirely.

This also runs when an existing identity joins a new server (after tapping an identity on the Choose ID page). 

The exact implementation needs to be planned out for this. For now we can just skip implementing it, but it will be important later.

### Passkey Explainer Page
- "Create a passkey to protect this identity"
- Shows the profile picture and display name as they will appear on the Choose ID page in the future
- "Passkeys are stored securely in your password manager or iCloud, and synced across all your devices. You'll use it to sign back into this identity if you lose this device. [More about passkeys →]" (links to explainer page on our website)
- **⚷ Create Passkey** — primary action. Triggers a WebAuthn registration ceremony: the system presents a sheet (1Password, iCloud Keychain, or hardware key), the user confirms with Face ID, and a new passkey is created for the `theavalanche.net` relying party.
- "Use a recovery phrase instead →" — generates a high-entropy memorable phrase, user writes it down
- "Skip recovery setup →" — proceeds without recovery (server may nag later)

### Recovery Explainer Page
- "Recover an identity"
- **⚷ Recover using Passkey** — primary action. Triggers a WebAuthn authentication ceremony: the system presents a sheet showing all passkeys stored for `theavalanche.net`. The user picks one and confirms with Face ID. The app receives the PRF-derived symmetric key and the DID from the user handle. If the user picks an identity that is already signed-in on this device, we explain that they're already signed in and prompt to pick another.
- "Enter your recovery phrase instead →" — text entry for the written-down phrase

### Progress Console
A monospace-text console that scrolls through status updates as the app works in the background. Used for both signup and recovery.

After completion, the console transitions to the signed-in Chats screen, which will hopefully have a welcome message or something, but that's up to the server.

