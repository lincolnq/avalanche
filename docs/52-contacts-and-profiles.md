# 52 — Contacts and profiles

> **Status:** Partial — contact rows with curation, blocking, message requests, and a local nickname; encrypted profiles (display name + avatar); the persisted per-outcome fetch throttle; contact and profile-key sync via the storage service. Not built: `profile_version` liveness, favorites, notes, `removed_at`, `preferred_identity` as a stored field, contact backup, profile-key rotation, cross-server profile proxying. One urgent privacy gap (profile key to strangers, S-02).
> **Last verified against code:** 2026-10-03

## Summary

Users own their contact book: who they know, what they call people, who they've blocked. It lives on the device (synced across the identity's devices by the storage service, `05`), never as a server-side relationship graph. Each user publishes an **encrypted profile** (display name, avatar) that only holders of their **profile key** can read; the key rides inside E2E messages. Contacts are per identity in storage and unified across identities at query time in the UI.

Code: `core/crates/store/src/contacts.rs` (`contacts` table), `contact_profiles` and `account_info_cache` (`store/src/schema.rs`), `core/crates/app-core/src/profile.rs` (blob format), `messaging.rs` (`SenderGate`, `handle_inbound_profile_key`, `sync_contact_avatar`), `lib.rs` (`block_contact`, `accept_request`, `refresh_contact_profile`, `fetch_and_cache_profile`, `save_shared_contact`), sync adapters in `app-core/src/storage_sync.rs` (`ContactAdapter`, `ContactProfileAdapter`).

Goals:

- I set my display name and picture once; anyone I message sees them.
- "Message Alice" is natural and does the right thing in the background, including picking the right one of my identities.
- I can privately nickname people, and still see their real display name (I may need to introduce them by it).
- My contact book outlives any one server or identity.

## Design principles

1. **Interaction-driven, like Signal and iMessage.** No "Add to contacts" gesture. "People I know" surfaces from deliberate interaction (DMing, accepting, saving a shared contact). Everyone else the client has seen (group co-members, request senders) exists in the table but only appears in search.
2. **Default to Signal for the technical model.** Encrypted profile blob, profile-key distribution via messages, message-request gate.
3. **Server never sees plaintext profile data.** A seized server yields encrypted blobs and a list of DIDs, not a roster with real names. This is the load-bearing protection for activist users.
4. **Contacts are local-only.** The server has no contact-list concept.
5. **One contact book, identity-aware.** The user sees one book across all identities. **As built**, each identity has its own `contacts` table (in its `IdentityStore`, `06`), and the UI merges them at query time (iOS `AppState.AccountContact` carries the set of identities that know a DID). The acting identity for a new conversation defaults to the identity that most recently talked to that contact. *Caveat:* unlocking the app exposes the merged book to whoever holds the device; there is no per-identity unlock.
6. **Per-DID, not per-conversation.** A contact identifies a peer DID.
7. **Two profile layers.** The substrate profile (encrypted, key-gated) is separate from any Project profile (collected by and scoped to a Project, `20`).

## The contact record

Anything that makes the client aware of a DID — a DM, a shared group, a profile fetch — creates or touches one row per DID.

### What a row holds

*Built* (`contacts` table, per identity):

- **`did`** — primary key.
- **`is_curated`** — "the user knows this person." Set by any deliberate gesture; sticky.
- **`last_interaction_at`** — recency sort.
- **`is_blocked`** — see `12`.
- **`has_pending_request`** — an un-accepted inbound first-time DM.
- **`nickname`** — private local name ("the name I know them by"). Set today only by saving a shared contact card (`35`); there is no rename UI yet. Not synced across devices yet.

*Built, separate tables:* `contact_profiles` (`did`, decrypted `display_name`, `profile_key`, `fetched_at`); `account_info_cache` (bots: `display_name`, `is_bot`); `avatar_cache` (device-local decrypted avatars, `55`); `profile_fetch_state` (throttle, below).

*Planned fields:* `is_favorite`, `notes`, `photo_override`, `removed_at`, `first_sent_at`, `cached_profile_version`, `safety_number_verified_at`. *Proposed (depends on `13`):* `learned_route_server`.

### What is_curated drives

The single source of truth for "the user knows this person." Built uses:

- **Message-request gate** — inbound from a curated sender delivers normally; anyone else is a request (`SenderGate`, `messaging.rs`); blocked senders are dropped after decryption. **Exception: senders whose account record says `is_bot` also pass** — see *Known gaps*.
- **Read receipts** — only sent to curated senders (`send_read_receipt`).
- **Search / compose sectioning** — "People" = curated, "Other" = everyone else.

Planned uses: a standalone People list; contact backup (curated + blocked rows).

Blocking is orthogonal to curation: a row stays curated after being blocked, so the relationship is remembered.

### What changes a row

Deliberate gestures set `is_curated = true` (sticky). Non-deliberate events never do.

