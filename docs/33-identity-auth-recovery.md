# Identity, Authentication, and Recovery

This document describes the user-facing flows for creating accounts, logging in, and recovering access after device loss.

## Background

Actnet has no phone numbers or emails. Identity is a DID (`did:plc`), a cryptographic identifier hosted in the public PLC directory. Each DID has two kinds of keys:

- **Rotation keys** — control the DID itself. Can change signing keys, service endpoints, or transfer the identity. These are the "root authority."
- **Signing keys (verification methods)** — used for day-to-day operations (encrypting messages, authenticating to servers). Don't control the DID unless also listed as rotation keys.

This separation is what makes recovery possible: you can lose your signing key and use a rotation key to issue a new one.

### Privacy: DID document and server discovery

The DID document is public. To avoid leaking all of a user's organizational affiliations, the DID document lists only a single "home" homeserver as its service endpoint — whichever server the user signed up on first (changeable in settings). Cross-server message routing uses contact exchange: when two users connect, they learn each other's homeserver addresses and store them locally. The PLC directory is not used for ongoing message routing, only for initial DID verification and recovery.

The recovery blob (stored on every homeserver the user is registered on) contains the full list of servers. During recovery, the app resolves the DID via PLC directory → finds the home server → downloads the recovery blob → discovers all other servers.

---

## User Stories

### 1. First signup: new identity at a rally

**Story:** Sam scans a QR code at a rally. They've never used actnet before. They type a name, create a passkey with Face ID, and they're in — with recovery already set up.

**Flow:**

1. Scan QR / tap invite link.
2. App validates invite token with the server.
3. "What's your name?" screen — display name required, photo optional.
4. "Create a passkey to protect your identity" — brief explanation ("This lets you recover your account if you lose your phone. It syncs through your password manager or iCloud Keychain."). Sam authenticates with Face ID. 1Password (or iCloud Keychain) creates and stores the passkey. (Skipping this step is possible, but discouraged - see below.)
5. App generates keys in the background:
   - DID rotation key (P-256 keypair)
   - Device identity key (Ed25519 keypair)
   - Signal protocol prekeys (signed, one-time, Kyber)
6. App performs a WebAuthn authentication ceremony with the PRF extension, deriving a symmetric key. Encrypts the rotation key and identity keypair into a recovery blob. (Skip this step if no passkey was generated)
7. App signs a DID genesis operation with the rotation key and submits it to the PLC directory. The DID now exists publicly.
8. App registers with the homeserver: uploads device identity public key, prekeys, DID, and (if present) the encrypted recovery blob.
9. Server auto-enrolls Sam into the rally's groups per the invite token.
10. Push notification permission prompt.
11. Sam lands in Chats with groups populated. Recovery is already active.

