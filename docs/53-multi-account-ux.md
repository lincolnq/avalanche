# 53 — Multi-account UX

> **Status:** Partial — the Accounts screen, identity detail (contact QR, DID, delete identity), server detail, and leave-server are built. Adding a server to an existing identity, activity stats, reachability state on rows, remove-from-device, and change-home-server are not.
> **Last verified against code:** 2026-10-03

## Summary

How identities and their server memberships are shown in Settings, and the actions available on each. The app holds one or more **identities**; each identity has one or more **server memberships** (accounts). Exactly one server per identity is its **discovery (home) server**.

In practice every identity has exactly one server today, because joining a second server with an existing identity is not built. The screens are designed for the general case.

Headings "Delete identity" and "Leave confirmation" are cited from code (`docs/53 §Delete identity`, `§Leave`) and are stable.

## Known gaps

- **"Add a server to an existing identity" doesn't register anything.** iOS `AppState.joinServer` and Android `AppViewModel.joinServer` only append a `ServerInfo` to the local account; they never call the new server. The row appears, but the identity has no account there (`06` §9).
- **Server rows show name and the `home` tag only.** No activity recency or message counts.
- **No reachability state on rows** (ServerDown / Abandoned / removed-by-server). The core's connection-state tiers these depend on are not built (`34`).
- **No "Remove from this device" action** and **no "Change home server"**: the identity detail screen's home-server row shows a "not implemented yet" stub (`IdentityDetailView.swift`).
- **The contact QR is a personal invite token**, not a separate `/contact/<token>` type (`51`).

## Current design

### Accounts screen

**Built** (`AccountsView.swift`, Android and Desktop equivalents). Top to bottom:

- **Scan Invite** — opens the QR scanner.
- Every identity, with its server rows grouped under it. Identity groups appear in creation order; the header is the display name. Each server row shows the server's name and a `home` tag on the discovery server.
- **Add an account** — scan an invite QR, enter an invite link, or **recover a different identity** (`AddAccountView.swift`).
- Get Help, About.

### Identity detail screen

**Built** (`IdentityDetailView.swift`). Tapping an identity header shows:

- Display name and editable photo.
- **Contact QR code** with copy and share. It encodes the identity's personal invite link, `https://go.theavalanche.net/i/<token>` with `{s: home server, d: DID}` (`51`); scanning it lands the scanner in a DM with this identity.
- The DID, verbatim.
- **Home server** row (migration stub; Known gaps).
- An explainer of what is public: "Your home server is listed publicly so people can reach you. Your display name, other server memberships, contacts, and messages are not public." (Under `50` §Proposed P3 the home server would no longer be public either.)
- **Delete identity**, destructive.

### Delete identity

**Built** (`app-core/src/lib.rs` `delete_identity`; all three platforms). Wipes the identity from the network as completely as the protocol allows.

Confirmation: "This will delete <name> from <N> servers and mark the identity deleted in the public registry. This cannot be undone. Your other identities on this device will not be affected."

Order (load-bearing):

1. For each account, leave every group (best effort), then delete the account on the server (best effort; proceed even if a server is uncooperative).
2. Submit a rotation-key-signed **PLC tombstone**. This must succeed; on failure the core returns `IdentityDeletionFailed` and keeps local state so the user can retry.
3. Only then wipe identity.db and every device.db.

### Server detail screen

**Built** (`ServerDetailView.swift`). Server name and actual URL (the name alone does not identify the operator). The discovery server shows "Home server for <name>" and a note pointing to the identity detail screen for changing home or deleting the identity; it has no Leave button. Other servers show **Leave this server**.

### Leave confirmation

**Built** (`app-core/src/lib.rs` `leave_server`). Confirmation: "You'll be removed from any groups and Projects on <server>. People you share other servers with will still be able to reach you there. New contacts will reach you at <home server>."

On confirm the core leaves every group hosted on that server (courtesy leave actions, `03`), then deletes the account on the server. Because each core is bound to one server today, leaving removes that account from the device. Leave is the **graceful** path and assumes the server is reachable.

## Planned

### Add a server to an existing identity

From an invite, pick an existing identity; the core registers the identity on the new server (fresh device registration and prekeys for that server, same identity key), adds an account context, and re-uploads the recovery blob with the new server list. Requires the multi-account `AppCore` contexts in `06` §9. Under the client-side federation proposal (`13`), a second server is something you join for that community, not something you need in order to talk to people there.

### Row activity and reachability

Each server row shows activity recency ("active today") and a single glanceable activity count, sorted by activity within the identity. When the core's reachability tiers exist (`34`):

- **Online / Retrying** — normal row; brief outages show in the global banner.
- **ServerDown** (unreachable ≳ 2 min) — the row shows "Unreachable since X". No banner.
- **Abandoned** (unreachable > 7 days) — same, plus **Remove from this device** on the detail screen.
- **Removed by server** (HTTP 403, `34`) — a non-discovery row is auto-removed with a one-time notice; a discovery row shows "<Home> removed this identity" and routes to **Change home server**.

### Remove from this device (unreachable server)

Distinct from Leave: a **local-only de-routing** when a server is gone. Confirmation: "We haven't been able to reach <server> in <N> days. Removing it here stops the app from retrying and hides its connection status. You may still appear as a member there if it comes back — this only affects this device."

Two load-bearing constraints:

- **Preserve crypto.** Drop the membership and routing but keep the Signal sessions and sender keys for that membership's conversations, so a future offline transport (`14`) can still carry them. Removal is de-routing, not de-provisioning.
- **Groups stay in the list** as unreachable rows. They are real memberships the user hasn't left.

### Change home server

The discovery server has no remove path: a 403 or long outage there routes to **Change home server** (migration, `13`), which completes even when the old home is unreachable. Today that means a rotation-key-signed PLC update; under `50` §Proposed P3 it becomes a signed move notice delivered to contacts.

## Rationale and rejected alternatives

- **Every (identity, server) pair is a visible row (decided).** Users need to see where each persona is registered; hiding servers behind identities would make leave and reachability opaque.
- **The discovery server cannot be left in place (decided).** New contacts find the identity there; dropping it without migrating would strand the identity.
- **Delete identity tombstones PLC before wiping locally (decided).** The tombstone is the authoritative "gone" signal; wiping first would make a failed tombstone unretryable.
- **Remove-from-device wipes crypto (rejected).** Would silently kill conversations an offline transport could still carry.
