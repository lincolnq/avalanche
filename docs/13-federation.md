# Federation

> **Status: design only, nothing here is implemented.** This document describes the intended federation model and is the working specification we'll build against. No code in the repo implements any of the routing, attestation, multi-homing, or DID-based discovery described below. The `federation` crate is a stub. Treat this as a target, not a description of current behavior.

## Why federation

DIDs + easy migration give us identity portability, but they don't give us **cross-server communication**. Federation is the only thing that does, and that's what earns its complexity:

- **No single operator can be coerced into severing the network.** Subpoena, seizure, or operator capture affects one server's users, not the whole network. Everyone else keeps talking.
- **Different communities have incompatible operator requirements** — jurisdiction, funding, trust. Forcing them onto one server means someone accepts an operator they don't trust, or they don't talk at all. Federation lets each community self-host and still coordinate across movements.
- **Operator incentives stay honest.** In a federated world, users vote with their feet continuously, not only during a crisis.
- **Retrofitting federation later is much harder than designing for it now.** Crypto envelope, addressing, key distribution, and group semantics all bake in assumptions about who-talks-to-whom.

Short version: federation is what makes the threat model real instead of aspirational.

## What federation actually is

At minimum, servers need to:

1. **Accept inbound messages from other servers** — peer server hands us an encrypted blob addressed to one of our users; we queue it for delivery to the user's devices connected to us.
2. **Send outbound messages to other servers** — when a recipient is not a local member, federate the ciphertext to one of the recipient's servers.
3. **Serve prekeys to other servers** — peers can fetch our users' prekey bundles to start sessions.
4. **Resolve DIDs across servers** — given a DID, determine which server is currently published as the user's discovery server (via PLC or equivalent).
5. **Authenticate the origin server** of every inbound federation request — verify that a message claiming to come from `b.example` actually came from `b.example`. **Not** a prior trust relationship; just origin authentication via a server identity key the sending domain publishes at a discoverable location (e.g., `.well-known/actnet-server`). No peering handshake, no allowlist by default.
6. **Abuse controls at the server boundary** — per-origin rate limits, reputation accrued over time, operator-controlled blocklists for known-bad origins. Federation is semi-open by default, but based on user attestations to mitigate spam.

For DMs this is basically (1)–(5). Cross-server groups add complexity (Sender Keys distribution, membership changes) but build on the same primitives. See `docs/01-technical-implementation.md` for the group encryption model.

## Identity and routing model

Every DID has:

- Exactly one **discovery server** (the address published in the DID document via PLC). Used as the catch-all destination by senders who don't share any server membership with the recipient. Changing it = migration (PLC update). Often called "home" in the UI because users understand that word.
- Zero or more **member servers** beyond the discovery server. Each membership is a per-server account record signed by the user, granting access to that server's features and acting as a delivery point for DMs with co-members.

There is no single "routing home." Each member server independently:

- Holds the user's prekey bundle (one per server, see [Prekey distribution](#prekey-distribution)).
- Maintains an inbound queue for the user.
- Accepts device websocket connections from the user.
- Delivers messages from co-members directly to the user's devices.

The discovery server is just *one of* the user's member servers, distinguished only by being the one published in PLC.

### Why this model

Servers have first-class features available only to members (Projects, server-hosted groups). Users need to participate in those features on multiple servers — and once you're a participating member, it's both more efficient and more privacy-preserving for same-server conversations to stay on that server rather than being routed through some other "home" server.

Concretely:

- **Same-community conversations never federate.** An activist group all hosted on `safe-haven.org` talks among themselves with zero cross-server traffic. Federation only kicks in when conversations actually cross server boundaries.
- **Metadata is naturally distributed.** No single server holds the full social graph of a user. Each member server sees only the slice of conversations involving its own co-members.
- **Resilience improves materially.** Discovery server outage only blocks *new* contacts (those with no shared membership). Existing conversations on shared servers keep working.

### Routing rule

The client picks which of its member servers to send through; that server then makes the local-vs-federate decision.

**Default route.** Send via the sender's discovery server S. Then S decides:

1. **Is R a local member of S?** → queue locally on S. R's devices connected to S receive it. **No federation.**
2. **Otherwise** → resolve R's DID to R's discovery server H. Federate the ciphertext to H. R's devices connected to H receive it.

**Learned route.** When the client receives a message from contact C via a websocket to server X, and X is one of the client's member servers, the client records "for C, route via X." Subsequent sends to C go via X instead of the discovery server. If a send via the learned route fails because R is no longer a member of X, the client falls back to the discovery server and forgets the learned route. Storage lives on the contact row (see `52-contacts-and-profiles.md`).

### Why this converges

Four cases for any pair (A, B):

