# Attachments — Design

Status: decisions locked (2026-06-27); ready to spec the implementation. See *Decisions* below.

How a user sends a photo, video, audio clip, voice note, PDF, or arbitrary file, end-to-end encrypted, with the homeserver (or its object store) holding only ciphertext it cannot read. This follows Signal's proven model closely; where we diverge it's called out.

`01-technical-implementation.md` § "Media and attachments" sketches the four-step encrypt-then-upload flow. This doc is the full version: the encryption scheme, the server endpoints, the storage backends, the pointer format, and — the parts that sketch skips — **lifecycle/GC, padding, thumbnails, limits, and multi-device/forwarding behavior**.

The wire envelope is `core/proto/content.proto`. There is **no `MediaMessage` type today** — attachments are an unbuilt feature, and `TextMessage` already reserves fields `2 to 10` for "attachments, mentions, reply_to, formatting." So this design adds attachments *inside `TextMessage`* (a captioned photo is one `TextMessage` with a `body` and a pointer), not as a separate `oneof` body variant. (Older drafts of `01` showed a standalone `MediaMessage media = 2` envelope; that never shipped and the doc has been corrected to match the real proto.)

## The shape of the problem

A message and its attachment travel by different paths:

- The **message** is a small E2E ciphertext that goes through the normal Double Ratchet → server queue → recipient path (`10-server-implementation.md`).
- The **blob** is potentially megabytes. It does not go through the message queue. It is encrypted with a one-off symmetric key, uploaded to bulk storage, and referenced by a **pointer** carried inside the message.

The server stores the blob as opaque bytes and never sees the key — the key lives only in the E2E-encrypted message. This is the whole trick: bulk storage is dumb and untrusted; confidentiality rides entirely on the pointer being inside the encrypted envelope.

## Core model (Signal's encrypt-then-upload)

1. **Pad** the plaintext to a bucket size (see *Padding* below).
2. **Encrypt** the padded plaintext locally with fresh random key material.
3. **Allocate** an upload slot from the server → get an `attachment_id` and an upload URL.
4. **Upload** the ciphertext to that URL (homeserver disk or S3-compatible object store).
5. **Send** a normal E2E message — a `TextMessage` whose attachment pointer(s) carry the key, the digest, the content type, the size, and the `attachment_id`.
6. **Recipient** reads the pointer, downloads the ciphertext by `attachment_id`, **verifies the digest before decrypting**, decrypts, unpads, renders.

The homeserver never sees plaintext. An observer of the storage layer sees only padded ciphertext blobs of bucketed sizes, addressed by opaque ids.

## Encryption scheme

**Recommendation: AES-256-CBC + HMAC-SHA-256 (encrypt-then-MAC), exactly as Signal does for attachments** — *not* the AES-256-GCM the rest of the app uses for messages.

Rationale, in order of weight:

- **"Default to copying Signal."** This is audited, deployed-at-scale code for precisely this job.
- **Incremental verification of large files.** Signal's incremental-MAC variant lets a client verify a video as it streams in, rather than buffering the whole file before the first byte is trusted. GCM is all-or-nothing per the standard. For multi-hundred-MB video this matters.
- **No 64 GB-per-key / nonce-reuse fragility.** Not a real limit at our sizes, but CBC+HMAC sidesteps the question entirely.

Key material is **64 bytes**: a 32-byte AES key ‖ a 32-byte HMAC key, generated fresh per attachment (never reused, never derived from the ratchet). This 64-byte blob is the `key` field in the pointer.

The **digest** in the pointer is `SHA-256(ciphertext ‖ HMAC-tag)` — i.e. over exactly the bytes the recipient downloads. The recipient recomputes it before doing anything else; a mismatch aborts the download with no decryption attempt. This binds the pointer to the exact stored bytes and means a malicious storage layer cannot substitute content.

