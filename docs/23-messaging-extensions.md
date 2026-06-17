# Messaging Extensions — Core vs. Project

Status: draft for review.

As we add richness to messaging — polls, slash commands, live location, link previews, surveys, GIFs — the recurring question is *which of these belong in the lean, audited core, and which should be delivered as Projects?* (Projects are the platform's extension mechanism: scoped server-side apps and bot accounts, reached via the Network tab and the Project API — see `20-project-security.md`, `01-technical-implementation.md` Stage 6.)

This doc gives the boundary as a few rules and a deliberately small set of surfaces. The thesis: keep the in-conversation surface boring (native, auditable, E2E), push all real interactivity into an explicitly-opened webview, and most features fall out cleanly — the few that can't are obvious.

## Examples at a glance


| Example                                   | Lives in            | Surface                        | Mechanism / why                                                                             |
| ----------------------------------------- | ------------------- | ------------------------------ | ------------------------------------------------------------------------------------------- |
| Emoji reactions                           | **Core**            | long-press → on-bubble cluster | Substrate grammar; must work in a bot-free 1:1 DM (`33-reactions.md`)                       |
| Replies / threading                       | **Core**            | long-press / compose           | Substrate grammar (`32-threading.md`)                                                       |
| Read receipts, typing                     | **Core**            | automatic                      | Substrate grammar (`31-read-tracking.md`)                                                   |
| `@user` mention autocomplete              | **Core, on-device** | composer `@`                   | Member list from core conversation state; mention is a body range; notifies even when muted |
| Generic link preview                      | **Core, on-device** | auto when a URL is in the body | Sender-fetches / recipient-never-fetches privacy invariant (`35-attachments.md`)            |
| Share live location                       | **Core**            | "+" menu → map                 | Streaming, auto-expiring E2E primitive; works in a DM; reused by Action Day                 |
| Simple polls                              | **Core built-in**   | "+" menu → poll                | Reaction-shaped PEER votes; E2E, DM-capable, offline; no bot                                |
| Bot-formatted announcement                | **Split**           | bot-posted message             | Rich-text body ranges, authored by the bot                                                  |
| Custom emoji / reaction pack              | **Split**           | reaction picker                | Project contributes assets into the core react mechanism                                    |
| Bot slash commands                        | **Split**           | composer `/` (autocomplete)    | Bot advertises commands; the command is a normal message it interprets                      |
| Giphy / sticker / GIF search              | **Project**         | "+" menu → webview             | Entry point opens a webview that returns a GIF you send                                     |
| Cardstack / swipe survey                  | **Project**         | magic link → webview           | Bot posts a magic link → webview; the bot posts results back                                |
| "Create task from this", "Translate this" | **Project**         | message long-press → webview   | Entry point opens a webview with the message as context                                     |
| Action Day map + markers                  | **Project**         | Network tab → webview          | Destination Project; builds on the core location primitive                                  |


## The three rules that draw the line

1. **Explicit handoff only.** A Project may extend messaging only through a user-initiated handoff — invoking an entry point, opening a Project's webview, tapping a launcher. **Ambient, compose-time, always-listening capabilities stay in core and on-device.** We won't do Projects that can read your composer as you type, and there is no Project-rendered UI sitting *inside* the conversation watching it.
2. **The 1:1-DM litmus test.** *Does it have to work in a one-on-one DM with no bot present?* If yes → core. Reactions, replies, read receipts, generic unfurl, live location, and simple polls all pass → core. A cardstack survey doesn't (it needs a bot to collect results and a webview to run) → Project.
3. **Mechanism vs. content.** Where a feature is Project-extensible, **core owns the mechanism, the surface, and the privacy-sensitive parts; the Project contributes content or a webview app at a well-defined seam — and never sits in the middle of a private interaction.** Slash commands (Project-registered vocabulary), custom emoji (asset pack), and the cardstack (a webview app) are all this shape.

## The model in one picture

There are exactly three places a feature can live, and the design pushes hard toward keeping the first one boring:

- **In the conversation (core).** Native, auditable, E2E. *Substrate grammar and built-ins* (reactions, replies, receipts, live location, simple polls), *generic link previews*, and *bot-posted messages* (plain or rich-text, possibly carrying a magic link). **Nothing in the feed collects input or renders untrusted UI.**
- **A full-screen Project webview.** All real interactivity (GIF search, forms, multi-step flows, the cardstack, maps) lives here, behind an explicit "open this Project" handoff, sandboxed from E2E data.
- **The Project's bot, as a conversation member.** Posts messages into the conversation, observes reactions/replies as lightweight signals, and posts results back from webview sessions.

The things we explicitly **do not** build: inline interactive cards, in-feed form controls, and display cards. Once a webview exists for real interaction, every in-feed interactive widget is a redundant middle layer that adds native cross-platform rendering, a submission-routing substrate, and an in-feed phishing surface — to do badly what the webview does well. Lightweight in-feed actions (approve/deny, "I'm in") are served by **reactions or replies a member bot observes**; a tappable launcher is just a **magic link in a (rich-text) bot message** — self-authenticating and allowlist-gated (see `20-project-security.md`, *Project permissions*); anything with real input goes to the webview.

## The surfaces core exposes

It really comes down to one primitive, plus rich text. (And one anti-surface, listed to mark the boundary.)

### 1. The one primitive: open a Project webview

Everything interactive is "open a Project's webview, with context and a scoped token." There are two ways a webview opens: a **registered entry point** and a **magic link**.

A Project registers **entry points** — `{ id, label, icon, scope }` — that core surfaces in one of two places:

- the composer **"+" menu** (`surface:compose`),
- the **message long-press menu** (`message:context-on-action`) — the entry point receives that message as context, an explicit, per-message disclosure to the Project, consent-gated by the tap.

A **magic link** is the third path, and it is *not* a registered entry point: it's a Project-issued, self-authenticating link that opens the Project's webview wherever it's tapped — including from inside a message (e.g. a bot posts "Priorities deck — tap to rank 12 items"). The link carries no credential; the clicking device injects a Project-scoped identity token at tap time, and only for Projects on the clicker's own vetted allowlist (the no-open-redirect rule). Because the client recognizes it by URL against that allowlist rather than by a registered id, *anyone* can share a magic link — it isn't restricted to the owning bot or to conversations the bot is a member of. It is typically dressed with a rich-text label (a link span) for presentation. See `20-project-security.md`, *Project permissions* (`identity:magic-links`).

Slash commands are **not** one of these — they don't invoke anything client-side. They're a separate, lighter thing (next).

When invoked, the webview opens and does the work. There are exactly **two return modes**:

- **Return content you send.** The webview hands back a *content reference* via an outbound **deeplink** (`theavalanche://compose/attach?url=…&type=…`); the client fetches the bytes, drops them into the composer as a preview, and *you* explicitly send it as a normal message/attachment. Giphy is this: pick a GIF → it's fetched and inserted into your message. The Project needs no bot in the conversation — it's a pure compose helper (service mode), the Telegram inline-bot pattern. (The deeplink carries a *URL*, not the bytes — see *No JS bridge* below.)
- **The bot posts the result.** The webview is a destination; the Project's bot (a conversation member) posts the outcome back — a survey tally, an RSVP summary. This happens **server-side** (the webview tells its own backend; the bot posts), so it needs nothing from the client. This is the cardstack.

### 2. Full-screen Project webview (the workhorse)

A Project ships a web UI; the client renders it in an isolated webview, reached only by the explicit invocations above. The constraints are load-bearing:

- **No JS bridge — I/O is URL params in, deeplinks out.** The webview is a plain sandboxed page with *no* privileged JS API to the native app. Everything inbound (which Project, which group, entry params, a **scoped identity token** — zkgroup-style, proving "this user, in this group, with these granted scopes," see `20-project-security.md`) arrives as **query params on the launch URL**, exactly as the project-token already does. Everything outbound is an **intercepted deeplink** the page navigates to — `…/attach?url=…` to return content, `…/close` to dismiss. Use a custom scheme caught by the webview's navigation delegate (Universal Links fire unreliably from within your own app's webview). This is a strictly smaller native attack surface than a bridge, and it keeps `20-project-security.md`'s "no JS bridge" stance intact.
- **Zero access to E2E data.** Inbound params and outbound deeplinks are the *only* channels; the webview never sees message history, the local store, or keys. Opening a Project means **leaving the E2E conversation for that Project's server-backed trust domain** — fine because it's explicit and full-screen, but the chrome must signpost it.
- **Return-content is a reference, fetched and confirmed.** The `attach` deeplink carries a *URL* (+ type/metadata), not bytes — URLs can't hold a multi-MB blob, and the Project hosts the artifact (Giphy's CDN, or its own backend for generated content). The client fetches it **sender-side** (https-only, size-capped, origin-allowlisted — an SSRF/privacy guard, same fetch posture as on-device unfurl), previews it in the composer, and **never auto-sends** — the user explicitly sends. Conversation posts from the cardstack go through the Project's **bot, server-side**, not the client.
- **Per-Project isolation.** Each Project gets its own webview data store/context; navigation locked to its origin/allowlist; no `file://`, CSP on. One Project can't read another's storage or the user's other web sessions.
- **Honest costs.** Remote webview content is not part of the reproducible build and can change server-side — so the security guarantee here is **isolation + explicit consent + no access to secrets**, *not* "audited code." Webviews need connectivity (no offline), opening one is a metadata hit to the Project's origin, and full-screen arbitrary content is a phishing surface (persistent Project attribution + first-open scope consent required).