**Technical details:**
- Passkey relying party: a universal actnet domain (e.g. `theavalanche.net`), not the homeserver's domain. (This means recovery of a passkey identity can only be done by our official mobile apps and/or web application on our domain)
- PRF extension: a fixed salt is provided during authentication. The authenticator returns a deterministic symmetric key. This key encrypts the recovery blob (rotation key + identity keypair).
- DID genesis operation submitted to `plc.directory` (or configured PLC directory).
- Server registration: `POST /v1/accounts` with identity key, registration_id, device_id, prekeys, and recovery blob (stored opaque ciphertext).
- Server stores the DID document with the device's public key as a verification method and the homeserver as a service endpoint.
- What if the user skips recovery? Then no recovery blob is generated. This is fine, but it means if Sam loses their phone then they will not be able to sign into that identity at all. The server knows this, and can nag the user if they want.
- We'd also be fine supporting a written-down recovery key as an alternative flow to passkey. We generate a high entropy memorable password and tell the user to write it down, then that's the key to encrypt the recovery blob. (We save it in the Secure Enclave instead of a passkey so we don't have to have them enter their password to re-encrypt server list later)

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

The app prompts a passkey authentication (Face ID via 1Password) to re-encrypt the recovery blob with the updated server list. This is the only user-visible step beyond tapping "Join as Sam."

**Technical details:**
- Server resolves the DID via the PLC directory and checks that the presented identity key matches a verification method in the DID document.
- `POST /v1/accounts` with the existing DID, identity key, registration_id, device_id, prekeys.
- The recovery blob now needs to include server 2 in its server list. The app performs a WebAuthn authentication ceremony with PRF to derive the symmetric key, re-encrypts the blob, and uploads the updated blob to both server 1 and server 2. This ensures recovery from either server discovers all servers.
- **Written-down recovery key case:** If the user chose a written-down recovery key instead of a passkey, we don't want to make them retype it every time they join a server. Instead, the recovery key's derived symmetric key is cached in the Secure Enclave after first entry. The app retrieves it with a biometric prompt (same UX as the passkey path) to re-encrypt the blob without the user needing to dig out their recovery phrase.

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

**Story:** Sam loses their phone. They get a new one, install actnet, and recover their activist identity using the passkey synced through 1Password.

**Flow:**

1. Install actnet on new phone. Tap "Recover existing identity."
2. App initiates a WebAuthn authentication ceremony.
3. 1Password syncs to the new phone and presents Sam's passkey(s). Sam selects the one for their activist identity and authenticates with Face ID.
4. PRF extension derives the symmetric key. The WebAuthn user handle contains Sam's DID.
5. App resolves the DID via the PLC directory → discovers the home server → downloads the encrypted recovery blob. (Fallback: Sam enters a server URL manually.)
6. App decrypts the recovery blob with the PRF-derived key. This yields the DID rotation key, identity keypair, and the full list of homeservers.
7. App restores the identity keypair — same identity key as before.
8. App generates a new device_id and signs a device replacement request with the rotation key. On each homeserver: the server verifies the rotation key signature against the PLC directory, revokes the old device (invalidates its session tokens and prekey bundles), and registers the new device_id with fresh prekeys. The identity key stays the same, so contacts see no safety number change.
9. Sam is back. Existing sessions continue seamlessly.
10. If Sam has additional identities: Settings → Add account → "Recover existing identity" → repeat steps 2-9 with the next passkey. Each recovery is independent — one passkey per identity, one Face ID prompt each.

**Technical details:**
- WebAuthn authentication with PRF extension. Same salt as during setup produces the same symmetric key.
- Recovery blob downloaded via `GET /v1/recovery/{did}` (new endpoint, unauthenticated — the blob is opaque ciphertext, safe to serve publicly).
- **Device replacement:** The old and new devices share the same identity key, so the server can't distinguish them by key alone. The rotation key — which only exists in the recovery blob, not on the device during normal operation — serves as proof of authority to replace the device. The app signs a replacement request with the rotation key, the server revokes the old device_id (invalidating its session tokens so it can no longer authenticate), and registers the new device_id. This is a new server endpoint: `POST /v1/devices/replace`, authenticated by rotation key signature rather than session token.
- If the blob includes the full identity keypair: sessions continue seamlessly, no safety number change.
- If the blob includes only the rotation key: Sam can sign a DID update to add a new signing key, but sessions reset and contacts see a safety number change. This is the fallback if blob updates fell behind.
- After recovery, app re-authenticates to each homeserver via challenge-response with the restored identity key.

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
| DID rotation key (P-256) | Device (SQLCipher) + encrypted in recovery blob on server(s) | Signing DID operations (key rotation, recovery) |
| Device identity key (Ed25519) | Device (SQLCipher) + encrypted in recovery blob | Signal protocol: session establishment, message encryption, server auth |
| Prekeys (signed, one-time, Kyber) | Public halves on server; private halves on device (SQLCipher) | Signal protocol: X3DH session initiation |
| Passkey (P-256) | 1Password / iCloud Keychain / hardware key | Deriving the symmetric key that decrypts the recovery blob |
| Recovery blob | Homeserver(s), encrypted | Contains rotation key + identity key + server list, decryptable only with passkey |
| Session token | Device (memory/keychain) | Authenticating API requests to homeserver |


---

## Notes

### Bluesky-linked identities (future)

The flows above cover standalone actnet identities. A future extension could allow users to connect an existing Bluesky identity via ATProto OAuth — authenticating with Bluesky to prove ownership of a `did:plc` that already exists, then registering that DID on an actnet homeserver. This would let public organizers use the same identity across both networks.

Key differences from standalone identities:

- No passkey needed — Bluesky is the identity authority, recovery is "log in with Bluesky again"
- No PLC directory writes — Bluesky manages the DID document
- No recovery blob — device loss means new keys, sessions reset, contacts see a safety number change (same tradeoff Signal makes with phone numbers)
- No automatic server list recovery — the user must remember which servers they were on and re-authenticate with Bluesky on each one individually
- Could extend to other OAuth providers (Google, Apple, etc.) that create a new actnet DID on the user's behalf with the OAuth provider as the recovery authority

Privacy tradeoff: connecting a Bluesky account means Bluesky can verify that identity on actnet. For sensitive organizing, users should create a separate standalone identity instead.

### Other OAuth providers

Any OAuth provider could serve as an identity authority using the same pattern as Bluesky. The difference is that non-Bluesky providers don't use `did:plc`, so the actnet homeserver would create a new DID on the user's behalf and link it to the OAuth identity. Recovery = re-authenticate with the provider. Same lossy recovery (new keys, safety number change) as the Bluesky case.


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

