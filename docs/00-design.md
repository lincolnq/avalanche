# 00 — Avalanche: design overview

> **Status:** Overview. Each subsystem doc carries its own status block; the table in
> "Where things stand" summarizes them.
> **Last verified against code:** 2026-10-03

Goal: make organizing at scale more practical, more effective, and more fun.

## Documentation map

Start here, then `09-security-posture.md` for anything security-relevant, and
`02-todos-deferred.md` for what's next. `DIGEST.md` compresses all of it. Status vocabulary
(Built / Partial / Planned / Proposed / Speculative / Superseded) is defined in
`docs/CLAUDE.md`.

| Area | Docs |
|---|---|
| Core | [`00`](00-design.md) this overview · [`01`](01-technical-implementation.md) technical overview (repo, crates, stack, CI) · [`02`](02-todos-deferred.md) todo list / roadmap · [`03`](03-groups.md) groups · [`04`](04-multi-device.md) multi-device · [`05`](05-device-data-sync.md) device data sync (storage service) · [`06`](06-identity-device-store-split.md) identity/device store split · [`07`](07-app-core-philosophy.md) app-core · [`08`](08-supergroups.md) supergroups (speculative) · [`09`](09-security-posture.md) security posture |
| Server & protocol | [`10`](10-server-implementation.md) homeserver · [`12`](12-abuse-handling.md) abuse handling · [`13`](13-federation.md) federation · [`14`](14-bitchat-fallback.md) mesh fallback (speculative) · [`15`](15-push-notifications.md) push · [`16`](16-push-notification-service-extension.md) iOS notification service extension |
| Projects | [`20`](20-project-security.md) Project security model · [`21`](21-chatbot-project.md) testbot · [`22`](22-adminbot.md) adminbot · [`23`](23-messaging-extensions.md) messaging extensions (core vs. Project) · [`24`](24-vetted-onboarding-project.md) vetted onboarding (gatekeeper) · [`25`](25-project-login.md) Sign in with Avalanche |
| Messaging UX | [`30`](30-mobile-ux.md) mobile UX · [`31`](31-read-tracking.md) read tracking · [`32`](32-threading.md) replies & threading · [`33`](33-reactions.md) reactions · [`34`](34-connection-state.md) connection state · [`35`](35-attachments.md) attachments & link previews · [`36`](36-message-editing-deletion.md) editing & deletion · [`37`](37-chat-organization.md) chat organization |
| Deploy & infra | [`41`](41-relay-deployment.md) push relay deployment · [`42`](42-server-upgrades.md) server deployment & upgrades |
| Identity & accounts | [`50`](50-identity-auth-recovery.md) identity, auth, recovery · [`51`](51-invite-tokens.md) invite tokens · [`52`](52-contacts-and-profiles.md) contacts & profiles · [`53`](53-multi-account-ux.md) multi-account UX · [`54`](54-bot-presentation.md) bot presentation · [`55`](55-avatars.md) avatars · [`56`](56-desktop-passkey-external-browser.md) desktop passkeys |
| Platforms | [`60`](60-android-implementation.md) Android · [`61`](61-desktop-implementation.md) Desktop · [`62`](62-feature-parity.md) feature parity matrix · [`63`](63-desktop-updates.md) Desktop auto-update (Proposed) |

## Premise

A social network whose primary acquisition vector is participation in collective action.
People install the app because a Project (a canvass, a strike, a rally, a phonebank) requires
it; they stay because the network captures the social connections they form through that
participation.

- **Activism is the acquisition vector; the social experience is the retention vector.**
  Action gets people in the door. Their real-life friendships keep them around.
- **Building Projects is now easy; building good encrypted comms is still hard.** Anyone can
  build a campaign tool. Building a Signal-quality messenger is hard, and organizers badly
  need something shaped like Signal rather than Slack or Discord. So: a boring, reliable
  encrypted **substrate**, and many **Projects** on top of it.
- **App-first, and it should feel like Signal.** Messaging is the primary experience: one
  unified inbox across all your servers, sorted by recency. Servers and Projects are
  browsable in their own tab, but you never navigate "into a server" to read messages.

A consequence for prioritization: the **path from "a campaign sends you a link" to "you're
inside their Project and its groups"** is the product's front door. It deserves at least as
much care as chat polish (see `23` and `02`).

## Goals

- **Projects** — easy to build organizing tools on top of the network, with deep auth
  integration.
- **Decentralized** — no single party holds everyone's data; orgs run their own servers;
  people can still reach each other across servers.
- **E2E-encrypted DMs, groups, and announcement channels.**
- **Bots and agents as first-class participants** within Projects, always visibly.
- **Good Signal-style apps** on iOS, Android, and Desktop — comms first, push, polish.
- **Easy for non-technical organizers to run a server.** An organizer with no ops
  background should be able to stand up their org's homeserver from the website's
  configure tool in one paste (see "Running a server" below).
