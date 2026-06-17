# Multi-Account UX

How signed-in identities and their server memberships are surfaced in Settings, and the actions available against each.

## Model recap

The app holds **identities** (DIDs the user controls), and each identity has one or more **server memberships**. Every (identity, server) pair is a row in this UI. Exactly one server per identity is the **discovery server** (published in PLC); the others are additional memberships. See `13-federation.md` for the protocol model.

## Accounts screen (replaces current Settings page)

A 'Scan Invite' row opens the QR scanner. 

Below that, the screen lists every (identity, server) pair, grouped by identity.

```
Scan Invite

[Fred]
  ─ safe-haven.org           home   ·  active today      ·  142 msgs this week
  ─ org.example                     ·  active 3 days ago ·  8 msgs this week

[Anonymous Coward]
  ─ pseudo.example           home   ·  active 1 hr ago   ·  37 msgs this week
  ─ other.example                   ·  active 2 wks ago  ·  0 msgs this week

[+ Sign in to another account]
```

### Sorting

- **Identity groups** are sorted in the order the user created them (oldest first). The group header is the identity's display name.
- **Server rows within a group** are sorted by activity count on that server (highest first).

### Server row contents

Each row shows:

- Server name (the user-visible name, not the URL).
- A `home` tag if this is the identity's discovery server.
- Recency of activity on that server — last sent or received message timestamp, formatted relatively ("active today", "3 days ago").
- A short activity count for context (e.g., messages exchanged via this server in the last 7 days). Exact metric TBD; intent is one glanceable signal.

### Reachability state on the row

A server that's been continuously unreachable past the short blip window stops
being a transient banner and becomes a property of its row here. The state comes
from the per-membership reachability tier owned by the core
(`34-connection-state.md` §"Layer 2"):

- **Online / Retrying** — normal row (activity recency as above). A brief outage
  shows in the global connection banner, not here.
- **ServerDown** (unreachable ≳ 2 min) — the row replaces activity recency with
  a warning glyph + "Unreachable since X". No banner.
- **Abandoned** (unreachable > 7 days) — same, plus the detail screen surfaces a
  **Remove from this device** action (below).
- **Removed by server** — if the server actively refused the membership (HTTP
  403, `34` §"Auth rejection"), a non-discovery row is auto-removed with a
  one-time "You were removed from [Server]" notice; a discovery row instead shows
  "[Home] removed this identity" and routes to **Change home server**.

### Sign in to another account

A single entry point at the bottom of the list. Tapping it offers two paths:

- **Recover an identity** — restore an identity that isn't in the list from a saved passkey / recovery key. Creates a new identity group. See `50-identity-auth-recovery.md`.
- **Add a server to an existing identity** — paste or scan an invite link, then pick which existing identity to join the server as. Adds a new row to that identity's group. The standard server trust-delta screen (`13-federation.md` §Server-join trust-delta screen) gates the join.

The branch between these is presented as two buttons inside the sheet; the user is not asked to disambiguate before tapping the entry point.

## Identity detail screen

Tapping an identity group header pushes a detail screen for that identity. This is the screen for actions that affect the identity as a whole, distinct from any single server membership.

Contents:

- Display name and small profile photo (with edit affordance — see `30-mobile-ux.md` for the name model).
- **Contact QR code** shown directly below the name and photo. Encodes a `/contact/<token>` URL for this identity (per `13-federation.md` §QR code / invite link types) so another user can scan it to add this identity as a contact. Small 'copy' and 'share' buttons alongside the QR code to copy the link or invoke the system share sheet.
- The identity's DID, shown verbatim.
- Created date.
- **Home server** row — shows the current discovery server name and URL, with a chevron that brings you into the Homeserver migration flow (`13-federation.md` §Discovery-server migration).
- Public listing explainer: small text that links to an FAQ page (tbd) with more information about what's listed publicly on the DID, what can only be seen by contacts and what's private.
- **Delete identity** button at the bottom, destructive styling.

### Delete identity

Destructive. Wipes the identity from the network as completely as the protocol allows.

