# Activism Social Network — Design Sketch

Goal: Make organizing at scale more practical, more effective and more fun!

## Documentation map

Docs are numbered by category. First digit = area, second digit = sequence within that area.

| Prefix | Area | Docs |
|--------|------|------|
| `0x` | Core design | [`00`](00-design.md) this doc · [`01`](01-technical-implementation.md) technical implementation · [`02`](02-todos-deferred.md) deferred TODOs / backlog · [`03`](03-groups.md) groups · [`04`](04-multi-device.md) multi-device · [`05`](05-device-data-sync.md) device data sync · [`06`](06-identity-device-store-split.md) identity/device store split · [`07`](07-app-core-philosophy.md) app-core philosophy · [`08`](08-supergroups.md) supergroups (large-scale broadcast channels) |
| `1x` | Server & protocol | [`10`](10-server-implementation.md) server implementation · [`11`](11-core-api-sketch.md) core API sketch · [`12`](12-abuse-handling.md) abuse handling · [`13`](13-federation.md) federation · [`14`](14-bitchat-fallback.md) bitchat fallback · [`15`](15-push-notifications.md) push notifications |
| `2x` | Projects | [`20`](20-project-security.md) project security · [`21`](21-chatbot-project.md) chatbot project · [`22`](22-adminbot.md) adminbot · [`23`](23-messaging-extensions.md) messaging extensions (core vs. Project) · [`24`](24-vetted-onboarding-project.md) vetted onboarding (gatekeeper) · [`25`](25-project-login.md) project login ("Sign in with Avalanche") |
| `3x` | Messaging & conversation UX | [`30`](30-mobile-ux.md) mobile UX · [`31`](31-read-tracking.md) read tracking · [`32`](32-threading.md) threading · [`33`](33-reactions.md) reactions · [`34`](34-connection-state.md) connection state · [`35`](35-attachments.md) attachments · [`36`](36-message-editing-deletion.md) message editing & deletion |
| `4x` | Deployment & infra | [`40`](40-deployment.md) deployment · [`41`](41-relay-deployment.md) relay deployment · [`42`](42-server-upgrades.md) server upgrades |
| `5x` | Identity, accounts & contacts | [`50`](50-identity-auth-recovery.md) identity / auth / recovery · [`51`](51-invite-tokens.md) invite tokens · [`52`](52-contacts-and-profiles.md) contacts and profiles · [`53`](53-multi-account-ux.md) multi-account UX · [`54`](54-bot-presentation.md) bot identity & presentation · [`56`](56-desktop-passkey-external-browser.md) desktop passkeys via external browser |
| `6x` | Mobile & Desktop platforms | [`60`](60-android-implementation.md) Android implementation · [`61`](61-desktop-implementation.md) Desktop implementation · [`62`](62-feature-parity.md) feature parity matrix |

## Premise

A social network whose primary acquisition vector is participation in collective action. People install the app because a Project (a canvass, a strike, a rally, a phonebank) requires it; they stay because the network captures the social connections they form through that participation.

Three core insights driving the design:

- **Activism is the acquisition vector; the social experience is the retention vector.** Action gets people in the door. Their real-life friendships keep them around. This network is the substrate that reflects that social graph and enables sustained action over time.
- **Building "Projects" is now easy; building good encrypted comms is still hard.** With modern tooling, anyone can build a campaign tool, a turf manager, a phonebanking app. It is very hard to build a Signal-quality messenger, but we desperately need something shaped like Signal (and not Slack, Discord, etc). We are a) building a powerful comms substrate, then b) numerous projects on top of it to catalyze rapid grassroots change.
- **App-first, and it should first and foremost feel like Signal.** Messaging is the primary experience: a unified inbox of all your conversations across all your activism, sorted by recency. Servers and projects are browsable in a dedicated tab, but you don't navigate into a server to read messages; you just read messages.

## Goals

