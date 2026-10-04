# 35 — Attachments, link previews, and shared contact cards

> **Status:** Partial — encrypted attachments (LocalFs backend), link previews, the fullscreen viewer, outgoing image processing, and shared contact cards are built on iOS, Android, and Desktop. The download path has open security gaps (see *Known gaps*). Not built: S3 backend, blurhash, auto-download settings, storage-management controls, background upload throttling.
> **Last verified against code:** 2026-10-03

How a user sends a photo, video, audio clip, PDF, or arbitrary file end-to-end encrypted, with the homeserver holding only ciphertext it cannot read. This follows Signal's model closely; divergences are called out.

## Summary

A message and its attachment travel by different paths. The **message** is a small E2E ciphertext on the normal Double Ratchet / Sender Key path. The **blob** is encrypted with a one-off key, uploaded to dumb server storage, and referenced by an `AttachmentPointer` carried inside the encrypted message. The server never sees the key. Link previews reuse the same machinery (the og:image is a normal encrypted attachment). Shared contact cards ride inline in the same `TextMessage` (no blob).

Code: proto `core/proto/content.proto` (`AttachmentPointer`, `LinkPreview`, `SharedContact`); crypto `core/crates/crypto` (`attachments::encrypt/decrypt`); client `core/crates/app-core/src/messaging.rs` (`download_attachment_inner`, `anti_spoof_previews`); net `core/crates/net/src/lib.rs` (`allocate_attachment_upload`, `upload_attachment_blob`, `download_attachment`); server `core/crates/server/src/routes/attachments.rs`, `blobstore.rs`, GC in `tasks/mod.rs`; local store `core/crates/store/src/attachments.rs` (`message_attachments`, `message_link_previews`).

## The shape of the problem

- The **message** goes through the normal ratchet → server queue → recipient path.
- The **blob** (potentially megabytes) does not go through the message queue. It is encrypted with a one-off symmetric key, uploaded to bulk storage, and referenced by a **pointer** inside the message.

Bulk storage is dumb and untrusted; confidentiality rides entirely on the pointer being inside the encrypted envelope.

## Core model (Signal's encrypt-then-upload)

*Status: Built.*

1. **Pad** the plaintext to a bucket size (see *Padding*).
2. **Encrypt** locally with fresh random key material.
3. **Allocate** an upload slot (`POST /v1/attachments`) → `attachment_id`, an upload descriptor, the absolute `download_url`, and the TTL deadline.
4. **Upload** the ciphertext by replaying the upload descriptor verbatim (`upload_attachment_blob`); the client is backend-blind.
5. **Send** a normal E2E `TextMessage` whose `AttachmentPointer`(s) carry the key, digest, content type, size, and URL.
6. **Recipient** downloads by URL, **verifies the digest before decrypting**, decrypts, unpads, renders.

## Encryption scheme

*Status: Built.* **AES-256-CBC + HMAC-SHA-256 (encrypt-then-MAC), exactly as Signal does for attachments** — not the AES-256-GCM used elsewhere.

- **Default to copying Signal** — audited, deployed-at-scale code for this exact job.
- **Incremental verification of large files.** Signal's incremental-MAC variant lets a client verify a video as it streams in; GCM is all-or-nothing.
- **No GCM per-key/nonce fragility** to reason about.

Key material is **64 bytes**: 32-byte AES key ‖ 32-byte HMAC key, fresh per attachment, never derived from the ratchet. The stored blob is `IV(16) ‖ AES-256-CBC(PKCS7(padded)) ‖ HMAC-SHA-256(IV ‖ ct)`. The **digest** in the pointer is SHA-256 over exactly the stored bytes; a mismatch aborts with no decryption attempt, so a malicious storage layer cannot substitute content.

The `crypto` crate wraps libsignal's primitives (`signal_crypto` AES-CBC + HMAC, `libsignal_protocol::incremental_mac`) at the pinned commit rather than reimplementing them.

### Padding

*Status: Built.* Ciphertext length leaks information, so plaintext is padded to a bucket using Signal's monotonic ~5% geometric steps. The unpadded length rides in the encrypted pointer (`size_bytes`) so the recipient trims after decrypting.

## Server: upload & download

*Status: Built (LocalFs backend).* `core/crates/server/src/routes/attachments.rs`.

### Allocate an upload slot

```
POST /v1/attachments
Auth: required
Body: { size_bytes }            # ciphertext size, for cap/quota checks
Response: 201 {
  attachment_id,                # opaque server-minted UUID
  upload: { url, method, headers },   # where/how to PUT the ciphertext
  download_url,                 # absolute URL carried in the pointer
  expires_at_ms                 # blob TTL deadline
}
```