### 3. Rich text (body ranges)

Formatted text is supported at the substrate level for all messages, but **authored by bots first** — the human composer can stay plain. Following Signal: a message carries a plain `body` string plus a list of **formatting ranges** as an overlay.

Plaintext is always present, so notifications, search, and old clients degrade gracefully; styles are an overlay with no parsing ambiguity or injection surface. Surfacing formatting controls to human users later is a non-breaking change.

**Mentions are another range kind** (a member reference, not a style or link), produced by `@`-autocomplete — so `@user` rides the same body-ranges mechanism. Unlike bot-facing formatting, mentions *are* surfaced to humans from day one: the `@` typeahead is sourced from core conversation membership (not a Project), and core treats a mention as a notification signal — a mentioned member is pinged even in a muted or large channel (cf. the badge discipline in `31`/`33`).

### 4. Bot command autocomplete (slash commands)

Slash commands are **not** an entry point and invoke nothing client-side. A slash command is **a normal E2E message you send**; a bot that's a conversation member may interpret it. The autocomplete is purely a **reminder** of what that bot understands — message-authoring assistance, not a Project surface.

- A bot advertises a **command manifest** in its profile / Project metadata — `[{ command: "/ban", args: "@user [reason]", description: "Remove a member" }, …]`. The client syncs it and, when you type `/` in a conversation where that bot is present, shows the typeahead.
- "Invoking" a command is just **sending the text**. So there's **no new wire format** (commands are `TextMessage`s) and **no frontend requirement** — an old or minimal client just types `/ban …` by hand and it still works. This is what makes it a good control surface for a bot's advanced features: predictable, discoverable, and zero UI to build.
- It's the **same channel** as talking to the bot in plain English — both are messages to a member bot. Slash is the deterministic, fixed-grammar path; natural language is the fuzzy one. The autocomplete just documents the deterministic options.
- **Graceful and transparent.** "Someone might interpret it" — if no bot understands the command, it's just text. Issue it in the group (visible to all — accountability for, say, an admin action) or DM the bot (private). Both fall out for free because it's a message.

