# 16 — Notification Service Extension (rich, reliable iOS push)

> **Status:** Partial — Stages 1–4 and 6 are built and verified on device: shared storage, the relay alert payload, the NSE target with the `fetch_notifications` FFI and cold-launch tap routing, cross-process WAL hardening, and the background lifecycle. Stage 5 (Signal-parity presentation) is built but gated off until Apple grants the notification-filtering entitlement; the interim rewrite model is active. The relay's APNs payload shape is selected by `APNS_PUSH_MODE` (default `silent`). Whether production runs `alert` is a deployment setting not recorded in the repo — check the relay host.
> **Last verified against code:** 2026-10-03

## Summary

iOS doesn't reliably wake a backgrounded app for a silent push. So, like Signal, we send a **visible alert push** with `mutable-content: 1`. A **Notification Service Extension (NSE)** intercepts it on-device, **fetches and decrypts** the real messages using the full app-core, and presents real sender and body. Apple's push path carries no content, no ciphertext, and no pseudonym.

Code: relay `core/crates/relay/src/main.rs` (`ApnsPushMode`, `send_alert`, `send_silent`); app-core `lib.rs::fetch_notifications`; store `db.rs` (WAL, `busy_timeout`, `GatedConnection`); iOS `mobile/ios/NotificationServiceExtension/NotificationService.swift`, `mobile/ios/Shared/AppGroup.swift`, `AppState.swift` §Background lifecycle.

## Problem

The relay's original **silent / `content-available`** push only wakes the main app when Background App Refresh is on, the app wasn't force-quit, and iOS's background budget allows it. Rarely opened apps get deprioritized. Apple accepts the push, but the app never wakes, so the user gets silence. A plain alert push would be reliable but could only say "New message", because the payload can't carry plaintext and the main app can't rewrite a displayed alert.

## Current design

### Shared storage (Stage 1)

*Status: Built; verified on device.* The NSE is a separate process, so three things moved to shared storage:

1. **DB key → shared Keychain access group**, readable after first unlock.
2. **SQLCipher DBs → App Group container** (`group.net.theavalanche.app`), with a one-time migration from `applicationSupport`.
3. **Account list → App Group `UserDefaults`.**

### Relay alert payload (Stage 2)

*Status: Built.* `send_alert` sits alongside `send_silent`: `PushType::Alert`, `mutable-content: 1`, a generic body, high priority, and **not** `content-available` (so it doesn't also wake the app and race the NSE). The body is Signal's hedged "You may have new messages", shown only if the NSE never runs. `APNS_PUSH_MODE=alert|silent` (default `silent`) picks the shape at relay startup, so a relay deploy alone changes nothing. FCM and UnifiedPush payloads are unchanged.

### NSE + fetch FFI (Stage 3)

*Status: Built; rich banner and tap routing verified on device.* The NSE links the full `AppCoreFFI.xcframework` and calls **`fetch_notifications(db_path, db_key)`** for every account in the shared list. **There is no per-account targeting:** the push carries no hint, and the NSE drains every account. That's simpler, and the payload stays content-free.

For each account, `fetch_notifications` opens the store, polls the DM mailbox and every group queue over HTTP, and runs each message through the **same `process_decrypted` path the WebSocket loop uses**. So content lands in local history and is acked exactly once, after the durable write. It returns display-ready `NotifItemFfi`s and starts no background tasks.

### Footprint

*Measured.* The ~24 MB NSE memory cap was the one thing that could have forced a slim or ciphertext-in-push design. A throwaway probe measured an NSE cold baseline of **2.0 MB** and **2.9 MB** after app-core's heavy paths. `phys_footprint` doesn't charge clean file-backed `__TEXT`, and app-core's dirty working set for fetch + decrypt is about 1 MB. **Decision: full core, fetch-based.** The slim-decrypt and ciphertext-in-push alternatives were dropped: they existed only to dodge a limit that doesn't bind, and both cost privacy or duplicate code.

## Stage 4

*Cross-process coordination. Status: Built.*

Decrypting advances the ratchet and writes the store, and fetching acks messages server-side, so the NSE and the app share mutable crypto state.

- **Locking:** both DBs open in **WAL mode with a 5 s `busy_timeout`** (`store::db::apply_key`). Within a process, all DB work is serialized on one connection, so the only contention is app vs. NSE. Covered by a two-connection contention test.
- **Fetch race:** the mailbox is remove-on-**ack**, so concurrent app and NSE polls can both receive a message. The first to decrypt advances the ratchet, persists (idempotent by `server_id`), and acks. The second's decrypt fails on the moved ratchet and is skipped, with no duplicate row and no event. The window is small (a push fires only when there's no live socket), but a cold launch from a tap can overlap an NSE fetch, so this is by design.

