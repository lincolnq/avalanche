# Connection state & graceful degradation

How the app represents "can I reach the things I care about?" and how that
representation degrades gracefully as a server goes from a momentary blip, to
down for hours, to gone for good — without ever pinning a useless banner on the
screen or silently dropping the user's messages.

## Purpose

`AppCore` is the single source of truth for connectivity. iOS (and the node
bots) render directly from it — no inference from receive-loop side effects, no
client-side timers that drift, no stuck banners. This doc is the design; the
base layer (instantaneous state) is built and shipping, the degradation layers
are specified here for staged implementation (see §"Implementation phasing").

## The shape of the problem: three dimensions

Connectivity is not one boolean. Three independent dimensions compose:

1. **Instantaneous state** — right now, is the WebSocket to a given server open,
   connecting, or backing off? (Built; §"Layer 1".)
2. **Outage duration** — *how long* has a server been continuously unreachable?
   A 5-second blip and a 5-day outage are the same instantaneous state but call
   for completely different UX. (Design; §"Layer 2".)
3. **Transport** — *which* path can carry a conversation? Today there is exactly
   one (server WebSocket). The bitchat mesh (`14-bitchat-fallback.md`) and, later,
   a Nostr relay tier are additional transports that can carry already-established
   DM/group ciphertext when the server can't. Reachability is ultimately "is there
   *any* transport for this," not "is the server up." (Future; §"Layer 3".)

The earlier version of this design modeled only dimension 1, with a binary "any
account not `Connected` → show banner" rule. That's why a permanently-dead
server pinned a global banner forever and the reconnect task hammered it every
≤30 s indefinitely. The fix is to make duration and transport first-class.

Everything below is per **(identity, server) membership** — the same unit the
multi-account UI uses (`53-multi-account-ux.md`). A user with three memberships
has three independent connectivity states that aggregate for display.

---

## Layer 1 — Instantaneous connection state (built)

### `ConnectionState`

A public type exported via UniFFI, owned by `AppCore`:

```rust
pub enum ConnectionState {
    /// Initial state, or after explicit teardown. No reconnect task running.
    Disconnected,
    /// An attempt is in flight (handshake, lazy auth).
    Connecting,
    /// WebSocket is open. Steady state.
    Connected,
    /// Last attempt failed; backing off until `next_attempt_at_ms`.
    /// `unreachable_since_ms` marks when the *current outage* began — set on
    /// the first failed attempt, carried across every backoff cycle, cleared
    /// only on `Connected`. This is the input to Layer 2.
    Reconnecting { next_attempt_at_ms: i64, unreachable_since_ms: i64 },
    /// The server actively refused the membership (HTTP 403 on the
    /// authenticated WS connect / a membership-scoped request): we've been
    /// kicked, not merely unreachable. Terminal — the reconnect task stops.
    /// See §"Auth rejection is not unreachability".
    Unauthorized,
}
```

All timing is owned by the core; iOS only subtracts from "now". `Reconnecting`
carries both the next-retry instant (for the countdown) and the outage-start
instant (for the duration tier) so the client never computes either.

### State ownership & the reconnect task

`AppCore` holds a `tokio::sync::watch` channel as the canonical observable and a
`mpsc` channel for `IncomingEvent`s (messages + receipt updates collapsed into
one stream). A single background **reconnect task** is the only thing that
touches the WebSocket; it is an invariant of a constructed `AppCore` (spawned by
`login` / `create_account` / `finalize_account` / `recover_from_blob`, torn down
on drop). FFI methods read state via the watch channel and events via the mpsc;
they never drive the socket.

The task loops: `Connecting` → try connect → on success `Connected` + run the
receive loop until it errors → on failure publish `Reconnecting` with jittered
backoff and sleep. `publish()` wraps `send_if_modified` so no-op transitions
don't wake waiters.

### Offline-safe construction + lazy auth

`login` does **no network call** — it builds an unauthenticated `net::Client`
from local DB state and returns immediately, so the app launches and renders the
local-DB conversation list even with every server down. The `Client` owns token
state behind a `Signer` abstraction (the identity key from `store`, wired via
`IdentitySigner`); every authenticated call goes through `ensure_authenticated`
(challenge/response on demand, idempotent under concurrency) and a transparent
**401 → drop-token → re-auth → retry-once** wrapper. Session tokens are
in-memory only; a cold launch re-auths lazily on the first call.