- **Projects** — easy to build activism action organizing tools on top of the network; deep auth integration; work over the user's connections graph.
- **Decentralized** — people's data is not exposed to any one party by default, but people can still make friends and communicate across servers.
- **DMs, groups, and channels with E2E encryption** — for organizing, mobilization, and chat. Controllable by Projects.
- **First-class support for bots and agents** within Projects.
- **Internet and non-internet comms** as applicable: can we make at least some core messaging features work without full internet, e.g., over local wifi/bluetooth/etc? Can we do mesh comms?
- **Ways of identifying highly engaged people** to boost and empower them.
- **Good Signal-style mobile apps** — comms first, push notifications, polish.
- **Personal profiles / homepages / feeds** — built as Projects on top of the substrate (with a minimal public-profile primitive in the substrate itself; see Identity below).

## Architecture summary

### Threat model

The two primary threats the design is tuned for:

**Server seizure.** Law enforcement or a hostile actor seizes or shuts down a homeserver. A seized server should not yield the contacts, group memberships, message history, or real names of its users. Affected users should be able to reconstitute their identity and connections on another server.

**Surveillance.** To what extent can adversaries leverage persistent identities against individuals? For example, can an adversary determine that a specific person is a member of a specific organization's server? If a person uses the same identity across servers, can an adversary on one server discover their membership on others? For activist groups this can be a core threat: membership lists can be used to target individuals. The attack surface includes server APIs, the PLC (public identity directory), network traffic analysis, push relay metadata, and federation metadata.

The design is not currently hardened against state-actor surveillance of high-risk individuals (which would require onion routing, cover traffic, and a more aggressive identity story) or optimized purely against corporate data harvesting (which would require less). Either direction is possible from this foundation, but this is the target.

### Why end-to-end encryption

With E2E encryption, messages are encrypted on your device and only your recipients can decrypt them — the server relays ciphertext it cannot read. A seized server yields nothing useful; an org can't be compelled to hand over content it doesn't have. Paired with message expiry, it also hardens against seized devices: a confiscated phone only yields whatever hasn't yet disappeared. The tradeoff is that features like search and AI assistance require client-side computation, which is addressed where relevant.

### Identity: DIDs as the substrate primitive

Normally when you sign up for a service, your account belongs to that service — if it disappears, so does your account. Here, your identity is a **DID** (decentralized identifier): a cryptographic identity you control, which a homeserver hosts but does not own. If a homeserver is seized or shut down, you move your DID to another server and bring your connections, group memberships, and credentials with you. We use `did:plc`, the same method Bluesky uses, which also means your identity could be ported across both networks without migration.

Each DID has a minimal profile attached — display name (required), avatar, short bio. The profile is client-owned: stored locally and pushed as an encrypted blob to the user's discovery server, which acts as the authoritative copy and proxies fetches for any other server (see `docs/52-contacts-and-profiles.md`). The server stores ciphertext it cannot read — a seized server yields DIDs but not real names. One DID, one name everywhere. Changing your name updates it on all servers. If you want different names in different contexts, you create separate **identities** (separate DIDs; see Terminology below). Like Signal, when you create your account, you'll be prompted to write down or store a recovery key someplace safe.

### Terminology: identity, account, device

Three terms are used precisely throughout these docs — keep them distinct:

- **Identity** — a **DID**: the cryptographic identity a person controls, portable across servers (above). It holds the long-term identity key. **Separate identities are the compartmentalization boundary** — distinct, deliberately unlinkable personas.
- **Account** — an **(identity, server) pair**: one DID registered on one homeserver. A single identity can live on multiple servers because the same person can be part of different communities. Server-side rows — `accounts`, prekeys, message queues, storage — are keyed per account.
- **Device** — one installation of the app (phone, tablet, desktop) belonging to an identity. Every device of an identity shares that identity's identity key but keeps its own per-device session/prekey state (see `docs/04-multi-device.md`). A device registers an account on each server its identity uses.

Durable user data (contacts, group keys, settings) is scoped to the **identity**: synced across its devices and replicated across its accounts, but never shared across identities (see `docs/05-device-data-sync.md`, `docs/53-multi-account-ux.md`).