- **Speculative:** non-internet comms (mesh), public profiles/feeds as Projects, tools for
  identifying highly engaged people (with care — see the security note under Projects).

## Architecture in one page

**Security posture.** Tuned for two threats: **server seizure** (a seized homeserver should
not yield contacts, memberships, message history, or real names) and **surveillance**
(membership lists are targeting data; limit what any party can link). Not hardened against
targeted state-actor surveillance of individuals (no onion routing or cover traffic). What
each adversary actually learns *today* — including where the implementation currently falls
short of the design — is in `09-security-posture.md`. Read it before relying on any privacy
claim made elsewhere.

**Two governing technical principles.** Don't implement crypto — use libsignal. Make whole
vulnerability classes impossible — Rust for all security-critical code. Default to copying
Signal's approach; diverge only where the product requires it (multi-server identity,
Projects, multi-account).

**Shape of the system.** A Rust core (`app-core`) does all crypto, networking, and local
encrypted storage, and is shared by every client: iOS (SwiftUI), Android (Compose), Desktop
(Tauri), and bots (Node via napi). Homeservers (Rust/Axum/Postgres) store and relay opaque
ciphertext. A small push relay turns content-free wakeups into APNs/FCM/UnifiedPush pings.
See `01` and `07`.

**Terminology** — keep these distinct:

- **Identity** — a cryptographic identity a person controls (today a `did:plc` DID; see `50`).
  Separate identities are the compartmentalization boundary: deliberately unlinkable personas.
- **Account** — an (identity, server) pair: one identity registered on one homeserver.
- **Device** — one install of the app. Devices of an identity share its identity key but keep
  their own sessions, prekeys, and sender keys (`04`).

Durable user data (contacts, group keys, settings) is identity-scoped: synced across the
identity's devices (`05`), never shared across identities.

**Identity and recovery (Built, with Proposed changes).** Today an identity is a `did:plc`
DID whose rotation key is derived from a passkey's PRF output (or a 12-word recovery phrase), with an encrypted recovery
blob on the homeserver (`50`). Proposed (pending owner review): wrap a random root key
under the passkey rather than deriving it, add backup unlock methods, keep the top-priority
rotation key off devices, and make the private identity a self-certifying identifier that
is not published to the public PLC log — with `did:plc` as an optional public link. The
reasons are in `50` and `09`: the PLC log is public and permanent, and records the signup
server.

**Servers and federation.** People register on a homeserver — usually their org's. A homeserver
is the trust domain of an org: it knows its members' social graph within it, and should learn
as little as possible beyond it. Today there is **no federation**: a person can hold accounts on
several servers (multi-account, `53`), and conversations live on one server. **Proposed**
(pending owner review, `13`): *client-side federation* — servers never talk to each other;
to reach someone on another server, your app delivers directly to their server under sealed
sender, authorized by a delivery key only their contacts hold. This replaces the earlier
server-to-server design, now Superseded.

**Two kinds of groups.** *Action-bound groups* live on one homeserver, with roles, admin
policy, and Signal-private-group-style anonymous credentials (Built, `03`). *Cross-server
casual groups* — small, peer-managed, no admin — are Planned with federation. The rule: **if
a group needs an admin, it needs a homeserver.** Very large broadcast channels are
Speculative (`08`).

**Message expiry** is substrate-level: a timer in encrypted conversation state; clients delete
on schedule; the server deletes its copy on the same schedule (`03` §3.8, `36`).

**Push.** Homeservers never see device tokens. They send content-free wakeups to rotating
pseudonyms; a relay maps pseudonyms to tokens (Built, `15`, `41`). Note that the relay plus a
homeserver together can link pseudonyms; see `09`.

**Projects.** A Project is a separate service that (1) serves a web UI opened in an in-app
webview and (2) may run bot accounts that are ordinary encrypted participants. Because
everything is E2E, a Project that touches message content must use a bot, and **a bot's
presence in a conversation is always visible**. The homeserver admin installs Projects and
grants their capabilities (`20`, `22`). Projects authenticate users with homeserver-issued
opaque tokens, and "Sign in with Avalanche" (OAuth) lets external sites do the same (`25`).

**Substrate vs. Project heuristic.** If multiple Projects need it, or it touches encrypted
comms, it's substrate. If only one Project needs it, or it's purely public data, it's a
Project. The substrate's private connections graph is never exposed; a Project's public
follow graph (if any) is Project-level and separate. The finer core-vs-Project rules for
in-conversation features are in `23`.

**Stance on ATProto.** ATProto is public-by-default (public repos, public firehose) — right for
a Twitter replacement, wrong for encrypted organizing. We use ATProto-compatible DIDs and
nothing else. Public-social features (public profiles, feeds, event pages) belong in
Projects, potentially published to Bluesky under a user's DID. Under the Proposed identity
change, that public DID becomes an opt-in link from the private identity rather than the
private identity itself.

## Where things stand

