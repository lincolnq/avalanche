# 55 — Avatars (profile and group photos)

> **Status:** Partial — encrypted profile and group avatars are built in app-core and the server. iOS can set and display both. Android displays both but cannot set them. Desktop shows initials only. Known gaps: the shared blob namespace lets any account delete or overwrite other blobs; a newly linked device doesn't fetch its own avatar.
> **Last verified against code:** 2026-10-03

## Summary

An avatar is a small JPEG, encrypted on the client and stored on the homeserver as an opaque, **overwrite-in-place** blob. There's one per account and one per group. The server never sees the image or learns which group a group avatar belongs to. The pointer to the current image — a **version** and a **digest** — lives inside already-encrypted state:

- **Profile avatar:** inside the encrypted profile blob (`52`), encrypted under the user's **profile key**.
- **Group avatar:** inside the encrypted group state (`03`), encrypted under a key derived from the **group master key**.

So whoever can read the name can read the picture, and no new key distribution is needed.

Code:
- server `core/crates/server/src/routes/avatar.rs`, `blobstore.rs`, config `avatar_max_size_bytes`
- app-core `profile.rs` (`encrypt_avatar`, `decrypt_avatar`, `avatar_digest_b64`, `MAX_AVATAR_BYTES`), `lib.rs` (`set_own_avatar`, `clear_own_avatar`, `own_avatar`, `contact_avatar`), `messaging.rs` (`sync_contact_avatar`), `groups.rs` (`set_avatar`, `set_group_avatar`, `clear_group_avatar`, `group_avatar`, `fetch_group_avatar`)
- crypto `groups/group_key.rs` (`avatar_key`, `avatar_object_id`)
- store `avatar_cache` (device-local)
- iOS `Utils/AvatarEncoder.swift`, `Views/Common/AvatarCropView.swift`, `EditableAvatar.swift`

## Current design

### Image format and size limits

*Built.*

| Layer | Limit |
|---|---|
| iOS encoder | Crop to square, render at 512×512, JPEG quality binary-searched to ≤ 48 KiB (`AvatarEncoder`) |
| app-core | Plaintext ≤ 60 KiB (`MAX_AVATAR_BYTES`), else `AppError::AvatarTooLarge` |
| server | Ciphertext ≤ 64 KiB default (`AVATAR_MAX_SIZE_BYTES`); 256 KiB route body limit |
| server rate limit | 60 uploads per account per hour (`LIMIT_AVATAR_UPLOAD`) |

App-core does no image processing. The platform crops and compresses the image, and app-core encrypts and uploads it.

### Encryption

*Built.* AES-256-GCM, `nonce(12) ‖ ciphertext+tag` — the same layout as the profile blob.

- **Profile avatar key** = the user's profile key.
- **Group avatar key** = `GroupKey::avatar_key()`, a domain-separated hash of the master key (`"actnet-group-avatar-key-v1"`). Every member can compute it, so no key is distributed.

The SHA-256 digest of the ciphertext rides in the encrypted pointer. Recipients verify it before decrypting, so the server can't substitute a different image (it can only make the avatar fail to load).

### Server storage

*Built.* Avatar blobs go in the same `BlobStore` as attachments (`35`), but **without a TTL or a DB row**: the object id is deterministic, so a re-upload replaces the old bytes and storage is bounded by account and group count.

| Endpoint | Auth | Object id |
|---|---|---|
| `PUT /v1/profile/avatar` | session | Server-derived from the internal account id: `UUID(SHA-256("actnet-profile-avatar-id-v1" ‖ account_id)[..16])` |
| `DELETE /v1/profile/avatar` | session | same |
| `GET /v1/profile/avatar/{did}` | session | Server resolves the DID. Identical `404` for "no such DID" and "no avatar" (existence hiding, like `GET /v1/profile/{did}`) |
| `PUT /v1/groups/avatar/{id}` | session (rate limiting only) | Client-derived from the master key (`GroupKey::avatar_object_id`). Knowing it is the capability; the server never links it to a group (`03` §3.9) |
| `GET /v1/groups/avatar/{id}` | none | same |
| `DELETE /v1/groups/avatar/{id}` | session (rate limiting only) | same |

Uploads stream through the homeserver. The allocate-then-presigned-upload flow used for attachments is skipped because avatars are tiny.

### Profile avatar flow

*Built.*

1. **Set** (`set_own_avatar`): encrypt under the profile key → `PUT /v1/profile/avatar` → bump `avatar_version` and set `avatar_digest` in the profile plaintext → re-encrypt and re-upload the profile blob → cache the JPEG locally.
2. **Clear** (`clear_own_avatar`): delete the blob, re-publish the profile without an avatar ref, clear the local cache.
3. **Receive** (`sync_contact_avatar`): runs whenever a contact's profile is fetched and decrypted (`52`). If the advertised `avatar_version` is newer than the cached one, it fetches `GET /v1/profile/avatar/{did}`, verifies the digest, decrypts and caches. If the profile has no avatar, it drops the cached copy. Errors are swallowed and never block profile or message handling.