- **Receive DM** — create row if missing. Curated → deliver. Else set `has_pending_request` (or drop if blocked).
- **Send DM** — create row, curate, clear `has_pending_request`. Refused locally if blocked.
- **Accept request** — clear `has_pending_request`, curate.
- **Delete request** — clear `has_pending_request`; delete the conversation's local history. Row stays as profile cache.
- **Report** (`12`) — set `is_blocked`, clear `has_pending_request`, file the report.
- **Block / Unblock** — toggle `is_blocked`.
- **Save a shared contact card** (`35`) — set `nickname`, curate.
- **Group co-membership** — create row if missing; no flag changes. Co-members are not curated.
- **Profile fetch / key receipt** — update `contact_profiles`; no flag changes.

*Planned:* favorite, note, photo override, nickname edit (curate on first non-empty write); "Remove from People" (`removed_at`); hard delete.

### Which identity to message from

*Built:* a conversation is bound to exactly one of the user's identities by construction (`37`). For a *new* conversation, the compose flow picks the identity that most recently talked to the chosen contact and lets the user change it (`ComposeMessageView.swift`).

*Planned:* persist an explicit `preferred_identity` per contact (user-editable), with a repair flow when it points at an identity no longer on the device. Groups bind their own identity (the one that joined), independent of any contact's preference.

## The substrate profile

### Contents

*Built:* JSON `{ display_name, avatar_version?, avatar_digest? }` (`profile::ProfilePlaintext`). Unknown fields are ignored by older clients, so no schema version. Avatar bytes live out-of-band (`55`). *Planned:* `bio`.

### Profile key

A 32-byte random key generated at account creation, stored in the identity store. It does **not** rotate on profile edits. *Planned:* rotation for revocation (e.g. after blocking), which forces redistribution to remaining contacts.

### Encrypted blob

AES-256-GCM under the profile key, `nonce(12) ‖ ciphertext+tag`, uploaded as opaque bytes (`PUT /v1/profile`). Display-name change → re-encrypt with the same key, re-upload.

### Profile key and version distribution

*Built:* `ContentMessage.profile_key = 17` rides the outer envelope. App-core attaches the sender's own profile key to outgoing DMs, group messages, delivery receipts, and read receipts (`own_profile_key`). Recipients cache it in `contact_profiles` when the embedder calls `fetch_and_cache_profile` (iOS does this on every inbound message carrying a key).

**Not built:** `profile_version`. It is not in `content.proto` (fields 19–30 are reserved). Without it, a recipient only learns a profile changed when it refetches.

*Planned:* invite tokens carry the inviter's profile key so the auto-DM can render the inviter's name immediately (check `51` for the current token contents).

### Liveness via `profile_version`

*Status: Planned.* Add `profile_version` (uint64) to the envelope; a sender bumps it on every profile edit; a recipient whose cached version differs refetches. This is the primary liveness path in Signal and makes the conversation-open fetch a fallback. Until then, liveness comes only from the conversation-open refresh and cold-cache renders below, so a name or avatar change can take arbitrarily long to reach a contact who doesn't open the conversation.

## Fetching profiles

### When the client fetches

Two endpoints, disjoint DID sets:

- **Encrypted profile blob** (`get_profile`, authenticated, `404` for both "no such DID" and "no blob") — humans; decrypted client-side.
- **Public account record** (`get_account_info`) — bots; humans publish no plaintext name. Cached in `account_info_cache` so bot names and hexagon avatars (`54`) resolve offline.

*Built triggers:*

1. **Conversation open** — `refresh_contact_profile` refetches if the throttle allows.
2. **Inbound profile key** — `fetch_and_cache_profile` when a message carries a key that differs from the cache.
3. **Cold-cache render** — the name resolver fetches, subject to the throttle.

*Planned trigger:* version mismatch on inbound (needs `profile_version`), bypassing the throttle.

No daily background sweep.

### Client-side rate limiting

*Status: Built.* Modeled on Signal's `ProfileFetcher`. The decision is keyed on the **outcome of the last attempt**:

| Last outcome | Skip window |
|---|---|
| Success | 5 min |
| Network failure | 1 min |
| Not authorized | 30 min |
| Not found | 6 hours |
| Rate limited | 5 min |
| Other failure | 30 min |

**Persisted across launches** in `profile_fetch_state` — an improvement over Signal's in-memory LRU, since our in-memory name caches reset every launch. **Decided in core**, not the client: the UI calls on every conversation appear; core no-ops when fresh. Shared with node bots.

- **In-flight dedup:** one fetch per DID at a time.
- **Group-open fan-out:** member name fetches are shuffled and spaced (~100 ms).
- **Negative-row hygiene:** a failed/empty bot lookup records a throttle outcome; it never writes an empty-name row into `account_info_cache`.

### Authoritative storage and cross-server fetches

*Built (single server):* the profile blob lives on the account's homeserver; `GET /v1/profile/{did}` is authenticated and returns an identical `404` for "no such account" and "no blob", so an authenticated caller can't use it to confirm membership.

