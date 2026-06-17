# Testbot Project — Design & Implementation Plan

## Goal

Build the first Project on the avalanche platform: a chatbot that users can talk to via encrypted DMs. This serves as both a useful dev tool (fake conversations for testing) and the proof-of-concept for the Project model.

## What a "Project" is (minimal version)

A Project is a standalone service that:

1. **Serves a web UI** that the mobile app opens in a webview.
2. **Owns bot accounts** that participate in encrypted DMs using the standard Signal protocol, like any other user.

Because all groups and DMs are E2E encrypted, the homeserver cannot mediate message content or group membership — it doesn't have keys. Any Project that touches messages or manages groups must do so through bot accounts that are full Signal protocol participants. This means every non-trivial Project follows the same pattern: a standalone service with bots. The chatbot is a representative example, not a special case.

The homeserver's role in the Project model is minimal: it registers bot accounts (like any other account), relays encrypted messages, and issues Project tokens for user authentication (see [Project Security](20-project-security.md)).

## The chatbot Project

### Architecture

```
┌─────────────────┐     ┌──────────────┐     ┌──────────────┐
│  Mobile App      │     │  Chatbot      │     │  Homeserver   │
│                  │     │  Service      │     │               │
│  Network tab ────┼────▶│  Web UI       │     │               │
│  (webview)       │     │  :3001        │     │               │
│                  │     │               │     │               │
│  Chats tab  ◀────┼────▶│  Bot accounts ◀────▶│  :3000        │
│  (encrypted DMs) │     │  (app-core)   │     │               │
│                  │     │               │     │               │
│            ──────┼────▶│  (token verify)├───▶│  /v1/project- │
│  (token request) │     │               │     │  token/verify │
│            ◀─────┼─────┤               │     │               │
│                  │     │  Claude Haiku │     │               │
└─────────────────┘     └──────────────┘     └──────────────┘
```

The chatbot service is a Rust binary that uses `app-core` for all crypto and messaging. It's a full Signal protocol participant.

### Authentication flow

The Project uses homeserver-issued tokens to verify user identity. See [Project Security](20-project-security.md) for the full design. The flow for the chatbot:

1. User taps "Chatbot" in the Network tab.
2. App calls `POST /v1/project-token` on the homeserver → gets a short-lived opaque token.
3. App opens webview to `http://localhost:3001/?token=<token>`.
4. Web page stores the token and includes it as `Authorization: Bearer <token>` on API calls.
5. Chatbot service verifies the token by calling `GET /v1/project-token/verify?token=<token>` on the homeserver.
6. Homeserver returns the user's DID (or 401 if invalid).

The chatbot caches verified tokens for a few minutes to avoid a round-trip on every request.

### Web UI

A single HTML page served at `GET /`. The page shows:

- A heading ("Chatbot")
- A "Text Me" button

When the user taps "Text Me", the page calls `POST /api/text-me` with the token in the Authorization header. The service verifies the token, gets the user's DID, creates a bot, and sends an opening message.

### Bot lifecycle

1. **Creation:** user taps "Text Me" → `POST /api/text-me`.
2. **Registration:** the service creates a new account on the homeserver using `AppCore::create_account_with_store()` with an in-memory store. The bot gets its own DID, identity keys, and prekeys.
3. **Opening message:** the bot sends an encrypted DM to the user: "Hey! I'm a chatbot. Ask me anything."
4. **WebSocket listener:** the bot connects to the homeserver's WebSocket endpoint (`GET /v1/ws?token=<session_token>`). When a message arrives, it decrypts it, sends the plaintext to Claude Haiku, and sends the response back as an encrypted DM. The WebSocket also handles the initial drain of any queued messages on connect.
5. **State:** all bot state (accounts, conversation history) lives in-memory. Bots die when the service restarts. Each bot gets its own in-memory SQLCipher store. Orphaned bot accounts remain on the homeserver but are harmless — queued messages expire via the server's normal message TTL, and the orphaned account/device rows are inert.

### Claude Haiku integration

The bot calls the Anthropic API with a simple system prompt:

> You are a friendly chatbot on the avalanche platform. Keep your responses concise and conversational. You're chatting with an activist — be supportive and helpful.

Each bot maintains a conversation history (the decrypted messages it has sent and received) and passes the full history to Claude on each turn. The API key is provided via the `ANTHROPIC_API_KEY` environment variable.

### API

```
GET  /                 → HTML page (web UI)
POST /api/text-me      → creates bot, sends opening message (requires valid project token)
GET  /api/bots         → list active bots for this user (requires valid project token)
```

All endpoints except `GET /` require a valid Project token in the `Authorization: Bearer` header.

## Mobile app changes

