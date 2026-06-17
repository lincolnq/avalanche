# Mobile App UX

## First launch

On first launch with no identity, the app shows a splash screen with two paths:

- **Scan invite QR code** — opens the camera to scan an invite code
- **Enter invite link** — paste or type a link manually

There is no "create account" flow independent of a server invitation. You always join a server.

## Invite links

Invite links are web URLs on the homeserver's domain (e.g., `https://myorg.example.com/invite/<token>`). QR codes encode the same URL. The link opens in the browser, where the homeserver serves a landing page that:

- Explains what's happening ("You've been invited to join [Org Name]")
- Links to the App Store / Play Store if the app isn't installed
- Has an "Open in Avalanche" button that links to `https://go.theavalanche.net/invite/<server>/<token>`, which opens the app via Universal Links (iOS) / App Links (Android)

## Registration flow

### New user (no existing identity)

When the app receives an invite link (either via deep link or QR scan):

1. The app contacts the server and validates the invite token.
2. **Display name screen.** "What's your name?" with a text field and optional photo upload. Display name is required; photo is not. This is the only screen the user must interact with.
3. The app generates keys (identity key, prekeys) and registers with the server in the background. The user never sees this.
4. The server creates the account, generates a DID, and auto-enrolls the user into whatever groups/Projects the invite token specified.
5. **Push notification permission prompt.** iOS requires explicit permission; ask here with context ("Get notified when your team sends a message").
6. The user lands in the **Chats tab** with their groups already populated.

If the invite token specifies an onboarding Project (e.g., a conference registration flow), that Project's onboarding UI runs between steps 4 and 5. The Project can collect additional information (organization, role, dietary restrictions, whatever it needs). The substrate display name is already set; the Project collects Project-specific data.

Total interaction for the minimal case: scan, type name, tap continue, approve push notifications. A few seconds of background work. That's it.

### Existing user (already has one or more identities)

When the app receives an invite and the user already has identities:

1. The app shows a choice of identities:
   - **Join as [Alice]**
   - **Create a fresh identity**
   - **More options...**
   The Alice prefill is your most recently used identity. More options presents a list of all your signed-in accounts alongside which servers they are bound to.
2. If joining as an existing identity: the app registers that DID with the new server (signs a challenge to prove ownership, uploads fresh prekeys). One confirmation tap.
3. If creating a new identity: full new-user flow — new DID, new name, new identity.
4. Either way, auto-enrollment and onboarding proceed as above.

Creating a separate identity is the right choice when you want to keep identities apart — e.g., organizing pseudonymously with one group while using your real name with another. Most users will just tap their existing name.

## Account recovery (passkey)

During initial signup, after entering a display name, the app prompts the user to create a passkey. This is a single biometric prompt (Face ID / fingerprint) — the passkey is stored in the user's password manager or iCloud Keychain and syncs across their devices automatically. The passkey protects an encrypted recovery blob (containing the user's DID rotation key and identity keypair) stored on each homeserver the user is registered on. See `docs/50-identity-auth-recovery.md` for the full design.

## Display name

Display name is attached to a DID, required at account creation. It is what other users see in chats. The name is client-owned — stored locally and pushed to every server the DID is registered on. Changing your name updates it everywhere. This is the same model Signal uses for profile names.

If you want different names in different contexts, create separate identities (separate DIDs). There are no per-server name overrides — one DID, one name.

## Multi-account

The app supports multiple identities (multiple DIDs). Each has its own display name, keys, and set of servers. All identities' chats and servers appear together in the Chats and Network tabs — you don't switch identities to see different content. Each chat and server has a subtle indicator showing which identity it's associated with.

When you send a message, you send as whichever identity is a member of that group. When starting a new DM with someone reachable from multiple identities, the app asks which identity to use, defaulting to the one that shares a server with the recipient.

### Multiple identities in the same conversation

It is possible to join the same group or server with multiple identities. The app doesn't prevent this, but warns you: "You're already on [Server] as [Alice]. Join as [Bob] too?" The server sees two unrelated accounts and can't tell they're the same person.