> **Note vs. the rest of the app.** Messages use AES-256-GCM (the `profile_key` in `content.proto`, the ratchet, etc.). Attachments deliberately use CBC+HMAC instead, for the reasons above — so the `AttachmentPointer.key` field is a 64-byte concatenation, not a GCM key. This is the one spot where attachments diverge from the app's default AEAD. **(DECIDED 2026-06-27: copy Signal — CBC+HMAC. See Decisions.)**

> **Implementation note.** The pinned libsignal (`4c460615`) already exposes the primitives we need, so the `crypto` crate *wraps* them rather than reimplementing: `signal_crypto::aes_256_cbc_encrypt`/`aes_256_cbc_decrypt`, `signal_crypto::CryptographicMac::new("HmacSha256", …)`, and `libsignal_protocol::incremental_mac::{Incremental, Validating, calculate_chunk_size}` (Signal's streaming-verification variant, already re-exported via `libsignal-protocol`). Add `signal-crypto` as a workspace dep pinned to the same commit; the transitive `aes`/`cbc`/`hmac`/`sha2` crates are already in the workspace, so no new external dependencies.

### Padding

Encrypting reveals nothing about content, but *ciphertext length* leaks information — a 2.1 MB blob is plausibly one specific photo. So we pad plaintext up to a **bucket size** before encrypting, using the same monotonic bucketing function Signal uses (round up in ~5% geometric steps). The unpadded plaintext length is carried in the encrypted pointer (`size_bytes`), so the recipient trims after decrypting. The storage layer and any network observer see only bucketed sizes.

## Server: upload & download

New endpoints under `/v1/` — authenticated, rate-limited allocate + upload, and an **unauthenticated** download (see below) — plus a storage abstraction with two backends.

### Allocate an upload slot

```
POST /v1/attachments
Auth: required
Body: { size_bytes }            # ciphertext size, for quota/limit checks
Response: 201 {
  attachment_id,                # opaque; also the download id
  upload: { url, method, headers },   # where/how to PUT the ciphertext
  expires_at                    # blob TTL (see Lifecycle)
}
```

- **Local-FS backend:** `upload.url` points back at the homeserver (`PUT /v1/attachments/:attachment_id`); the server writes the body to disk under that id.
- **S3 backend:** `upload.url` is a short-lived **presigned S3 PUT**; the client uploads directly to object storage and the homeserver stays out of the data path. `headers` carries any required `Content-Length`/checksum headers.

The server records the `attachment_id`, owner account, declared size, and `expires_at` in a small `attachments` table — enough for quota accounting and TTL GC, never the key or content.

### Upload

`PUT /v1/attachments/:attachment_id` (local backend only) — streams the ciphertext to disk. For the S3 backend the client PUTs straight to the presigned URL and never touches this route.

### Download

```
GET /v1/attachments/:attachment_id
Auth: required
Response: 200 (octet-stream, local backend)  |  302 → presigned GET (S3 backend)
```

- **Unauthenticated.** *(Revised 2026-06-27 — earlier this doc made download authenticated; reversed.)* Anyone who presents the (unguessable, opaque) id may fetch the blob — the id *is* the capability, and it only exists inside an E2E message sent to you. The server cannot enforce "only the intended recipient" anyway (sealed sender, no plaintext). Authenticating the GET would add almost nothing here: the id is a random UUID, not a guessable DID, so it can't be used to probe membership (unlike `GET /v1/profile/{did}`, which *is* authenticated for exactly that reason); and the bytes are E2E ciphertext, so confidentiality is the unguessable id + the decryption key, not an ACL. Leaving download open is what lets a cross-server recipient fetch from the *sender's* homeserver without holding credentials there, and lets a future S3 backend serve the pointer's URL (or a presigned object URL) with no homeserver auth hop. This matches Signal's CDN and the S3 presigned-URL model. **Allocate and upload stay authenticated** — those need an owning account for the size cap, the per-account byte quota, and the upload owner-check.
- **Range requests** are supported by both backends so large media can stream / resume.

## Storage backends

A `Storage` trait in the server with two impls, selected by config — mirrors the "small deployments use local FS, larger use S3" split already stated in `01`:

- **`LocalFs`** — blobs under a configured directory, served by Axum. Zero extra infra; fine up to a single modest VPS. This is the default for a fresh self-host.
- **`S3`** — any S3-compatible endpoint (MinIO, Backblaze B2, AWS S3, R2). The homeserver only mints presigned URLs and tracks metadata; bytes flow client↔store directly. This is the answer at any real scale (see capacity planning in `01`).

The `attachment_id` is backend-agnostic; switching backends is a config + migration concern, not a protocol change.

**Presigned URLs, no client SDK.** Both backends look identical to the client: PUT bytes to a URL, GET them from one. Only the homeserver is provider-aware — it mints the signature server-side (SigV4 via e.g. the `object_store` crate); the client does plain HTTP with no provider library. The same server code points at any S3-compatible store (MinIO, B2, R2, S3, GCS-interop) by config. Caveats: the client must replay exactly the headers the server signed (hence `upload.headers` above); presigned URLs expire in minutes (re-allocate on stall) independent of the blob TTL; we sign a fixed `Content-Length` and rely on the allocation-time quota check rather than letting a bare PUT self-cap; single-PUT only (our size cap stays well under the ~5 GB multipart threshold). CORS config is needed only if a future web client uploads direct-to-bucket.

## The pointer (`AttachmentPointer` in `TextMessage`)

Attachments hang off `TextMessage`, claiming `attachments = 2` from its reserved `2 to 10` block. Design choices:

- A `TextMessage` carries **zero or more** attachment pointers (`repeated`), so an album of photos, or a "PDF + caption" (`body` + one pointer), is one message — no separate media-message type, no mutually-exclusive `oneof` arm.
- The pointer carries a **full download `url`** (e.g. `https://sender-homeserver/v1/attachments/{id}`). *(Revised 2026-06-27 — earlier this doc proposed a bare `attachment_id` resolved against the recipient's own homeserver; a full URL is simpler, the id-stability argument is moot given the ~45-day TTL, and a URL pointing at the homeserver's own download route survives a LocalFs→S3 backend switch since that route 302s to a presigned URL. The server keeps an internal `attachment_id` for storage keying; only the URL crosses the wire.)* The GET is unauthenticated (see *Download*), so the recipient needs no account on the hosting homeserver — it just fetches the URL. Pre-federation the URL points at the shared homeserver; post-federation the recipient's homeserver may proxy-cache and rewrite it (see Federation).
- Per-attachment metadata travels in the pointer so the UI renders well before the full blob is fetched.

```protobuf
message AttachmentPointer {
  string url            = 1;   // full download URL on the hosting homeserver
  string content_type   = 2;   // MIME
  bytes  key            = 3;   // 64 bytes: AES-256-CBC key ‖ HMAC-SHA-256 key
  bytes  digest         = 4;   // SHA-256 over the exact stored ciphertext+tag
  uint64 size_bytes     = 5;   // *unpadded* plaintext size

  // UX metadata (all optional)
  string file_name      = 6;   // for docs: "minutes-2026-06.pdf"
  uint32 width          = 7;   // image/video
  uint32 height         = 8;
  uint32 duration_ms    = 9;   // audio/video
  string blurhash       = 10;  // tiny placeholder string for images/video
  bytes  thumbnail      = 11;  // small inline encrypted preview (see below)
  string caption        = 12;  // per-attachment, distinct from TextMessage.body
  uint32 flags          = 13;  // bitset: VOICE_NOTE, GIF, BORDERLESS, ...
}

message TextMessage {
  string body                            = 1;
  repeated AttachmentPointer attachments = 2;  // claims one slot of the reserved 2..10
  // field 3 = repeated LinkPreview preview (see Link previews below)
  // Still reserved for the rest of the roadmap: mentions, reply_to, formatting.
  reserved 4 to 10;
}
```

`flags` lets us distinguish a *voice note* (waveform UI, autoplay-on-tap) from an attached audio file, and mark animated GIFs, without separate message types. A pointer's `caption` is per-attachment; `TextMessage.body` is the message-level text — an album with one shared message uses `body`, a single captioned photo can use either.

## Thumbnails & previews

Two complementary mechanisms so a chat scrolls instantly without pulling megabytes:

- **`blurhash`** — a ~20–30 byte string rendering a blurred color placeholder immediately, before any download. Free, inline, no decryption.
- **inline `thumbnail`** — a small (e.g. ≤ 8 KB) encrypted JPEG/WebP preview embedded *in the message itself* (encrypted with the message, not uploaded as a separate blob). Gives a real low-res preview with no network round-trip. The full blob downloads on demand (or eagerly, see below).

For PDFs/docs we render an icon + `file_name` + size; no thumbnail unless the sender's client rasterizes page one.

## Outgoing image processing (client-side, Signal-aligned)

Before upload, the **client re-encodes every outgoing image** rather than shipping the original bytes — matching Signal's pipeline. One pass does three things:

- **Bake in EXIF orientation.** Camera/Photos JPEGs store rotation as an EXIF tag. `UIImage` honors it on display but Android's `BitmapFactory` does not, so an un-normalized photo arrives sideways for `BitmapFactory`-based recipients. Re-encoding from a decoded/upright bitmap makes the stored bytes canonically upright (`imageOrientation == .up` / no rotate tag), so every recipient renders it correctly regardless of decoder.
- **Strip EXIF/metadata.** A fresh JPEG encode drops the source's GPS coordinates, device model, timestamps, etc. This is a **privacy** requirement (don't leak the sender's location/device to the recipient), and is the main reason we re-encode unconditionally rather than only when rotation is needed. (The earlier orientation-only fix left upright photos passing through with full EXIF — a metadata leak.)
- **Cap resolution.** The longest edge is capped (currently 2048 px; Signal's "standard" tier is ~1600) and JPEG quality fixed (~0.9). A single tier for now; a user-facing quality toggle and byte-size targeting are possible later.

This is **mobile-side only** — app-core does no image processing (it just encrypts/uploads the bytes it's handed). The shared constants live in `mobile/ios/Shared/OutgoingImage.swift` (`OutgoingImage.maxDimension`/`jpegQuality`, `UIImage.preparedForSending`) and `mobile/android/.../Views/Chats/AttachmentViews.kt` (`OUTGOING_MAX_DIMENSION`/`OUTGOING_JPEG_QUALITY`, `processOutgoingImage`), applied at every ingestion point: photo picker, clipboard paste, and (where supported) the system share-in. HEIC sources are transcoded to JPEG as a side effect.

One exception to *where* the re-encode runs: the **iOS share extension does not decode or re-encode the image itself**. A share extension has a ~120 MB memory ceiling, and decoding a 24–48 MP photo into an uncompressed bitmap (~96–192 MB) blows it — the process gets jetsam-killed mid-decode before it can hand anything off. So the extension copies the original encoded bytes to the App Group undecoded (`NSItemProvider.loadDataRepresentation`, a few MB), and the **main app** runs the resize/EXIF-strip (`prepareImageForSending`) when it stages the shared image into the composer, where there is a real memory budget. The output is identical (an upright, capped, metadata-stripped JPEG); only the location of the heavy work differs.

**Why JPEG, and the forward-compat guarantee.** We send JPEG (not HEIC/WebP) because it decodes everywhere with no dependency — HEIC has unreliable Android decode below API 29 and no desktop/web decode, and WebP has no native *encoder* on iOS (decode is fine). Crucially, the **receive path is already format-agnostic**: the "is it an image" test is a `image/` *prefix* match (not `== image/jpeg`) and decoding uses the system decoders (iOS `CGImageSource`/`UIImage`, Android `BitmapFactory`), both of which decode JPEG **and WebP** by sniffing the bytes. So switching the *send* side to WebP later is backward-compatible with already-shipped clients — they'll decode it fine. That guarantee covers {JPEG, WebP} only; it would **not** extend to HEIC/AVIF, so those stay off the table. If we ever do adopt WebP on send, the realistic shape is uniform WebP on both platforms (accepting an iOS libwebp encoder dependency), gated behind first removing the hard-coded `image/jpeg` content type so the format flows from the encoder.

## Link previews

*Status: implemented (2026-06-28).* When a message body contains a URL, we show a rich preview card (title, description, image, source domain) — and it reuses the attachment system wholesale, so it needs no new storage machinery. This follows Signal's `Preview` shape exactly.

**Where generation runs (revised 2026-06-28).** The earlier plan had app-core fetch + parse the page. We moved the *fetch + OpenGraph parse to the native client layer* — iOS uses `LPMetadataProvider` (the OS's own link-metadata fetcher, what iMessage uses); Android uses **Jsoup** (the JVM-standard HTML parser) to fetch + extract the OG tags — both real parsers, no hand-rolled HTML parsing. Rationale: it keeps outbound-HTTP-to-arbitrary-URLs + an HTML-parsing dependency out of app-core (which also runs in bots/server contexts — an SSRF surface), and iOS gets a much higher-quality result for free. **app-core keeps the protocol parts only**: the `LinkPreview` wire type, taking the native-supplied og:image bytes through the *existing* `upload_attachment` path (so the image is a normal encrypted blob), threading previews through send/receive/store, and enforcing the anti-spoof rule on receive (`anti_spoof_previews`). Generation is client-invoked only (never automatic in core). Wired for both DM and group sends; the card renders for both.

The load-bearing privacy invariant: **the sender generates the preview at compose time; the recipient never fetches the URL.** The sender's client fetches the page's Open Graph metadata, downloads the `og:image`, and uploads it as a *normal encrypted attachment*. The whole preview travels inside the E2E message. The recipient just renders embedded data and downloads the image like any other attachment by `attachment_id` — nothing on the receive side ever reaches out to the link. (If recipients auto-fetched, a sender could paste a tracking URL and harvest the IP of everyone the message reaches.) Generating previews is a per-account opt-out; off means the bare URL is sent with no fetch at all.

A link preview is a `repeated LinkPreview preview = 3` on `TextMessage` (next slot of the reserved block), with its image as a plain `AttachmentPointer`:

```protobuf
message LinkPreview {
  string            url         = 1;  // must appear in TextMessage.body (see below)
  string            title       = 2;  // og:title
  AttachmentPointer image       = 3;  // og:image, via the attachment system
  string            description = 4;  // og:description
  uint64            date        = 5;  // article published date, unix millis; 0 = unknown
}
```

Rendering notes:
- **Source domain is derived** from `url` (e.g. `www.vox.com`), not a separate field. Shown as `domain · date` in the footer when `date` is present.
- **Layout comes from the image dimensions**, not a flag: a large landscape `og:image` renders as a hero card (image on top); a small/square one renders as an inline left thumbnail. The `AttachmentPointer.width`/`height` already carry what the renderer needs.
- **Anti-spoofing — render only if `preview.url` occurs in `TextMessage.body`.** Otherwise a sender could show a trustworthy-looking card that links elsewhere. The preview is *additive*: the raw URL stays in `body` (which is also what makes this check possible), and the card sits above it.

## Lifecycle & garbage collection

This is the part the existing sketch under-specifies, and it's where E2E forces a different design than a normal CDN.

**The server cannot reference-count blobs.** It cannot see which message references which `attachment_id` — the pointer is encrypted. So "delete the blob when its message is deleted" is not directly implementable server-side. Consequences:

- **Blobs have a TTL, not a reference count.** Every uploaded blob gets `expires_at` at allocation; a background GC task deletes expired blobs unconditionally. This is the primary reclamation mechanism. The **media TTL is deliberately longer than the message-queue retention** in `10-server-implementation.md` — the queue drops a message the moment it's delivered, but a blob must survive long enough for a recipient who's been offline for a while, or who links a fresh device, to still pull it. Signal's delivery CDN retains media for **45 days** for exactly this reason; **~45 days is our default** too (a tunable). Note this is the *delivery buffer*, not a backup — a blob older than the TTL is gone from the server (see *On-device storage management* for what that implies).
- **Recipients download eagerly**, or at least well within the TTL — a blob is not guaranteed to outlive its message in the long term. In practice clients fetch on receipt (subject to wifi/data settings), so the blob is on every recipient device long before TTL.
- **Orphan blobs are expected and fine.** If a client uploads then fails to send (crash, cancel), the blob simply expires unreferenced. No cleanup handshake needed; the TTL is the cleanup.
- **Expiry alignment.** When a message has a disappearing-messages timer (`31-read-tracking.md` notes `read_at` future-proofs this; groups set it in `05`-stage state), the *client* deletes its local copy of the blob on schedule. The server copy is already TTL-bounded; for tight timers the client can additionally call an authenticated `DELETE /v1/attachments/:id` best-effort, but correctness does not depend on it (the server deletes on TTL regardless, and cannot be trusted to delete on demand anyway).
- **Forwarding / re-sharing re-uploads.** Forwarding a media message **re-encrypts under a fresh key and re-uploads**, rather than reusing the original `attachment_id`. Reasons: the original may have expired; reusing it would let the storage layer correlate "same blob fetched by different conversations"; and a forwarded copy should not be deletable by the original sender's timer. This matches Signal.

## On-device storage management

Media is the dominant consumer of on-device storage, so the client needs to let users reclaim space. The **delivery buffer's TTL is the hard constraint here**: the server is a short-lived relay, not a backup, so anything you free locally is only re-fetchable while the blob is still within its (~45-day) window. That cleanly splits what we can offer into two tiers.

**Tier 1 — local-only controls (no backup substrate required).** Everything here works against the delivery buffer alone, and is the whole story for the foreseeable roadmap:

- **Media auto-download settings**, per network type (wifi / cellular / roaming) and per media kind — mirror Signal. Turning auto-download off leaves the blob on the server (within TTL) until the user taps to fetch; it's both a data saver and a storage saver.
- **Trim old local media** — a "keep media for {forever, 1 year, 6 months, 30 days}" policy and/or a per-conversation length cap, deleting local copies on a schedule. Past the server TTL these are unrecoverable, and the UI must say so plainly.
- **Review-and-delete by size** — browse attachments largest-first and delete. **UX requirement: deleting a blob must keep its message**, rendering an explicit "media expired / removed — re-download if still available" placeholder. (Signal historically deletes the whole message when you delete its media, a long-standing complaint; we should not copy that.)

**Tier 2 — true offload (requires a durable encrypted-backup substrate we have not designed).** The "free the bytes now, re-hydrate full-res on demand" experience — keep the inline thumbnail locally, drop the full blob, fetch the original back when the user opens it — is **not implementable against the delivery buffer**, because re-hydration would fail for anything past the TTL. Signal ships exactly this, and it is gated behind their paid Secure Backups tier *precisely because* it re-fetches from the user's own durable encrypted archive, not from the 45-day delivery CDN. For us this is a feature of a **separate encrypted message/media backup store that does not yet exist** (distinct from the identity/auth recovery in `5x`). Until that substrate is designed, Tier 2 is out of scope; this section exists to record *why* the obvious "offload media" toggle can't be built on the attachment-delivery system alone.

## Limits, quotas, abuse

- **Per-attachment size cap** (config; e.g. 100 MB default) enforced at allocation via the declared `size_bytes` and again at upload.
- **Per-account upload quota / rate limit** via the existing `middleware/rate_limit.rs` and the `rate_limit_counters` table — both a request-rate limit on `POST /v1/attachments` and a rolling bytes-per-window cap, so a single account can't fill the disk.
- **Content scanning is impossible by construction** — the server holds only ciphertext, so CSAM/malware scanning of the kind centralized providers do cannot run server-side. Abuse handling is therefore **report-based** (see `12-abuse-handling.md`): a recipient reports a message, and the report carries the decrypted evidence the reporter chooses to attach. This is an inherent, accepted property of E2E media, not a gap to fix.

## Multi-device

- The blob is **uploaded once**. Every recipient device, and the sender's own linked devices, receive the *same pointer* (the message fans out per-device per `01` multi-device) and download the *same blob* with the *same key*. No per-device re-upload.
- A device that was offline at send time downloads on reconnect, within TTL — same as message-queue drain.

## Federation (deferred to Stage 9)

Cross-server media is out of scope until federation. The open question to resolve then: does the recipient's homeserver **proxy/cache** the blob from the sender's homeserver (recipient downloads locally; sender's store not exposed to remote clients), or do remote clients fetch **cross-server directly** with a federated auth token? Proxy-and-cache is the likely answer — it keeps the download route uniform (always "my own homeserver by `attachment_id`") and avoids exposing one org's object store to another org's users. Noted here; specced in `13-federation.md` when we get there.

## What we are explicitly NOT doing (first cut)

- **No server-side transcoding / re-compression.** The client is responsible for sane upload sizes (downscale huge photos, offer quality choice for video). The server stores exactly what it's given. Server transcoding would require plaintext — impossible.
- **No view-once / disappearing-after-view media** in the first cut. It's a client-enforced policy on top of the same pointer (don't persist, delete after render); worth doing later, not a substrate change.
- **No streaming-upload of in-progress recordings.** Voice notes and videos are finalized, then encrypted-and-uploaded as a whole blob. Live streaming is the Calls/broadcast path (`01` § Calls), not attachments.
- **No deduplication across attachments.** Fresh key + fresh blob every send (see Forwarding). Dedup would leak equality of content to the storage layer.

## Decisions (locked 2026-06-27)

All resolved; this section is the record. Numbers marked *(config)* are tunables with the stated default — not protocol constants.

1. **Encryption scheme — CBC+HMAC (Signal-exact).** Chosen over reusing AES-256-GCM, for incremental streaming verification of large media and Signal-parity, accepting the one-off divergence from the app's default AEAD. The `crypto` crate wraps libsignal's existing primitives (see *Encryption scheme* implementation note). `AttachmentPointer.key` is the 64-byte concat.
2. **Default blob TTL — 45 days** *(config)*. Signal-parity; comfortably exceeds the 30-day message-queue retention in `10-server-implementation.md`, so a slow or newly-linked recipient can still pull. Lowerable by a self-hoster.
3. **Per-attachment size cap — 100 MB** *(config)*. Checked at allocation (declared `size_bytes`) and again at upload. Stays well under the single-PUT/multipart threshold.
4. **Per-account upload quota — request-rate limit on `POST /v1/attachments` + a rolling ~500 MB/hour bytes cap** *(config)*, both via the existing `middleware/rate_limit.rs` + `rate_limit_counters`. The requirement is that a bytes-per-window cap exists; the number is tunable.
5. **Storage backend — LocalFs only in the first cut**, built behind the `BlobStore` trait so the S3/presigned-URL backend slots in later with no protocol change. Defers the `object_store` dependency and presigning surface out of the first increment. (The demo server is a single box; LocalFs makes 1:1 media fully useful.)
6. **Naming — the blob layer is `BlobStore` / `routes/attachments.rs` / `/v1/attachments`**, deliberately NOT "Storage": `routes/storage.rs` + `/v1/storage/*` already exist for the unrelated device-data-sync identity store (`05-device-data-sync.md`).

### Spec-time decisions (2026-06-27, during `/new-feature`)

7. **Pointer is a full `url`, not a bare `attachment_id`** — see the *pointer* section above for the revision and rationale.
8. **Groups and DMs ship together.** Attachments live in `TextMessage`, and the core `send_to_target` path already handles both DM and group targets with the same `Body`, so group attachments are nearly free. The shared conversation composer gets the picker for both.
9. **Thumbnails: downscaled inline JPEG, no blurhash generation in this cut.** The `blurhash` proto field stays defined (free, forward-compatible) but is left unpopulated; generation is deferred. `width`/`height` + a small inline `thumbnail` are produced client-side via native image APIs.
10. **Eager download by default.** Recipients auto-download blobs on receipt (no per-network auto-download settings UI yet). The settings/tap-to-download controls in *On-device storage management* are deferred; the substrate (this PR) supports them later.

### Deferred to the immediate follow-up (not this increment)

- ~~**Link previews**~~ — **implemented 2026-06-28** (see *Link previews* above). The remaining follow-up here is the S3 `BlobStore` backend.

### Federation

Cross-server media stays deferred to Stage 9 (see *Federation* above) — proxy-and-cache is the likely answer.

## Staging

Attachments are **not yet built** — `content.proto` reserves space (`TextMessage` fields 2–10) but defines no pointer, and there is no server attachment endpoint or blob-storage layer (`routes/storage.rs` is the unrelated device-data-sync store). 1:1 media depends on neither groups, projects, nor federation — it is fully useful on its own and lands just after the Stage 3 messaging already shipped.

**First cut (this increment):**
- `AttachmentPointer` added to `content.proto` as `repeated AttachmentPointer attachments = 2` in `TextMessage`; `key` is the 64-byte CBC+HMAC concat.
- `crypto` crate: thin attachment encrypt/decrypt + incremental-MAC wrapper over libsignal (per *Encryption scheme* note).
- Server: `attachments` metadata table, `POST /v1/attachments` (allocate) + `PUT`/`GET /v1/attachments/:id` (LocalFs), the `BlobStore` trait with the `LocalFs` impl, TTL GC task, size/quota limits.
- Client encrypt → upload → send-pointer → download → verify → decrypt → render, plus blurhash + inline thumbnails — on **iOS, Android, and Desktop** per the cross-platform parity rule.

**Immediate follow-up (next increment):** link previews; then later the S3 `BlobStore` backend.

## Unaddressed TODO — background upload throttling (Lincoln)

*Not implemented in the first cut.* Today an upload that hits the rate limit
just errors: the server returns `429` (no `Retry-After`) and the client abandons
the send — no queue, no retry, no backoff (same as everywhere else in the app).

For most actions, erroring on a rate limit is the right behavior. **Upload is
the exception.** A client legitimately may want to push *everything it's
currently trying to upload* and not care how long it takes — so uploads should
run **in the background and self-throttle** rather than fail:

- **Short-term limit (per-minute / burst):** the client should *naturally throttle
  itself* — back off and keep going at the allowed rate until the queue drains,
  not surface an error. This wants a `Retry-After` (or equivalent rate signal)
  from the server's `429` and a background upload queue that paces itself to it.
- **Long-term cap (per-day-ish):** there still needs to be a hard ceiling. If a
  client tries to push, say, a full day's worth of its rate limit all at once,
  *that* should error — the throttle is for pacing legitimate bursts, not for
  absorbing an unbounded backlog.

So: short-term overage → pace and continue silently; gross overage (≈ a day's
quota at once) → hard error. Needs server `Retry-After` on the `429`, a
client-side background upload queue with backoff, and the two-tier (burst vs.
daily) limit shape. Lincoln considers this an important unaddressed to-do.
