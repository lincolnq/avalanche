# 54 — Bot Identity & Presentation

> **Status:** Partial — bot chrome is built on iOS, Android and Desktop: hexagon avatars and cut-corner bubbles for accounts the server reports as `is_bot`. The verified (checkmark) tier, the self-declared `account_kind` profile field, and the hedged "not verified" tier are not built. `is_bot` is self-declared at registration, yet it currently bypasses the message-request gate.
> **Last verified against code:** 2026-10-03

## Summary

Users need to know at a glance when they're talking to a bot. The activist threat model makes this sharp in both directions: a bot posing as a friendly human organizer (astroturf, "send me the list"), and a human posing as the trusted official bot ("confirm your recovery phrase"). The presentation has to defend both without claiming more than it can prove.

The core idea: "bot status" is two independent properties, **provenance** (is this an *official* bot my server vouches for?) and **automation** (is this identity a bot at all?). Only provenance can be verified. Automation is always self-declared. Both are shown with **client-applied chrome** that avatar bytes can't override.

## Current design

### What the client knows

- **`is_bot`** on the server's account record, returned by `GET /v1/accounts/{did}` (`core/crates/server/src/routes/accounts.rs`) and cached in `account_info_cache`. It is set from the registration request (`routes/registration.rs`), so it is **self-declared by whoever registers the account**: any registrant may set it, and nothing requires a bot to.
- **`project_bot`** on the same record (built, S-03): true when the account is linked to an installed Project on that server (`project_bots`, adminbot included). It is **server-vouched**: since S-01 a bot gets that link only by registering with its Project's bot signup key. Cached alongside `is_bot` in `account_info_cache`.
- **No official flag on accounts.** The only `official` column is on `directory_entries`, and nothing sets it (`20` §Officialness).

### Chrome

**Built on all three platforms** (iOS `mobile/ios/Actnet/Sources/Views/Common/ContactAvatar.swift`, `Views/Chats/MessageBubble.swift`; Android `Views/Common/Hexagon.kt`, `CutCornerRectangle.kt`; Desktop `desktop/src/components/ContactAvatar.tsx`):

- **People render in a circle; bots in a hexagon.**
- **Bot message bubbles have cut, chamfered corners**; people's stay rounded. A literal octagon can't hold text, so it's a rectangle with chamfered corners. A reader scanning a group thread can spot automated messages without checking avatars.
- The frame and bubble shape are chosen by the client from `is_bot`, never from anything in the avatar or message, so an image can't undo or forge them. The local user is never rendered as a bot.

Every bot currently gets the same chrome with no badge, because the tiers below aren't built.

### Interactions

- **Message requests** — `SenderGate::passes` lets a `project_bot` sender through without a request (`core/crates/app-core/src/messaging.rs`); a merely self-declared bot is a request like anyone else (S-03). The conversation-list path uses the same rule (`list_project_bot_dids`).
- **Editing and deletion (`36`)** — `36` specifies a wider envelope for bots (no edit cap, 30-day window, no retained history). It isn't wired: app-core has no bot-specific edit path.

## Known gaps

1. **No verified tier.** No checkmark exists anywhere, so a human impersonating "the official adminbot" looks the same as the real one, apart from name and avatar.
2. **`is_bot` from the server is presented as if vouched.** The chrome uses the self-declared `is_bot`, so a self-declared bot looks the same as a Project bot. `project_bot` now exists to tell them apart; the tiers below aren't drawn yet.

## Planned

- **Verified tier from installation.** Operator-marked official Projects' linked bots get the checkmark (`22` §Planned). Same-server only: the checkmark means nothing from a server you have no account on.
- **Hedged tier for self-declared bots:** bot chrome, no checkmark, and "Automated (not verified)" on the contact card.

### The three tiers (target)

1. **Verified bot** — your homeserver vouches for it (linked to an installed, official Project). Bot frame plus the checkmark badge, and "Official bot · run by {server}".
2. **Self-identified bot** — declares itself a bot but carries no server vouching. Bot frame, no checkmark (optionally a hollow "unverified" mark), "Automated (not verified)".
3. **Person (default)** — no bot signal. The *absence* of a bot signal is **not** a claim of being human; there's just nothing to show.

A verified bot keeps its own Project avatar; the tier governs the chrome, not the picture. The chrome carries into the conversation list, message bubbles, the contact card, compose chips and autocomplete, and group member lists.

## Speculative

- **`account_kind` in the encrypted profile blob** (`person` default, `bot`) as the self-declaration channel for identities without a server account record. Unknown values degrade to `person`. Overlaps with `is_bot`; decide whether both are needed once bots on other servers (`13`) are common.
- **A server policy requiring registered bot accounts to declare themselves.** It can't reach bots invited from elsewhere.
- **Default avatars for art-less bots**: a deterministic, obviously synthetic glyph picked from the bot's DID, shipped as a small helper in the bot SDK. Presentation only; never overrides a bot's own avatar.
- **Badge art:** checkmark vs a robot glyph, and how loud the tier-2 label should be in lists versus on the contact card.

## Rationale and rejected alternatives

- **Two axes, not one.** Provenance can be vouched for by your own server over an authenticated connection; automation never can, because a bot a user invites can simply not declare itself. Collapsing them would either overclaim (treating a declaration as proof) or underclaim (ignoring the server's vouching).
- **Chrome, not image constraints.** The client picks frame and badge from server records, so the avatar can't override them; chrome doesn't fight legitimate branding; and shape registers before you read anything (cf. Slack/Discord app frames).
- **Rejected: a mandatory constrained avatar palette for bots as a security mechanism.** The asymmetry ("a bot can't look like a person") only holds if something the bot can't lie about constrains the image, and the only such thing is the server's official flag, which makes the palette redundant with the badge. It only constrains honest bots, so it doesn't stop the dishonest case. It also strips legitimate bots of their branding. The dangerous direction, a human posing as the official bot, is covered by the checkmark.