When multiple of your identities are present in a conversation, the app shows a small identity indicator with a way to switch. It defaults to whichever identity you last sent from in that conversation. Messages from your active identity render as "you" (right side); messages from your other identities render like any other participant (left side, with name and avatar). Seeing your own name on the left is a strong cue that you may be on the wrong identity.

## Navigation

Three tabs:

- **Calls.** Voice and video calls.
- **Chats.** Unified inbox across all servers, sorted by recency. Every DM and group you belong to appears here. This is the default tab and primary surface.
- **Network.** Hierarchical list of servers you're on. Each server expands to show its Projects. Tapping a Project opens it full-screen with its own navigation.

Projects open as full-screen views. Group chats managed by a Project appear in the Chats tab like any other chat; the Project view is for non-chat surfaces (maps, dashboards, sign-up flows). Projects and chats are deep-linkable in both directions.

## Compose

> **Status: partially implemented.** The core slice is live in
> `mobile/ios/Actnet/Sources/Views/Chats/ComposeMessageView.swift`: chip
> field, autocomplete sectioned into People / Other from the local
> contacts table, direct `did:`-prefix entry, dedupe, group-name
> auto-default, and dispatch (1 chip → DM in existing thread, 2+ chips
> → `create_group` + `invite_members` fan-out + first message).
>
> Not yet implemented: From pill with "Send as" sheet, server pinning,
> yellow / red chips for cross-server / unreachable recipients, paste
> as multi-recipient, profile preview on chip tap, partial-failure
> banner on group create, custom group icon at creation time.
> Group-name override (user editing the placeholder) is also not yet
> wired — the auto-default is what's sent.

A single compose flow creates both DMs and groups. Like iMessage, the *number of recipients* decides at send time; there is no separate "New Group" menu item.

**Entrypoint.** A bottom-right floating action button (pencil icon) on the Chats tab. Top-right works on iPad in split-view layouts. No multi-step "choose new DM vs new group" prompt — that's a Signal pattern this app intentionally drops.

### Recipient field

A Messages-style chip field, not Signal's stacked contact-list-with-checkmarks UI.

- Confirmed recipients render as **chips** showing display name (and small avatar). You can tap to highlight a chip; backspace while highlighted, or backspace at an empty caret after a chip, deletes.
- **Autocomplete** is backed by the local contacts table per `docs/52-contacts-and-profiles.md`. Results mirror that doc's search shape:
  - **People** section first (rows where `is_curated`) — matched against nickname, profile `display_name`, notes, and DID prefix.
  - **Other** section below (every non-removed, non-blocked row) — matched against `display_name` and DID prefix only. These are folks the user has encountered (group co-members, message-request senders) but hasn't curated yet.
- **Direct DID entry.** If what's typed looks like a DID (`did:[anything]`), Enter accepts it as-is. This is the path for adding someone whose DID you have but haven't met through the contact graph yet; the chip is shown grayed out because the client isn't yet sure if that DID can be reached on the chosen server.
- **Empty state.** A brand-new user has no contact rows at all. The autocomplete dropdown shows a single hint: "Type a DID, or wait — anyone you message will appear here." No system-contact import; per `docs/52-contacts-and-profiles.md` we don't pull from OS contacts.
- **Dedupe silently.** Adding the same recipient twice, or yourself, is a no-op.
- A **`+` button** on the right edge of the recipients box opens a contact browser modal: search bar at top, **People** and **Other** sections (same shape as the inline autocomplete), multi-select with a "Add N" confirm bar.
- **Cross-server suggestions** stay visible in autocomplete but render **greyed out** below the same-server matches when a server has already been pinned by an existing chip — they're discoverable but de-prioritised, since adding one would conflict with the current server choice.
- **Server pinning.** The first added chip pins the server — the From pill picks whichever (identity, server) pair reaches that recipient (max-overlap, falling back to most-recently-used on tie). Subsequent recipients incompatible with the pinned server are added as yellow chips, *not* an auto-flip; the user has to tap the From pill and choose a different server explicitly to switch. This makes the active-server choice always reflect an intentional act, never a silent flip caused by adding one chip.

**Pasted text** is parsed as a list when it contains separators (commas, semicolons, or newlines). Each token is run through the autocomplete matcher; matches become chips, anything that parses as a DID becomes a (grayed-out) chip, anything else gets dropped on the floor. A paste with no separators is treated as a single token typed into the field.