Liveness therefore follows profile liveness. Until `profile_version` exists (`52`), a contact sees a new avatar only after their client refetches the profile (on opening the conversation, subject to the throttle).

### Group avatar flow

*Built.*

1. **Set or clear** (`set_avatar` in `groups.rs`): upload (or best-effort delete) the blob at the master-key-derived object id, then submit a `modify_avatar` group change. That change carries `GroupAvatar { version, digest, size_bytes }` sub-encrypted in the group state and is gated by `modify_title_role`. The change emits an `AvatarChanged` system event, rendered in the timeline per the group system-message convention.
2. **Receive** (`fetch_group_avatar`): call when a group is opened. If the cached version is behind the version in the local group state, fetch, verify, decrypt and cache. If the state has no avatar, drop the cache.

The authoritative "this group has avatar v3 with digest D" is the group state. The blob is just storage.

### Local cache

*Built.* Decrypted JPEGs go in `avatar_cache`, a **device** table keyed by `(kind, id)` with a version. It is not synced and is rebuilt by fetching.

### Platform support

| | Set own | Set group | Display contacts / groups / own |
|---|---|---|---|
| iOS | Yes (Settings → identity, crop sheet) | Yes (group detail, admin-gated) | Yes |
| Android | No | No | Yes (`AppViewModel` `contactAvatar` / `fetchGroupAvatar` / `ownAvatar`) |
| Desktop | No | No | No (initials and bot hexagon only) |

This is a parity gap against the three-platform rule (`62`).

## Known gaps

1. **Shared blob namespace: any account can delete or overwrite other blobs (P1).** The group avatar routes accept *any* UUID and act on the shared `BlobStore`, which also holds attachments and profile avatars.
   - **Attachments:** `DELETE /v1/groups/avatar/{id}` with an attachment id (visible in any message the caller received) deletes that attachment for every recipient. `PUT` overwrites it, and recipients' digest checks then fail.
   - **Profile avatars:** their ids are SHA-256 of the internal account id, which is a sequential `BIGINT IDENTITY`. So they're enumerable: any authenticated account can delete every user's profile avatar, and anyone can download every profile-avatar ciphertext without authentication. That bypasses the authenticated `GET /v1/profile/avatar/{did}` and reveals which account numbers have avatars, though not the images.

   Integrity holds because of the digests, but availability and existence-hiding don't. **Fix:** give each kind its own namespace (a separate directory or key prefix per kind), salt the profile-avatar id with a server secret, and reject group-avatar ids that collide with other kinds.
2. **Group avatar delete and overwrite aren't tied to the group role (P2).** The real gate is the `modify_avatar` state change (`modify_title_role`). The blob itself can be replaced by anyone who knows the object id: any current member, a removed member, or anyone who ever saw an invite link carrying the master key. Recipients reject the result on a digest mismatch, so this lets them blank the avatar but not spoof it. A per-version object id (derive the id from master key + version) would make old-key holders unable to touch the current blob.
3. **A freshly linked device doesn't fetch its own avatar** (`own_avatar` is a local read only, populated by `set_own_avatar`). The avatar appears only after it's set again.
4. **`set_group_avatar` holds the core lock across the upload and the group change** (it's on the group path, the documented exception in `core/CLAUDE.md`). The upload could move outside the lock.
5. **Android and Desktop parity** (table above).

## Planned

- Fix gaps 1–3.
- Avatar setting on Android and Desktop; photo avatar display on Desktop.
- Faster avatar liveness through `profile_version` (`52`).

## Speculative

- A user-set private `photo_override` for a contact (`52`).
- Avatar in shared contact cards (`SharedContact` reserves fields for it).

## Rationale and rejected alternatives

- **Avatar as a normal attachment pointer inside the profile** (the earlier `52` sketch) — replaced by overwrite-in-place blobs. Attachments have a 45-day TTL and are per-send; an avatar is long-lived and replaced, so a deterministic id with no row and no garbage collection fits better.
- **Distributing a separate avatar key** — unnecessary. Reusing the profile key (or deriving from the master key) means anyone who can read the name can read the picture, and nobody else can.
- **Server-visible group-to-avatar mapping** — rejected. The master-key-derived object id keeps `03` §3.9's "no group links on the server" property.
- **Allocate plus presigned upload for avatars** — skipped. At ≤ 64 KiB, streaming through the homeserver is negligible.
