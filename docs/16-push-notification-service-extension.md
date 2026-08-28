# 16 — Notification Service Extension (rich, reliable iOS push)

**Status:** Stages 1–3 implemented (not yet deployed). Stage 1 (shared storage)
is verified on device. Stages 2 (relay alert payload, behind the `APNS_PUSH_MODE`
toggle — default `silent`) + 3 (NSE target + fetch FFI) are built and compile.
The relay can deploy safely (default = no change); flip `APNS_PUSH_MODE=alert` to
test the NSE on a device build, then back. The rich banner + cold-launch tap
routing are **verified on device**. Stage 4 (cross-process WAL/`busy_timeout`
hardening + contention test) is **done**. The memory feasibility experiment
(Stage-0 probe) was done and its scaffolding removed — its result is recorded
under "Footprint" below. Stage 5 (Signal-parity presentation: per-message local
notifications, placeholder suppressed, fail-silent) is implemented but **gated
off pending Apple's notification-filtering entitlement** — without it iOS
displays the original payload on empty completion (verified on device); the
interim rewrite model is active. See "Stage 5" below.

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
- ~~**Degrades to a generic banner, never to silence** (the asymmetry vs. today).~~
  **Reversed in Stage 5** — see "Stage 5: Signal-parity presentation" below for
  the rationale and what replaced it.

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

4. **Cross-process DB / ratchet coordination.** (Stage 4 — done.)
   Decrypting a message **advances the Double Ratchet and writes the store**, and
   fetch **acks** (deletes) messages server-side. The NSE and app therefore share
   *mutable crypto state*. Because they share the same DB file (dep #2), state is
   consistent — but two processes may open it.
   - **DB locking:** both files now open in **WAL mode with a 5 s `busy_timeout`**
     (`store::db::apply_key`), so a reader and a writer coexist and a cross-process
     write collision waits/retries instead of failing with `SQLITE_BUSY`. Within a
     process all DB work is serialized on the one tokio-rusqlite connection, so the
     only contention is app-vs-NSE. Covered by a two-connection contention test
     (`store::db` tests).
   - **Fetch race:** the mailbox is remove-on-**ack**, not remove-on-fetch, so if
     the app and NSE ever poll the same message concurrently, both receive it. The
     first to decrypt advances the ratchet, persists (idempotent by `server_id` —
     first-write-wins), and acks; the second's decrypt then fails on the moved
     ratchet and is skipped (logged, no duplicate row, no event). Safe, if
     slightly wasteful. The window is naturally small — a push only fires when the
     recipient has no live WS — but a cold-launch-from-tap can overlap an in-flight
     NSE fetch, so this is by-design, not assumed-impossible.
   - **iOS suspension caveat (`0xdead10cc`):** iOS force-terminates an app that is
     suspended while holding a file lock on a file in a **shared (App Group)
     container** — which is where the DBs now live (dep #2). Mitigation is the
     existing discipline: keep write transactions short so a lock is essentially
     never held across suspension. Not observed, but worth watching if suspension
     terminations appear in crash logs; the fuller fix (close/relinquish on
     background) is deferred.

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
  app-core fetch FFI within the ~30 s budget, post **one local notification per
  fetched message**, and complete the triggering push with **empty content**
  (suppressing the placeholder). `serviceExtensionTimeWillExpire` → complete
  silently. See "Stage 5" below for why banners are decoupled from pushes.
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

## Stage 5: Signal-parity presentation (banners decoupled from pushes)

The Stage-3 model — each push's `contentHandler` rewrites its own banner, extras
posted as additional local notifications, generic fallback on any failure —
coupled banner count to push count. The mailbox is drain-once
(`fetch_notifications` acks what it fetches), so in a burst of N messages one
NSE invocation drains all N and the other N−1 invocations fetch nothing and
deliver their generic "New message" best-attempt. Observed on device as N rich
banners plus several stray "New message" banners that never resolve.

The fix copies Signal's presentation model in full
(`docs/signal-research/notification-service-extension.md`):

- **Every fetched message is posted as its own local notification** via
  `UNUserNotificationCenter.add()` — including the newest; the triggering push
  is never rewritten.
- **The triggering push is completed with empty content** (no alert
  title/body), which suppresses its banner. An empty successful fetch shows
  nothing — correct, because a sibling invocation or the main app already
  presented those messages.
- **Failures complete silently too** (fetch error, timeout, superseded fetch).
  One exception: the device hasn't been unlocked since boot
  (`errSecInteractionNotAllowed` reading the DB key), which shows a single
  static "unlock your phone" banner per process — the one failure the user can
  act on.
- **A stacked push supersedes a queued fetch**: fetches are serialized in the
  NSE process, and a queued fetch that a newer push has overtaken skips its
  fetch (any fetch drains every mailbox). Signal's
  `enqueueCancellingPrevious`, adapted to our synchronous FFI — a blocking
  in-flight fetch is not interruptible, only queued work is skipped.

### The entitlement gate (found on device verification)

Suppressing the placeholder is not possible with a plain NSE: **iOS ignores an
empty completion and displays the original payload unless the app holds
`com.apple.developer.usernotifications.filtering`** — a per-account grant
requested from Apple with a justification (E2EE messaging is the canonical
approved use case). Signal ships it in `SignalNSE/SignalNSE-AppStore.entitlements`
(the research doc missed it because the dev-signing entitlements file doesn't
contain it). Verified on device here: with empty completion and no entitlement,
a single message produced both the rich local notification and the "New
message" placeholder, simultaneously.

Until the grant lands, `NotificationService.hasFilteringEntitlement` is false
and the NSE runs the **interim rewrite model**: the newest fetched message
rewrites the triggering banner (single-message case shows exactly one banner),
extras are posted as local notifications, and no-op/failure paths deliver the
generic placeholder — so a burst can still leave stray generic banners. That
stray-banner gap is exactly what the entitlement closes.

**Action to enable full parity:** request the entitlement from Apple
(developer.apple.com, notification-filtering entitlement request), add the key
to the NSE entitlements in `project.yml`, flip `hasFilteringEntitlement` to
true, and re-run the Stage 5 device checks.

This **reverses the earlier "degrades to a generic banner, never to silence"
decision.** Why: a generic banner on the no-op paths is not a degraded signal
but a false one (there is no unseen message), and it fires routinely on every
burst, whereas real fetch failures are rare. Signal accepts rare silence and
compensates with APNs token-health detection; ours is tracked in `docs/02`
("lost-push detection") and is deliberately not built yet.

**Rejected alternative — hybrid fallback:** keep the generic banner for
*error/timeout* paths and go silent only on success-with-zero-items. Provably
never shows a false banner, preserves the never-silence property for real
failures. Rejected (for now) in favor of exact Signal parity and simpler
behavior; revisit if silent misses are observed in practice before lost-push
detection lands.

The relay's fallback alert body (shown only when the NSE never runs — crash or
system throttle) changed from "New message" to Signal's hedged wording,
"You may have new messages".

## Background lifecycle (0xDEAD10CC and the socket handoff)

Status: implemented (Stage 6). Modeled on Signal's teardown — see
`signal-research/socket-and-suspension-lifecycle.md` for the researched source
behavior.

Moving the databases into the App Group container (dep 2) made the app subject
to iOS's shared-container rule: **a process suspended while holding a file or
SQLite lock on a shared-container file is killed** (`0xDEAD10CC`,
`RUNNINGBOARD` code 3735883980). Confirmed in the field: nine kills in one day
on the TestFlight build (crash log 2026-08-26), presenting as "the app resets
to the first tab" (cold relaunch) — exactly the failure open question 2 below
anticipated. The trigger is any in-flight write at the freeze instant: the
reconnect drain after a foreground, the expire reaper, receipt/read-state
writes. Nothing told the core to stop initiating transactions.

A second, coupled problem: a socket left open across suspension keeps this
device in the server's live-connection map (`routes/messages.rs` only fires
the relay push for devices with no live WS), so messages arriving while
suspended produce **no push and no banner** until the dead TCP path rots.
Ironically the crashes were masking this — a killed process closes its socket,
so pushes flowed. Fixing the crash alone would have regressed notifications.