This fits rule 3 (mechanism vs. content): core owns the typeahead surface, the bot owns the command vocabulary. See `22-adminbot.md` for the motivating use — driving an admin bot's advanced controls without an LLM and without a frontend.

### (Not an API) On-device compose hooks

Generic link unfurling and `@user` mention autocomplete (and anything compose-time) run in **core, on-device**, triggered by your own composer content and sourced from core state (the conversation's member list, the URL you typed). These are **intentionally not exposed to Projects** — exposing the compose buffer to a third party is exactly what rule 1 forbids. The composer *offers* Project entry points (`+`) and a bot-command reminder (`/`), but never *streams* its contents to anyone. Note the `@` typeahead reads core membership, whereas the `/` typeahead reads a bot's advertised manifest — that's why one is core and the other is split.

## Substrate representation

The wire envelope is `core/proto/content.proto`, and the extensions here are tiny — the feed gains formatting and a built-in poll, and nothing that routes user input through messages:

- **Rich text & mentions** — `repeated BodyRange ranges` on `TextMessage`, claiming the field reserved for "formatting / mentions" (`TextMessage` reserves `4 to 10` for mentions / reply_to / formatting per `35-attachments.md`). One field carries styles, link spans (= magic-link launchers), and member-mention spans; no card variant exists.
- **Simple polls** — a core built-in, not a Project surface. A poll is a small structured message; a vote is a **reaction-shaped PEER substrate message** keyed on `(poll, voter)`, last-vote-wins, **client-tallied** exactly like the reaction cluster in `33-reactions.md` — E2E, no bot, works in a DM, eventually consistent.
- **Slash commands** — no wire change at all: a slash command is a plain `TextMessage`. The only Project-side data is the bot's **command manifest** (in its profile / Project metadata), which the client syncs to drive the typeahead.

Everything interactive beyond a poll vote is either content the user sends as a normal message (Giphy result) or HTTP between the webview and its Project — never a bespoke interaction message on the wire.

## How the examples are delivered

**Giphy / stickers** — a Project entry point in the "+" menu. Tapping it opens the Giphy webview; you search and pick; the webview returns the GIF's URL via an `attach` deeplink, the client fetches it, and *you* send it as a normal attachment. No bot, no conversation membership — a pure compose helper.

**Simple polls** — a core built-in surfaced under "+". The poll is a structured message; votes are reaction-shaped PEER messages every client tallies locally. No bot, fully E2E, works in a 1:1 DM, offline-tolerant. Deliberately *not* a Project (a Project poll couldn't run in a DM or offline). It serves the dominant "quick question to the group" job.

**Cardstack / swipe survey** — the "vote yes/no through a deck of prompts" idea wants fluid, instant, *local* advancement, which an in-feed surface can't give. So it's a **Project webview**: the bot posts a **magic link** ("Priorities deck — tap to rank 12 items") as a rich-text launcher; tapping opens the Project's webview; the user swipes there; the Project collects answers and its **bot posts the result** back. Framed as its own tool (triage / prioritization), it doesn't compete with the simple poll — differentiation, not suppression, lets both survive.

**Bot-formatted announcement** — an adminbot posts a message with **rich-text ranges** (API 3): bold headers, links, emphasis. No new surface; just formatting on a normal bot message.

**Message actions ("create task", "translate")** — a Project entry point in the long-press menu; the selected message is disclosed to the Project (by opening its webview with that message as context) on the explicit tap.

**Slash commands** — a bot's command-manifest typeahead (API 4). Typing `/` reminds you what the conversation's bot(s) understand; picking one *sends a normal message* the bot interprets. Not an entry point, opens nothing — just message-authoring help. A one-line `/ban @user spam` is the whole interaction, which is why it suits adminbot's advanced controls.

`**@user` mentions** — core, on-device. The `@` typeahead lists members from core conversation state; the chosen mention is a body-range (API 3) core renders and notifies on. Same gesture as `/`, but the vocabulary is core membership (not a bot manifest) and the result is a core-interpreted range (not a bot-bound message) — so it's core, and `/` is split. `@bot` also works (a bot is a member) and stacks with slash: `@adminbot /ban @user`.

**Lightweight in-feed bot actions (approve/deny, "I'm in")** — no special surface: the bot watches **reactions and replies** it can see as a member. "React ✅ to approve" is a one-tap bot interaction with nothing new on the wire.

**Custom emoji / reaction packs** — *not* delivered through the webview primitive. React, the cluster, and the E2E routing are all core (`33-reactions.md`); a Project contributes only an **asset pack** into the core reaction picker — Project supplies vocabulary, core owns the mechanism.

**The core-only features use no Project surface, by design** — reactions, replies, read receipts (substrate grammar), generic unfurl (ambient/on-device per rule 1), simple polls (core built-in), and live location (a streaming, auto-expiring E2E primitive that must work in a bare DM; the Action Day Project then *builds on* the same primitive).

## Open questions / decisions

1. **Full-screen webview surface spec.** The webview surface is arguably bigger than this doc — the inbound launch-param/scoped-token format, the outbound deeplink vocabulary (`attach`, `close`, …), the sender-side fetch guard (allowlist/size/SSRF), isolation guarantees, content packaging (remote-loaded vs. signed bundle), and App Store constraints need their own pass, likely a dedicated `2x` doc, with a security review alongside `20-project-security.md`. (Note: no JS bridge — I/O is URL params in, deeplinks out.)
2. **New Project scopes & threat model** — entry-point registration, webview launch, return-content, and "post a result back" are new scopes with their own risks: command-name squatting, webview phishing, result-spam. An initial scope set (incl. `identity:magic-links`) is now drafted in `20-project-security.md`, *Project permissions (admin-granted scopes)*; the threat-model pass against it still needs to happen before committing.
3. **Rich text scope** — inline ranges (bold/italic/strike/monospace/spoiler) ship first, Signal-shaped. Decide whether bot content ever needs *block-level* structure (lists, headings, quotes, code blocks) and, if so, how to add it without it becoming a layout/junk-drawer vector.
4. **Poll details** — single vs multi-select, closing a poll, anonymity (note: client-tallied votes are inherently visible to members; a secret ballot would need a trusted aggregator, i.e. a Project, leaving the DM/offline/E2E sweet spot). Spec with the poll built-in.

