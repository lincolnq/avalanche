# 16 — Notification Service Extension (rich, reliable iOS push)

**Status:** Stages 1–3 implemented (not yet deployed). Stage 1 (shared storage)
is verified on device. Stages 2 (relay alert payload, behind the `APNS_PUSH_MODE`
toggle — default `silent`) + 3 (NSE target + fetch FFI) are built and compile.
The relay can deploy safely (default = no change); flip `APNS_PUSH_MODE=alert` to
test the NSE on a device build, then back. Rich-banner behavior is not yet
verified on device. Stage 4 (cross-process WAL/`busy_timeout` hardening) is still
pending. The memory feasibility experiment (Stage-0 probe) was done and its
scaffolding removed — its result is recorded under "Footprint" below.

## Problem

iOS notifications are unreliable today. The relay sends a **silent / `content-available`
background push** (`relay/src/main.rs::send_silent`, `PushType::Background`). iOS
only wakes the *main app* for that push when Background App Refresh is enabled,
the app wasn't force-quit, and the system's opportunistic background budget
allows it — and it deprioritizes rarely-opened apps. Net effect: after the app
has been idle for a while, the relay logs `sent APNs wakeup` (Apple accepted it)
but the app is never woken, so no local notification is posted → **silence**.
(See `docs/15` for the relay/push architecture; this doc replaces the silent-push
leg on iOS.)

A plain visible **alert push** would be reliable, but Avalanche is content-free by
design — an alert push can only carry a generic "New message" (the payload can't
contain plaintext, and the main app can't rewrite a displayed alert). Generic
banners are not an acceptable substitute for a messenger.

## Goal

Match Signal's model: a **visible alert push** (so iOS displays it regardless of
Background App Refresh / force-quit / throttling) carrying **`mutable-content: 1`**,
intercepted by a **Notification Service Extension (NSE)** that runs on-device,
**fetches + decrypts the actual message(s)**, and rewrites the banner with the real
sender and body — falling back to a generic banner if it can't finish.

- **Reliable:** the NSE and the alert display are part of the notification
  pipeline, independent of Background App Refresh and the background-execution
  budget; they run even for a force-quit app.
- **Private:** Apple's push path still carries **no message content and no
  ciphertext** — only a token + a generic alert (+ optionally an opaque
  pseudonym, see Targeting). The NSE fetches and decrypts locally.
- **Degrades to a generic banner, never to silence** (the asymmetry vs. today).

## Non-goals

- Android / Desktop. Android already uses **data-only high-priority FCM** that wakes
  its own in-process handler (`docs/15`); it does not use an NSE. Desktop is a
  desktop app. The relay change here is **APNs-branch-only** — FCM and UnifiedPush
  payloads are unchanged.
- Rich media / attachment thumbnails in the banner (future).

## Load-bearing dependencies (the real work)

An NSE is a **separate process** from the app. Three things it needs are today
scoped to the app's private sandbox and must move to shared storage; a fourth is a
cross-process correctness question.

1. **DB decryption key → shared Keychain access group.**
   `SecureEnclaveKeyManager` stores a random 32-byte passphrase as a
   `kSecClassGenericPassword` with **no `kSecAttrAccessGroup`**
   (`Utils/SecureEnclaveKeyManager.swift`). It's a plain keychain item (not a
   non-exportable SE key), so it *can* be shared: add a keychain access group
   entitlement to both the app and the NSE and store the key under it. Keep
   `kSecAttrAccessibleAfterFirstUnlock*` so the NSE can read it while the screen
   is locked (post first-unlock).

2. **Per-account SQLCipher DBs → App Group container.**
   DBs live in `applicationSupportDirectory/actnet` (`AppState.swift:431`), which is
   app-private and unreadable by the extension. Move them into the App Group
   container (`containerURL(forSecurityApplicationGroupIdentifier: "group.net.theavalanche.app")`).
   The App Group is already provisioned (app + `ShareExtension`, `project.yml`).
   Requires a **one-time migration** of existing DB files on app launch, and
   repathing `dbPath` resolution.

3. **Account list → shared storage.**
   The NSE must know which accounts exist (dbFilename + serverUrl + did) to open and
   fetch. That list is in app-private `UserDefaults` today. Move it to
   `UserDefaults(suiteName: "group.net.theavalanche.app")` (or a small shared
   manifest file). (Related: `docs/02` "move identity list out of UserDefaults".)

4. **Cross-process DB / ratchet coordination.**
   Decrypting a message **advances the Double Ratchet and writes the store**, and
   fetch **acks** (deletes) messages server-side. The NSE and app therefore share
   *mutable crypto state*. Because they share the same DB file (dep #2), state is
   consistent — but two processes may open it. Plan: SQLCipher in **WAL mode with a
   generous `busy_timeout`**, keep NSE write transactions short, and confirm
   app-core's "single connection per process" model is safe across processes (it's
   one connection *per process*; SQLite handles inter-process locking). A push only
   fires when the app isn't WS-connected, so app-vs-NSE contention windows are
   narrow, but must not corrupt or double-advance. **This is the highest-risk area
   and needs a focused test.**

## Relay changes (`core/crates/relay`, APNs branch only)

- An alert path (`send_alert`) sits alongside `send_silent`: `PushType::Alert`,
  `mutable-content: 1`, a generic `alert` body ("New message"), Priority::High, and
  **not** `content-available` (avoids also waking the app's background handler /
  racing the NSE).
- **`APNS_PUSH_MODE` env toggle** selects the shape at startup: `silent` (default)
  keeps the established content-available wakeup; `alert` sends the NSE payload.
  Defaulting to `silent` means the new relay can be deployed with no behavior
  change, flipped to `alert` for a live test, and flipped back — no redeploy, and
  no risk of stranding users on the alert payload before the NSE app build ships.
