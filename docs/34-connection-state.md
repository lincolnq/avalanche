# 34 — Connection state & graceful degradation

> **Status:** Partial — Layer 1 (instantaneous `ConnectionState`, the reconnect task, offline-safe login, lazy 401 re-auth) and iOS background parking are built. Not built: outage-duration tiers, persisted outage clock, opportunistic probing for long outages, the 403/`Unauthorized` path, the outbound send queue, device-offline detection, and the tiered banner. The current banner shows whenever *any* account is not connected — the exact failure mode this doc was written to fix.
> **Last verified against code:** 2026-10-03

## Summary

`AppCore` is the single source of truth for connectivity; UIs and bots render from it with no client-side timers. Connectivity has three dimensions: **instantaneous state** (built), **outage duration** (planned), and **transport** (speculative, for mesh). The goal is that a server going from a blip, to down for hours, to gone for good degrades gracefully: no permanently pinned banner, no endless hammering of a dead server, and no silently dropped messages.

Everything is per **(identity, server) membership** — the unit `53` uses — and aggregated for display.

Code: `core/crates/app-core/src/connection.rs` (`reconnect_loop`), `lib.rs` (`ConnectionState`, `connection_state`, `wait_for_connection_state_change`, `next_events`, `reconnect_now`, `prepare_for_background`, `resume_from_background`), `net/src/lib.rs` (`ensure_authenticated`, 401 retry), iOS `Views/Common/OfflineBanner.swift`, `AppState.aggregateConnectionState`, Desktop `components/OfflineBanner.tsx`.

## Layer 1 — Instantaneous connection state

*Status: Built.*

### `ConnectionState`

```rust
pub enum ConnectionState {
    Disconnected,                              // initial, or parked while backgrounded
    Connecting,                                // attempt in flight (lazy auth + WS handshake)
    Connected,                                 // WS open
    Reconnecting { next_attempt_at_ms: i64 },  // last attempt failed; backing off
}
```

There is **no** `unreachable_since_ms` and **no** `Unauthorized` variant yet (both Planned, below).

### State ownership & the reconnect task

`AppCore` holds a `tokio::sync::watch` for state and an mpsc for `IncomingEvent`s. A single background **reconnect task** is the only thing that touches the WebSocket. It holds a `Weak<AppCore>` and exits when the core is dropped. Its loop:

