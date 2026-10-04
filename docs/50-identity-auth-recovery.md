# 50 — Identity, authentication, and recovery

> **Status:** Partial — signup and blob-path recovery with a passkey or a 12-word phrase are built on iOS and Android (Desktop is phrase-only; passkeys are specced in `56`). The no-blob recovery path, joining a second server with an existing identity, and every multi-device-aware part of recovery are not built. Proposed changes to the identity model below need project-owner review.
> **Last verified against code:** 2026-10-03

## Summary

Avalanche has no phone numbers or emails. An identity is a `did:plc` DID in the public PLC directory. A passkey (WebAuthn PRF) or a written phrase deterministically produces the DID's **rotation key** and the key that encrypts a server-side **recovery blob**. The blob holds the Signal identity key and the rest of the bootstrap state, so a recovered device keeps the same safety number. Day-to-day authentication to a homeserver is a challenge-response signed with the identity key; passkeys are only used at signup and recovery.

This doc describes what is built, its known weaknesses, and a proposed redesign (§Proposed) that came out of the October 2026 review.

## Known gaps

Security-relevant items are also tracked in `09`.

- **The rotation key is stored on every device.** It is "re-derived from the passkey on demand" in the design, but in practice it is persisted permanently in identity.db at signup, at recovery, and on every linked device (`store/src/account.rs` `save_rotation_key`; callers in `app-core/src/lib.rs`), and shipped inside the link bundle (`core/proto/provisioning.proto` field 2). The genesis op lists it as the **only** rotation key (`app-core/src/plc.rs` `build_genesis_op`). Anyone who extracts it from any device can rewrite or tombstone the DID permanently; there is no higher-priority key to override them.
- **The no-blob recovery path does not exist.** Step 9 of story 4 (fresh identity key via a rotation-key-signed PLC update) has no code: `build_identity_update_op` is only called at signup. If every copy of the recovery blob is lost, the identity cannot be recovered today, even with the passkey.
- **Recovery is single-server and single-slot.** `recover_from_blob` calls `/v1/devices/replace` on the first server in the blob only, reusing `min(existing device_ids)` as both old and new device id (`app-core/src/lib.rs`). Other devices and other servers are untouched (`04` §7).
- **`GET /v1/recovery/{did}` is an unauthenticated lookup.** No auth, no rate limit, and it returns the account's `device_ids` alongside the blob (`server/src/routes/recovery.rs`). Anyone who knows a DID can ask any server whether that DID is registered there, how many devices it has, and (from the blob size, which grows with group count) roughly how many groups it is in. This undercuts the membership-privacy goal (`09`).
- **The signup server is published permanently.** The genesis op includes `services.avalanche_homeserver = signup_server_url`, and the DID is the hash of that op. PLC's audit log is public and append-only, so "DID X was created at server S at time T" is public forever, even after a home-server migration. The privacy claim that the home server can be omitted from the DID document is not achievable with this genesis design.
- **The identity key is mislabelled in the DID document.** It is a libsignal Curve25519 key, but `did_key_ed25519` strips the `0x05` prefix and publishes it with the Ed25519 multicodec (`0xed`) (`app-core/src/plc.rs`). Anyone verifying against it as Ed25519 will fail. Earlier versions of this doc also called it Ed25519; it is Curve25519 used with XEdDSA signatures.
- **Any `*.theavalanche.net` origin can run the recovery ceremony.** The relying party ID is the registrable domain `theavalanche.net` and the PRF salt is a fixed constant (`actnet-recovery-v1`). WebAuthn lets any subdomain origin request assertions for that RP. The demo homeserver `av.theavalanche.net` serves installed Projects' web code under `/p/<slug>/` (`infra/deploy/bundle/lib/common.sh`), so third-party Project code runs on an eligible origin. If a user approves the passkey prompt in a browser on such a page, that page receives the PRF output, which is the root of the identity.
- **Domain seizure is a recovery outage.** On iOS the passkey RP depends on the `theavalanche.net` association file being served. If the domain is seized or lapses, no user can create or use passkeys in the app.
- **One vault holds every persona.** All of a user's identities' passkeys sit under one RP, labelled `"<name> @ <server>"`. Anyone who can browse the password manager can link the personas.
- **Server-side PLC fetches have no timeout** (`server/src/plc.rs`, `routes/registration.rs` `verify_did_plc` use bare `reqwest::get`). PLC being slow stalls registration, link, and replace requests.
- **Joining a second server with an existing identity is not built.** The apps' `joinServer` only appends to a local server list (`53` Known gaps).