> 401 (token stale) and 403 (membership revoked) are deliberately different: 401
> is the transparent re-auth path above; 403 is terminal — see below.

### FFI surface

```rust
fn connection_state(&self) -> ConnectionState;              // cheap snapshot
fn wait_for_connection_state_change(&self, last: ConnectionState)
    -> Result<ConnectionState, AppErrorFfi>;                // blocks until changed
fn next_events(&self) -> Result<Vec<IncomingEvent>, AppErrorFfi>; // drains the queue
```

iOS runs one `stateTask` (loops `wait_for_connection_state_change`) and one
`eventTask` (loops `next_events`) per account.

---

## Layer 2 — Outage duration → reachability tiers (design)

Layer 1 says *what's happening now*; Layer 2 derives *how bad it's gotten* from
`unreachable_since_ms`. The tier is **computed in core** (so devices and bots
agree and the thresholds live in one place) and exposed alongside the state.

| Tier | Continuously unreachable | Intent |
|---|---|---|
| **Online** | — (`Connected`) | normal |
| **Retrying** | < 2 min | transient blip — global banner with countdown |
| **ServerDown** | 2 min … 7 days | treat as a property *of that server*, not the app |
| **Abandoned** | > 7 days | effectively gone — offer to remove it |

Thresholds (`RETRYING_MAX = 2 min`, `ABANDONED_MIN = 7 days`) are core constants,
tunable. The core exposes the tier (e.g. a `reachability()` accessor returning
`{ tier, unreachable_since_ms, next_attempt_at_ms }`) rather than making the
client recompute it.

### Persistence & cold-launch seeding

`unreachable_since_ms` is **persisted** per membership. On a cold launch against
a server that's been dead for three days, the app must land **directly** in the
silent `ServerDown`/`Abandoned` tier — the startup probe
(`Disconnected` → `Connecting` → `Reconnecting`) must **not** reset the outage
clock and flash a 2-minute banner. The reconnect task seeds `unreachable_since`
from the persisted value on its first failed attempt after launch, so the clock
is continuous across process restarts. A successful `Connected` clears the
persisted value. This is the crux of killing the "annoying banner on every
launch" failure.

### Reconnect strategy: timed while Retrying, opportunistic while ServerDown

The probe cadence is **tiered**, because a 5-second blip and a 5-day outage call
for opposite strategies:

- **Retrying** (short) — jittered timed backoff, 1 s → cap 30 s. We expect
  imminent recovery, the user is likely foregrounded and waiting, so probe
  eagerly.
- **ServerDown / Abandoned** (long) — **stop the self-scheduling timer.** A fixed
  long-interval timer (e.g. "every 5 min") is the wrong tool: on iOS a
  backgrounded process is suspended, so the timer doesn't fire on schedule
  anyway — it just produces a probe burst whenever the OS happens to wake the
  app, while draining radio in any foreground idle time. Instead the reconnect
  loop **parks** (awaits a `reconnect_now` signal) and probes **opportunistically**
  — only when there's a reason to expect success or evidence the user cares:

  | Trigger | Source |
  |---|---|
  | App enters foreground | iOS `scenePhase` → active calls `reconnect_now()` |
  | Network path changes to satisfied | `NWPathMonitor` → `reconnect_now()` |
  | User acts on that server | opening a conversation/Settings hosted there, or attempting a send, calls `reconnect_now()` |

  A parked task costs nothing while the process is suspended — no background
  timer, no wakeups, no radio. The first foreground or network change after the
  server returns reconnects within one probe.

A single probe-rate **floor** (don't auto-probe the same membership more than
once per ~couple minutes, even if triggers fire rapidly) keeps a flurry of
triggers from hammering a dead server while foregrounded — the same
last-attempt throttle shape used for profile fetches (`52`).

**`reconnect_now()`** is the general opportunistic-probe entry point (not just an
`NWPathMonitor` hook): it fires the parked task's `Notify`, skipping any wait.

**Fallback liveness.** When a fallback transport (Layer 3) is actively carrying
this membership's traffic and we want prompt server-return detection, the tight
probe applies **only while foregrounded/active** — a suspended app can't probe
regardless, and bitchat runs its own 30 s server-probe while mesh is active
(`14-bitchat-fallback.md` §3).