The design ("we're going to background", per Signal):

- **scenePhase → `.background`**: `AppState.sceneDidEnterBackground` takes a
  `beginBackgroundTask` assertion and changes nothing else. The socket stays
  open and messages keep processing (with live local banners) for the runtime
  iOS grants (~30s) — rapid app-switching keeps a warm connection.
- **Expiration handler** (the only "about to suspend" signal iOS provides):
  `quiesceForSuspension` calls `prepare_for_background()` per core, in
  parallel on GCD (never `Task.detached` — blocking FFI must stay off the
  cooperative pool). Rust side: sets the `backgrounded` watch flag → the
  receive loop sends a clean WS Close (server drops us from the live map →
  pushes fire for later messages) and the reconnect loop parks → waits
  (bounded, 2s) for the state to leave `Connected` → suspends the store gates.
  Then the assertion is released and the process suspends holding nothing.
- **Store gate** (`store::GatedConnection`): every SQLite call on both
  databases holds a read guard; suspension flips a flag (parking new calls),
  write-acquires to drain the in-flight call, and then **closes the
  connection**. Draining alone is not enough — a WAL-mode connection holds a
  shared lock on its database file for its entire *open* lifetime, and iOS
  kills for exactly that: a first fix that only drained still died at
  17:59:13 on 2026-08-26, six seconds after a fully clean quiesce, with
  RunningBoard's termination context naming all six idle database files. The
  first call after resume transparently reopens (re-applying the SQLCipher
  key and per-connection pragmas, and reinstalling the storage-sync commit
  hook, which is per-connection state). The gate retains the DB path +
  passphrase in memory to make that possible — a deliberate trade; the
  passphrase is already memory-resident at every open. Exhaustive by
  construction — future writers are gated automatically. Work parked
  mid-pipeline holds no lock and resumes where it left off.
- **scenePhase → `.active`**: end the assertion (if the window is still open,
  nothing was torn down — the socket never dropped); if the expiration path
  ran, `resume_from_background()` reopens the gates and reconnects.

