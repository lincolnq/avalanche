# Bot Identity & Presentation — UX Proposal

Status: draft for review. This is a UX proposal.

## Why

A user needs to know, at a glance, when the identity they're talking to is a
bot — an automated participant rather than a person. The activist threat model
makes this sharper than the usual product concern: an adversary running a bot
that poses as a friendly human organizer is a real attack (astroturf,
social-engineering, "I'm the campaign coordinator, send me the list"), and so
is the inverse — a human posing as the trusted *official* bot ("I'm the verified
admin bot, confirm your recovery phrase"). The presentation has to defend both
directions without overclaiming what it can prove.

Bots are full Signal-protocol participants (`21-chatbot-project.md`): same
account shape, same DID. The server *does* vouch for bots it installed (an
`official` flag, plus `is_bot` on registered bot accounts — `52-contacts-and-profiles.md`),
but for an *arbitrary* identity — a bot a user invited that registered as an
ordinary account — automation is only ever **self-declared**. So "bot status"
is partly **server-vouched** (provenance) and partly a claim we merely **relay**
(automation), and the UI must show *which*.

`36-message-editing-deletion.md` already branches on "is this author a bot"
(bots get a wider edit/delete envelope). This doc supplies the definition that
branch leans on: what makes an identity read as a bot, and how strongly.

## Two axes, not one

The single phrase "bot status" collapses two independent properties with very
different enforceability. Keeping them separate is the whole design:

- **Provenance — is this an *official* bot?** Vouched for by your homeserver. An
  `official` flag on the bot's account record (`20-project-security.md`, read via
  `get_account_info` / `account_info_cache` — the same path as `display_name` /
  `is_bot` in `52-contacts-and-profiles.md`) tells the client this is a Project the
  operator installed within this server's trust domain. The trust is the **trust
  chain** — you believe your own homeserver over its authenticated connection —
  not a cryptographic signature, so it is **same-server only**. It can't be forged
  by another *account*: the flag lives on the server's record for the real bot, so
  an impersonator simply doesn't carry it. This is the ✓ badge.
- **Automation — is this identity a *bot* rather than a *human*?** For an
  arbitrary identity this is **only ever self-declared** and **not enforceable**.
  A bot a user invited into a casual group (`00-design.md`: participants may
  invite bots, no special privileges) can simply *not* declare itself. Nothing
  in the protocol forces the disclosure.

The UI must never present a self-declared "bot" as if it were a proven one, and
must never let a missing declaration imply "definitely human." Provenance is a
claim our homeserver vouches for; automation is a claim we relay.

## Core model — a three-tier presentation

Every identity renders at exactly one of three trust tiers, decided client-side:

1. **Verified bot** — your homeserver reports the `official` flag on this bot's
   account record. Render with the **bot frame + ✓ badge** and an attributable
   line ("Official bot · run by {server}"). This is the only tier whose "official"
   claim is vouched for — by your own server, same-server only; the ✓ means
   nothing coming from a server you don't have an account on.
2. **Self-identified bot** — profile declares `account_kind = bot` (see below)
   but carries no valid attestation. Render with the **bot frame, no ✓**, and an
   explicitly hedged label ("Automated (not verified)"). Honest bots get
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
identities render in a squared frame humans don't get.

Why chrome rather than constraining the avatar image:

- **Unspoofable by the avatar.** The *client* chooses the mask and badge from the
  server's account record (the `official` flag, `is_bot`) and the self-declared
  `account_kind`, not from the uploaded bytes. A bot cannot ship an image that
  "undoes" the bot frame, and a user account cannot ship one that forges the ✓ —
  the flag isn't in anything the account uploads.
- **It doesn't fight branding.** Real Project bots (the Q&A bot, an engagement
  bot, a campaign's own bot) want *their own* avatar — a forced generic image
  would erase exactly the identity they need. Chrome leaves the image free.
- **It's glanceable before you read anything.** Shape registers pre-attention,
  faster than parsing a name or label.

Concretely: **people render in a circle; bots render in a hexagon.** The ✓ badge
distinguishes tier 1 from tier 2; tier 2 may instead carry a small hollow/grey
"unverified" mark so the two are never confused.

**The same geometric language extends to message bubbles.** A bot's bubbles
render with cut, octagon-ish corners; people's stay rounded. It's the same
client-applied-chrome principle — the bubble shape is decided by the client from
the sender's bot signal, not anything in the message — so a reader scanning a
group thread can see which messages came from an automated participant without
checking each avatar. A *literal* regular octagon can't hold a text bubble
(it would crop the text or waste space on long messages), so the realization is
a rectangle with chamfered corners: an octagon when square, a beveled rectangle
at bubble proportions. (Implemented; the avatar-frame badge tiers above are not
yet — today every bot renders the hexagon frame and octagon bubbles, with no
badge, until the official-flag badge tier lands.)

## On the constrained-avatar-palette idea

A tempting alternative: make bots pick their avatar from a limited platform
palette, so bot profiles are visually distinct and "a user could pretend to be a
bot, but a bot couldn't easily pretend to be a user." It's cute and gives the
ecosystem a shared look. But as a *security* mechanism it mostly doesn't hold,
and it's worth being precise about why:

- **The asymmetry only exists if something the bot can't lie about constrains the
  image.** The only such thing is the server's `official` flag (which an account
  can't set for itself). So a *security-grade* palette is parasitic on — and
  redundant with — the badge we already have. For
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
  ✓ — an impersonator account doesn't carry the server's `official` flag.

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
- **The `official` flag (and `is_bot`) on the bot's account record**
  (`20-project-security.md`, `22-adminbot.md`), served by `get_account_info` and
  cached in `account_info_cache` (`52-contacts-and-profiles.md`). Set by the
  operator at install, this drives tier 1 — server-vouched, so it doesn't rely on
  the self-declared `account_kind` for official bots. A well-run server's bots
  thus all carry the vouched signal.

No new transport: both ride the profile blob and the public account-record paths
that already exist.

## Surfaces

The tier and its chrome travel with the avatar, so they appear wherever an
avatar or name does:

- **Conversation list** — the DM row's avatar takes the bot frame (hexagon);
  for a 1:1 with a verified bot, the header may add "Official bot · {server}".
- **Message bubbles** — a bot sender's bubbles get cut (octagon-ish) corners
  instead of rounded ones, so bot messages are distinguishable inline in a
  mixed group thread, not just by the avatar.
- **Contact card / profile** — the full hedged or attributed line ("Official
  bot, run by {server}" / "Automated (not verified)"), plus the bot's
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
- **Auto-accept of bot invites (`20`/`22`)** — the `invites:auto-accept` scope (a
  same-server, client-honored grant), typically held by official bots; the ✓ tier
  here surfaces the same server-vouched officialness visually. A self-identified
  (tier 2) bot without that scope gets the normal accept/decline UX, not
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
- **How loud tier 2 should be.** "Automated (not verified)" is honest but
  wordy; a quieter treatment may suffice in-list with the full line on the
  contact card. Tune once it's in front of users.
- **Server-policy hook (later).** A server could *require* its registered bot
  accounts to carry `account_kind = bot`, closing the gap for bots it runs — but
  it can't touch user-invited bots, and the substrate has no general "is a bot"
  flag. Whether to add that policy lever is a `22`-side question, not a UI one.