## Stage 5

*Signal-parity presentation. Status: Built, gated off.*

The Stage-3 model rewrote each push's own banner, coupling banner count to push count. The mailbox is drain-once, so in a burst of N messages one NSE invocation drains all N and the other N−1 find nothing. Each of those shows the generic placeholder, giving N rich banners plus stray "New message" banners.

The fix copies Signal (`docs/signal-research/notification-service-extension.md`):

- **Every fetched message is its own local notification** (`UNUserNotificationCenter.add`); the triggering push is never rewritten.
- **The triggering push completes with empty content**, suppressing its banner. An empty fetch shows nothing, which is correct: a sibling invocation or the app already presented those messages.
- **Failures complete silently**, with one exception: if the device hasn't been unlocked since boot (`errSecInteractionNotAllowed` reading the DB key), show a single static "unlock your phone" banner per process.
- **A newer push supersedes a queued fetch.** Fetches are serialized in the NSE, and a queued fetch overtaken by a newer push skips its fetch, since any fetch drains every mailbox. The in-flight blocking FFI can't be interrupted.
- `os_log` on every branch (subsystem `net.theavalanche.nse`).

**The entitlement gate.** iOS ignores an empty completion and shows the original payload **unless the app holds `com.apple.developer.usernotifications.filtering`**, a per-account grant requested from Apple; E2EE messaging is the canonical approved use. Verified on device: without it, one message produced both the rich notification and the placeholder. Until the grant lands, `NotificationService.hasFilteringEntitlement` is `false` and the NSE runs the **interim rewrite model**:
- the newest message rewrites the triggering banner;
- any other messages are posted as local notifications;
- no-op and failure paths show the generic placeholder.

So a burst can still leave stray generic banners. **To enable:** obtain the entitlement, add it to the NSE entitlements in `project.yml`, flip `hasFilteringEntitlement`, and re-run the Stage 5 device checks.

**Decision: fail silent, not "degrade to a generic banner".** On the no-op paths a generic banner is a *false* signal (there is no unseen message), and it fires on every burst, whereas real fetch failures are rare. Signal accepts rare silence and compensates with APNs token-health detection. Ours (lost-push detection) is Planned in `02` and not built. Until it lands, rare silent misses are possible once the entitlement is on.

## Background lifecycle (0xDEAD10CC and the socket handoff)

*Status: Built (Stage 6).* Modeled on Signal's teardown (`docs/signal-research/socket-and-suspension-lifecycle.md`).

Moving the DBs into the App Group container made the app subject to iOS's shared-container rule: **a process suspended while holding a file or SQLite lock on a shared-container file is killed** (`0xDEAD10CC`). This showed up in the field: nine kills in one day on TestFlight (2026-08-26), presenting as "the app resets to the first tab." A second, coupled problem: a socket left open across suspension keeps the device in the server's live-connection map (`routes/messages.rs` pushes only to devices without a live socket), so messages that arrive while suspended produce **no push**. The crashes had been masking this, because a killed process closes its socket.

The design:

- **scenePhase → `.background`:** `AppState.sceneDidEnterBackground` takes a `beginBackgroundTask` assertion and changes nothing else. The socket stays open and messages keep flowing (with live local banners) for the ~30 s iOS grants, so rapid app-switching keeps a warm connection.
- **Expiration handler** (the only "about to suspend" signal): `quiesceForSuspension` calls `prepare_for_background()` per core in parallel on GCD — never `Task.detached`, because blocking FFI must stay off the cooperative pool. On the Rust side:
  1. set the `backgrounded` flag;
  2. the receive loop sends a clean WS Close, so the server drops the socket and pushes resume;
  3. the reconnect loop parks;
  4. wait (bounded, 2 s) for the state to leave `Connected`;
  5. suspend the store gates.

  Then the assertion is released and the process suspends holding nothing.
- **Store gate** (`store::GatedConnection`): every SQLite call on both DBs holds a read guard. Suspension parks new calls, drains the in-flight one, and then **closes the connection**. Draining alone wasn't enough: a WAL-mode connection holds a shared lock for its whole open lifetime, and a drain-only build still died with RunningBoard naming all six idle DB files. The first call after resume reopens, re-applying the key, pragmas, and the storage-sync commit hook. The gate keeps the DB path and passphrase in memory to make that possible, a deliberate trade since the passphrase is memory-resident at every open anyway. Future writers are gated automatically.
- **scenePhase → `.active`:** end the assertion. If the expiration path ran, `resume_from_background()` reopens the gates and reconnects.

