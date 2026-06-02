# Contacts and profiles

> **Status: partially implemented.** A minimal slice of the contact row is in
> place: a local `contacts` table (`core/crates/store/src/contacts.rs`) with
> `did`, `is_curated`, `last_interaction_at`; `list_contacts` /
> `touch_contact` FFI on `AppCore`; auto-population on send-DM (curating),
> group invite (curating), inbound DM and inbound group message
> (non-curating); People / Other sectioning wired into the iOS compose
> autocomplete (`ComposeMessageView.swift`). The existing
> `contact_profiles` table still owns cached display names and profile
> keys; this doc's `contact row` is the union of the two until they merge.
>
> Not yet implemented: `profile_version` push-style liveness,
> conversation-open dormancy fetch, message-request gating, blocking,
> nicknames / notes / favorites / `photo_override`, `preferred_identity`,
> `learned_route_server`, `safety_number_verified_at`, contact backup,
> federated profile proxying. Treat the rest of this document as a target.

Goal: Users have sole and persistent ownership of their "contact book": their contact roll, nicknames, and personal notes about people they talk to on Avalanche:

- I should be able to set my display name and profile picture, and anyone who messages me sees that by default.
- A user who knows Alice should be able to say "I want to message Alice" and have that be a natural and simple thing, like it is in Signal or texting, while defaulting to doing the 'right thing' in the background in almost all instances. 
- I should be able to nickname pseudonomous people privately in my contacts, and expect those nicknames not to be shared or leaked beyond my device. And I should be able to view a contact's display name even if I have them nicknamed, since I might need to introduce them to someone else someday by their display name rather than my nickname.
- My contact book exists beyond the servers that I'm a member of, and beyond the identities that I have chosen to share with others; but it helps me by remembering how I message people so I don't accidentally de-anonymize myself. If I lose access to any of my identities, I should still be able to restore my contact book and reestablish contact with my contacts under other identities.

## Design principles

1. **Interaction-driven, like Signal and iMessage.** There is no separate Contacts app or "Add to contacts" gesture. The list of "people I know" surfaces from interaction: anyone you've nicknamed, noted, or DM'd is in the People list, sorted by recency. Everyone else the client has seen — group co-members, senders of unaccepted requests, DIDs whose profile we've cached — exists in the table but doesn't appear in the People list, only in search. Nicknames, notes, and favorites are deliberate curation gestures.
2. **Default to Signal for the technical model.** The encrypted-profile blob, profile-key distribution via messages, message-request gate, and federation primitives all match Signal — its crypto and protocol design have been validated repeatedly and we should not reinvent them. UX similarities to Signal flow downstream from the Gmail-shaped principle and the underlying protocol shape, not from a desire to mirror Signal's UI.
3. **Server never sees plaintext profile data.** Display names, avatars, and bios are encrypted with a per-user profile key, distributed only to people the user chooses to share with. A seized server yields encrypted blobs and a list of DIDs — not a membership roster with real names. This is the load-bearing protection for activist users.
4. **Contacts are local-only.** Client-side state. The server has no "contact list" concept. Individual fields (profile keys) travel inside encrypted messages, but the relationship graph is not server-side.
5. **One contact book, identity-aware.** Contacts live in a single unified table across all the user's identities, not per-identity. This matches the goal: the contact book exists *beyond* any one identity. Each contact has a `preferred_identity` field — the user's default sender for this contact — populated at add-time from the active identity and user-editable thereafter. That field is what prevents accidental de-anonymization: tapping "message Alice" sends from her `preferred_identity`, not from whichever identity is foregrounded. Nicknames, notes, and photo overrides are person-level and visible under any of the user's identities. **Caveat:** this means an unlock of the app exposes the merged contact book to whoever has the device. We do not currently ship per-identity unlock (a PIN per identity, etc.); if we ever do, revisit the storage decision then.
6. **Per-DID, not per-conversation.** A contact identifies a peer DID. Conversations reference contacts; contacts don't enumerate conversations. A contact can exist for a DID you share no conversation with.
7. **Two profile layers.** The substrate profile (name, avatar, bio) is encrypted and distributed via profile keys. Project profiles (attendee directory fields, team roles) are separate — collected by the Project, scoped to the Project, explicitly consented to. Different systems, different purposes.

