# Attachments — Design

Status: draft for review.

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

> **Note vs. the rest of the app.** Messages use AES-256-GCM (the `profile_key` in `content.proto`, the ratchet, etc.). Attachments deliberately use CBC+HMAC instead, for the reasons above — so the `AttachmentPointer.key` field is a 64-byte concatenation, not a GCM key. This is the one spot where attachments diverge from the app's default AEAD. **(Decision to confirm — see Open decisions.)**

### Padding

Encrypting reveals nothing about content, but *ciphertext length* leaks information — a 2.1 MB blob is plausibly one specific photo. So we pad plaintext up to a **bucket size** before encrypting, using the same monotonic bucketing function Signal uses (round up in ~5% geometric steps). The unpadded plaintext length is carried in the encrypted pointer (`size_bytes`), so the recipient trims after decrypting. The storage layer and any network observer see only bucketed sizes.

## Server: upload & download

Two new authenticated, rate-limited endpoints under `/v1/`, plus a storage abstraction with two backends.

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

- **Authenticated.** Any logged-in account on the homeserver may fetch any blob *by id* — knowledge of the (unguessable, opaque) id is the capability, and the id only exists inside an E2E message sent to you. The server cannot enforce "only the intended recipient" because it doesn't know who that is (sealed sender, no plaintext). This matches Signal: the CDN serves by opaque key to any authenticated client; secrecy is the unguessable key + the decryption key, not an ACL.
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
- The pointer references an **`attachment_id`**, not a raw URL — the recipient resolves it against *their own* homeserver's download route. (URLs are deployment detail; ids are stable.)
- Per-attachment metadata travels in the pointer so the UI renders well before the full blob is fetched.

```protobuf
message AttachmentPointer {
  string attachment_id  = 1;   // server-allocated; download key
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

## Link previews

When a message body contains a URL, we show a rich preview card (title, description, image, source domain) — and it reuses the attachment system wholesale, so it needs no new storage machinery. This follows Signal's `Preview` shape exactly.

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

## Open decisions

1. **Encryption scheme: CBC+HMAC (Signal-exact) vs reuse AES-256-GCM.** Recommendation above is CBC+HMAC for incremental verification and Signal-parity; it diverges from the app's default AEAD, so this needs an explicit yes before `AttachmentPointer` is added to `content.proto`.
2. **Default blob TTL** — ~45 days (Signal-parity; longer than the message queue, per *Lifecycle*) vs shorter. Shorter is safer for storage but risks a slow / newly-linked recipient missing a blob; eager download mitigates.
3. **Per-attachment size cap and per-account quota numbers** — need concrete defaults for the deploy config.

## Staging

Attachments are **not yet built** — `content.proto` reserves space (`TextMessage` fields 2–10) but defines no pointer, and there is no server attachment endpoint or storage layer. The work is one focused increment: add `AttachmentPointer` to `content.proto`, build the server `attachments` table + endpoints + storage backends, and the client encrypt/upload/download/render path. It lands well alongside or just after the 1:1 messaging already shipped in Stage 3, and depends on neither groups, projects, nor federation — 1:1 media is fully useful on its own.