## Current design

### Keys

- **Rotation key (P-256)** — listed in PLC `rotationKeys`. Authorizes DID operations (genesis, updates, tombstone) and device replacement or linking on homeservers. Derived from the passkey PRF or phrase via HKDF label `"actnet-rotation-v1"` (`app-core/src/recovery.rs` `derive_recovery_keys_from_prf`, `derive_rotation_key_from_seed`). Persisted on every device (Known gaps).
- **Blob key (AES-256)** — HKDF label `"actnet-blob-v1"` from the same seed. Encrypts the recovery blob. Cached in identity.db so later blob updates need no passkey prompt. Not present on linked devices.
- **Identity key (libsignal Curve25519)** — the Signal identity key, random at signup, shared by all the identity's devices (`04` §1). Published as the `#avalanche` verification method in the DID document. Used for Signal sessions and for homeserver challenge-response auth.
- **Storage key** — random, identity-level, encrypts durable-state records (`05` §4). Carried in the recovery blob and the link bundle.

### DID derivation

The genesis op deliberately **omits the identity key**, so the DID is `hash(genesis(derived_rotation_pub, signup_server_url))`. Signup writes two PLC ops back to back (`app-core/src/plc.rs`):

1. **Genesis** — `rotationKeys = [rotation_pub]`, `services = {avalanche_homeserver: signup_server_url}`, no verification methods. The DID is fixed by this op's hash.
2. **Update** — adds the identity key as verification method `#avalanche`, `prev` = genesis CID.

A recovering device with the seed and the signup server URL can recompute the genesis op and therefore the DID with no lookup (`derive_did_from_passkey`).

### Passkey ceremony

- RP ID: `theavalanche.net` (iOS `PasskeyManager.relyingParty`, Android `PasskeyManager.RELYING_PARTY`).
- PRF salt: the fixed string `actnet-recovery-v1`.
- `user.id` (userHandle): the signup server URL bytes. Returned on every assertion, so recovery needs no typed input.
- `user.displayName`: `"<name> @ <server>"`, cosmetic.
- Discoverable credential (no `allowCredentials` at recovery).

### Recovery phrase

**Built.** A 12-word BIP39 mnemonic (128 bits), generated in `app-core` (`generate_recovery_phrase`). The first 32 bytes of the BIP39 seed (`recovery_phrase_to_seed`) replace the PRF output in the same HKDF, so the same signup and recovery code paths are reused. The phrase does not carry the server URL: signup shows the home server URL next to the words, and recovery asks for both. Signup re-prompts for three words before creating the account.

### Skipping recovery

If the user skips both passkey and phrase, the rotation key is random (`generate_rotation_key`) and no blob key exists. The identity is unrecoverable on device loss.

### Recovery blob

**Built** (`app-core/src/recovery.rs`, v4). Plaintext protobuf `RecoveryBlob { identity_keypair, servers[], profile_key, display_name, groups[{master_key, server_index}], storage_key }`; envelope `version(1) || nonce(12) || AES-256-GCM`. Server URLs are interned. It does **not** contain the rotation key.

Uploaded at signup and re-uploaded silently (using the cached blob key) when blob-relevant state changes. `PUT /v1/recovery` is session-authenticated and rate-limited; `GET /v1/recovery/{did}` is unauthenticated (Known gaps). The intended replication to every server the user is on depends on multi-server identities, which are not built.

### Signup (story 1)

1. Scan or tap an invite (`51`); the app validates it with the server.
2. Name (required) and photo (optional).
3. Create a passkey (or choose a phrase, or skip).
4. HKDF the seed into the rotation key and blob key; generate the identity key and prekeys.
5. Build, sign, and submit the genesis and update PLC ops.
6. Encrypt the recovery blob.
7. `POST /v1/accounts` with the DID, identity key, an identity-key signature over `register:{did}:{server_url}`, registration ID, device ID 1, prekeys, the blob, and the invite token. The server verifies the signature and checks the identity key against the DID's `#avalanche` verification method in PLC (`routes/registration.rs`), and applies the registration gate (`24`).
8. Land in Chats; the post-onboarding redirect (if any) opens the inviter's DM.

### Recovery, blob path (story 4)