### Membership privacy

The network is designed to limit server membership being externally observable:

- **There are no default ways to enumerate all DIDs on a server.** Some Projects may offer membership/attendee lists though.
- **Server APIs do not confirm DID existence to unauthenticated parties.** Endpoints that could reveal whether a DID is registered (profile fetches, account lookups) require authentication. 
    - Note, however, that anyone can join a server with open membership and confirm the existence of DIDs on that server, so if you want this form of privacy it requires closed membership and vetting of applicants.
- **DID-to-server association is controlled.** The homeserver URL in PLC directory entries is optional. Servers that need membership privacy omit it, relying on invite links and contact exchange for discovery instead.
- **Profiles are encrypted.** Display names, avatars, and bios are stored as encrypted blobs on the server, decryptable only by contacts who hold the user's profile key. A seized server knows which DIDs are registered but cannot produce real names. 
    - Note, however, that some Projects may retain unencrypted membership/attendee lists.
- **Push pseudonyms are per-server.** The push relay sees pseudonym-level timing but cannot link a user's activity across servers. Pseudonyms rotate periodically.
- **Federation is selective.** Homeservers choose who they federate with. Security-sensitive orgs can federate narrowly or not at all, limiting cross-server metadata exposure.

Network traffic analysis (which IPs connect to which servers) is not currently addressed beyond TLS — onion routing or VPN usage is left to the user.

### Federation model

People register on a homeserver — typically the server of an org they're affiliated with, a campaign they're joining, or a community they trust. Homeservers "federate", meaning the servers connect to each other so people on different servers can find each other, DM, and form social ties.

The privacy posture aims higher than Matrix's defaults:

