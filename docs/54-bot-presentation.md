# Bot Identity & Presentation — UX Proposal

Status: draft for review. This is a UX proposal.

## Why

A user needs to know, at a glance, when the account they're talking to is a
bot — an automated participant rather than a person. The activist threat model
makes this sharper than the usual product concern: an adversary running a bot
that poses as a friendly human organizer is a real attack (astroturf,
social-engineering, "I'm the campaign coordinator, send me the list"), and so
is the inverse — a human posing as the trusted *official* bot ("I'm the verified
admin bot, confirm your recovery phrase"). The presentation has to defend both
directions without overclaiming what it can prove.

Bots are full Signal-protocol participants (`21-chatbot-project.md`): same
account shape, same DID, no special server-side flag. So "bot status" is not a
fact the substrate hands us — it's something we either **attest** to or
**self-declare**, and the UI must show *which*.

`36-message-editing-deletion.md` already branches on "is this author a bot"
(bots get a wider edit/delete envelope). This doc supplies the definition that
branch leans on: what makes an account read as a bot, and how strongly.

## Two axes, not one

The single phrase "bot status" collapses two independent properties with very
different enforceability. Keeping them separate is the whole design:

- **Provenance — is this an *official* bot?** Cryptographically verifiable. The
  `OfficialBotAttestation` chain in `22-adminbot.md` (server trust root → adminbot
  delegation cert → attestation, embedded in the bot's profile blob) lets the
  client prove an account is an official bot run within this server's trust
  domain. Cannot be forged. This is the ✓ badge.
- **Automation — is this account a *bot* rather than a *human*?** For an
  arbitrary account this is **only ever self-declared** and **not enforceable**.
  A bot a user invited into a casual group (`00-design.md`: participants may
  invite bots, no special privileges) can simply *not* declare itself. Nothing
  in the protocol forces the disclosure.

The UI must never present a self-declared "bot" as if it were a proven one, and
must never let a missing declaration imply "definitely human." Provenance is a
claim we verify; automation is a claim we relay.

## Core model — a three-tier presentation

Every account renders at exactly one of three trust tiers, decided client-side:

1. **Verified bot** — carries a valid `OfficialBotAttestation` whose chain
   verifies against the hosting server's pinned trust root and isn't expired.
   Render with the **bot frame + ✓ badge** and an attributable line ("Official
   bot · run by {server}"). This is the only tier whose "bot" claim is provable.
2. **Self-identified bot** — profile declares `account_kind = bot` (see below)
   but carries no valid attestation. Render with the **bot frame, no ✓**, and an
   explicitly hedged label ("Automated account · not verified"). Honest bots get
   honest framing; the user is told the claim is unverified.
3. **Person (default)** — no bot declaration, no attestation. Rendered as a
   human contact per `52-contacts-and-profiles.md`. The *absence* of a bot
   signal is **not** a positive "this is a human" claim — we just have nothing to
   show, exactly as today.

A verified bot that *also* runs under a Project keeps its Project branding; the
tier governs the chrome, not the picture.

## The distinction lives in client-applied chrome, not in the image

The load-bearing signal is **chrome the avatar bytes cannot override** — a
distinct **frame shape** plus a corner **badge glyph**, applied by the client at
render time based on the tier it computed. This mirrors Slack/Discord, where app
accounts render in a squared frame humans don't get.

Why chrome rather than constraining the avatar image:

- **Unspoofable by the avatar.** The *client* chooses the mask and badge from the
  attestation/declaration, not from the uploaded bytes. A bot cannot ship an
  image that "undoes" the bot frame, and a human cannot ship one that forges the
  ✓.
- **It doesn't fight branding.** Real Project bots (the Q&A bot, an engagement
  bot, a campaign's own bot) want *their own* avatar — a forced generic image
  would erase exactly the identity they need. Chrome leaves the image free.
- **It's glanceable before you read anything.** Shape registers pre-attention,
  faster than parsing a name or label.

Concretely: **people render in a circle; bots render in a hexagon.** The ✓ badge
distinguishes tier 1 from tier 2; tier 2 may instead carry a small hollow/grey
"unverified" mark so the two are never confused.

## On the constrained-avatar-palette idea

A tempting alternative: make bots pick their avatar from a limited platform
palette, so bot profiles are visually distinct and "a user could pretend to be a
bot, but a bot couldn't easily pretend to be a user." It's cute and gives the
ecosystem a shared look. But as a *security* mechanism it mostly doesn't hold,
and it's worth being precise about why:

- **The asymmetry only exists if something the bot can't lie about constrains the
  image.** The only such thing is the attestation. So a *security-grade* palette
  is parasitic on — and redundant with — the badge we already have. For
  self-run, unattested bots there is no enforcement point: a malicious bot writes
  its own client and sets any avatar it likes. "A bot couldn't *easily* pretend
  to be a user" reduces to "the bot SDK doesn't offer that affordance" — real
  friction against lazy impersonation, but not a guarantee, and it shouldn't be
  sold as one.
- **It harms legitimate bots.** Forcing the Q&A bot or a campaign's bot onto a
  generic palette strips the branding that makes them recognizable and
  trustworthy.
- **The direction it constrains is the less dangerous one.** It stops a bot from
  *looking* like a human only if the bot is honest enough to use the palette in
  the first place — i.e. it doesn't stop the dishonest case at all. The genuinely
  dangerous case (a human posing as the *official* bot) is already covered by the
  unforgeable ✓.

**Recommendation:** don't make the palette mandatory and don't lean on it for
security. Keep the chrome (frame + badge) as the load-bearing distinction, and
give art-less bots a default avatar from a shared platform set — the long tail
(testbot, adminbot, simple utility bots) that would otherwise show a blank or
initials placeholder. That keeps the "branded, semi-consistent bots across the
ecosystem" charm, costs nothing, and makes no claim it can't keep.

The default set itself is **not specified here** — it's a starting convention,
not protocol. Something obviously-synthetic and disjoint from the human
initials-in-a-circle default (a flat geometric glyph on a solid color, picked
deterministically from the bot's DID so it's stable and curation-free), most
likely shipped as a small shared TypeScript helper the bot SDK can call. It's
presentation only: it never overrides a bot's own uploaded avatar, and the
frame + badge remain the actual signal regardless of which image is shown.

## What carries the signal on the wire

Two additions, both reusing existing machinery:

- **`account_kind` in the substrate profile blob** (`52-contacts-and-profiles.md`).
  An enum (`person` default, `bot`) alongside `display_name`/`avatar`/`bio`.
  Self-declared, distributed and cached exactly like every other profile field;
  it drives tier 2 vs tier 3. Unknown values degrade to `person` (older clients
  ignore it — same forward-compat rule as the rest of the blob).
- **An `is_bot`/`kind` field on `OfficialBotAttestation`** (`22-adminbot.md`).
  Adminbot already attests `subject_did`, `display_name`, `purpose`; marking the
  subject a bot there makes tier 1 fully attestation-driven rather than trusting
  the self-declared `account_kind` for official bots. A well-run server's bots
  thus all carry the proven signal.

No new transport: both ride the profile/attestation paths that already exist.

## Surfaces

The tier and its chrome travel with the avatar, so they appear wherever an
avatar or name does:

- **Conversation list & message bubbles** — bot frame + badge on the avatar;
  for a 1:1 with a verified bot, the header may add "Official bot · {server}".
- **Contact card / profile** — the full hedged or attributed line ("Official
  bot, run by {server}" / "Automated account · not verified"), plus the bot's
  `purpose` string when attested.
- **Compose / recipient chips & autocomplete** — the frame carries into chips so
  a user adding a recipient sees they're addressing a bot before sending.
- **Group member list** — bots grouped or marked so a member can see at a glance
  which participants are automated (relevant given `00-design.md`: a bot in a
  group decrypts like any member).

## Interactions with other features

- **Editing/deletion (`36`)** — the bot envelope (no edit cap, 30-day window, no
  retained history) keys off the self-declared `account_kind`. This doc is where
  that "is this a bot" question is answered.
- **Auto-accept of official-bot invites (`22`)** — already gated on the
  attestation chain; the ✓ tier here is the same verification surfaced visually.
  A self-identified (tier 2) bot's invite gets the normal accept/decline UX, not
  auto-accept.
- **Contacts (`52`)** — `account_kind` is just another cached profile field;
  nothing about curation, blocking, or the People list changes. A bot can be a
  contact like anyone.
- **Multi-account (`53`)** — the user's *own* bots (a Project they run) surface
  with the same chrome from the contact's perspective; nothing identity-specific.

## Behavior to pin down (not blockers)

- **Badge art** — the ✓ vs a "robot" glyph for the verified badge, and the
  tier-2 "unverified" mark. Pure visual-design choices. (Frame shape is settled:
  hexagon for bots, circle for people.)
- **How loud tier 2 should be.** "Automated account · not verified" is honest but
  wordy; a quieter treatment may suffice in-list with the full line on the
  contact card. Tune once it's in front of users.
- **Server-policy hook (later).** A server could *require* its registered bot
  accounts to carry `account_kind = bot`, closing the gap for bots it runs — but
  it can't touch user-invited bots, and the substrate has no general "is a bot"
  flag. Whether to add that policy lever is a `22`-side question, not a UI one.