| Area | Status | Doc |
|---|---|---|
| 1:1 encrypted messaging, receipts, reactions, edits/deletes, attachments, link previews | Built | `31`, `33`, `35`, `36` |
| Action-bound groups (zkgroup credentials, sealed-sender group send, expiry, roles, avatars) | Built (with known gaps) | `03`, `55` |
| Identity: did:plc + passkey PRF recovery, recovery blob | Built (no-blob path missing) | `50` |
| Multi-device linking + storage-service sync | Partial | `04`, `05`, `06` |
| Multi-account (several identities and servers in one app) | Partial | `53` |
| Push (relay, APNs/FCM/UnifiedPush, iOS NSE) | Built | `15`, `16`, `41` |
| Contacts, profiles, blocking, message requests, reporting | Built (with known gaps) | `12`, `52` |
| Projects: install via manifest, capabilities, directory, Project tokens, OAuth login, adminbot | Partial | `20`–`25` |
| Platforms: iOS (reference), Android, Desktop | Built; parity tracked in `62` | `60`–`62` |
| Federation (client-side) | Proposed | `13` |
| Threading beyond quote-reply | Planned / Speculative | `32` |
| Calls | Speculative (not designed in detail) | — |
| Supergroups, mesh fallback | Speculative | `08`, `14` |

## Signup and invitations

Registration is keys-only: no name, email, or phone number. A server runs in **open** or
**closed** registration mode (closed is the default); closed servers admit only holders of a
signed invite token from an installed gatekeeper Project, a bootstrap token with an operator
secret (the configure tool's first-members invite carries the shareable one), or, for a
Project's bots, that Project's bot signup key (`24`, `51`). Invite links (`https://go.theavalanche.net/i/<token>`) carry the server address
and optional onboarding/redirect hints; the server and its Projects decide what the token
grants.

Representative flows the design must keep easy:

- **Mass action signup** — scan a QR at a rally → install → register → land in the action's
  groups and Project.
- **Conference onboarding** — token targets the conference's onboarding Project, which collects
  whatever profile it needs and enrolls the user in channels.
- **Personal invite** — a friend's link opens a DM with them after signup.

The first is the front door, and it is not smooth today: links do not survive an App Store
install, and there's no `/project/<t>` deep link. Fixing both is Proposed (`23`, `02`).

## Running a server

**The configure tool is the canonical setup path.** The website's "Set up your homeserver"
page (`web/layouts/_default/configure.html`, `web/assets/configure/`) asks for a server URL
and name, then generates a cloud-init to paste into a DigitalOcean droplet plus a
first-members invite QR. The deploy bundle it fetches installs and configures everything,
and `avalanche-update` upgrades in place (`42`).

The principle: **setup must stay doable by a non-technical organizer.** No secrets to invent
or copy around, strong defaults for every setting, one paste. Secrets that need protecting
are generated on the server itself (for example the superuser bootstrap secret never passes
through the browser). Any change to server config, env vars, or the deploy must keep the
configure tool working as a one-paste setup; test it when you touch them.

## Mobile app

Tabs (iOS reference, `MainTabView.swift`): **Chats** (default — the unified inbox, with
per-identity tabs when you have more than one, `37`), **Network** (your servers and the
Projects each publishes to you), **Settings**, and **Search**. Projects open full-screen in
a webview with their own navigation; their group chats appear normally in Chats. Projects and
chats deep-link to each other (`23`). Inside a conversation the tab bar is hidden so the
composer owns the bottom edge.

## First-party Project ideas

**Built:** testbot (an AI chatbot reachable by DM, `21`) and adminbot (server administration
by chat, `22`).

Everything below is **Speculative** — sketches of Projects that would make the premise real.
Two security cautions apply to all of them: whatever a Project stores server-side is
seizable, and any list of "who is most active" or "where is everyone" is a target list.

- **Invite codes / server setup** — configurable invite QR codes (groups to auto-join, roles);
  a first-run QR on a fresh server that grants the first user admin.
- **Channel directory** — browse and join a server's open or request-to-join groups. The
  Network tab's directory entries (`22`) are the start of this.
- **Q&A bot** — answers participant questions grounded only in admin-provided documents,
  citing its source, and says so when it can't find an answer.
- **Team assignment** — sign-up into named teams, each with an action-bound group; team leads
  get scoped roster access; swap requests.
- **User directory / shared calendar** — opt-in attendee directory; shared event calendar.
- **Action Day** — map of admin-set markers plus an announcement channel. The original sketch
  uploaded live participant locations to the homeserver; under our threat model that must be
  E2E (shared within the group only) or not built.
- **Collaborative documents** — CRDT edits encrypted to a group, with periodic encrypted
  snapshots.
- **Engagement tracking** — surfacing helpful contributors to organizers. The original sketch
  used observer bots and an organizer dashboard. Any such ranking held on a server is a list
  of the most valuable people to target. If built, keep it client-side or ephemeral, and
  make the bots' presence obvious.