1. **A and B share no server.** Every message federates (A's discovery ↔ B's discovery). Unavoidable.
2. **A and B share a server Z that is neither's discovery.** No traffic flows through Z by default, so neither client ever learns about the overlap. Treated as case 1 — federation forever. Suboptimal but rare in practice, and the alternative (active membership probing of every contact across every member server) is wildly disproportionate to the benefit.
3. **A is a member of B's discovery server X (or vice versa).** Asymmetric initially:
   - First A → B: A sends via A's discovery Y, federates to X, X delivers locally to B.
   - First B → A: B sends via X (B's discovery). X has A locally, delivers via X-websocket. **A learns "for B, route via X."**
   - All subsequent A ↔ B traffic is local on X.

   Converges to no-federation after one round trip in each direction.
4. **A and B share their discovery server.** Both default to it; local delivery from the first message. No federation ever.

The only case the learned-route optimization fails to catch is case 2, and we accept that cost.

### What the server knows

Each server only knows its own membership table. S cannot answer "is R a member of server Z" for any Z ≠ S, and the protocol never requires it to. The client orchestrates routing because the client is the only party that knows both (its own member-server list) and (the contact it wants to message).

### Protocol implications

- **Membership records are per-server, signed by the user.** Each server keeps a local account row keyed by DID, plus server-local state.
- **Prekeys are maintained per-server.** Every server the user is a member of holds its own prekey bundle for the user — independent signed prekey, Kyber prekey, and one-time prekey set, all signed by the user's identity key. The device uploads bundles to each server on join and replenishes per-server.
- **Devices register with every member server.** Each device uploads per-device prekeys to every server the user is a member of, and maintains a websocket (or push subscription) to each. This ensures the device receives messages regardless of which server they were delivered to.
- **Leaving a member server is cheap.** Delete the membership record; no PLC, no migration. Server discards prekeys, queued messages, and device registrations for that user.
- **Threat model widens slightly with each membership.** Every server the user joins sees the metadata of messages they exchange with co-members there. Surfaced in the join flow.
- **Changing the discovery server does not affect memberships.** Memberships are tied to the DID; the DID still resolves (to the new discovery server) after migration.

## Prekey distribution

Each server the user is a member of maintains its own prekey bundle for each of the user's devices.

### Sender flow

1. Sender's client asks the server S it's currently sending through for recipient R's prekey bundles (one per R's device).
2. S checks its local membership table: is R a local member?
   - **Yes** → serve from S's local prekey store for R's devices. No federation.
   - **No** → resolve R's DID to R's discovery server H. Federate to H for the bundles.
3. Return bundles to sender's client; X3DH proceeds per recipient device.

### Why this works

- **OTPKs partition naturally.** Each server's one-time prekeys are consumed only by senders going through that server's prekey endpoint. No cross-server coordination, no consumption races. The "one-time" property holds within each server's bundle independently.
- **Each server gets its own freshly-generated keys.** The device generates distinct signed prekeys, Kyber prekeys, and OTPKs for each member server — no key material is shared across servers. Only the identity key is shared (it signs every bundle), which is what assures senders the bundle is authentic regardless of which server served it. Per-server keys reduce cross-server correlation and limit blast radius if any one server's prekey store is compromised.
- **Common case skips federation entirely.** Combined with same-server message routing, two co-members of any server exchange messages with no cross-server traffic at all — prekey fetch *and* delivery stay local.
- **No metadata leak in the DID document.** The DID resolves only to the discovery server; member-server affiliations are not globally published. Each server only knows about its own members.

### Device-side cost

The device maintains roughly N × M prekey state (N member servers × M devices of the user, but per-device the device only manages its own bundles, so each device manages N bundles):

- On joining a member server: upload initial bundle (signed prekey, Kyber prekey, OTPK batch) for this device.
- Periodically: rotate signed prekey per server.
- On OTPK exhaustion on any server: replenish that server's OTPK batch.
- On leaving a server: nothing to clean up locally; server discards bundles along with the membership record.

Bounded by membership count (small in practice). No coordination between servers required.

## Devices, websockets, and push

The device's per-server connection model:

- **Foreground (app open):** the client opens a websocket to each member server to receive messages in realtime. With typical membership counts (1–3 servers in steady state) this is fine.
- **Background:** no persistent websockets. Each member server has registered a per-(user, server) pseudonym → device token mapping at the push relay (see `docs/41-relay-deployment.md`). When a server has a queued offline message, it calls the relay's wakeup endpoint with the pseudonym; the relay fires a silent APNs/FCM push to the device token. The device wakes, briefly connects to that server to pull, and goes back to background.

## DID resolution

DID → current discovery server is an indirection through a DID resolver (PLC or equivalent). Flow:

1. Sender's server resolves the recipient's DID → discovery server URL.
2. Used either to fetch prekeys (when recipient is not a local member) or to federate a message (same condition).

### Subtleties

- **PLC is a centralization point.** Bluesky's PLC is operated by Bluesky. We'll assume it keeps working for now; we could support other PLC types but that's for future work.
- **Caching DID resolutions is dangerous.** If we cache `did → server` and a user migrates, we'll keep sending to the old discovery server. Short TTLs, or a signed "I've moved" record the old server can hand out.
- **The old discovery server can lie during migration.** If a user migrates away from a hostile server, it can refuse to forward, claim the user never left, or serve stale prekeys. The DID layer must make the *user's* signed migration record authoritative, not the server's say-so.