1. If backgrounded (`16` §background lifecycle), publish `Disconnected` and park until foregrounded.
2. Publish `Connecting`; try to connect (lazy auth + WS handshake), bounded by a **15 s** `CONNECT_TIMEOUT`.
3. On success: `Connected`, subscribe group push pseudonyms, top up prekeys, run the receive loop until it errors. Backoff resets only if the connection lasted ≥ 5 s (so a flapping server doesn't bounce 1-2-4-1-2-4 forever).
4. On failure: publish `Reconnecting { next_attempt_at_ms }` with jittered (0.75–1.25×) exponential backoff, **1 s → cap 30 s**, and sleep. `reconnect_now()` wakes the sleep early and resets backoff.

The loop retries **forever at ≤ 30 s** regardless of how long a server has been down. That is the Layer 2 gap.

### Offline-safe construction + lazy auth

`login` does **no network call**: it builds an unauthenticated `net::Client` from local state, so the app renders local conversations with every server down. Authenticated calls go through `ensure_authenticated` (challenge/response on demand, idempotent under concurrency) and a transparent **401 → drop token → re-auth → retry once** wrapper. Session tokens are in memory only.

### FFI surface

```rust
fn connection_state(&self) -> ConnectionState;
async fn wait_for_connection_state_change(&self, last: ConnectionState) -> ConnectionState; // native async export
async fn next_events(&self) -> Vec<IncomingEvent>;                                           // native async export
fn reconnect_now(&self);
fn prepare_for_background(&self);    // iOS only
fn resume_from_background(&self);    // iOS only
```

The two long waits are native async exports, so platform loops can park on them without pinning a thread (root `CLAUDE.md` pattern 4).

Who calls `reconnect_now` today: Desktop's banner Retry button. iOS gets the equivalent through `resume_from_background` (which unparks the loop with reset backoff). Nothing calls it on network change or on user action. Android has the method wired but no caller.

### Current banner

iOS (`OfflineBanner` / `aggregateConnectionState`) and Desktop show one global pill whenever **any** account is not `Connected`, with a spinner always on and a "retrying in Ns" countdown from the earliest `next_attempt_at_ms`. With one permanently dead server among several, the banner never goes away. There's no in-conversation badge; the pill color is hardcoded rather than a design token.

## Layer 2 — Outage duration → reachability tiers

*Status: Planned.*

| Tier | Continuously unreachable | Intent |
|---|---|---|
| **Online** | — (`Connected`) | normal |
| **Retrying** | < 2 min | transient blip — global banner with countdown |
| **ServerDown** | 2 min … 7 days | a property *of that server*, not the app |
| **Abandoned** | > 7 days | effectively gone — offer to remove it |

- Add `unreachable_since_ms` to `Reconnecting`: set on the first failure, carried across backoff cycles, cleared on `Connected`. Compute the tier **in core** (thresholds `RETRYING_MAX = 2 min`, `ABANDONED_MIN = 7 days`) and expose it (e.g. `reachability() -> { tier, unreachable_since_ms, next_attempt_at_ms }`).
- **Persist the outage clock** per membership, so a cold launch against a server dead for three days lands directly in the silent tier instead of flashing a 2-minute banner. Clear it promptly on success.

### Reconnect strategy: timed while Retrying, opportunistic while ServerDown

- **Retrying** — jittered timed backoff, 1 s → 30 s (as today).
- **ServerDown / Abandoned** — **stop the self-scheduling timer** and park on `reconnect_now`. On iOS a suspended process doesn't fire timers anyway; a long timer just produces probe bursts on wake and drains the radio while idle. Probe only on:

  | Trigger | Source |
  |---|---|
  | App enters foreground | iOS `scenePhase` / Android lifecycle → `reconnect_now()` |
  | Network path becomes satisfied | `NWPathMonitor` / `ConnectivityManager` → `reconnect_now()` |
  | User acts on that server | opening a conversation or settings hosted there, or a send → `reconnect_now()` |

- A **probe-rate floor** (at most one auto-probe per membership every couple of minutes) keeps a flurry of triggers from hammering a dead server.

## Auth rejection is not unreachability

*Status: Planned. Nothing on the server returns a membership 403 yet.*

**401 vs 403 — the contract:**

- **401** — unauthenticated (token missing or expired). Handled transparently in Layer 1. Never terminal.
- **403** — authenticated but the **membership** is revoked (kicked, operator cut-off, tombstoned). Terminal.

This requires the server to keep **token issuance identity-scoped** and put the membership check on the *use* of the token (WS connect, membership-scoped requests), so a kick is distinguishable from a transient auth failure.

On a persistent 403: add an `Unauthorized` state; stop the reconnect task.
- **Non-discovery membership:** remove it locally with a one-time notice ("You were removed from [Server]"), preserving crypto state (below).
- **Discovery (home) server:** don't silently drop it; surface "[Home server] has removed this identity" and route to Change home server (`13`).
- If real deployments produce spurious 403s, require the refusal to persist across a couple of attempts before treating it as revocation.

## Layer 3 — Transport dimension

*Status: Speculative (gated on `14`).* With a BLE mesh, reachability becomes "is there *any* transport for this conversation." Two hooks to keep:

1. **One composed model.** Mesh state composes into this model (per-membership server state × device-global mesh state); no parallel state machine.
2. **The long tier becomes actionable when a fallback exists** ("[Server] unreachable — enable Bluetooth mesh").

Nothing in Layers 1–2 should assume the server is the only path.

## Send semantics: queue and retry

*Status: Planned.* Today a send to an unreachable server fails immediately and the message shows as failed. The plan: while a membership is `Retrying` or `ServerDown`, outbound goes to a persisted **pending** state (visually distinct from failed) that drains on reconnect. It flips to failed only past a ceiling (e.g. `Abandoned`) or on a non-retryable error. The queue is transport-agnostic, so it can later drain over mesh.

The store already has an outbound `message_queue` table with `enqueue` / `drain` / `mark_delivered` (`store/src/messages.rs`), but app-core never uses it. Wire it in or delete it.

## Device-offline vs. server-down

*Status: Planned.* Use `NWPathMonitor` (iOS) / `ConnectivityManager` (Android) / `navigator.onLine` (Desktop):

- **No network path** → a simple persistent "No internet connection" banner that clears itself; never degrades into per-server markers.
- **Network up, this server unreachable while others are fine** → the per-server tiers.

The path monitor also feeds `reconnect_now()`.

## UX across tiers

*Status: Planned* (except the single global pill described under Layer 1).

**Only `Retrying`-tier memberships feed the global banner.** Once a membership crosses into `ServerDown`, it leaves the banner and becomes a property of that server.

- **Online** → nothing.
- **Retrying** → banner. Copy aggregates counts:
  - all servers offline → "Offline · retrying in Ns" / "Reconnecting…";
  - some offline → "N servers offline", no countdown.

  **Spinner iff some offline server is `Connecting`** right now.
- **No network path** → "No internet connection" (overrides the above).
- **ServerDown / Abandoned** → no global banner:
  - **Settings** (`53`): the server row shows "Unreachable since X"; `Abandoned` rows offer removal.
  - **Conversations:** groups hosted there show an inline "unreachable" marker; their sends sit pending.

### Banner placement: list vs. open conversation

On the chat list, show the floating pill. Inside a conversation, suppress the pill and show a compact disconnected glyph on the back button instead, so the header stays visible. The suppression signal is the existing `AppState.currentConversationId`. The back-button badge (transient, app-wide) and the per-conversation unreachable marker (long outage of *this* server) answer different questions and can coexist.

### Scoping conversations to a server

- **Groups** map via `groups.hosting_server_url`. **Groups stay in the list** as unreachable rows even after their server is removed.
- **DMs** need per-peer routing to be marked. Until then, don't mark individual DMs unreachable.

## Removal, migration & the crypto constraint

Constraints for the flows in `53`:

- **Removal must preserve crypto.** "Remove server from this device" drops routing but **keeps Signal session and sender-key state**, so conversations could still work over another transport. Removal is de-routing, not de-provisioning.
- **A dead or refusing home server → migrate, don't drop** (`13`).

## Known gaps

1. **Dead server pins the banner forever** and is probed every ≤ 30 s indefinitely (Layer 2 unbuilt). P1 — user-visible and battery-draining with multiple accounts.
2. **No send queue** — sends fail fast. P2.
3. **No 403 handling** — a kicked account retries forever. Blocked on server-side kicks existing. P2.
4. **No network-path awareness** on any platform. P2.
5. **Server-side silent socket death.** The client sends a keepalive every 45 s and tears down a connection with no inbound traffic for about two intervals (`net/src/ws.rs`). The server only echoes keepalives; it never pings and has no idle timeout. A socket abandoned by a crashed or suspended client therefore stays in the server's live map until TCP gives up, and the server only pushes to devices *without* a live socket, so pushes stop meanwhile. P1. Tracked in `02` and `16`.
6. **Reconnect-task cancellation** is drop-driven (`Weak::upgrade`); worst case it waits out the 15 s connect timeout. A `CancellationToken` would be cleaner. P3.

## Planned (implementation order)

1. **Layer 2a** — `unreachable_since_ms`, persisted clock, tier in core, opportunistic parking for `ServerDown` with a probe floor; wire `reconnect_now` to foreground and user action on all platforms; switch banner aggregation to `Retrying`-only; Settings and group unreachable markers.
2. **Server keepalive** — server-originated WS ping plus an idle timeout that drops sockets which stop answering.
3. **Layer 2b** — persisted pending send state, drain on reconnect, failed ceiling.
4. **Device-offline** — path monitors and the "No internet" banner.
5. **Auth rejection** — `Unauthorized` on 403, auto-removal, home-server → migration.
6. **Layer 3** (speculative) — mesh composition.

## Rationale and rejected alternatives

- **"Any non-Connected account → banner"** (the original design and today's code) — rejected: a dead server pins the banner forever. Duration has to be first-class.
- **Fixed long-interval timer for long outages** — rejected: doesn't fire while suspended on iOS, bursts on wake, drains the radio. Opportunistic probing instead.
- **Treating 403 as unreachability** — rejected: a refusal isn't an outage, and retrying can't fix it.
- **Wiping crypto state on server removal** — rejected: kills conversations a fallback transport could still carry.
- **Marking DMs unreachable before per-peer routing exists** — rejected: false negatives.