Deliberate divergences from Signal: no cross-process connection lock (our NSE fetches over HTTP and never takes over a socket), and quiescing is a store-layer gate rather than task cancellation (Swift can't cancel our tokio writers; parking gives the same "quiet at suspension" invariant).

**Platform scope: iOS only.** Android has no shared-container suspension rule and wants background socket delivery; Desktop never suspends. Both expose the FFI methods but never call them.

Accepted residual risks:
- A store call blocked on the cross-process busy timeout (≤ 5 s) can outlive the quiesce wait.
- iOS can suspend from some non-`.background` states without warning.
- Sockets left behind by crashes need the **server-side WS idle timeout**, which doesn't exist yet (`34` Known gaps).

## Privacy analysis

With the alert payload, Apple sees the device token plus a generic alert (that *something* arrived, and when) — the same as the silent push, plus visibility. No sender, content, ciphertext, or pseudonym. This is the Signal posture. Forensics caveat: decrypted banner text then lives in iOS's notification store (`docs/signal-research/foreground-notifications.md`).

## Known gaps

1. **Filtering entitlement not granted** → stray generic banners on bursts (interim model).
2. **No lost-push detection** — required before fail-silent is safe to turn on.
3. **Server-side WS idle timeout missing** — orphaned sockets suppress pushes (`34`).
4. **Relay mode is deploy-time state outside the repo** — record the production `APNS_PUSH_MODE` in the deploy config (`41`).
5. **Push-delivered messages from strangers** show sender and body like any other message. That matches Signal's request notifications, but decide it explicitly alongside the message-request privacy work in `52`.

## Planned

- Obtain the entitlement; flip Stage 5 on and run its device checks.
- Lost-push detection (APNs token-health), before or together with Stage 5.
- **Targeting (deferred):** if fetching every account ever strains the ~30 s budget, the relay could include the recipient's own opaque pseudonym so the NSE fetches one account. Privacy cost: Apple would see a rotating pseudonym alongside the token. Not needed today.

## Staged plan

1. Shared storage — **Built; verified on device.**
2. Relay alert payload behind `APNS_PUSH_MODE` — **Built.**
3. NSE target + `fetch_notifications` + cold-launch tap routing — **Built; verified on device.**
4. Cross-process hardening (WAL, `busy_timeout`, contention test) — **Built.**
5. Signal-parity presentation — **Built, gated off** pending the entitlement.
6. Background lifecycle — **Built.** Mechanism confirmed on device (~30 s window, ~50 ms quiesce). Crash-free confirmation needs a few days of normal use on the close-on-suspend build.

## Test plan

- app-core: `fetch_notifications` returns the right items, advances the ratchet, acks after the durable write, handles the missing-key buffer.
- Cross-process: two connections on one SQLCipher DB interleaving decrypt/ack writes — no corruption, no double-advance (exists in `store::db` tests).
- Device (manual): with the app terminated, a DM and a group message from another account produce **rich** banners.
- Stage 5 device checks (once entitled):
  - (a) a single message shows exactly one rich banner, and the placeholder never flashes;
  - (b) a burst of 8 shows 8 rich banners and zero placeholders;
  - (c) airplane mode after the push lands → nothing shown, and the message appears on app open;
  - (d) a reboot without unlocking → one "phone locked" banner;
  - (e) with the app open and a live WS → in-app notification only;
  - (f) the `net.theavalanche.nse` log identifies the branch taken.
- Migration: an existing install upgrades and opens after the App Group move.
- Stage 6 device checks (launch by tapping the icon, since an attached debugger prevents suspension):
  - (a) messages sent ~10 s, ~60 s and ~5 min after backgrounding arrive via socket, then push, then push;
  - (b) rapid home-screen round-trips during a burst leave zero new `Actnet-*.ips` files;
  - (c) reopening within seconds shows no reconnect flap;
  - (d) the server logs a clean `ws:` disconnect about 30 s after backgrounding.

## Rationale and rejected alternatives

- **Silent `content-available` push** — rejected as the primary path: iOS doesn't reliably deliver it to rarely opened or force-quit apps.
- **Plain alert push with generic text** — rejected: a messenger needs real sender and body.
- **Slim decrypt path / ciphertext-in-push** — rejected after measuring the footprint: they cost privacy or duplicate crypto to dodge a memory cap that doesn't bind.
- **Hybrid fallback** (generic banner on error or timeout, silent on success-with-zero) — rejected for now in favor of exact Signal parity. Revisit if silent misses show up before lost-push detection lands.
- **Drain-only store quiesce** — tried and failed in the field (idle WAL connections hold locks); replaced by close-on-suspend.
- **Per-account targeting in the push** — deferred; fetch-all keeps the payload content-free.