The server checks the per-attachment size cap and per-account rate limit, and records `attachment_id`, owner account, declared size, and expiry in the `attachments` table — never the key or content.

### Upload

`PUT /v1/attachments/{id}` (LocalFs) — owner-only; body capped at the per-attachment limit (110 MB route body limit, 100 MB default cap).

### Download

```
GET /v1/attachments/{id}
Auth: none
Response: 200 octet-stream (single byte-range supported)
```

- **Unauthenticated by design.** The unguessable, server-minted id *is* the capability, and it only exists inside an E2E message. The server cannot enforce "only the intended recipient" anyway (sealed sender, no plaintext); the id is a random UUID, so it cannot probe membership; the bytes are E2E ciphertext. Leaving download open lets a recipient fetch from a homeserver it has no account on and lets a future S3 backend serve a presigned URL directly. Matches Signal's CDN. Allocate and upload stay authenticated (they need an owning account for cap, quota, and owner check).
- **Range requests:** a single `bytes=` range is supported. The server currently reads the whole blob into memory per request (`blob_store.get` returns `Vec<u8>`), which is fine for LocalFs at today's sizes but not for streaming large media.

### Delete

`DELETE /v1/attachments/{id}` — owner-only, best-effort, idempotent. TTL GC is authoritative. No client calls it today.

## Storage backends

*Status: LocalFs Built; S3 Planned.*

