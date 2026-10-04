# 23 — Messaging Extensions: Core vs. Project

> **Status:** Partial — the boundary rules are decided. Of the surfaces described, only the bridgeless Project webview with intercepted deep links is built (plus core features documented elsewhere: reactions `33`, link previews and contact cards `35`, receipts `31`). Entry points, magic links, return-content, rich text, mentions, polls, slash-command autocomplete and live location are not built.
> **Last verified against code:** 2026-10-03

## Summary

As messaging grows richer (polls, slash commands, live location, GIFs, surveys), each feature needs a home: the lean, audited core, or a Project. This doc draws that line with three rules and a deliberately small set of surfaces. The thesis: **keep the in-conversation surface boring (native, auditable, E2E) and push all real interactivity into an explicitly opened Project webview.**

It also records the acquisition-path work that matters more than any of these surfaces: getting someone from "my campaign sent me a link" to "I'm inside the Project", across an app install.

## Current design

### The three rules

1. **Explicit handoff only.** A Project extends messaging only through a user-initiated handoff: opening its webview, invoking an entry point, tapping a launcher. Ambient, compose-time, always-listening capabilities stay in core and on-device. No Project reads your composer, and no Project-rendered UI sits inside a conversation.
2. **The 1:1-DM litmus test.** *Must it work in a one-on-one DM with no bot present?* Then it's core.
3. **Mechanism vs. content.** Where a feature is Project-extensible, core owns the mechanism, the surface and the privacy-sensitive parts; the Project contributes content or a webview at a defined seam and never sits in the middle of a private interaction.

### Where features live