## QR code / invite link types

QR codes and invite links are HTTPS URLs on the universal-link domain. The **URL path** is the type discriminator; the client routes to the matching flow based on path. The user is not asked to disambiguate.

```
https://go.theavalanche.net/contact/<base64url_token>   → contact-add flow
https://go.theavalanche.net/invite/<base64url_token>    → server-join flow
https://go.theavalanche.net/project/<base64url_token>   → Project-join flow (joins server first if needed)
```

Each token is an opaque base64url-encoded payload that the appropriate server endpoint resolves to the data the client needs (DID + discovery hint for contacts, server metadata + invite-gate info for servers, Project descriptor for Projects). Token resolution happens server-side so the URL stays short enough to QR-encode comfortably and the client doesn't need to parse structured payloads from the URL itself. See `docs/51-invite-tokens.md` for the invite-token format.

### Defaults by path

- **`/contact/<token>`** → add the contact, open the chat. The token resolves to DID + discovery hint; the user never sees the hint. If the user shares no server with the contact, messages federate via the contact's discovery server — that's fine, federation is what makes this work.
- **`/invite/<token>`** → show the **server trust-delta screen** (operator, jurisdiction, what the server will see, policy link), then join on confirm. No contact added.
- **`/project/<token>`** → if not already a member of the Project's server, show the server trust-delta screen as a precondition, then join. Then join the Project. The server-join is incidental scaffolding to the user's actual goal.

### What's deliberately not in invite flows

- **No "would you also like to join their server?" prompt when adding a contact.** Adding a contact doesn't require joining their server, and prompting muddies the action the user clearly wanted.
- **No "change your discovery server?" option.** Migration is a rare, deliberate operation. It lives in Settings, behind a path the user has to go looking for. Surfacing it in an invite flow is noise and a footgun.
- **No sheet that asks the user to pick between contact / server / migrate.** The link type already encoded the decision.

### Server-join trust-delta screen

Shown before joining any server (including as part of a Project invite). Plain language:

> **Join b.example?**
>
> You'll be able to use Projects and groups hosted there, and to message other members of *b.example* directly through their server.
>
> *b.example* will see: which groups and Projects you use here, when you're active here, and the metadata (timing, size, who-to-whom — not contents) of messages you exchange with other *b.example* members. They will not see your messages with people who aren't members of *b.example*.
>
> Operated by Z, based in W. [Policies]

Confirm → prove DID control (sign challenge with identity key) → done.

## Discovery-server migration (Settings only)

Migration is reachable only via settings. Not from invite flows, not from contact adds.

Migration is less load-bearing than in a single-home model: the discovery server is only the catch-all for senders without co-membership. Most of the user's conversation continuity lives in memberships, which persist across migration.

The flow itself (when invoked):

1. **What this means** — plain-language explanation: identity stays, all memberships persist, the new server becomes the published address in your DID, reversible.
2. **Trust delta** — old vs. new operator, jurisdiction, policies.
3. **Authenticate to the new server** — sign challenge with identity key. If the new server is invite-gated, submit the invite token here. (If the user isn't already a member of the new server, this also creates the membership.)
4. **Execute migration** — visible progress for each step:
   1. Authenticated to new server (and joined if needed)
   2. PLC updated (point of no return for new-contact reachability)
   3. Old server notified
5. **Handle failures**:
   - **Old server uncooperative** (compromised, offline, hostile): migration still works — DID + identity key are the user's. New senders can reach the user at the new discovery server as soon as PLC updates. Don't block on old server.
   - **PLC update fails**: stop, do not proceed. Without PLC, new senders can't find the user at the new discovery server. Retry with backoff.
   - **Mid-flight crash**: migration is resumable. Persist state and offer resume on relaunch.

### Things that don't break on migration

- **DID and identity key are unchanged.** Everyone who's ever talked to the user still knows the same DID.
- **Contacts** are references to DIDs; they keep working.
- **Double Ratchet sessions** are between devices, not mediated by the server; existing sessions keep working.
- **Local message history** lives in SQLCipher on the device; migration doesn't touch it.
- **Memberships on other servers are unaffected.** They were never tied to which server was the discovery server.

## UI legibility for multi-homing

- **DMs and contacts are server-agnostic in the UI.** The client merges inbound queues from all member servers; the user never has to think about which server delivered which message.
- **Server-scoped things show their scope.** Groups, Projects, server-hosted channels include a small server chip / label. Visible, not aggressive.
- **No mode switching for basics.** Messaging to anyone is always available from anywhere; no need to "switch to" a server to send a DM. Servers are not top-level navigation.

## What we're deliberately not building:

- **Multiple discovery servers per DID.** DID resolves to exactly one server.
- **Automated server recommendations.** Let users pick deliberately at signup; don't optimize this funnel.
- **Server-discovery directories.** Out of scope for the federation layer; if it happens, it's a separate product surface.
