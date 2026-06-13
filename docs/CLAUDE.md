# docs/ — documentation guide

## Numbering scheme

Doc filenames follow `NN-description.md`. The first digit is the category:

| Prefix | Category | What's here |
|---|---|---|
| `0x` | Core design | Goals, architecture, threat model, backlog |
| `1x` | Server & protocol | Homeserver implementation, API, abuse, federation |
| `2x` | Projects framework | Project security model, first-party Projects |
| `3x` | Messaging & conversation UX | Mobile UX, read tracking, identity, invites, contacts, connection state |
| `4x` | Deploy & infra | Deployment guides, relay deployment |
| `5x` | Identity & accounts | (reserved for future identity/account docs) |

## Which doc to read for what

| Topic | Doc |
|---|---|
| Goals, why E2E, federation model, threat model | `00-design.md` |
| Tech stack, crate layout, staged build plan | `01-technical-implementation.md` |
| Backlog / deferred TODOs | `02-todos-deferred.md` |
| Group protocol design | `03-groups.md` |
| Server endpoint design, DB schema, message relay | `10-server-implementation.md` |
| API reference sketch | `11-core-api-sketch.md` |
| Abuse handling, block lists, report flow | `12-abuse-handling.md` |
| Federation protocol | `13-federation.md` |
| Project security model, scoped bot permissions | `20-project-security.md` |
| Chatbot Project design | `21-chatbot-project.md` |
| Adminbot design and behavior | `22-adminbot.md` |
| Mobile UX flows and screen designs | `30-mobile-ux.md` |
| Read tracking, delivery receipts | `31-read-tracking.md` |
| Mesh / BitChat fallback over BLE | `32-bitchat-fallback.md` |
| Identity, passkeys, recovery blob, device loss | `33-identity-auth-recovery.md` |
| Invite tokens, onboarding flow | `34-invite-tokens.md` |
| Contacts, profile keys, profile encryption | `35-contacts-and-profiles.md` |
| Connection state machine, reconnect UX | `36-connection-state.md` |
| Multi-account UX | `37-multi-account-ux.md` |
| DigitalOcean / Hetzner deployment guide | `40-deployment.md` |
| Push relay deployment | `41-relay-deployment.md` |
| Cross-platform feature status | `feature-parity.md` |

## signal-research/

Background reading on how Signal handles specific problems (push notifications, profile key transmission, etc.). Reference material — not design decisions for this project.

## Adding a new doc

Pick the next available number in the appropriate category. Update the documentation map table in `docs/00-design.md` and add a row to the lookup table above.