Divergences from Signal, both deliberate: no cross-process connection lock
(our NSE fetches over HTTP, it never takes over a socket), and quiesce is a
store-layer gate rather than task cancellation (our writers live in tokio
where Swift can't cancel them; parking achieves the same "quiet at suspension"
invariant).

Platform scope: iOS-only. Android has no shared-container suspension rule and
wants background WS delivery; Desktop never suspends. Both get the FFI methods
for surface parity but never call them.

Known residual risks (accepted): a store call blocked on the cross-process
busy timeout (NSE contention, ≤5s) can outlive the quiesce wait — rare, both
processes' transactions are short; and iOS can suspend without any warning
from some non-`.background` states — those windows are short and hold no
in-flight work in practice. Server-side WS ping/idle-timeout (tracked in
docs/02) is the backstop for sockets left by crashes, which no client-side
protocol can clean up.

## Staged plan

1. **Shared storage foundation** (deps 1–3): keychain access group, DB → App Group
   container + migration, account list → shared. **Done; verified on device.**
2. **Relay alert payload** (APNs branch) — alert + `mutable-content`, no targeting,
   behind the `APNS_PUSH_MODE` toggle (default `silent`). **Done.**
3. **NSE target + app-core fetch FFI** (full core — footprint resolved); fetch all
   accounts, rewrite banner, generic fallback. Plus cold-launch tap routing.
   **Done; rich banner + tap verified on device.**
4. **Cross-process hardening** (dep 4): WAL/busy_timeout + contention test. **Done.**
5. **Signal-parity presentation** (see above): per-message local notifications,
   placeholder suppressed, fail-silent + phone-locked banner, superseded-fetch
   skip, `os_log` on every branch (subsystem `net.theavalanche.nse`).
   **Implemented but gated off** behind `hasFilteringEntitlement` — blocked on
   the Apple filtering-entitlement grant; interim rewrite model active
   (see "The entitlement gate").
6. **Background lifecycle** (see above): store suspension gate, clean WS close
   at background-task expiration, resume on activation. Fixes the `0xDEAD10CC`
   kills the App Group move introduced *and* the suppressed-push dead zone an
   open-but-frozen socket causes. Landed in two rounds: the drain-only
   version still died (idle WAL connections hold file locks — see "Store
   gate" above), so `suspend` now also closes the connections. The lifecycle
   os_log trail confirmed the mechanism on device (~30s window, ~50ms
   quiesce; messages 10–25s after backgrounding via the live socket, ~35s+
   via push → NSE). Crash-free confirmation (no new `Actnet-*.ips`
   accumulating) needs device re-verification of the close-on-suspend build
   plus a few days of normal use.

## Test plan

- app-core: unit/e2e for the new `fetch_and_decrypt_for_notification` FFI (returns
  correct items; advances ratchet; acks; handles missing-key buffer).
- Cross-process: a test that opens the same SQLCipher DB from two connections and
  interleaves decrypt/ack writes — assert no corruption / no double-advance.
- Device (manual, the only real proof): terminate the app, send a DM and a group
  message from another account, confirm a **rich** banner (real sender + body)
  appears.
- Stage 5 device checks: (a) a single message shows exactly one rich banner and
  the placeholder never appears, not even briefly (this verifies the
  load-bearing assumption that empty completion suppresses the alert);
  (b) a burst of 8 shows exactly 8 rich banners and zero "New message";
  (c) airplane-mode after the push lands → nothing shown, message appears on
  app open (the accepted fail-silent behavior — confirm it feels right live);
  (d) reboot without unlocking → one static "phone locked" banner, rich banners
  resume after unlock; (e) app open with a live WS → in-app notification only,
  no NSE banner; (f) after each scenario the `net.theavalanche.nse` log lines
  in Console.app identify the branch taken.
- Migration: existing install with DBs in `applicationSupport` upgrades and still
  opens after the move to the App Group container.
- Stage 6 device checks (launch by tapping the icon — an attached debugger
  prevents suspension and voids the test): (a) background the app and send
  messages at ~10s / ~60s / ~5min — the 10s one arrives via the live socket
  (local banner, no relay log entry), the later ones via push → NSE (relay
  entry, banner with the NSE's fetch delay); (b) repeat yesterday's
  reproduction — rapid home-screen round-trips during a message burst across
  all accounts — then check Settings → Analytics Data: **zero new
  `Actnet-*.ips`**; (c) reopen within a few seconds of backgrounding —
  conversation updates instantly with no reconnect flap (the window kept the
  socket); (d) server logs show a clean `ws:` disconnect ~30s after
  backgrounding, not a lingering live connection.

## Open questions / decisions

1. Does the relay change to the APNs payload need project-owner sign-off (privacy
   posture shift from content-free wakeup → generic alert)? — mitigated for now by
   the `APNS_PUSH_MODE` toggle defaulting to `silent`.
2. **iOS suspension / `0xdead10cc`** (dep 4 caveat): RESOLVED — the predicted
   terminations appeared in the field (nine in one day, 2026-08-26), and the
   close/relinquish-on-background design landed as Stage 6 ("Background
   lifecycle" above).
