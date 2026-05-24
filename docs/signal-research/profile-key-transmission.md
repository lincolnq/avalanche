# Profile Key Transmission and Profile Refresh Triggers

How profile keys are distributed and how recipients know when to re-fetch a profile.

## Profile Key Transmission (Outgoing)

### Included in normal messages (opportunistic distribution)

`ProtoUtils.addLocalProfileKeyIfNecessary` (`SignalServiceKit/Protos/ProtoUtils.swift:14`) attaches the local profile key to outgoing `DataMessage` and `CallMessage` protobufs, but **only if the thread is in the profile whitelist** (i.e., you've shared your profile with that contact or group).

This is called from:
- `TSOutgoingMessage.buildDataMessage` (for all regular outgoing messages)
- `OutgoingCallMessage` (for call signaling)
- `ProfileKeyMessage` (dedicated profile key message, see below)

### Dedicated `ProfileKeyMessage` (explicit distribution)

A standalone `ProfileKeyMessage` (`SignalServiceKit/Messages/ProfileKeyMessage.swift`) is a `TransientOutgoingMessage` (not persisted to the database) with the `profileKeyUpdate` flag set. It is sent in these scenarios:

1. **Accepting a message request** (`ConversationViewController+MessageRequest.swift:245`) — when you unblock/accept a contact, you send them your profile key.
2. **Unhiding a recipient** (`RecipientHidingManager.swift:408`) — when you un-hide a previously hidden contact.
3. **Reactive profile key sharing** (`OWSMessageDecrypter.swift:111-144`) — when you receive a message from someone but they don't have your profile key, Signal reactively sends it. This is rate-limited per contact via `reactiveProfileKeyAttemptInterval` from RemoteConfig.
4. **Call link profile sharing** (`CallLinkProfileKeySharingManager.swift:67`) — sharing your profile key with call link participants.

## Profile Key Reception (Incoming)

### Processing received profile keys

When any `DataMessage` arrives with a `profileKey` field (`MessageReceiver.swift:974-976`):

```
setProfileKeyIfValid(profileKey, for: envelope.sourceAci, ...)
```

This calls `OWSProfileManager.setProfileKeyData` (`OWSProfileManager.swift:756-812`) which does:

1. **If the key is the same as cached** (line 783-784): **returns immediately — no fetch triggered**.
2. **If the key is different** (or no key was cached):
   - Clears the profile key credential (for versioned profile fetches)
   - Resets unidentified access mode to `.unknown`
   - Enqueues a profile fetch via `fetchProfileSync` (line 800-803)
   - Updates the stored profile key

**Critical implication:** Receiving a message with an **unchanged** profile key does **NOT** trigger a profile re-fetch. The profile key in messages is purely for key distribution, not for change notification.

The same logic applies to profile keys in:
- `CallMessage` payloads (`MessageReceiver.swift:1780-1781`)
- `StoryMessage` payloads (`StoryManager.swift:87-97`)

## How Recipients Actually Detect Profile Changes

Since an unchanged profile key doesn't trigger a fetch, profile changes (name, avatar, about) are detected through these mechanisms:

### 1. Opening a conversation (`ConversationViewController.swift:404-415`)

When `viewDidAppear` fires, Signal fetches profiles for **all** thread participants with `isOpportunistic: true`. This is the primary way users see updated profiles — opening the chat triggers a re-fetch.

### 2. Stale profile fetcher — daily background job (`StaleProfileFetcher.swift`)

Scheduled daily via `AppDelegate.swift:607-619`. Fetches profiles for:
- Users you've messaged in the last **30 days**
- Whose profile was last fetched **>1 day ago** (or never)
- Limited to **25 profiles per cycle**, ordered by staleness
- Uses `isOpportunistic: true`

### 3. Displaying an unknown contact (`OWSContactsManager.swift:1201-1219`)

When rendering a display name for a contact with no cached profile data, triggers a fetch. Rate-limited to once per **30 minutes** per ACI.

### 4. User taps to unblur an avatar (`OWSContactsManager.swift:301-310`)

Non-opportunistic, urgent fetch + avatar download.

### 5. App becomes active / network reconnects (`OWSProfileManager.swift:247-256`)

Calls `updateProfileOnServiceIfNecessary` — primarily for the **local** user's own profile, not contacts.

### 6. Storage service sync (`StorageServiceProto+Sync.swift:1468-1478`)

If syncing from storage service reveals a different profile key for a user, triggers a fetch.

### 7. Group operations needing credentials (`GroupsV2Impl.swift:1400-1427`)

When GroupsV2 needs profile key credentials (e.g., for group send endorsements), fetches profiles for members missing valid credentials.

### 8. Safety number confirmation (`SafetyNumberConfirmationSheet.swift:372`)

Opportunistic profile fetch when showing safety number UI.

## Fetch Mechanism

### One HTTP request per user — no batching

Each profile fetch is a **separate HTTP request** to the Signal server. There is no batch/bulk profile fetch API. When you open a group conversation, Signal iterates through every participant and fetches each profile individually (`ConversationViewController.swift:409-413`):

```swift
for serviceId in Set(serviceIds).shuffled() {
    try Task.checkCancellation()
    let context = ProfileFetchContext(groupId: try? thread?.groupIdentifier, isOpportunistic: true)
    _ = try? await profileFetcher.fetchProfile(for: serviceId, context: context)
}
```

The order is randomized (`.shuffled()`) to avoid always hitting the same profiles first.

### Versioned vs. unversioned fetches

Each fetch attempts a **versioned profile request** first (`ProfileFetcherJob.swift:122-146`). This uses the profile key + a profile key credential to authenticate, and the response contains encrypted profile fields (name, bio, avatar URL, etc.) that can be decrypted client-side.

If a versioned fetch fails (no profile key, no credential, or 401 auth error), it falls back to an **unversioned fetch** (`ProfileFetcherJob.swift:167-192`). Unversioned fetches return less data and can't decrypt the profile, but still provide identity keys, capabilities, badges, and unidentified access info.

### Authentication methods for fetches

Fetches can authenticate via:
1. **UD access key** (derived from the target's profile key) — for sealed sender / versioned fetches
2. **Group Send Endorsements (GSE)** — when fetching within a group context, used as fallback auth (`ProfileFetcherJob.swift:155-165`, `287-319`)
3. **Identified auth** (standard auth) — final fallback

### What a fetch returns and stores

A successful fetch (`ProfileFetcherJob.swift:326-496`) updates the local database with:
- Decrypted profile name, bio, bio emoji
- Avatar URL path + downloads the avatar if changed (skips download if URL path matches cached)
- Profile badges
- Identity key
- Payment address
- Unidentified access mode
- Capabilities
- `lastFetchDate` (set to `Date()` — used by stale profile logic)

Avatar download is skipped if the `avatarUrlPath` hasn't changed from what's already cached (`ProfileFetcherJob.swift:371-374`).

### Retry and backoff

Individual fetches retry up to **3 times** with backoff (`ProfileFetcherJob.swift:103`).

## Caching and Rate Limiting

### In-memory LRU cache (`ProfileFetcherImpl`)

`ProfileFetcherImpl` maintains an **LRU cache** of recent fetch results (`ProfileFetcher.swift:76`):

```swift
private let recentFetchResults = LRUCache<ServiceId, FetchResult>(maxSize: 16000, nseMaxSize: 4000)
```

Each entry stores the outcome (success/failure type) and completion timestamp. This cache is used to decide whether an opportunistic fetch should proceed or be skipped.

### Opportunistic fetch rate limiting

Opportunistic fetches (`isOpportunistic: true`) are skipped if a recent fetch exists in the LRU cache (`ProfileFetcher.swift:325-347`):

| Last result | Skip window |
|---|---|
| Success | 5 minutes |
| Network failure | 1 minute |
| Not authorized | 30 minutes |
| Not found | 6 hours |
| Rate limited | 5 minutes |
| Other failure | 30 minutes |

### Throttling between opportunistic fetches

When multiple opportunistic fetches are queued (e.g., opening a large group), they are throttled with a minimum delay between requests (`ProfileFetcher.swift:294-323`):
- **Normal:** 100ms between fetches
- **After hitting server rate limit:** 20 seconds between fetches, for 5 minutes

The server-side rate limit is a token bucket: bucket size 4320, refilling at 3/minute.

### Deduplication of in-flight fetches

`ProfileFetcherImpl` tracks in-progress fetches per `ServiceId` via `inProgressFetches` (`ProfileFetcher.swift:95-99`). Multiple callers requesting the same profile can attach waiter continuations to an in-flight fetch rather than starting duplicate requests.

### Database-level caching

Profile data is persisted in the `OWSUserProfile` database table. The `lastFetchDate` column is used by `StaleProfileFetcher` to identify profiles needing refresh (>1 day old). The `avatarUrlPath` column is compared against the server response to skip redundant avatar downloads.

## Summary: The Profile Change Flow

1. Alice changes her name
2. Alice re-encrypts her profile with the **same** profile key, uploads new blob to server
3. **Nothing is pushed to contacts** — no message, no push notification
4. Bob detects the change via one of:
   - **Opening the conversation** with Alice (most common — triggers fetch in `viewDidAppear`)
   - **Daily stale profile job** picks up Alice's profile (if Bob messaged her in last 30 days)
   - **Rendering Alice's name** somewhere in the UI when no profile is cached
5. Bob's client fetches Alice's encrypted profile from the server, decrypts with the cached profile key, and updates the UI

The profile key included in every message is **not** a change signal — it's purely a key distribution mechanism so that new contacts (or contacts after a key rotation) can decrypt the profile.