A `BlobStore` trait (`server/src/blobstore.rs`) with a `LocalFs` impl: one file per UUID under `ATTACHMENT_BLOB_DIR` (default `/var/lib/avalanche/attachments`; ids are UUID-validated so a path can't escape the base dir). The same store also holds avatar blobs (`55`).

**Planned: S3-compatible backend.** The homeserver mints presigned URLs (SigV4, e.g. via `object_store`); the client does plain HTTP and never needs a provider SDK — the upload descriptor already carries URL + method + headers, so no client change is needed. Caveats: the client must replay exactly the signed headers; presigned URLs expire in minutes (re-allocate on stall); sign a fixed `Content-Length` and rely on the allocation-time quota; single-PUT only (well under the multipart threshold).

## The pointer (`AttachmentPointer` in `TextMessage`)

*Status: Built.* `TextMessage.attachments = 2` (`repeated`), so an album or "PDF + caption" is one message — no separate media type.

```protobuf
message AttachmentPointer {
  string url            = 1;   // full download URL on the hosting homeserver
  string content_type   = 2;   // MIME
  bytes  key            = 3;   // 64 bytes: AES-256-CBC key ‖ HMAC-SHA-256 key
  bytes  digest         = 4;   // SHA-256 over the exact stored ciphertext blob
  uint64 size_bytes     = 5;   // *unpadded* plaintext size
  string file_name      = 6;
  uint32 width          = 7;
  uint32 height         = 8;
  uint32 duration_ms    = 9;
  string blurhash       = 10;  // defined; not generated
  bytes  thumbnail      = 11;  // small inline preview (downscaled JPEG)
  string caption        = 12;
  uint32 flags          = 13;  // bitset: VOICE_NOTE, GIF, BORDERLESS, ...
  reserved 14 to 20;
}
```

`TextMessage` fields today: `body = 1`, `attachments = 2`, `preview = 3` (link previews), `contact = 4` (shared contact cards), reserved `5 to 10` (mentions, reply_to, formatting).

The pointer carries a **full download URL**, not a bare id (Decision 7). The URL points at the homeserver's own download route, which survives a LocalFs → S3 switch (the route can 302 to a presigned URL). **The recipient currently fetches whatever URL the sender put in the pointer** — see *Known gaps*.

## Thumbnails & previews

*Status: inline thumbnail Built; blurhash not generated.*

- **Inline `thumbnail`** — a small downscaled JPEG embedded in the pointer (encrypted with the message), so a chat scrolls without pulling megabytes.
- **`blurhash`** — the field exists but no client generates it (Decision 9).
- PDFs/docs render an icon + `file_name` + size.

### Fullscreen image viewer

*Status: Built (iOS, Android, Desktop).* Tapping an image opens a viewer that pages through **every** image attachment in the conversation in timeline order. It decodes at a higher cap than the inline bubble (~4096px iOS, 2048px Android) and reuses the inline loader and decode cache.

- **iOS / Android** — pinch and double-tap zoom, drag to pan when zoomed; horizontal paging only while unzoomed; swipe down or close button dismisses. iOS: `TabView(.page)` over per-page `UIScrollView`s (`ImageViewerView.swift`). Android: `HorizontalPager` with `userScrollEnabled = !zoomed`.
- **Desktop** — `←`/`→` and on-screen arrows, wheel/pinch zoom, double-click, click-drag pan, Esc/close/backdrop dismiss. The transform is applied via the CSSOM to satisfy the strict CSP (`ImageViewerModal.tsx`).

Lives at the shared rendering layer (`AttachmentView` → `MessageBubble` → `ConversationView`), so DMs and groups behave identically.

## Outgoing image processing (client-side, Signal-aligned)

*Status: Built (iOS, Android).* The client re-encodes every outgoing image rather than shipping original bytes. One pass:

- **Bakes in EXIF orientation** (Android's `BitmapFactory` ignores the EXIF rotation tag, so un-normalized photos arrive sideways).
- **Strips EXIF/metadata** — GPS, device model, timestamps. A **privacy requirement**, and the main reason the re-encode is unconditional.
- **Caps resolution** — longest edge 2048 px, JPEG quality ~0.9. One tier for now.

App-core does no image processing. Constants: `mobile/ios/Shared/OutgoingImage.swift` (`OutgoingImage.maxDimension`/`jpegQuality`, `UIImage.preparedForSending`) and Android `Views/Chats/AttachmentViews.kt` (`OUTGOING_MAX_DIMENSION`/`OUTGOING_JPEG_QUALITY`, `processOutgoingImage`), applied at the photo picker, clipboard paste, and share-in.

The **iOS share extension does not decode or re-encode** — decoding a 24–48 MP photo blows its ~120 MB memory ceiling. It copies the encoded bytes to the App Group and the main app runs the resize/strip when it stages the image into the composer.

**Why JPEG, and the forward-compat guarantee.** JPEG decodes everywhere. HEIC has unreliable Android decode below API 29 and no desktop decode; WebP has no native iOS encoder. The receive path is format-agnostic (`image/` prefix match, system decoders that sniff JPEG and WebP), so switching *send* to WebP later is backward-compatible. That guarantee covers {JPEG, WebP} only, not HEIC/AVIF.

## Link previews

*Status: Built (iOS, Android, Desktop).* When a message body contains a URL, the sender shows a rich preview card (title, description, image, source domain). It follows Signal's `Preview` shape and reuses the attachment system for the image.

**Where generation runs.** Fetch and OpenGraph parsing happen in the **native client layer**, not app-core: iOS uses `LPMetadataProvider`; Android uses Jsoup; Desktop uses a Rust Tauri command (`fetch_link_preview` in `desktop/src-tauri/src/lib.rs`, 5 MiB og:image cap). This keeps outbound fetches to arbitrary URLs and an HTML parser out of app-core, which also runs in bots. App-core owns only the protocol: the `LinkPreview` wire type, uploading the og:image through the normal attachment path, threading previews through send/receive/store, and the anti-spoof rule (`anti_spoof_previews`). Generation is always client-invoked; DMs and groups both.

**The load-bearing privacy invariant: the sender generates the preview at compose time; the recipient never fetches the URL.** If recipients auto-fetched, a sender could paste a tracking URL and harvest the IP of everyone the message reaches. The whole preview travels inside the E2E message.

```protobuf
message LinkPreview {
  string            url         = 1;  // must occur in TextMessage.body (anti-spoof)
  string            title       = 2;
  AttachmentPointer image       = 3;  // og:image via the attachment system (optional)
  string            description = 4;
  uint64            date        = 5;  // published date, unix millis; 0 = unknown
  reserved 6 to 10;
}
```

Rendering: the source domain is derived from `url`; layout comes from image dimensions (landscape → hero card, small/square → inline thumbnail); **render only if `preview.url` occurs in `TextMessage.body`** (otherwise a sender could show a trustworthy card that links elsewhere).

**Not built:** the per-account "don't generate link previews" setting this doc previously promised. There is no opt-out today.

## Shared contact cards

*Status: Built.* A `SharedContact { did, name }` in `TextMessage.contact = 4` — a person's DID plus the name the sender knows them by. Structured and inline (no blob): tiny, immutable, renders without a fetch. **Deliberately carries no `profile_key`** — sharing it would let a third party decrypt the subject's profile without their consent; the recipient learns the real profile on first contact. Recipient actions: save (sets a local nickname and curates the row, `52`), message, copy. Code: `SharedContactFfi` (`app-core/src/lib.rs`), `save_shared_contact`, iOS `SharedContactCard.swift`.

## Lifecycle & garbage collection

*Status: Built (TTL GC); client cleanup Partial.*

**The server cannot reference-count blobs** — it can't see which message references which id. Consequences:

- **Blobs have a TTL, not a reference count.** Every blob gets `expires_at` at allocation; a background task (`server/src/tasks/mod.rs`) deletes expired rows and blobs. **Default 45 days** (`ATTACHMENT_BLOB_TTL_SECS`), deliberately longer than the 30-day message-queue retention so an offline or newly linked recipient can still pull. This is a delivery buffer, not a backup.
- **Orphan blobs are fine.** Upload-then-fail just expires.
- **Expiry alignment.** When a message has a disappearing timer, the client should delete its local copy on schedule. **Today it does not** — see *Known gaps*.
- **Forwarding re-uploads** under a fresh key rather than reusing the original id: the original may have expired, reuse would let storage correlate "same blob, different conversations", and a forward shouldn't be deletable by the original sender. (Forwarding UI is not built yet.)

## On-device storage management

*Status: Planned (Tier 1), Speculative (Tier 2).* The delivery buffer's TTL is the hard constraint: anything freed locally is re-fetchable only within ~45 days.

**Tier 1 — local-only controls** (no backup substrate needed):

- **Auto-download settings** per network type and media kind (mirror Signal). Not built — today every attachment downloads when its bubble renders.
- **Trim old local media** ("keep media for {forever, 1y, 6m, 30d}"). Past the TTL, unrecoverable — the UI must say so.
- **Review-and-delete by size.** Deleting a blob must **keep its message** with an explicit "media removed" placeholder (Signal deletes the whole message, a long-standing complaint).

**Tier 2 — true offload** (free the bytes, re-hydrate on demand) is not implementable against the delivery buffer; it needs a durable encrypted media backup store that does not exist. Signal gates this behind paid Secure Backups for the same reason.

## Limits, quotas, abuse

*Status: Built.*

- **Per-attachment cap:** 100 MB default (`ATTACHMENT_MAX_SIZE_BYTES`), checked at allocation and at upload.
- **Per-account quota:** request-rate limit on `POST /v1/attachments` plus a rolling bytes cap (500 MB/hour default, `ATTACHMENT_BYTES_PER_HOUR`).
- **Content scanning is impossible by construction** — the server holds only ciphertext. Abuse handling is report-based (`12`). This is an accepted property of E2E media.

## Multi-device

*Status: Built.* The blob is uploaded once; every recipient device and the sender's own devices receive the same pointer and download the same blob with the same key.

## Federation

*Status: Planned, shape depends on `13`.* Pointers already carry absolute URLs and download is unauthenticated, so a recipient on another server can fetch from the sender's homeserver directly. If federation goes client-side (`13`, Proposed), that is the whole story, constrained by the host rule below. If it stays server-to-server, proxy-and-cache at the recipient's homeserver is the alternative.

## Known gaps

1. **Arbitrary-URL download (security, P0).** The recipient fetches whatever `AttachmentPointer.url` the sender supplied, with no host check, no scheme check, and no response-size cap (`net::Client::download_attachment` reads `resp.bytes()` in full; `messaging.rs:download_attachment_inner`). Attachments download automatically when the message renders — including in an un-accepted message request — so:
   - **IP harvest:** any sender can point the URL at their own server and learn every recipient's IP and fetch time. This is exactly the leak the link-preview design forbids.
   - **Memory DoS:** a malicious host can stream an unbounded body.
   - **Local-network probe:** `http://` and private-address URLs are fetched too.
   - **Fix (Planned):** only fetch from the hosting homeserver(s) the recipient knows for that conversation (or carry `{server, attachment_id}` and build the URL locally); require `https`; cap the read at the padded size implied by `size_bytes` plus slack; do not auto-download for message requests.
2. **Download holds the core lock across the network (P1).** `AppCore::download_attachment` takes `inner` and then does the HTTP fetch (`app-core/src/lib.rs:download_attachment`), violating the `core/CLAUDE.md` rule. A slow host stalls every other locked operation (sends, group ops) for up to the request timeout. Combined with gap 1, a sender can stall a recipient's whole app. Fix: clone the client out of the lock; this path needs no ratchet state.
3. **Deletion and expiry leave attachment keys and plaintext behind (P1).** `tombstone_message` (FOR_EVERYONE), `delete_message_for_me`, and `delete_expired_messages` (disappearing messages) remove `message_history` rows but **not** the matching `message_attachments` / `message_link_previews` rows (no FK cascade; only `delete_conversation` cleans them). The decryption keys and inline thumbnails survive. The platform plaintext caches survive too: every platform writes the decrypted blob to disk keyed by attachment id — iOS `Caches/attachments/<id>` (`AppState.attachmentData`), Android `filesDir/attachments/<id>` (`AppViewModel.attachmentData`), Desktop `app_cache_dir/attachments/<id>` — unencrypted and never cleaned on delete, expiry, or logout. A seized device yields media the user believed was deleted or expired.
4. **Cleartext attachment cache (P1).** Same caches as above sit outside the SQLCipher store. Encrypt them (or keep them in-memory) and clear them on account removal.
5. **Server holds whole blobs in memory per download (P3).** Fine at LocalFs scale; revisit with S3/streaming.

## Planned

- Fix gaps 1–4.
- S3 `BlobStore` backend.
- Auto-download settings and Tier-1 storage controls.
- Link-preview generation opt-out setting.
- **Background upload throttling.** Today a rate-limited upload errors (`429`, no `Retry-After`) and the send is abandoned. Uploads should pace themselves: short-term overage → back off and continue silently (needs `Retry-After` and a background upload queue); gross overage (≈ a day's quota at once) → hard error.
- Best-effort `DELETE /v1/attachments/{id}` from the author on delete-for-everyone and on short disappearing timers (correctness doesn't depend on it; TTL is authoritative).
- Desktop link-preview fetch hardening (SSRF guard on redirects, `image/*` content-type check, tighter body cap) and re-verifying the digest on Desktop cache hits.

## Speculative

- View-once media (a client-enforced policy over the same pointer).
- WebP on send (backward-compatible, see above).
- Tier-2 media offload once an encrypted backup substrate exists.

## What we are explicitly NOT doing

- **No server-side transcoding** — impossible without plaintext; clients send sane sizes.
- **No streaming upload of in-progress recordings** — finalize, then upload. Live media is the calls path.
- **No cross-attachment dedup** — dedup would leak content equality to storage.

## Decisions (locked 2026-06-27)

Numbers marked *(config)* are tunables.

1. **Encryption — CBC+HMAC (Signal-exact)**, accepting the one divergence from the app's default AEAD, for incremental verification and Signal parity.
2. **Default blob TTL — 45 days** *(config)*, exceeding the 30-day queue retention.
3. **Per-attachment cap — 100 MB** *(config)*.
4. **Per-account quota — rate limit + ~500 MB/hour bytes cap** *(config)*.
5. **Storage backend — LocalFs first**, behind `BlobStore`, S3 later with no protocol change.
6. **Naming — `BlobStore` / `routes/attachments.rs` / `/v1/attachments`**, deliberately not "Storage" (that's the `05` storage service at `/v1/storage/*`).
7. **Pointer is a full `url`, not a bare `attachment_id`.** Simpler, survives a backend switch. The download being unauthenticated means a recipient needs no account on the hosting server. *Consequence found later: the client must constrain which hosts it will fetch from (Known gap 1).*
8. **Groups and DMs ship together** — attachments live in `TextMessage`, and `send_to_target` handles both.
9. **Thumbnails: downscaled inline JPEG, no blurhash generation** in the first cut.
10. **Eager download by default** — no per-network settings UI yet.

## Staging

1. **First cut — Built.** `AttachmentPointer` in `content.proto`; `crypto` encrypt/decrypt over libsignal; server `attachments` table, allocate/upload/download, `BlobStore` + `LocalFs`, TTL GC, cap/quota; client encrypt → upload → send → download → verify → decrypt → render on iOS, Android, and Desktop.
2. **Link previews — Built.**
3. **Shared contact cards — Built.**
4. **Next:** security fixes (Known gaps 1–4), then auto-download settings, S3 backend, upload throttling.

## Rationale and rejected alternatives

- **AES-256-GCM for attachments** — rejected: no incremental verification for large media; Signal-parity wins.
- **Authenticated download** — rejected: adds nothing (unguessable id, ciphertext bytes, sealed sender means the server can't enforce recipients anyway) and blocks cross-server fetches and presigned URLs.
- **Bare `attachment_id` resolved against the recipient's own homeserver** — rejected for the full URL (simpler; id stability moot given the TTL). The host-constraint gap is the cost; fix it on the client rather than reverting.
- **app-core fetching link previews** — rejected: puts an SSRF surface and an HTML parser into the core that bots and servers also run; native fetchers (`LPMetadataProvider`) give better results.
- **Recipient-fetched link previews** — rejected: IP harvesting.
- **Reference-counted blob deletion** — impossible: the server can't see which message references which blob.
- **Reusing the blob on forward** — rejected: correlation leak, expiry coupling, and deletability by the original sender.
- **Deleting the whole message when its media is deleted (Signal's behavior)** — rejected: keep the message with a placeholder.