---

## Auth rejection is not unreachability

A server that keeps **refusing** us is not *down* — it's telling us the
membership is gone (removed by an admin, operator cut-off, account tombstoned).

**401 vs 403 — the contract.** These are deliberately split so the client can
tell "log in again" from "you've been kicked":

- **401** means *unauthenticated* — token missing/expired/invalid. Handled
  transparently in Layer 1 (drop token → re-auth → retry once). Never terminal.
- **403** means *authenticated but forbidden* — the identity is valid (it can
  still mint a token), but its **membership** has been revoked. Re-authenticating
  can't fix it. Terminal.

This requires the server to keep **token issuance identity-scoped** and put the
membership check on the *use* of the token: the auth endpoint mints a token for
any valid identity key, and the **403 surfaces at WS-connect / a
membership-scoped request**, after a successful token grant. If issuance itself
failed for a kicked member, the client couldn't distinguish a kick from a
transient auth glitch. (No kick exists server-side yet; this is the spec for
when it does.)

A persistent **403** transitions the membership to `Unauthorized` and the
reconnect task stops (no point retrying a refusal).

On `Unauthorized` for a **non-discovery** membership: **remove the server
immediately** — drop the membership locally and surface a one-time notice ("You
were removed from [Server]"). This reuses the local force-remove path
(`53-multi-account-ux.md`), which **preserves Signal session / sender-key state**
(see §"Removal must preserve crypto"). For the **discovery (home)** server, a 403
is more severe — the identity's published home has cut it off; do not silently
drop it. Surface "[Home server] has removed this identity" and route to **Change
home server** / migration (`13-federation.md`), the same remedy as a dead home
server.

(401 remains the transparent re-auth path in Layer 1 — token staleness, not
revocation.)

---

## Layer 3 — Transport dimension (future; bitchat)

Today the only transport is the server WebSocket, so "membership reachable" ==
"server reachable." `14-bitchat-fallback.md` adds a BLE mesh that floods
already-established DM/group ciphertext peer-to-peer, independent of any server.
When it lands, a conversation's reachability becomes **"is there any transport
for it"**:

- **Server** — the default path (this doc, Layers 1–2).
- **Mesh** — carries DMs and group messages for sessions/memberships *already
  established via a server*. Cannot establish new sessions (needs server
  prekeys) and cannot run zkgroup state mutations. So mesh is a steady-state
  carrier, not a replacement.

Two design hooks are reserved now so mesh slots in rather than bolting on:

1. **One composed connectivity model, not two.** bitchat §3 sketches its own
   `Online`/`Disconnected`/`MeshActive` machine. That must compose *into* this
   model — per-membership `ConnectionState` + tier (server transport) **×** a
   device-global mesh transport state — not run in parallel. When mesh is
   implemented, its state is added here, not in a separate machine.
2. **The long tier becomes actionable, not silent, when a fallback exists.**
   `ServerDown` goes quiet *today* (no fallback to offer). Once mesh exists, the
   long-tier banner converts from passive ("retrying…") to actionable ("[Server]
   unreachable — enable Bluetooth mesh"), matching bitchat's "Enable Bluetooth
   mesh" CTA. The per-message/per-conversation indicator likewise becomes a
   transport position (server ✓ / mesh `#` / nothing-available) rather than a
   boolean — bitchat §5.4 already specs the `#` glyph.

Nothing in Layers 1–2 should assume the server is the only path; that
assumption is what would force a rewrite when mesh arrives.

---

## Send semantics: queue and retry

A send to an unreachable server does **not** fail-fast. While a membership is
`Retrying` or `ServerDown`, outbound goes to a **pending/queued** state (visually
distinct from `.failed`), persisted, and **drains automatically on reconnect**.
A message only flips to user-facing `.failed` after a ceiling (e.g. exceeding the
`Abandoned` threshold, or an explicit non-retryable error). This is what makes a
multi-day outage tolerable: the user keeps composing; messages flow when the
server (or, later, mesh) returns. The queue is transport-agnostic by design — a
queued message can later drain over mesh without re-queuing.

This is a behavior change from the current fail-on-throw send path and is the
most user-visible part of the degradation work.

---

## Device-offline vs. server-down

If the *device's* network is gone, every membership goes `Retrying` together —
but that's a self-correcting condition, not a fleet of dead servers. We
distinguish the two with `NWPathMonitor`:

- **No network path** (airplane mode, no signal) → a persistent, simple **"No
  internet connection"** banner regardless of duration. It clears itself the
  moment connectivity returns; never degrades to per-server markers.
- **Network up, this server unreachable while others are fine** → the per-server
  tiers above.

`NWPathMonitor` also feeds `reconnect_now()` so a regained network retries
immediately instead of waiting out the backoff.

---

## UX across tiers

The banner stops being global. Aggregation rule: **only `Retrying`-tier
memberships feed the global banner.** The moment a membership crosses into
`ServerDown`, it leaves the banner and becomes a property of *that server*.

- **Online** → no indicator.
- **Retrying** (any membership) → banner; copy and spinner depend on *how many*
  servers are offline and whether any is actively attempting (see below).
- **No network path** → persistent "No internet connection" banner (overrides
  the above).

#### Banner copy & spinner

The banner can't collapse to a single aggregate state — partial outages read
differently from total ones — so it aggregates **counts** across memberships:
`offline` (banner-eligible, i.e. `Retrying`-tier, not-`Connected`), `total`
servers, whether `any is Connecting`, and the earliest `next_attempt_at_ms`.

**Copy:**

- **All servers offline** (none connected) → the per-attempt copy with countdown:
  "Offline · retrying in Ns" while waiting in backoff, "Reconnecting…" while an
  attempt is in flight. Earliest `next_attempt_at_ms` drives the 1 Hz countdown.
- **Some servers offline** (≥1 still connected) → "N servers offline" ("1 server
  offline" singular). No countdown — which server's would it show? — just the
  count.

**Spinner:** the activity indicator means "an attempt is in flight *right now*,"
not "the banner is up." Show it **iff any offline server is `Connecting`**; hide
it while every offline server is merely `Reconnecting` (parked / waiting out
backoff). So a server sitting in backoff shows the banner without a spinner; the
spinner appears only during the brief connect attempt.

Brief `Connecting`/`Disconnected` at launch render like the offline cases above
(no flicker). `ServerDown+` memberships are *not* counted here — they've left the
banner for the Settings / per-conversation surfaces.
- **ServerDown / Abandoned** → **no global banner.** Instead:
  - **Settings** (`53`) — the server row shows "Unreachable since X" with a
    warning glyph in place of activity recency; `Abandoned` rows expose a remove
    affordance.
  - **Conversations** — rows/headers whose hosting server is `ServerDown+` show
    an inline "unreachable" marker; their sends sit in the pending queue.
  - *(Future)* if mesh is available, the long-tier surfaces an "enable mesh" CTA
    instead of going fully quiet.

### Banner placement: list vs. open conversation

The transient banner (Retrying / connecting / "No internet connection") renders
differently depending on where the user is, because a header-blocking pill is
fine on the list but obscures a conversation's title/header when one is open:

- **Chats list (root)** — the floating pill, as today, at the top of the list.
- **Inside a conversation** — **suppress the pill**; instead show a compact
  "disconnected" badge integrated with the **nav back button** (a small
  `wifi.slash`-style glyph overlaid on / beside the back chevron, tinted to match
  the banner). The conversation header stays fully visible.

The app already tracks the active conversation (`AppState.currentConversationId`,
set on the conversation view's appear / cleared on disappear), which is the
suppression signal: the root pill hides while `currentConversationId != nil`, and
the open conversation owns the back-button badge.

This badge represents the same **transient** states the pill does (aggregate
across accounts). It is distinct from the per-conversation **unreachable marker**
below: the badge means "the app is reconnecting"; the marker means "*this
conversation's* server has been down a long time" (`ServerDown+`, which has no
pill at all). The two can coexist — a back-button badge for a brief global blip,
a header marker for a long-dead hosting server — but they answer different
questions.

### Scoping conversations to a server

- **Groups** map cleanly via `groups.hosting_server_url` — a group is unreachable
  exactly when its hosting membership is `ServerDown+`. **Groups stay in the
  list** as permanently-unreachable rows even after the server is removed (they
  remain mesh-reachable in the future and represent real memberships).
- **DMs** depend on per-peer route mapping (the deferred `learned_route_server`,
  `52-contacts-and-profiles.md`). Until that lands, **do not** mark individual
  DMs unreachable (avoids false negatives when a peer is reachable via another
  server) — DMs rely on the short-window banner and the Settings indicator.

---

## Removal, migration & the crypto constraint

The removal flows live in `53-multi-account-ux.md`; two constraints originate
here:

- **Removal must preserve crypto.** "Remove server from this device" (whether
  user-initiated for an `Abandoned` server or auto-triggered by a 403) drops the
  **server membership / routing** but must **keep the Signal session and
  sender-key state** for that membership's conversations. Mesh (Layer 3) carries
  traffic off exactly that local crypto state; wiping it on removal would
  silently kill conversations that still function over mesh. Removal is
  *de-routing*, not *de-provisioning*.
- **Dead/refused home server → migrate, don't drop.** A discovery server can't be
  locally removed (new contacts resolve the identity there via PLC). Both the
  `Abandoned` and `Unauthorized` cases for a home server route to **Change home
  server** (`13-federation.md` §Discovery-server migration), which is PLC-signed
  with the rotation key and so completes even when the old home is unreachable.

---

## Implementation phasing

Rough tiers; each is independently shippable.

1. **Layer 1 — instantaneous state (done).** `ConnectionState`, the reconnect
   task, offline-safe `login`, `net::Signer` + lazy/401-retry auth, the three FFI
   methods, per-account iOS `stateTask`/`eventTask`, the countdown banner.
2. **Layer 2a — duration tiers.** Add `unreachable_since_ms` to `Reconnecting`,
   persist it, compute the tier in core, seed it on cold launch. Switch
   `ServerDown` from timed backoff to **opportunistic probing** (park on
   `reconnect_now`; triggers: foreground, network change, user action) with a
   probe-rate floor. Re-point the iOS banner aggregation at the `Retrying`-only
   rule. Settings + group unreachable markers.
3. **Layer 2b — queue & retry.** Pending send state, persistence, auto-drain on
   reconnect, `.failed` ceiling.
4. **Auth rejection.** `Unauthorized` state on 403; non-discovery auto-removal;
   home-server → migration routing.
5. **Device-offline.** `NWPathMonitor` integration: the "No internet connection"
   banner and the network-change trigger for `reconnect_now()` (the
   `reconnect_now` entry point itself and its foreground / user-action triggers
   land in 2a; this adds the network-path trigger).
6. **Layer 3 — transports (future).** Compose mesh state into this model;
   actionable long-tier banner; transport-position message indicator. Gated on
   `14-bitchat-fallback.md`. A later Nostr tier slots in the same way.

---

## Risks & open items

- **Lock-during-IO in the reconnect task.** `try_connect_ws` must clone the bits
  it needs (server URL, token, identity handle) out of the `inner` lock before
  the HTTP challenge + WS handshake, or it blocks `send_dm` for seconds.
- **Reconnect-task cancellation latency.** Drop-driven via `Weak::upgrade`;
  worst case is the in-flight network call (TCP timeout, ≤30 s). A
  `CancellationToken` is a clean upgrade.
- **`next_events` single-consumer.** Wrapped in `Mutex<Receiver>`; two callers
  serialize rather than deadlock. Documented on the FFI method.
- **Persisted-clock correctness.** The cold-launch seeding must be careful that a
  *successful* reconnect clears the persisted `unreachable_since` promptly, or a
  recovered server could briefly mis-render as still-down.
- **403 transience.** "Remove on 403" assumes a 403 on the auth handshake is
  authoritative. If real deployments produce spurious 403s, require the refusal
  to persist across a couple of attempts before treating it as revocation.
- **DM route mapping** is the gating dependency for per-DM unreachable markers;
  tracked in `52-contacts-and-profiles.md`.

## Out of scope (tracked elsewhere)

- WS-level keepalive pings (silent-death detection) — server has the path;
  client needs to schedule sends and treat missed pongs as a disconnect.
- Core `tracing` events surfaced in the in-app log viewer.
- Session-token persistence across launches (the signed-request auth migration
  would obviate tokens entirely).
- Android port — mirror the iOS task layer and tier rendering.