## The contact record

Anything that makes the client aware of a DID — receiving a DM, sharing a group, fetching a profile — creates or touches a single row. There's one row per DID, holding whatever the system has learned plus whatever the user has added.

### What a row holds

- `**did`** — the peer's identifier. Primary key.
- `**profile_key`** — 32-byte symmetric key learned from an inbound message; enables you to decrypt this contact's profile blob from the server.
- `**display_name**`, `**profile_fetched_at**` — cache of the decrypted profile blob (can refresh from the server with `profile_key)`.
- `**cached_profile_version**` — the `profile_version` value (see envelope) that the cached blob was decrypted from. Used to detect change: when an inbound message carries a different version, refetch.
- `**is_curated**` — bool. Set the first time the user does anything deliberate with this row: sending a DM, accepting a request, favoriting, adding a nickname, writing a note, etc. This is the flag for "the user knows this person", meaning e.g. they appear in the People list.
- `**is_favorite**` — bool, 'stars' the contact so they appear first
- `**is_blocked**` — bool, see `12-abuse-handling.md`.
- `**has_pending_request**` — true if there's an inbound first-time DM. Set on inbound, cleared on Accept / Delete / Send / Block / Report.
- `**nickname**` — user-set private display-name override.
- `**notes**` — user-set free-form private notes.
- `**photo_override**` — user-set private image,  displayed in place of the contact's own avatar.
- `**preferred_identity**` — which of the user's identities is the default sender for this contact. Set on first interaction; user-editable. The de-anonymization guard: tapping "message" sends from `preferred_identity`, not from whichever identity is foregrounded.
- `**learned_route_server**` — cache of outbound routing destination. See `13-federation.md` - "Learned route".
- `**last_interaction_at**` — sort/recency.
- `**safety_number_verified_at**` (future) — out-of-band identity verification.

### What `is_curated` drives

The flag is the single source of truth for "the user knows this person." It drives:

- **People list** — rows where `is_curated AND NOT removed_at AND NOT is_blocked`, sorted by `last_interaction_at`.
- **Message-request gate** — inbound from `is_curated` rows passes; everyone else hits the request UI (unless `is_blocked`, in which case it's dropped post-decrypt).
- **Backup** — every `is_curated` row, plus every `is_blocked` row (so blocks survive). Everything else rebuilds from interaction on a new identity.
- **Search "primary" section** — `is_curated`. "Other" section — every other row in the table.

Blocking and removal are orthogonal flags that override visibility but don't change curation: a row stays `is_curated` even after being blocked or removed, so we remember the relationship existed. It just doesn't appear in the People list while one of those overrides is set.

### What changes a row

Deliberate user gestures flip `is_curated = true` (sticky) and do their own thing. Non-deliberate events (inbound, group co-membership) never flip `is_curated`.

- **Receive DM** — create row if missing. If `is_curated`, deliver normally. Else, set `has_pending_request = true` (or drop if `is_blocked`).
- **Send DM** — create row if missing. Set `is_curated = true` and `first_sent_at` (if null). Clear `has_pending_request`. Refused locally if `is_blocked`.
- **Accept request** — clear `has_pending_request`, send a delivery receipt (which counts as a send and so sets `is_curated` + `first_sent_at`).
- **Delete request** — clear `has_pending_request`. Row stays as profile cache.
- **Report Spam and Block** — set `is_blocked = true`, clear `has_pending_request`, forward report.
- **Block** (from anywhere) — set `is_blocked = true`.
- **Unblock** — set `is_blocked = false`.
- **Favorite / unfavorite** — toggle `is_favorite`. First time favorited sets `is_curated = true`.
- **Set / change nickname, note, or photo override** — write the field. First non-null write sets `is_curated = true`. Clearing the field later does NOT unset `is_curated`.
- **Remove from People** — set `removed_at`. Clear `is_favorite`. `is_curated` stays set. Row stays in the table.
- **Hard delete** — drop the row. Future inbound creates a fresh row with no history.
- **Group co-membership** — create row if missing, populate profile cache when keys arrive. No flag changes. Co-members are not auto-curated.
- **Profile fetch / key receipt** — update `profile_key`, `display_name`, `profile_fetched_at`. No flag changes.

### More about `preferred_identity`

Storing the identity you are talking to someone with is fairly important since we want to help prevent our users accidentally messaging someone with the wrong identity and unmasking themselves. The way we do this is just by storing one of our own identities on each contact row.

But `preferred_identity` references can stale: A homeserver being down for a long time, logging out on a device, and moving to a new device that hasn't signed in to the relevant identity all could leave the field pointing at something not currently usable to send.

Conversations are read-only if the preferred identity is unavailable at the time you open them, but you can override it at the conversation level: the read-only status is surfaced via a banner which explains the situation; if you tap it you are offered the option to send using a different identity or offered the option to recover the correct identity. Somewhere in settings there's a way to update preferred identity for all contacts with a given identity configured.

There is also some nuance once groups are introduced: let's assume you own identities A and B. You've indicated your preferred identity with C is B, but if you were added to a group with C under identity A, you need to be sending messages to that group as A. So it demonstrates that groups also need to track preferred identity separate from contacts.

## The substrate profile

The encrypted profile blob is what the cached `display_name` is decrypted from.

### Contents

For stage 4, the profile contains only `display_name` (required, set at account creation). Future fields use the same blob with no schema version — unknown fields are ignored by older clients:

- `avatar` (URL to an encrypted attachment + decryption key)
- `bio` (short text)
- `bio_emoji` (single emoji)

### Profile key

A 32-byte random symmetric key generated at account creation. Stored alongside the account's identity keys in the local SQLCipher DB. The profile key is the single secret that controls who can read your profile.

It does NOT rotate when you update your profile. It only rotates when you want to revoke access (e.g., after blocking someone), which forces re-distribution to all remaining contacts. Profile key rotation is out of scope for stage 4.

### Encrypted blob

The profile is JSON, encrypted with the profile key using AES-256-GCM, uploaded to the homeserver as opaque bytes. The user's client maintains a monotonic `profile_version` counter (uint64) alongside the blob — incremented on every profile update. When the user changes their display name: increment the counter, re-encrypt with the same key, re-upload, replacing the old blob.

The version counter is what recipients use to detect that the blob has changed; see "Liveness via `profile_version`" below.

### Profile key and version distribution

The outer `ContentMessage` envelope carries two profile fields:

- `profile_key` (32 bytes) — the key needed to decrypt the sender's profile blob. Stable; only changes on rare rotation events (e.g., revocation after blocking, deferred for stage 4).
- `profile_version` (uint64, varint-encoded) — the sender's current profile-version counter. Changes whenever the sender edits their profile.

Both fields ride on the outer envelope, not inside any body variant, so they accompany every message type (text, receipts, typing, etc.). Following Signal's model, both are included only for recipients the sender has chosen to share their profile with — for stage 4 that's everyone they DM.

Recipients cache `profile_key` once and rarely have to update it. They compare `profile_version` against `cached_profile_version` on every incoming message; mismatch triggers a refetch (see below).

**Invite tokens** carry the inviter's profile key and current version in their payload. When the new user registers and the auto-DM is created, they can immediately fetch and decrypt the inviter's profile. After that, both fields travel with regular messages. Invite tokens also carry the inviter's plaintext display name as a UX fallback so the invite-acceptance screen can show "Alice invited you" before any server communication.

### Liveness via `profile_version`

Profile changes propagate push-style, via the version field in inbound messages:

- Sender updates profile → counter increments.
- Sender's next outbound message carries the new `profile_version`.
- Recipient sees `profile_version` differs from `cached_profile_version` → refetch the blob, decrypt with the `profile_key`, update cache and UI.

For active conversations (DM or group), this means profile changes propagate within roughly one message round-trip in either direction. The recipient never has to ask "did this change?" — the answer is in the next message they receive from the sender, or in the next delivery receipt from a recipient they sent to.

For dormant contacts (nobody has sent or received in a while), version signal doesn't arrive. That's what the opportunistic fetch covers; see below.

## Fetching profiles

### When the client fetches

Three triggers, in order of how often they fire:

1. **Version mismatch on inbound** — an inbound message carries a `profile_version` that differs from `cached_profile_version` (or there's no cached profile yet). Fetch the blob, decrypt with `profile_key`, update cache. This is the primary path and handles all active contacts sending messages into any group you're in.
2. **Dormant-contact opportunistic fetch** — when the user opens a conversation, if both (a) the most recent inbound message from any contact in that group is older than ~1 week and (b) the last opportunistic fetch for this contact was more than ~1 week ago, refetch the blob. This is the safety net for contacts who changed their profile during a long silent period — nothing in the message stream signaled it.
3. **Cold-cache render** — UI needs to render a name for a contact with no cached profile. Fetch, rate-limited to once per 30 min per DID.

No daily background sweep. The version-in-envelope mechanism makes one unnecessary for active contacts; the conversation-open dormancy fetch covers the inactive ones at the moment the user actually cares.

### Client-side rate limiting

- **Dormancy threshold for conversation-open opportunistic fetch:** ~1 week (configurable). Tunable later if staleness shows up as a real UX problem.
- **Cold-cache fetch dedup:** once per 30 min per DID.
- **In-flight dedup:** only one fetch in flight at a time per DID, regardless of trigger.

Version-mismatch fetches don't get a separate rate limit — they're driven by genuine evidence of change, so suppressing them would defeat their purpose.

### Authoritative storage and cross-server fetches

Across the federation, the **discovery server** for a DID (the server published in PLC) is authoritative for the profile blob. Profile upload (`PUT`) is meaningful only on the discovery server — that's where the canonical blob lives. Migrating discovery servers re-uploads to the new one.

Profile fetch (`GET`) is satisfiable on **any** server the requester is authenticated to. The serving server resolves the DID, federates the fetch to the discovery server, and returns the response to the requester. There is **no server-side profile cache** — each fetch federates fresh. PLC resolution (DID → discovery-server URL) may be cached server-side with a multi-hour TTL since PLC documents are public and signed.

A 200 from server X says only "X was willing and able to fetch this profile," nothing about whether the target DID is a local member of X. All servers behave identically for all DIDs.

#### Federation abuse controls

Without a server cache, the serving server is a pure proxy for arbitrary federation traffic. Bound with:

- Per-(local-account, target-DID) rate limit on profile fetches.
- Per-(serving-server, discovery-server) outbound rate limit on federated fetches — prevents one server from being used to flood another with profile probes.

Reuses the federation rate-limit primitives in `13-federation.md`; no new mechanism.

#### Membership confirmation more broadly

The proxy-any-DID property closes the profile-fetch leak (200 carries no info about local membership) but does NOT plug the broader membership-confirmation problem. Prekey lookups are still per-server (`13-federation.md`) and the timing and error shape of a federated prekey fetch can still differentiate "this DID is on server X" from "this DID is elsewhere." Profiles are easier to make membership-blind because the blob is global user state; prekeys are inherently per-server. Closing the prekey leak is tracked in `00-design.md` and `13-federation.md`, not here.

#### Discovery-server seizure

If the discovery server is seized or offline, new profile fetches for affected users fail until the user migrates discovery servers (`13-federation.md` "Discovery-server migration"). Client-side caches keep working with the last-fetched blob; affected users just stop receiving updates until migration completes.

We considered per-member-server replication (each server the user joins holds its own copy, updates fan out client-side) for better seizure resilience. Rejected: duplicates the prekey-distribution complexity for state that changes orders of magnitude less often, and the seizure window is bounded by the user's ability to migrate, which is the same recovery path as for the DID document itself.

## Subsystem interactions

### Message requests (`12-abuse-handling.md` §1)

The gate passes iff `is_curated`. Everyone else lands in the request UI. Any deliberate gesture (send, accept, favorite, nickname, note, photo override) flips `is_curated`, so the gate auto-resolves once the user has expressed any intent about the person.

### Blocking (`12-abuse-handling.md` §2)

Sets `is_blocked = true`. The separate `blocked_dids` table described there folds into this — same key space, no reason to have two tables of DIDs the user has a relationship with. Blocking a DID you've never interacted with creates a row with `is_blocked = true` and nothing else set.

### Multi-device sync

Row mutations (favorite, block, nickname edit, removal, etc.) sync across the user's own devices via a `ContactRowUpdate` sync envelope variant. Same path as `BlockListUpdate` in `12-abuse-handling.md` §2; replaces it. Out of scope until multi-device sync exists; the data model is forward-compatible.

## Backup and survival

The contact book is a separate concern from any one identity. Per the goal, it exists *beyond* the servers the user is a member of and *beyond* the identities they've taken on. The mental model is Gmail: contacts are user-owned and survive everything else.

Concretely:

- **One unified backup blob** across all the user's identities — matches the unified storage model. Encrypted under a key derived from the recovery passphrase (distinct from any identity-key blob), so curation state can be restored independently of any one identity. If a user loses an identity, they restore and re-establish under a new identity.
- **Backed-up rows:** every `is_curated` row, plus every `is_blocked` row (so blocks survive even without curation). Pure profile-cache and group-co-member rows are not backed up — they rebuild from interaction.
- **Backed-up fields per row:** `profile_key` (load-bearing — without it, no name resolution post-restore), `is_curated`, hand-edited fields, `is_favorite`, `is_blocked`, `removed_at`, `first_sent_at`, `preferred_identity`. Cached `display_name` is nice-to-have, not load-bearing.
- **After restore**, `preferred_identity` references may point at lost identities — surface a one-time "pick a new default identity" UI to repair.
- **Format is stable JSON or protobuf** so the user can also export to another app.

Implementation deferred (see `02-todos-deferred.md`).

## UI rendering

### How names render

The primary name shown for a contact is:

1. The user-set nickname if set, otherwise
2. The cached profile `display_name`, otherwise
3. `"Unknown"` or truncated DID as placeholder.

The nickname does NOT erase the profile display name. In any contact-detail surface (contact card, conversation header tap-to-expand, etc.) the underlying display name remains visible as a secondary line. This matters because the user may need to introduce the contact to someone else by their actual display name — "tell Alice I said hi" — even when the user privately calls them something else.

### Surfaces

- **People list.** Rows where `is_curated AND NOT removed_at AND NOT is_blocked`. Sorted by `last_interaction_at`, with favorites pinned. This is the closest thing to a "contacts list" the app has, but it's really a recent-interactions list.
- **Conversation list.** Each row shows the contact's cached display name (or "Unknown").
- **Message bubbles.** Incoming messages show the sender's cached display name. In 1:1 DMs this is redundant with the conversation header but consistent.
- **Settings → Your Profile.** See current display name; edit it (re-encrypts and re-uploads); future avatar and bio.
- **Settings → Privacy → Blocked.** Rows where `is_blocked`.

### Search

Search returns two sections:

1. **People** — rows where `is_curated`. Matched against nickname, profile `display_name`, notes, and DID prefix.
2. **Other** — every other row in the table. Matched against profile `display_name` and DID prefix only; user-editable fields are usually empty.

There's no explicit "Save to contacts" affordance; any curation gesture (send, favorite, nickname, note) flips `is_curated` and moves the row from Other into People. The Other section exists primarily so the user can find someone they've seen in a group and start a conversation with them.

Rows where `removed_at` is set don't appear in either search section unless an inbound request raises them in the request UI with the re-grooming warning.

## How this extends to Projects

When Projects arrive (stage 6+), the substrate profile mechanism doesn't change. Projects interact with it through scoped permissions:

1. A Project (e.g., Attendee Directory) asks users to share their profile with the Project during onboarding.
2. The user consents. Their profile key is shared with the Project's bot via the normal encrypted channel (the bot is a group member).
3. The bot fetches and decrypts the user's profile blob, caching the result in Project-scoped storage.
4. The Project displays the cached name in its directory UI.

Alternatively, a Project can collect its own fields ("Organization," "Role," "Dietary restrictions") that don't exist in the substrate profile. These are Project-owned data, stored in the Project's tables, visible only to that Project's members.

The architectural point: substrate profiles and Project profiles are separate systems. The substrate profile is your identity to your contacts (encrypted, key-gated). A Project profile is what you've explicitly chosen to share with a specific Project. No migration between them — they coexist.

## Open questions

2. **Where does `learned_route_server` actually live?** (a) Contact row as described, or (b) per-conversation so group co-members get route hints independent of any 1:1 contact relationship. Group routing is mostly per-server already (action-bound groups live on one server). **Recommendation: (a), accept that contact-less group co-members don't get learned-route optimization.** Revisit if cross-server casual-group messaging becomes hot.
3. **Project-introduced people.** When a Project bot introduces two members, the introduced DID gets a row (profile cache populated) but `is_curated` stays false — the user has to do something deliberate to make them appear in the People list. Directory-style Projects may want a stronger "this is your team, here are everyone's contact details" surface — probably implemented as a bulk "favorite all" gesture inside the Project. Confirm UX shape when we design concrete Projects.
4. **Contact merging.** If I know two DIDs are really the same person, can I collapse them into one entry? The current model treats every DID as its own row; same person under two DIDs shows up as two People list entries. Merging adds a two-level shape (a person record with 1..N DID rows) and meaningful complexity. Deferred until we have user evidence that the unmerged model creates real toil.

## Stage 4 implementation scope

Build:

- [done] Profile key generation at account creation
- [done] Encrypted profile blob (display name only) upload at registration
- [done] Profile upload/fetch endpoints
- [done] Profile key included in outgoing messages
- [done] Profile key in invite tokens
- [partial] The contact row with the fields and predicates described above (no state enum) — minimal slice: `did`, `is_curated`, `last_interaction_at` in `core/crates/store/src/contacts.rs`. `nickname`, `notes`, `photo_override`, `preferred_identity`, `learned_route_server`, `safety_number_verified_at`, `has_pending_request`, `is_blocked`, `is_favorite`, `removed_at`, `first_sent_at`, `cached_profile_version` not yet added.
- [partial] Row creation wired into receive, send, group co-membership, profile-key receipt — DM send / inbound DM / inbound group message / group invite all touch the row; profile-key receipt still only writes to `contact_profiles` (not `contacts`), and group co-membership doesn't yet auto-create rows on `fetch_group_state`.
- [not started] Migration of `blocked_dids` callers to `is_blocked` (no blocking table exists yet)
- [not started] `profile_version` counter on outbound profile updates and in the `ContentMessage` envelope; fetch + decrypt on version mismatch; conversation-open dormancy fetch (~1 week threshold); local cache on the contact row
- [done] Edit display name in iOS settings
- [done] Show cached names in conversation list and message bubbles
- [not started] Nicknames, notes, favorites — the user-edit gestures are headline goals; ship the editing UX with the contact row
- [partial] People list surface (rows where `is_curated`) and search across all known rows with "People" / "Other" sectioning — sectioning is implemented in the compose autocomplete; standalone People list surface is not yet built.

Defer:

- Avatar and bio fields
- Profile key rotation (for revocation use case)
- Versioned profile fetches (Signal's credential-authenticated fetch)
- Unidentified access / sealed sender for profile fetches
- Federated profile proxying (start with single-server, light up cross-server fetches when federation lands)
- `learned_route_server` and `discovery_server_hint` caches (wire as federation routing lands)
- `ContactListUpdate` multi-device sync envelope
- Contact backup format
- Safety-number verification UI
- `photo_override` image-handling pipeline (field reserved on the contact record)