1. "Recover" → WebAuthn assertion (discoverable) with the PRF salt, or phrase + server URL.
2. Derive the rotation key and blob key; recompute the DID.
3. Resolve the DID via PLC to find the current home server (falls back to the signup URL).
4. `GET /v1/recovery/{did}` → decrypt.
5. Restore the identity keypair (same safety number), profile key and name, storage key, and server list.
6. Sign `replace:{did}:{old}:{new}:{nonce}` with the rotation key and call `/v1/devices/replace` on the first server (Known gaps).
7. For each group in the blob: persist a group row, fetch state, register a new delivery pseudonym, re-seed and distribute a sender key. Peers' old sender keys are not redistributable, so messages already sent under them are lost.
8. Storage sync pulls the rest of durable state (`05` §11).

### Day-to-day authentication (story 5)

**Built.** The database is unlocked with a platform-protected key. Session tokens are opaque and expire; on expiry the core does challenge-response (`POST /v1/auth/challenge` → `POST /v1/auth/token`, signing the nonce with the identity key) with no user interaction. Passkeys are never used day to day.

### Multiple identities (story 3)

**Built.** "Create a fresh identity" makes a new DID with fully independent keys and a separate passkey. Identities share no keys and no server-side state. All identities' chats appear in the unified inbox (`37`).

### What lives where (as built)

| Secret | Where it lives |
|---|---|
| Passkey / phrase | Password manager or paper. Produces the rotation key and blob key. |
| userHandle | Inside the passkey: the signup server URL. |
| Rotation key (P-256) | **identity.db on every device** (signup, recovery, link). Never on a server. |
| Identity key | identity.db on every device + the recovery blob. Public half in PLC. |
| Prekeys | Private halves in device.db; public halves on the server. |
| Recovery blob | Homeserver, encrypted under the blob key. |
| Blob key | identity.db on the signup or recovered device (not linked devices). |
| Storage key | identity.db on every device + the recovery blob + the link bundle. |
| Session token | Device. |

### Onboarding screens

Built on iOS and Android; Desktop uses the phrase path only.

- **Landing** — scan invitation (primary), enter invite code, recover, link this device (`04` §4).
- **Choose identity** — when the device already has identities and an invite arrives: pick an identity, create a new one, or recover. Picking an existing identity does not yet register on the new server (Known gaps).
- **New identity** — photo (optional), display name (required).
- **Passkey explainer** — create passkey (primary), use a recovery phrase instead, skip.
- **Recovery explainer** — recover with passkey (primary), or phrase + home server URL.
- **Progress console** — scrolling status during signup and recovery.
- **Server step** — a homeserver-provided onboarding webview. Not built; the invite response has no `server_step_url` (`51`).

## Planned

- **Build the no-blob recovery path**, or explicitly drop the claim. (The Proposed design changes what it means.)
- **Make recovery revoke the whole prior device set on every server** (`04` §7).
- **Add timeouts to server-side PLC fetches**, and rate-limit `/v1/devices/replace` by IP before doing any PLC fetch.
- **Fix the DID document key type** (publish the identity key with the X25519 multicodec, or as a libsignal-typed key), coordinated with the registration-time PLC check.
- **Stop hosting third-party content under the RP domain.** Move the demo server and its Projects off `*.theavalanche.net`, or move the passkey RP to a dedicated domain that serves only the AASA/assetlinks files and the desktop ceremony page (`56`).
- **Offer neutral passkey labels** (for example "Avalanche identity 2") for users who want personas unlinkable inside their own password manager.

## Proposed

**Pending project-owner review.** These change the identity contract (key hierarchy, PLC usage, wire formats, recovery endpoints) and need a migration plan for existing accounts before implementation. They were agreed in principle with the maintainer on 2026-10-03.

### P1. Wrap the root secret instead of deriving it

Generate a random 32-byte **root secret** at signup. Derive the rotation key and blob key from the root, not from the passkey. Store the root only **wrapped** (encrypted) under each recovery factor:

- the passkey's PRF output on the primary RP (`theavalanche.net` or its replacement);
- a **second passkey on a backup RP domain** with a different registrar and jurisdiction;
- the recovery phrase.

The wrapped copies are tiny and are stored with the recovery blob on every server the identity uses (and optionally exported to a file).

Why: today the root *is* the passkey's PRF output, so it can never be rotated, a compromised password manager is compromised forever, and a seized domain disables recovery for everyone. Wrapping makes each factor independently addable and revocable, and lets a backup domain keep recovery working through a domain seizure.