- FCM / UnifiedPush unchanged.
- Wire/contract change to the APNs payload → coordinate with the iOS side; additive
  for other transports.

## Footprint: MEASURED — full app-core fits (resolved)

The ~24 MB cap was the one thing that could have forced a slim/ciphertext design.
A throwaway probe (a real NSE running current-thread runtime + SQLCipher + a live
HTTPS request + libsignal, retaining allocations) measured, on device
(the probe FFI, the measurement NSE target, and the test-push script have since
been removed — this is the recorded result):

- NSE cold baseline: **2.0 MB**
- After app-core's heavy paths: **2.9 MB** (Δ ~1.0 MB) — vs the ~24 MB cap.

So **full app-core fits with ~21 MB of headroom.** `phys_footprint` does not charge
the ~13 MB of clean, file-backed `__TEXT` code — only *dirty* memory, and app-core's
dirty working set for fetch+decrypt is ~1 MB. Decision: **use the full-core,
fetch-based (Signal-style) design.** The "slim decrypt path" and "ciphertext-in-push"
alternatives are dropped — they only existed to dodge a memory limit that isn't real
here, and both cost privacy or duplication for no benefit.

## app-core changes

The NSE should **reuse the existing receive path**, not reimplement crypto:
`receive_messages` (DM mailbox) + `fetch_group_messages` (group pull) already
decrypt, advance ratchets, ack, and run the missing-key buffer/retry (docs/03
§3.7). The footprint experiment settled the design: **link the full
`AppCoreFFI.xcframework` into the NSE and call the same FFI** (the "slim decrypt
entry point" alternative is dropped — it only existed to dodge a memory cap that
turned out not to bind; see Footprint above).

Add a sync FFI the NSE calls, e.g. `fetch_and_decrypt_for_notification(account, sinceHint) -> [NotifItem]`,
that runs the receive path and returns display-ready (sender, body, conversationId)
items — so the extension holds no crypto logic.

## iOS changes

- **New `NotificationServiceExtension` target** in `project.yml` (mirror
  `ShareExtension`: `type: app-extension`, embedded, own entitlements with the App
  Group + the shared keychain access group; link `AppCoreFFI.xcframework`).
- `UNNotificationServiceExtension.didReceive`: read the account list + DB key from
  shared storage, **fetch every account** (no targeting — see below), call the
  app-core fetch FFI within the ~30 s budget, rewrite `bestAttemptContent`
  (title = sender, body = message) for the triggering notification, and **schedule
  additional local notifications** for any other new messages.
  `serviceExtensionTimeWillExpire` → deliver the generic best-attempt.
- Main-app changes: DB path + key + account-list migration to shared storage (deps
  1–3); the existing silent-push handler (`ActnetApp.swift:93`) can stay as a
  belt-and-suspenders wake but is no longer the primary path.

## Targeting (decision — deferred)

A device hosts multiple accounts sharing one token, so in principle the push
should say *which* account to fetch. **Decision: don't target for now — the NSE
fetches every account.** The alert push carries no hint; on receipt the NSE opens
each account in the shared list and pulls its mailbox/groups. Simpler, and keeps
the payload maximally content-free.

If the fetch-all cost ever bumps the ~30 s / memory budget (many accounts, or slow
network), add targeting as a self-contained follow-on: the relay includes the
recipient's own **opaque pseudonym** in the alert payload and the NSE maps it →
local account and fetches just that one. Privacy cost then: Apple's push path sees
a rotating pseudonym alongside the token it already sees — no identity/content.
Nothing in Stages 2–3 precludes this later.

## Privacy analysis

Today Apple sees: device token + content-free wakeup. With this change Apple's push
path additionally sees a **generic "New message" alert** (timing that a message
arrived). With targeting deferred there is **no pseudonym** in the payload either.
Still **no sender, no content, no ciphertext** — all fetched and decrypted
on-device by the NSE. This is the Signal posture. Note the known forensics caveat:
decrypted banner text then lives in iOS's notification store
(`docs/signal-research/foreground-notifications.md`).

## Staged plan

1. **Shared storage foundation** (deps 1–3): keychain access group, DB → App Group
   container + migration, account list → shared. Verify the app still works
   end-to-end (no NSE yet).
2. **Relay alert payload** (APNs branch) — alert + `mutable-content`, no targeting.
3. **NSE target + app-core fetch FFI** (full core — footprint resolved); fetch all
   accounts, rewrite banner, generic fallback.
4. **Cross-process hardening** (dep 4): WAL/busy_timeout, contention tests.

## Test plan

- app-core: unit/e2e for the new `fetch_and_decrypt_for_notification` FFI (returns
  correct items; advances ratchet; acks; handles missing-key buffer).
- Cross-process: a test that opens the same SQLCipher DB from two connections and
  interleaves decrypt/ack writes — assert no corruption / no double-advance.
- Device (manual, the only real proof): terminate the app, send a DM and a group
  message from another account, confirm a **rich** banner (real sender + body)
  appears; and that after killing the NSE mid-fetch you still get the generic
  fallback.
- Migration: existing install with DBs in `applicationSupport` upgrades and still
  opens after the move to the App Group container.

## Open questions / decisions

1. **Cross-process SQLite** correctness/coordination strategy (dep 4) — needs WAL +
   `busy_timeout` (not set today, `db.rs:98-107`).
2. Does the relay change to the APNs payload need project-owner sign-off (privacy
   posture shift from content-free wakeup → generic alert)?