*Planned / depends on `13`:* the discovery (home) server is authoritative for the blob. Under server-to-server federation, any server would proxy the fetch to the discovery server with no server-side cache, bounded by per-(account, target) and per-(server, server) rate limits. Under the Proposed client-side federation (`13`), the client fetches directly from the contact's home server instead — simpler, and no server learns who is looking up whom on another server. Either way the profile fetch should not reveal local membership; prekey fetches remain the harder membership-leak problem (`13`).

If the authoritative server is seized or down, new fetches fail until the user migrates; cached profiles keep working.

## Subsystem interactions

### Message requests (`12` §1)

The gate passes iff curated (or `is_bot`, see gaps). A request shows Accept / Delete / Report. Accept curates; Delete wipes the local conversation.

### Blocking (`12` §2)

`is_blocked` on the contact row; there is no separate `blocked_dids` table. Blocking a never-seen DID creates a bare row. Settings shows a Blocked list on iOS, Android, and Desktop.

### Multi-device sync

*Built:* contact rows (`did`, `is_curated`, `is_blocked`, …) and contact profiles (name + profile key) sync across the identity's devices through the storage service (`05`, `ContactAdapter` / `ContactProfileAdapter`), last-writer-wins per record. `nickname` is not carried yet.

## Backup and survival

*Status: Planned.* The goal (the Gmail mental model): the contact book survives loss of any one identity or server. Today it survives device loss via each identity's storage-service records (`05`), but not loss of the identity itself.

Planned shape: one encrypted backup across all of the user's identities, under a key derived from the recovery secret, containing curated and blocked rows with their profile keys and hand-edited fields, in a stable exportable format. After restore, any per-contact identity preference pointing at a lost identity triggers a one-time repair prompt.

## UI rendering

### How names render

1. Local nickname, else
2. Cached profile display name (or account-record name for bots), else
3. A truncated DID.

A nickname never erases the display name; contact-detail surfaces show the underlying name as a secondary line.

### Surfaces

*Built:* conversation list and bubbles show cached names; Settings → identity → edit display name and avatar (`55`); Settings → Blocked; compose/search sectioned into People and Other. *Planned:* a standalone People list with favorites pinned.

### Search

*Built (compose):* two sections — **People** (curated) and **Other** (everything else), matched on name; DIDs only when the query starts with `did:`.

## How this extends to Projects

Substrate profiles and Project profiles are separate systems. A Project that needs a user's name either learns it through the messaging channel (its bot receives the user's profile key like any other contact) or collects its own fields with the user's consent and stores them in its own tables (`20`).

## Known gaps

1. **Profile key sent to un-accepted request senders (S-02, High; P0 in `02`).** On every inbound DM — including from an un-accepted, un-curated stranger — app-core auto-sends a delivery receipt that carries the recipient's own profile key (`messaging.rs`, the auto-delivery-receipt block in the DM receive path, ~lines 1276–1291). Anyone who knows a user's DID can send one message and decrypt that user's real display name and avatar. Signal withholds the profile key until the user accepts. Fix: send the delivery receipt with an empty `profile_key` (or no receipt) unless the sender is curated.
2. **Self-declared bots bypass message requests (S-03, High; P0 in `02`).** `SenderGate::passes` admits `is_curated || is_bot`, and `is_bot` comes from the sender's server account record, which the account sets for itself at registration (`server/src/routes/registration.rs`, `req.is_bot`). Any spammer can register as a bot and skip the request gate. Fix: bot exemption only for bots whose `official` flag is server-vouched on the user's own server (`54`), or drop the exemption.
3. **No profile liveness (P2).** No `profile_version`; see above.
4. **`fetch_and_cache_profile` holds the core lock across the network** (`lib.rs:fetch_and_cache_profile` takes `inner`). Lift the fetch out of the lock (`core/CLAUDE.md`).

## Planned

- Fix gaps 1–4.
- `profile_version` in the envelope (contract change: additive proto field).
- Favorites, notes, nickname editing UI, People list, `removed_at`.
- Persisted per-contact identity preference.
- Contact backup across identities.
- Profile-key rotation on revocation.
- Safety-number verification UI.

## Proposed

- **Delivery keys from the profile key** (part of the Proposed client-side federation in `13`): derive a delivery key from the profile key so only contacts can send sealed deliveries. Makes gap 1's fix structural — a profile key is a sending capability, so it obviously can't go to strangers.

## Speculative

- **Contact merging** (one person, several DIDs). Deferred until there's evidence the unmerged model causes real toil.
- **Project-introduced people** — a bulk "save everyone on my team" gesture inside a directory-style Project.

## Rationale and rejected alternatives

- **Per-member-server profile replication** — rejected: duplicates prekey-distribution complexity for state that changes rarely; seizure recovery is the same as for the DID (migrate).
- **A separate `blocked_dids` table** — folded into `is_blocked` on the contact row: same key space.
- **A daily background profile sweep** — rejected: version-in-envelope plus conversation-open refresh covers it without background traffic.
- **In-memory fetch throttle (Signal)** — improved on: persisted, because our name caches reset each launch.
- **Per-row identity marking in the inbox** — rejected in `37`: the conversation is the context.