Confirmation sheet:

> **Delete this identity?**
>
> This will delete [Display Name] from [N servers] and mark the identity deleted in the public registry. This cannot be undone.
>
> Your other [N identities] on this device will not be affected. 
>
> [Delete] [Cancel]

On confirm:

1. For each server membership, run the same Leave cascade described below (courtesy leave events, then membership deletion).
2. Submit a tombstone operation to the PLC directory, signed with the rotation key. The DID resolves to a tombstoned state thereafter; future senders resolving the DID see it is gone.
3. Wipe local state for this identity: identity keypair, rotation key, recovery blob references, session tokens, local message history scoped to this identity.

Failure modes mirror migration: if any individual server is uncooperative, proceed anyway — the PLC tombstone is authoritative. If PLC submission fails, stop and offer retry; the identity isn't fully deleted until PLC reflects it.

## Server detail screen

Tapping a server row pushes a detail screen for that (identity, server) pair.

Contents:

- Server display name.
- **Actual server URL** (e.g., `https://safe-haven.org`). Always shown — name alone is not enough to identify the operator.
- Joined date.
- Activity summary (counts, last active).
- Operator / jurisdiction / policy links, same content as the trust-delta screen shown at join time.
- **Leave this server** button at the bottom for non-discovery memberships. The discovery server has no Leave button here — that affordance lives on the identity detail screen as **Change home server** (which routes through migration) or **Delete identity** (which leaves every server). The server detail screen for the discovery server displays an inline note pointing to the identity detail screen for those actions.

### Leave confirmation

Tapping Leave shows a confirmation sheet:

> **Leave b.example?**
>
> You'll be removed from N groups and M Projects on b.example. People you share other servers with will still be able to reach you there. New contacts will reach you at [discovery server name].
>
> [Leave] [Cancel]

On confirm, the client sends courtesy leave events for the affected groups and Projects, then deletes the membership on the server. If the user is offline or the server is uncooperative, the server tombstones the user from its hosted groups/Projects on its own schedule. Either path converges. See `13-federation.md` for the protocol-level cascade.

Leave is the **graceful** path and assumes the server is (eventually) reachable.
It is the normal way to drop a non-discovery membership.

### Remove from this device (unreachable server)

Distinct from Leave. This is the "the server is gone and isn't coming back" path,
available on a non-discovery row once it reaches the **Abandoned** tier
(unreachable > 7 days, `34-connection-state.md` §"Layer 2"), and triggered
automatically when a server refuses the membership (HTTP 403, `34`
§"Auth rejection").

Because the server is unreachable, there is no courtesy cascade — this is a
**local-only de-routing**. Confirmation sheet:

> **Remove [Server]?**
>
> We haven't been able to reach [Server] in [N days]. Removing it here stops the
> app from retrying and hides its connection status. You may still appear as a
> member there if it comes back — this only affects this device.
>
> [Remove] [Cancel]

Two constraints, both load-bearing (`34` §"Removal, migration & the crypto
constraint"):

- **Preserve crypto.** Removal drops the server **membership and routing** but
  must **keep the Signal session and sender-key state** for that membership's
  conversations. A future BLE-mesh transport (`14-bitchat-fallback.md`) carries
  DM/group traffic off exactly that local crypto state; wiping it on removal
  would silently kill conversations that still function offline. Removal is
  *de-routing*, not *de-provisioning*.
- **Groups stay in the list.** Groups hosted on the removed server
  (`groups.hosting_server_url`) are **not** deleted — they remain in the
  conversation list as permanently-unreachable rows (and stay mesh-reachable in
  the future). They represent real memberships the user hasn't left.

### Why the discovery server has no removal path

A 403 or long outage on the **discovery (home)** server cannot be resolved by
local removal — new contacts resolve the identity there via PLC. Both cases route
to **Change home server** (`13-federation.md` §Discovery-server migration), which
is PLC-signed with the rotation key and completes even when the old home is
unreachable. The discovery server's detail screen continues to point at the
identity detail screen for **Change home server** / **Delete identity**, never a
local remove.
