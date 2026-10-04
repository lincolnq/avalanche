# 30 — Mobile app UX

> **Status:** Partial — onboarding (invite, recover, link device), the four-tab shell, compose (DM / New Group / Note to Self), conversation view, and group detail are built on iOS and ported to Android. Not built: joining a second server with an existing identity (the flow is a local stub), calls, per-conversation mute, adding or removing group members from group detail, invite-link controls, server pinning in compose.
> **Last verified against code:** 2026-10-03

## Summary

The app should feel like Signal: one unified inbox of every conversation across all servers and identities, sorted by recency. Servers and Projects are browsable in their own tab, but you never "enter a server" to read messages. iOS is the reference implementation (`mobile/ios/Actnet/Sources`); Android mirrors it (`60`, `62`).

## First launch

*Status: Built.* With no identity, the splash screen (`SplashView.swift`) offers:

- **Scan invite QR code**
- **Enter invite link**
- **Recover account** (passkey or recovery phrase, `50`)
- **Link to an existing device** (`04`)

There's no "create account" path without a server invitation: a new identity always joins a server.

## Invite links

*Status: Built (see `51` for the token format).* Invite links and QR codes resolve to `https://go.theavalanche.net/i/<token>`, which opens the app via Universal Links / App Links, or a web landing page with store links when the app isn't installed. The token carries at least the server URL; the app validates it against `GET <server>/v1/invites/<token>`.

## Registration flow

### New user (no existing identity)

*Status: Built.*

1. Validate the invite token with the server.
2. **Display name** (required) and optional photo.
3. **Recovery setup** — passkey, or a written recovery phrase (`50`).
4. Keys are generated and the account registered in the background.
5. **Push permission prompt**, with context.
6. Land in **Chats**.

If the token names an onboarding step, a Project's onboarding web view runs between registration and the push prompt (`51`). The substrate display name is already set; the Project collects whatever else it needs.

The server cannot "auto-enroll" a new user into encrypted groups. Group membership needs the group master key, which only clients hold. Automatic group joins after an invite have to come from a client or a bot that holds the keys (see `51`, `24`).

### Existing user (already has identities)

*Status: Partial.* `IdentityPickerView` offers "Join [server] as…" each existing identity, "Create a new identity", or "Recover an identity".

- **Create a new identity** runs the full new-user flow.
- **Join as an existing identity is a stub.** `AppState.joinServer` appends the server to the identity's local server list and returns. It never registers the identity on the new server, so nothing is actually joined. See `53` and `06`.

Creating a separate identity is the right choice when you want personas kept apart, e.g. organizing pseudonymously in one group while using your real name in another.

## Display name and avatar

*Status: Built.* The display name is required at account creation and is part of the identity's encrypted profile (`52`). The avatar is optional (`55`). One identity, one name everywhere. To use different names in different contexts, create separate identities. There are no per-server name overrides.

## Multi-account

*Status: Built (account tabs).* The app supports several identities, each with its own name, keys, and servers. All identities' conversations appear together in one inbox; you don't switch identities to see content.

- **Each conversation belongs to exactly one of your identities by construction.** A DM from your pseudonymous persona and one from your real identity are different conversations. You therefore send as whichever identity the conversation belongs to; there's no per-message identity choice. Inbox rows carry **no per-identity marker** (`37`).
- With more than one identity, the Chats screen shows a row of **account tabs** (one avatar per identity, each with an unread badge) that filter the inbox (`37`).
- **New conversations** pick the acting identity in compose (below).

The earlier ideas of a per-row identity indicator and an in-conversation identity switcher are **superseded** by `37`. The conversation is the context.

*Speculative:* if a user joins the same group with two identities, warn them at join time ("You're already in this group as Alice").

## Navigation

*Status: Built.* Four tabs (`MainTabView.swift`; Liquid Glass tab bar on iOS 26):

- **Chats** — the unified inbox (default), with account tabs once you have several identities and a compose button in the header.
- **Network** — your servers, each listing the Projects it publishes (`22` directory). Tapping a Project opens it full-screen in a web view (`20`).
- **Settings** — identities, servers, linked devices, blocked contacts.
- **Search** — searches conversations (the iOS 26 floating search tab).

The tab bar is hidden inside a conversation, so the composer sits on the bottom edge.

**Calls** are Speculative (`01`); there's no calls tab. Project-to-chat deep links work in one direction: Projects open conversations via `https://go.theavalanche.net/conversation/<did>`. A conversation showing which Project it belongs to is Planned.

## Compose

*Status: Built.* `ComposeMessageView.swift`, with `NameGroupView.swift` for groups.

A single screen with a **To** field of recipient pills above an always-visible, type-to-filter contact list, and three persistent actions:

- **DM** — enabled with exactly one recipient. Opens the existing thread with that person if there is one, otherwise a new one.
- **New Group** — always available. Goes to the **Name Group** screen (title; the auto-default is the comma-joined member names), then `create_group` + invites + navigate.
- **Note to Self** — a DM with your own identity (`04` §5.5).

**From (acting identity):**
- Starts **empty**, showing the merged contact book across all identities.
- The first contact you pick fixes it to that contact's most recent identity. Alternatively you pick From explicitly, and the contact list filters to what that identity can reach.
- Single-identity users never see the From row.
- Because conversations are server-local until federation, this filtering also prevents building a cross-server group.

**Recipients:**
- The contact list is sectioned **People** (curated) and **Other** (everyone else seen), per `52`.
- Matching is on names; DIDs match only when the query starts with `did:`.
- Typing a full DID or scanning a contact QR adds that DID directly.
- Duplicates and yourself are ignored.

**Planned:**
- Server pinning and cross-server warnings (yellow/red pills), once identities have more than one server or federation exists.
- Pasting a list of recipients.
- Profile preview on tapping a pill.
- A "3 of 4 invited — Retry?" banner when some group invites fail.
- Draft persistence.

**Decided:**
- **Two or more recipients always create a new group**, even if an identical one exists (matches Messages, not Signal's membership dedup).
- **The group icon** is auto-generated (initials mosaic); admins can set a photo later (`55`).

## Conversation view

*Status: Built.* Text, attachments and link previews (`35`), reactions (`33`), edit and delete (`36`), disappearing-message timers, shared contact cards, group system messages for every membership or metadata change, and the message-request gate (Accept / Delete / Report) for un-accepted DMs (`12`, `52`).

## Group detail

*Status: Partial.* Tap the group header to open it. **Built on iOS:**
- group photo and rename;
- disappearing-message timer;
- member list (message a member, copy their contact card, make or remove admin);
- pending invites (shown as opaque ids);
- leave group.

**Planned:**
- **remove member** and **add members** (from the same contact browser as compose, scoped to the hosting server);
- **invite-link controls** (`JoinPolicy` Closed / RequestToJoin / OpenLink and the link password, `03` §3.10);
- **per-conversation mute and notification preferences** — not built anywhere. Big organizing groups need it more than threading;
- per-member block and report.

## Known gaps

1. Joining a second server with an existing identity doesn't register (above). P1.
2. No per-conversation mute. P1 for organizing-scale groups.
3. No add or remove members in group detail. P2.

## Rationale and rejected alternatives

- **Recipient count decides DM vs. group at send time with no explicit buttons** (the earlier design) — replaced by explicit **DM** and **New Group** actions with a naming step. Clearer, and it gives groups a name before they exist.
- **A "New Group" menu separate from compose** (Signal) — rejected. Compose is one screen; New Group is an action on it.
- **Per-row identity badges and an in-conversation identity switcher** — rejected in `37`.
- **Importing OS contacts** — rejected: contacts come from interaction (`52`).
