# Deferred TODOs

## Dev Infra
- Make it super easy to launch Postgres, the main server & relevant Projects all at once in dev

## Chatbot Project (finishing touches)
- Bot display name: conversations currently show raw DID instead of a friendly name
- Bot account marking: flag someplace to distinguish bot accounts in member lists

## Mobile app
- Recovery key UI: setup and backup flows (banner currently always shows, hardcoded false)
- Read receipts & scroll-position-based read marking (see docs/31-read-tracking.md, Stages B-D)
- Scroll position: remove invisible "bottom" anchor hack in ConversationView (Color.clear spacer) when scroll position saving is implemented

## Crypto / protocol
- Kyber prekey pool: upload one-time Kyber prekeys with server-side atomic consumption (like EC one-time prekeys), keep one last-resort key. Currently only a single last-resort key is used.
- Protobuf message envelope: plaintext is raw bytes, design calls for ContentMessage protobuf (proto/content.proto)
- DB encryption key from Secure Enclave instead of hardcoded "dev-placeholder-key"

## Server
- Message expiry: background task to delete expired messages, configurable per-group/DM
- Prekey vacuum: monitor prekey pool counts, send prekey_low WebSocket notifications
- Rate limiting middleware (DB schema exists but no endpoint enforcement)
- DID document resolution endpoint (GET /.well-known/did/:did)

## Big milestones (not yet started)
- Groups: action-bound (zkgroup) and cross-server casual (Sender Keys)
- Invite links & onboarding: QR codes, deep links, auto-enrollment into groups/Projects
- Push notifications: relay + per-(user, server) pseudonym rotation
- Projects framework: SDK, scoped bot permissions, JS bridge for webviews
- First-party Projects: channel directory, team assignment, action-day map, Q&A bot, collab docs, engagement tracking
- Federation: server-to-server protocol, cross-server DMs, full DID portability (PLC directory), guest access
- Calls: voice and video (VoIP)
- Public profiles: client-owned profile blobs (display name, avatar, bio) pushed to servers
- Multi-account support in mobile app