### Network tab

Currently empty. Change to show a list of Projects fetched from the homeserver.

The homeserver exposes `GET /v1/projects` (unauthenticated or authenticated — TBD), which returns the list of Projects installed on the server. For now, this list is hardcoded in the server config (e.g., an environment variable or a config file). The response is an array of `{ name, url, description }` objects.

The mobile app fetches this list and displays it in the Network tab. Tapping a Project:
1. Calls `POST /v1/project-token` on the homeserver to get a token.
2. Opens a `WKWebView` (iOS) / `WebView` (Android) to `{project_url}?token={token}`.

The webview should have visible chrome (a header bar with the Project name and a close button) so the user always knows they're in a Project view, not the native app.

### Message receive via WebSocket

The app currently has no real-time message receiving. For bot messages to appear in the Chats tab, we need:

- A WebSocket connection to the homeserver (`GET /v1/ws?token=<session_token>`) that receives messages in real time.
- Decryption of incoming messages via app-core.
- When a message arrives from an unknown DID, auto-create a Conversation.
- Store messages locally and update the conversation list.

This is Stage 3 work that's needed regardless. The chatbot Project motivates building it now.

### Conversation wiring

Currently `ConversationView` has a placeholder recipient DID. Conversations need:

- A `recipientDid` field so replies go to the right place.
- The send path to use this field instead of the hardcoded placeholder.

## Implementation order

### Step 1: Homeserver — Project list + token endpoints

New migration + three new routes. Small, self-contained.

**DB migration:**
```sql
CREATE TABLE project_tokens (
    token       TEXT PRIMARY KEY,
    account_id  BIGINT NOT NULL REFERENCES accounts(id),
    project_url TEXT NOT NULL,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL
);
```

**New endpoints:**
- `GET /v1/projects` — returns the list of Projects installed on this server. For now, the list is hardcoded in the server config (e.g., `PROJECTS` env var or a config file). Returns `[{ "name": "Testbot", "url": "http://localhost:3001", "description": "Chat with an AI bot" }]`.
- `POST /v1/project-token` (authenticated) — generate a 32-byte random token, store with user's account ID and 1-hour expiry, return the token.
- `GET /v1/project-token/verify?token=<token>` (unauthenticated) — look up the token, return `{ "did": "...", "project_url": "..." }` if valid, 401 if not.

Add expired-token cleanup to the existing background garbage-collection task.

**Also add to `net` crate:** `fetch_projects()` and `request_project_token(project_url)` methods.
**Also add to `app-core`:** FFI wrappers so the mobile app can call them.

### Step 2: Chatbot service (TypeScript)

Package: `node/packages/testbot/`

- A TypeScript service on `@actnet/app-core` (the napi binding), using Node's
  built-in `node:http` (no web framework) and global `fetch` for the Claude API.
- Starts an HTTP server on `:3001`.
- `GET /`: serves a static HTML page with the "Text Me" button.
- `POST /api/text-me`: verifies the Project token with the homeserver, registers
  an ephemeral bot account (a throwaway SQLCipher store in the OS temp dir —
  node has no in-memory store binding — so bots die with the process), sends the
  opening DM.
- Per-bot `for await (core.events())` loop: receive → read receipt + 👍 reaction
  → Claude → reply.
- Claude API key from `ANTHROPIC_API_KEY` env var (echoes when unset).
- Homeserver URL from `HOMESERVER_URL` env var (default `http://localhost:3000`).

### Step 3: Mobile — Network tab + webview

- Update Network tab to show a list of Projects (hardcoded for now).
- Before opening a Project, call `requestProjectToken()` via app-core.
- Open a `WKWebView` with the Project URL + token.
- Add visible chrome (header bar identifying the Project view).

### Step 4: Mobile — WebSocket message receive + conversation auto-create

- Connect to the homeserver's WebSocket endpoint after login.
- Decrypt incoming messages via app-core.
- On receiving a message from a new DID: create a Conversation with that DID as the recipient.
- Wire `ConversationView` to use `conversation.recipientDid` for sending.

## Open questions

1. **Should the chatbot service be a Rust crate or a separate process in another language?** Rust is simplest because it can use `app-core` directly for crypto. But it means the first Project example is Rust-only, which doesn't demonstrate the "Projects in any language" story. A future iteration could add an HTTP-based bot SDK that wraps the crypto operations.

2. **Bot display names.** The current protocol has no display name exchange. The bot's DID will show up as a raw `did:plc:...` string in the chat list. We could add a display name to the opening message payload, or defer this.

3. **Should we persist bot state to disk?** In-memory is simpler for a dev tool. Disk persistence would let bots survive restarts. Start in-memory.