- **Pragmatic compromise on metadata:** your homeserver knows your social graph (you trust it — it's your org), but other servers learn as little as possible when you interact across boundaries.
- **E2E encryption everywhere** for DMs and groups.
- **Selective federation:** homeservers decide who they federate with. Some orgs will federate broadly; security-sensitive orgs may federate narrowly or not at all.

### Two kinds of groups

The network distinguishes two group types with different security and federation properties. This is one of the more important design choices — most ideas in this space refuse to make this split and pay for it in either bad UX or weak guarantees.

**Action-bound groups (single-server, rich):**

- Tied to a Project on the hosting homeserver.
- Full member management, roles, vetting, moderation.
- Cryptographic guarantees roughly equivalent to Signal private groups (anonymous credentials, sealed sender, encrypted group state — see Signal's Private Groups writeup and the Chase/Perrin/Zaverucha paper for the underlying scheme). The difference is the issuer is the Project's homeserver rather than a single global service.
- Federated users can participate as **guests** with scoped access (see below).
- Can be configured as **announcement-only**: a unidirectional mode where only designated senders can post and members receive.

**Cross-server casual groups (small, peer-managed):**

- Ad-hoc DMs and small group chats spanning homeservers.
- E2E encrypted; basic chat features only — no rich shared state, no moderation tooling, no Project integration. Bots can be invited as members by participants, but have no special privileges beyond what any member has.
- Practical size limit (rough rule of thumb: under ~50 members). Above that, you really want admins, roles, and moderation, which means it should be a Project.
- The guiding rule: **if a group needs an admin, it needs a homeserver.**

### Message expiry

Message expiry is a substrate-level feature, not a Project option. Every encrypted group and DM supports a configurable expiry timer; once set, messages are deleted from all clients after the timer elapses. The timer is part of the group's encrypted state, so the homeserver cannot override or circumvent it.

Defaults are deliberately short — action-bound groups default to some reasonable post-action window (e.g., 30 days), cross-server casual groups default to something shorter (e.g., 7 days). Either type can be configured longer or shorter, but Projects can enforce a minimum (e.g., "this group must expire within 24 hours") and cannot grant a longer expiry than the substrate allows.

The homeserver deletes its own copies on the same schedule. Clients delete on timer. Neither can prove the other complied — this is the same limitation Signal lives with — but the design should not allow the homeserver to silently extend retention.

### Projects

A Project is an application that runs on a homeserver and uses the network's primitives — identity, encrypted channels, the user's connections graph — under scoped, user-granted permissions.

- A Project owns one or more action-bound groups on its homeserver.
- A Project can request scoped capabilities ("read availability for users in this turf team," "send mobilization pushes to RSVP'd attendees").
- Projects can host bots and agents in their trust domain. Bots are first-class — they participate in groups, can be addressed by users, can be granted scoped permissions like any other actor. When a Project needs to read or act on chat content, it does so by adding a bot as a group member with its own keys; there is no out-of-band read mechanism. The bot decrypts messages like any other member, and its presence in the group is visible to all members.
- Projects can be built by anyone; the network gatekeeps the primitives, not the Projects. Users approve scopes.

The Project layer is where the differentiation lives. The substrate aims to be boring and reliable; the Projects on top should be where rapid iteration happens.

### Guests across federation

When a user from homeserver A wants to participate in a Project on homeserver B without registering a full account on B, they join as a guest. Their homeserver issues an identity claim (signed pubkey, anonymous "valid user on some homeserver" credential, or a stronger named-org credential — Projects pick what they require). The Project grants scoped access for the duration of the engagement.

This is the model that lets the social-graph retention story work without forcing one-account-per-org. You participate in a rally as a guest; you walk away with cross-server friendships that persist after the Project ends.

### Project federation

Some Projects are inherently single-server (action mobilization, canvassing, phonebanking — anything where the org wants tight control). But many valuable Projects want to share data across instances of the same Project on different homeservers: a Twitter-style microblog, a shared events calendar across allied orgs, a cross-org skills/resources directory, a movement-wide mutual aid board. The Project framework supports this as a first-class primitive.

The substrate provides three things; the Project provides the rest:

1. **Authenticated server-to-server transport with identity claims.** When Project P on server A wants to talk to Project P on server B, the framework handles the connection, the identity verification (this user, on this DID, on this homeserver), rate limiting, and abuse signals. The Project doesn't reimplement any of this.
2. **Pub/sub event streams between Project instances.** A Project can publish typed events; instances on other servers can subscribe. This is the default data-sharing primitive and it's enough for most social use cases (which are append-only event streams when you look closely).
3. **RPC-style cross-instance calls.** For cases that genuinely need synchronous responses (e.g. "post a reply to this thread on the originating server"), the framework supports authenticated request/response between Project instances.

Schema and semantics are the Project's responsibility — what a "post" or a "follow" or an "event" means is up to the Project. Two homeservers running the same Project interoperate because they're literally running the same code with the same schema.

Deferred: shared-log / CRDT primitives for collaboratively edited cross-instance state; schema-versioned "compatible Projects" beyond same-code-same-schema. Both are real needs eventually but neither is needed at launch.

Federation is opt-in per-homeserver and per-Project. Some Projects will federate broadly (a public microblog); some narrowly (a vouching/credentialing system that only federates among trusted movement orgs); some not at all.

A consequence worth noting: **the network has two federation surfaces.** The substrate federates (identity, DMs, casual groups, push). Projects can opt into their own federation on top. They share transport and identity primitives but are otherwise distinct. A Project can choose to be server-local, federated with an allowlist, or broadly federated — and substrate-level federation policy (which homeservers does this homeserver talk to at all) is enforced underneath.

### Stance on ATProto

ATProto is the open protocol that powers Bluesky. We use it for identity but not for everything else — here's why.

We adopt **DIDs as the identity primitive in the substrate**, but not the rest of the ATProto stack. ATProto is a fundamentally public-by-default protocol: repos are public, the firehose is public, the design center is replication and indexing. This is the right call for Twitter-replacement and the wrong call for encrypted activism organizing. E2E encryption isn't part of ATProto, the repo model is awkward for group-owned encrypted state, and the protocol's evolution is driven by a different use case than ours.

Pragmatically:

- **DIDs in the substrate.** Self-sovereign, portable, not bound to a homeserver. The DID method is chosen to be ATProto-compatible.
- **The public-social side as Projects, possibly built on ATProto.** A Twitter-Project can publish to actual Bluesky (using the user's DID) rather than building a parallel public network. Public events, public org pages, public endorsements — anything public-by-default and indexable — is a candidate for an ATProto-backed Project.
- **The private substrate is our own protocol.** DMs, encrypted groups, action-bound Projects, push, the homeserver-as-org-trust-domain model. ATProto doesn't help here and trying to use it would fight the protocol's grain.

The structural payoff: a user has one DID that identifies them in both worlds. Their public presence (if they want one) lives on Bluesky's network via a Project; their private organizing lives in our substrate. Same identity, different trust domains, no bridging code required.

### Substrate vs. Project: the heuristic

**If multiple Projects need it, or it touches encrypted comms, it's substrate. If only one Project needs it, or it's purely public data, it's a Project.**

One important distinction: the substrate's **private connections graph** (people you've messaged, people in your action groups) is end-to-end encrypted and never exposed. A Project's **public follow graph** (people whose public posts you want to see) is public-by-default and Project-level. These are kept strictly separate.

### What spans servers vs. what doesn't

| Capability | Cross-server? |
|---|---|
| DMs, small group chat (text, media, reactions) | Yes — substrate |
| Group management, roles, moderation | No — Project-local |
| Persistent shared state (turf lists, call queues, RSVP lists, polls, docs) | No — Project-local |
| Bots and agents | No — live in the Project's trust domain |
| Push notifications | Yes — via the relay (below) |
| Identity (DID) | Yes — portable across homeservers |
| Public profile (minimal) | Yes — substrate-level |
| Project-to-Project federation (pub/sub, RPC) | Yes, when the Project opts in |
| Public feeds / microblogging / events | Yes, via Projects (potentially ATProto-backed) |

## Push notifications

On iOS and standard Android, only Apple (APNs) and Google (FCM) can wake a backgrounded app. If homeservers held device tokens directly, they — and Apple/Google — would learn too much. Instead, the app developer runs a **push relay**: homeservers send content-free wakeups to per-(user, server) pseudonyms; the relay maps pseudonyms to tokens and fires empty payloads. Apple/Google see only a ping; the relay sees pseudonym-level timing but no identity, content, or cross-server linkage. Pseudonyms rotate periodically. High-risk users can opt out and poll manually. Multiple relays are supported so the Avalanche-operated relay is not a privileged singleton.

On degoogled Android (no Google Play Services), FCM is unavailable. The app falls back to **UnifiedPush** if the user has a distributor installed, and to a WebSocket keepalive otherwise. On Desktop, the Tauri app maintains a persistent WebSocket connection — wakeup is a WebSocket frame that triggers a local OS notification via `tauri-plugin-notification`, with no external push infrastructure.

The Avalanche-operated relay will live at `https://relay.theavalanche.net` (not yet deployed). See `docs/15-push-notifications.md` for the full per-platform dispatch design and UnifiedPush registration flow, and `docs/41-relay-deployment.md` for ops.

## Signup and invitations

Substrate-level registration is keys-only: the client generates an identity key pair and prekeys, sends them to the server, and gets back a DID and session token. No name, email, or phone number. Profile data (handle, avatar, bio) is optional and can be set at any time.

The substrate provides an **invitation token** primitive that gates and customizes account creation. An invitation token is a short-lived secret generated by a server admin or an existing user. It encodes: server URL, usage limit (single-use or multi-use), expiry, optional auto-enrollment into groups/Projects, an optional onboarding Project whose flow runs post-registration, and an optional inviter DID for auto-creating a DM. Tokens are encoded into deep links (`https://go.theavalanche.net/invite/...`) that can be shared as QR codes or sent via any platform.

Three representative flows:

- **Mass action signup (QR code at a rally).** Admin generates a multi-use token that auto-enrolls into the action's groups. Scan → install → register → land in chat. No profile required. Minimal friction.
- **Conference onboarding (name + photo + channel browsing).** Token targets a conference Project's onboarding flow. After key registration, the Project collects name, photo, and whatever else it needs, then auto-enrolls the user into official channels. The Channel Directory Project handles browse-and-join for the rest. Profile requirements are the Project's concern, not the substrate's.
- **Personal invite (friend-to-friend via another platform).** User generates a single-use token with their DID attached. Friend taps link → registers → app auto-initiates a DM with the inviter. If the friend already has an account on another server, they can instead just establish a cross-server DM without creating a new account.

The substrate enforces token validity and handles auto-enrollment. Everything else — what profile fields to require, what onboarding UI to show — is delegated to Projects.

## Open questions / next decisions

1. **Project federation: how much further?** We commit to pub/sub + RPC at launch. Shared-log / CRDT primitives for collaboratively edited cross-instance state, and schema-versioned "compatible Projects," are deferred. Decide when (and if) to add them based on real Project demand.

2. **Key recovery UX.** Your DID is controlled by a signing key on your device plus a recovery key that can reclaim it if the signing key is lost. Options range from a seed phrase (most secure, most likely to be lost) to encrypted cloud backup via iCloud/Google (convenient but platform-dependent) to the homeserver holding the recovery key on your behalf (easy but partially undermines portability). Likely default to cloud backup or server-delegated recovery for most users, with full self-custody as an option.

3. **Non-internet comms.** Mesh / Bluetooth / LoRa is a meaningful differentiator but a large scope expansion. Defer until the core network ships, but keep the protocol from foreclosing it.

## Mobile app

### Navigation

The app has three tabs, Signal-style:

- **Calls.** Voice and video calls. Identical to Signal's calls tab.
- **Chats.** A unified conversation inbox across all servers, sorted by most recent message with unread indicators. Every DM and group chat you belong to appears here regardless of which server hosts it. This is the primary surface of the app and the default landing tab.
- **Network.** A hierarchical list of the servers you're on. Each server expands to show all Projects the server publishes to you. Tapping any other Project opens it as a full-screen view with its own internal navigation.

### Projects in the app

Projects open as full-screen views with their own internal navigation (tabs, maps, feeds, forms — whatever the Project needs). They are visually distinct from chat views. Any group chats that a Project manages appear normally in the Chats tab; the Project view is for the non-chat parts of the Project (maps, checklists, dashboards, sign-up flows). Projects and chats are deep-linkable in both directions: a conversation view shows the Project it belongs to and tapping opens it, and a Project view can link directly into any of its chats. Anyone — and particularly bots — can send a Project link in a chat message; tapping it opens that Project directly in the app. This is the primary way bots surface Project UI to users ("tap here to see the Action Day map," "your team checklist needs attention").

---

## First-Party Project Designs

These are the first Projects we build ourselves. They serve as the primary acquisition vector — people install the app because a specific action requires it — and as the proof-of-concept for the Project framework.

**Implemented:**

- **Testbot** — AI chatbot via encrypted DMs. See [docs/21-chatbot-project.md](21-chatbot-project.md). Package: `node/packages/testbot/` (TypeScript, on `@actnet/app-core`).

**Designs (not yet built):**

### Project: Server Setup Tool

A fast and easy way to set up your server. When the server is initialized and has no initial users, it displays a qr code someplace the init happened (eg the ssh terminal window or whatever). That code can be scanned one time by the first user as an invite code that gives you admin access.

### Project: Invite Codes Tool

Invites a user to connect with you on this homeserver. Configurable parameters for invite QR codes --- groups to autojoin, roles if applicable, and so on.

### Project: Channel Directory

A server's channel directory is itself a Project — a browsable listing of the action-bound groups on that server that are open or semi-open to new members. Groups can be listed as open (join instantly), application-required (request membership, an admin approves), or unlisted (not shown; join only by invite). The directory is the natural landing experience when someone first joins a server: it explains what's happening, what groups exist, and how to get started. Because it's a Project rather than substrate, each server controls the presentation entirely — a campaign server might show a curated onboarding checklist alongside the directory; a community server might show a simple searchable list. This recovers essentially all of Slack's channel-browser functionality while keeping the substrate free of any opinion about how servers organize themselves.

### Project: Q&A Bot

A bot that answers participant questions by grounding its responses in a corpus of documents and resources the server admin provides — action plans, FAQs, logistics docs, public web pages. When someone asks a question in a group chat or DMs the bot directly, it finds the relevant source, answers in plain language, and links to the source so the participant can read further. The bot is a first-class participant in action-bound groups via the Project framework's native bot support; it can be added to any group the admin chooses and granted scoped read access to the document corpus. It does not speculate beyond its sources — if it can't find a relevant answer, it says so and surfaces the closest document it found. This handles the common organizing problem of the same question being asked dozens of times across different teams with different levels of context, without requiring admins to be constantly available.

### Project: Team Assignment

A sign-up and team-formation Project for actions where participants are divided into named, color-coded teams. The sign-up flow creates a full account on the homeserver and collects profile and contact info, storing it encrypted under the user's keys with the Project holding only scoped read access. On completion, participants are dropped into their team's action-bound encrypted group, which carries Signal-style guarantees. The Project adds a thin role layer on top — team leads get a scoped permission to see the roster and contact info; members only see who has posted. Team-level state (a checklist, capacity counts) lives in the Project's server-side store. Team reassignment works by matching swap requests bidirectionally; admins can approve one-sided moves and trigger full reshuffles.

### Project: User Directory

A list of users who are on the server. As a user, you can see the other users' profiles, initiate DMs with them, and search their profiles. You can also edit how your own profile appears to others in the directory. Admins can configure settings like whether users opt in or out of being listed, and what fields appear for users to configure. This would serve as the attendee directory for a conference.

### Project: Shared Calendar

A shared calendar of events by day and optionally by room/track. Anyone can view the events or export them. Admins can configure who has access to add or modify events, either directly or via calendar sync. You can also copy events to your personal calendar.

### Project: Action Day

A situational-awareness feature for participants during an active action, centered on a map and an announcements feed rather than chat. The map shows admin-set destination markers (staging area, route, dispersal point), pushed to all participants via high-priority push and cached for offline use; participant location is uploaded to the homeserver and shown to other participants in real time, but is ephemeral — the homeserver discards location records on a short rolling window (e.g., a few minutes) and purges all location data when the action ends. Announcements are delivered through a dedicated action-bound encrypted group where members have receive-only access enforced at the protocol level — they cannot see the participant list or reply. Sender authenticity is verified by checking the sender's key against the group's admin keyset. Action Day can run standalone or be layered on top of Team Assignment, with participants enrolled in the announcement group as part of the Team Assignment sign-up flow.

### Project: Collaborative Documents

A wiki and document editing Project where content is visible only to group members. Documents live inside a long-lived action-bound group — either a dedicated server-wide group or an existing team group — so the group's existing key management handles membership and encryption. Edits are CRDT operations encrypted and broadcast via the group channel; clients apply them locally to reconstruct the document. The server stores an append-only log of encrypted operation blobs it cannot read. Periodic snapshots of the full document state (re-encrypted with the current group key) mean new members sync from the latest snapshot rather than replaying the entire history.

### Project: Engagement Tracking

A set of observer bots that sit in action-bound groups and surface particularly helpful contributions to organizers. The bots watch for signals of high engagement — substantive answers to questions, coordination help, consistent participation — and flag those moments to a small organizer-facing dashboard, along with the participant's profile and a link to the flagged message. Organizers use this to identify people worth reaching out to, elevating into leadership roles, or recruiting into deeper involvement. The engagement data is strictly Project-local and visible only to organizers with explicit access; it is never aggregated across servers or exposed publicly. The engagement data is kept strictly Project-local precisely because globally visible engagement scores would become a target list.