| Example | Lives in | Surface | Status |
|---|---|---|---|
| Emoji reactions | Core | long-press | Built (`33`) |
| Read receipts | Core | automatic | Built (`31`) |
| Link previews | Core, on-device | auto when a URL is in the body | Built (`35`) |
| Shared contact cards | Core | share sheet | Built (`35`) |
| Replies / threading | Core | long-press / compose | Planned (`32`) |
| `@` mentions | Core, on-device | composer `@` | Planned |
| Simple polls | Core built-in | "+" → poll | Speculative |
| Live location | Core | "+" → map | Speculative |
| Bot-formatted messages (rich text) | Split | bot-posted message | Speculative |
| Custom emoji packs | Split | reaction picker | Speculative |
| Bot slash-command autocomplete | Split | composer `/` | Speculative (typing `/cmd` to a bot works today; it's just text) |
| Giphy / stickers | Project | "+" → webview | Speculative |
| Card-stack survey | Project | magic link → webview | Speculative |
| "Create task from this" | Project | message long-press → webview | Speculative |
| "Flag this member" | Project | participant long-press → webview | Speculative |
| Action Day map | Project | Network tab → webview | Speculative (`00`) |

### The one built surface: a bridgeless Project webview

Opened from the Network tab or from a deep link. Its constraints are load-bearing:

- **No JS bridge.** Input is URL parameters on the launch URL (today only `?token=`); output is navigation to an intercepted `https://go.theavalanche.net/<action>/<arg>` link, caught by host match in the webview's navigation delegate (`20` §Webview). Routes today: `conversation/<did>`, `i/<token>`, `authorize`. Any intercepted link also dismisses the webview.
- **Zero access to E2E data.** The webview never sees message history, the local store or keys. Opening a Project means leaving the E2E conversation for that Project's server-backed trust domain, so the chrome always names the Project.
- **Honest costs.** Remote webview content is not part of the reproducible build and can change server-side. The guarantee is isolation, explicit consent and no access to secrets, not audited code. Webviews need connectivity, opening one is a metadata hit to the Project's origin, and full-screen arbitrary content is a phishing surface.

### What we do not build

Inline interactive cards, in-feed form controls and display cards. Once a webview exists for real interaction, every in-feed widget is a redundant middle layer: more native cross-platform rendering, a submission-routing substrate, and an in-feed phishing surface, doing badly what the webview does well. Lightweight in-feed actions ("I'm in", approve/deny) are **reactions or replies a member bot observes**.

## Known gaps

- **Webviews get no launch context.** A Project can't tell which conversation or group opened it, so "my turf team" can't be built without the user re-identifying it. See *Proposed*.
- **No Project deep link.** The app routes only `conversation`, `i`/`invite` and `authorize` (`mobile/ios/Actnet/Sources/App/AppState.swift`, `handleDeepLink`). A link can't drop someone into a Project, and the invite flow's post-onboarding redirect only ever opens a DM with the inviter (`core/crates/server/src/routes/invites.rs`).
- **Webview hardening and identity scoping** gaps are in `20` §Known gaps.

## Proposed

These change the Project interface contract and cross-platform deep-link behavior, so they need project-owner review before implementation. They serve the premise in `00` directly ("install because a Project needs it") and come before any of the conversation surfaces.

### Project links that survive install: `/project/<t>`

A link `https://go.theavalanche.net/project/<t>`, where `<t>` is base64url JSON carrying at least the server URL and the Project (slug or URL), and optionally an invite token for servers with closed registration.

- **App installed, account on that server:** open the Project's webview with a fresh token, minted from that account.
- **App installed, no account there:** run the invite onboarding (`51`), then open the Project as the post-onboarding step.
- **App not installed:** the `go.theavalanche.net` landing page sends the user to the store and preserves the link so the first launch resumes it. iOS has no native deferred deep linking. The candidate mechanism is the landing page copying the link and onboarding offering "Continue where you left off" via the paste prompt, with a typed code as the fallback. No third-party attribution SDK.

### Launch context: opaque per-conversation handles

When a webview is opened from a conversation (a Project launcher, or later an entry point), the client appends an opaque `ctx` handle identifying that conversation **to that Project only**. For a group: `ctx = HMAC(K_launch, project_origin)`, where `K_launch` is derived from the group master key. A Project whose bot is a member holds the master key, can compute the same value and map it to the group. Any other Project, and the server, learns nothing and can't correlate handles across Projects. The same construction over the 1:1 conversation with the Project's bot covers DMs. This gives Projects "which team opened me" without a JS bridge and without a new membership disclosure (`03` §3.9 intact).

## Planned

Core features, each with its own doc:

- Replies, as Signal-style quote-reply first (`32`).
- `@` mentions as a body-range kind: typeahead from core conversation membership, notifying even in muted conversations.

## Speculative

### Conversation surfaces for Projects

Everything interactive is "open a Project's webview, with context and a scoped token". Two ways in:

- **Registered entry points** `{ id, label, icon, scope }` in the composer "+" menu (`surface.compose`), the message long-press menu (`message.context-on-action`, which discloses that one message on tap), or the participant long-press menu in groups (`participant.context-on-action`). The participant menu is **bot-membership-gated**: it appears only where the Project's bot is already a visible member, so the tap discloses the actor's intent, not a new membership fact.
- **Magic links** — Project-issued links that open the Project's webview wherever they're tapped. The link carries no credential; the tapping device mints a token at tap time, only for Projects on its own vetted allowlist (no open redirect). Anyone can share one.

Two return modes:

- **Return content you send.** The webview navigates to `…/compose/attach?url=…&type=…`. The client fetches the URL sender-side (https-only, size-capped, allowlisted), previews it in the composer and **never auto-sends**. Giphy is this, with no bot needed.
- **The bot posts the result.** The webview tells its own backend, and the Project's bot (a member) posts the outcome. Nothing needed from the client.

Security posture for each surface is in `20` §Speculative.

### Rich text, mentions, slash autocomplete, polls

- **Rich text** — Signal-style: a plain `body` plus `repeated BodyRange` overlays on `TextMessage` (reserved fields 5–10 in `core/proto/content.proto`). Bots author it first; human formatting controls can come later without a wire change. Plaintext is always present, so notifications and old clients degrade cleanly. Mentions are another range kind.
- **Slash autocomplete** — a bot advertises a command manifest; when you type `/` in a conversation where that bot is present, the client shows a typeahead. Invoking a command is just sending the text, so there is no wire change and an old client still works. Core owns the typeahead, the bot owns the vocabulary.
- **Simple polls** — a structured message plus reaction-shaped PEER votes keyed on `(poll, voter)`, last vote wins, client-tallied like reactions. E2E, works in a DM, no bot. Votes are visible to members; a secret ballot needs a trusted aggregator, i.e. a Project.
- **Custom emoji packs** — a Project contributes assets into the core reaction picker, pre-fetched and size-capped.

### Developer-experience ideas

- **A first-party "messenger bot" Project with a narrow HTTP API** — DM a DID, post to a group, create a team group — run in the operator's trust domain, so campaigns can drive messaging without running Signal key state themselves.
- **A `create-avalanche-project` scaffold** — web app, bot, manifest and a compose file against `make dev-all`.
- **Offline-capable webviews** (service workers, app-bound domains) for canvassing.

## Rationale and rejected alternatives

- **Rejected: inline interactive cards and in-feed forms** (see *What we do not build*).
- **Rejected: Project access to the compose buffer.** Link unfurling and `@` autocomplete run on-device from core state precisely so no third party ever sees what you're typing.
- **Simple polls in core, card-stack surveys as a Project.** A Project poll couldn't work in a DM or offline. The survey wants fluid local interaction that only a webview gives; framing it as a different tool (triage) lets both exist.
- **Slash commands are messages, not invocations.** No new wire format, no frontend requirement, and the same channel as talking to the bot in plain language.