**Tapping a chip** does nothing in this version other than enabling delete (backspace, or the chip's `×`). Profile preview on tap is a later polish.

**Back-out** discards the compose state silently — no draft persistence in this version. Users who back out and then re-open the FAB land in a fresh compose.

### Identity & server pill

Centered above the composer, format `From: Alice (at safe-haven.org) ▾`. (Not `alice@safe-haven.org` — the parenthetical reads more naturally when "Alice" is the user's display name rather than a handle.)

- **Default on a fresh compose** (no recipients yet) is the last-used (identity, server) pair across all of the user's previous sends. The pill updates once the first chip is added, per the server-pinning rule above.
- **Single identity, single shared server:** non-interactive label.
- **Multiple identities or multiple shared servers reachable for the current recipient set:** tappable, opens a "Send as" sheet listing each `(identity, server)` pair with how many of the current recipients are reachable on it. Default selection: the pair reachable by the *most* recipients; ties broken by most-recently-used.
- The pill is **editable mid-compose.** Changing the identity or server re-validates the chip set: any recipient now unreachable on the new server flips to a yellow or red chip (see below).

### Cross-server / unreachable recipients

- **Yellow chip:** recipient is reachable on some server but not the one currently pinned. The pill suggests switching ("Switch to safe-haven.org to reach Bob"); tapping applies it.
- **Red chip:** recipient is reachable on no server in common with any of the user's identities. Send button stays disabled. Federated server-join invites — the planned fix — are deferred to the federation flow; the UX hook will land alongside it.
- An inline error band sits below the recipient field when any chip is yellow or red, summarising what would need to change.

### Group name

A small "Group name" field appears below the recipient chips once a second chip is added. It is **optional**; leaving it blank uses the auto-default. The placeholder text shows what the auto-default *would* be in real time, so a user who doesn't want to bother sees the eventual name and can leave it alone.

- **Auto-default:** comma-joined member display names, truncated ("Alice, Bob, Carol" or "Alice, Bob & 4 others"). Recomputed live as chips are added or removed.
- The field becomes the group's initial `title`. Admins can rename later from the group detail screen.

### Group icon

**Auto-generated** at creation from member initials in a Messages-style mosaic (no prompt at compose time). Admins can replace it later from the group detail screen. A custom icon at creation time is not worth the friction for the first version.

### Sending

- **Send button** stays disabled until the field resolves: ≥1 recipient with no yellow/red chips, plus either a non-empty message or an attachment.
- **1 recipient:** DM. If a thread already exists with that person on the chosen server, the new message tail-appends to *that* thread; we do *not* spawn a duplicate. The app navigates into the existing thread.
- **2+ recipients:** every send creates a **new group**, even if the same exact membership has existed before (matches Messages; does not match Signal's strict membership-dedup). Each compose is a fresh thread.
- **Partial failure on group create.** If `create_group` succeeds but one or more `invite_member`s fail (network, server reject), the group exists with whoever did succeed and the new thread surfaces a banner: "3 of 4 invited — Retry?". No rollback.
- **Send failure on first message.** Stay on compose with the message intact and an inline error; user can retry without losing state.
- **On success.** Push the new thread onto the nav stack, replacing compose. Back goes to the message list, not back to compose.

### Editing an existing group

Tap the group name/icon header at the top of a thread to open the group detail screen. From there we copy Signal:

- Group icon and name (admin-gated edit).
- Group expiry timer (admin-gated; action-bound groups only).
- Member list. Tap a member to remove, change role, or view their profile.
- Add members (opens the same contact browser as the compose `+` button, scoped to the hosting server).
- Invite-link toggle, which switches the group's `JoinPolicy` between `Closed` / `RequestToJoin` / `OpenLink` plus optional link password (per `docs/03-groups.md` §3.10).
- Notification preferences (mute, mentions) — local per-device, not group state.
- Leave group (emits `remove_members(self)`).
- Per-member block (DM-level) and report (per `docs/12-abuse-handling.md` once written).

This matches Signal's pattern; the bullet list above is the concrete mapping into our action surface.