Cost: the DID can no longer be recomputed from the passkey alone with no server state; recovery needs one wrapped copy. Today's no-blob path doesn't exist anyway. Open question: where the recovering device learns which server holds a wrapped copy. The 64-byte userHandle can carry the identity's identifier plus a short server hint; phrase users record the server as they do today.

### P2. Priority-ordered rotation keys; nothing root-level on devices

PLC's `rotationKeys` is ordered: a higher-priority key can nullify operations signed by a lower-priority key within a 72-hour window. Use that:

- **Top priority:** the root-derived recovery key. Never stored on any device; produced only during a recovery or a deliberate "security settings" ceremony.
- **Lower priority (optional):** a per-identity device key for routine DID updates, or no device-held rotation key at all.

Linking stops shipping any rotation key. `/v1/devices/link` is instead authorized by the existing device: its session plus a signature over the link request by the identity key, with the server checking that `new_identity_key` equals the identity's existing key. A stolen device can then at worst submit lower-priority ops that the owner nullifies with the recovery key within 72 hours (verify PLC's handling of a lower-priority tombstone before relying on this).

### P3. A private, unpublished identity; did:plc becomes an opt-in public link

The problems with anchoring the private identity in PLC: the log is public and permanent; the genesis op publishes the signup server; every key change and migration is publicly timestamped; signup, linking, and recovery all depend on a third-party service (Bluesky PBC) being up and willing.

Proposal:

- The private identity is a **self-certifying identifier**: a hash of a genesis document containing the root public key(s). It is never published to any global directory.
- The current key document (identity key, rotation keys, servers) is held by the identity's own homeservers and **passed to contacts over existing sessions**. Key changes and server moves are statements signed by the rotation key, delivered to contacts the same way. Contacts verify them against keys they already hold.
- Strangers find you through invites and QR codes, which already carry the server address (`51`). There is no lookup by bare identifier.
- **did:plc becomes optional.** A user who wants a public presence (for example on Bluesky) links a `did:plc` to their private identity with a signed attestation, only when they choose to.

This matches `00`'s own framing (public side as Projects, private substrate as ours) better than anchoring the private substrate in a public log. It pairs with the client-side federation proposal in `13`, which needs no global resolution either.

What it gives up: Bluesky identity sharing by default, and lookup of a stranger by bare DID. Existing `did:plc` identities would keep working as "published" identities through a transition.

### P4. Authenticated recovery-blob fetch

`GET /v1/recovery/{id}` requires a signature over a server challenge by a **fetch key** derived directly from each recovery factor (for example `HKDF(PRF, "actnet-fetch-v1")`), whose public half was registered when the factor was set up. That doesn't need the root (so it works with P1), stops DID-holders from probing servers for membership, and lets the server rate-limit per identity. Stop returning `device_ids`; a recovery that revokes the whole device set (`04` §7) doesn't need them.

## Speculative

- **Bluesky-linked identities.** Let a user authenticate with Bluesky (ATProto OAuth) to prove ownership of an existing `did:plc` and register it on a homeserver. No passkey, no PLC writes, no recovery blob; recovery is "log in with Bluesky again", with a safety-number change. Under P3 this becomes the opt-in public link rather than a separate identity kind. Privacy cost: Bluesky can see the identity is used on Avalanche.
- **Other OAuth providers as recovery authorities.** Same pattern, with the homeserver creating an identity linked to the OAuth account. Lossy recovery (new keys).

## Rationale and rejected alternatives

- **No phone number or email (decided).** Removes the strongest real-world identifier from the server.
- **Passkey-based recovery (decided).** The best recovery UX available: synced by the platform, no seed phrase to lose. P1 keeps it and removes its single points of failure.
- **Identity key not derived from the passkey (decided).** Keeps the Signal identity key random; the passkey controls the DID, the blob restores the identity key.
- **Genesis op without the identity key (decided, under review by P3).** Made the DID recomputable from passkey + signup server. Its cost — publishing the signup server — is one reason for P3.
- **Universal RP domain rather than per-homeserver RPs (decided).** One passkey works across servers and only official apps can run recovery. Cost: concentrates risk on one domain (Known gaps, P1).
- **Homeserver-held recovery keys (rejected in `00`'s original open question).** Would let a seized server take over identities.
- **Consumer cloud backup as the recovery substrate (rejected).** Re-centralizes on a subpoenable party; see `05`.
