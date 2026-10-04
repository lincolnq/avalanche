# 10 — Homeserver: current-state map

> **Status:** Built — a map of the homeserver as it exists. Subsystem designs live in their own docs.
> **Last verified against code:** 2026-10-03

## Summary

The homeserver (`core/crates/server`, binary `avalanche-server`) is an Axum + Tokio service
over PostgreSQL. It stores and relays opaque ciphertext, issues auth tokens and zkgroup
credentials, hosts group state blobs, attachments, storage-service records, and the Project
and admin APIs. It runs as a **single instance**. This doc is the index; follow the pointers
for design and rationale.

## Current design

### Source layout (`core/crates/server/src/`)

| Path | Contents |
|---|---|
| `main.rs` | Load config, connect Postgres, spawn background tasks, serve. `avalanche-server migrate` applies migrations and exits — migrations never run on startup (docs/42). `avalanche-server reset-adminbot` deletes the `did:local:adminbot` account and clears the one-time superuser claim, for operator recovery via `avalanche-reset-adminbot` (docs/22). |
| `config.rs` | All configuration from env vars, each with a default (`Config::from_env`). |
| `state.rs` | `AppState`: PgPool, config, zkgroup server secret params, sender-certificate chain, blob store, and the in-process connection maps (below). |
| `routes/` | One module per API area, merged in `routes/mod.rs`. |
| `db/` | One module per table group; every function takes `&mut PgConnection` so callers choose `acquire()` (auto-commit) or `begin()` (transaction). |
| `middleware/` | `auth.rs` (session-token extractor, superuser check), `rate_limit.rs` (limits per action), `client_ip.rs`. |
| `tasks/mod.rs` | Periodic background tasks. |
| `plc.rs`, `invite_token.rs`, `blobstore.rs` | PLC resolution, invite/bootstrap token parsing, attachment blob storage. |

### API surface (route modules)

| Module | Endpoints | Design doc |
|---|---|---|
| `registration` | `POST /v1/accounts` (open or closed registration, invite/bootstrap tokens) | 50, 24 |
| `auth` | `POST /v1/auth/challenge`, `POST /v1/auth/token` | below |
| `accounts` | `/v1/accounts/{did}` (display name, self-declared `is_bot`, server-vouched `project_bot`), `/v1/accounts/{did}/devices` | 52, 54 |
| `devices` | `POST /v1/devices/link`, `POST /v1/devices/replace` | 04, 50 |
| `provisioning` | `/v1/provisioning/sessions`, `/v1/provisioning/{id}/{slot}` (device-linking mailbox) | 04 §4 |
| `prekeys` | `PUT /v1/prekeys`, `GET /v1/prekeys/{did}/{device_id}`, `GET /v1/prekeys/status` | — |
| `messages` | `POST/GET/DELETE /v1/messages` (HTTP fallback for 1:1 delivery) | below |
| `websocket` | `GET /v1/ws?token=` | below |
| `groups` | `/v1/groups…` (create, fetch, changes, credentials, endorsements, send, messages, push binding, server params) | 03 |
| `attachments`, `avatar` | `/v1/attachments…`, `/v1/profile/avatar…`, `/v1/groups/avatar/{id}` | 35, 55 |
| `profile` | `/v1/profile`, `/v1/profile/{did}` | 52 |
| `recovery` | `GET /v1/recovery/{did}`, `PUT /v1/recovery` | 50 |
| `storage` | `/v1/storage/items`, `/v1/storage/snapshot` | 05 |
| `push` | `/v1/push/register`, `/v1/push/unregister` | 15 |
| `invites` | `GET /v1/invites/{token}` | 51 |
| `projects`, `oauth` | `/v1/projects`, `/v1/project-token[/verify]`, `/v1/oauth/…` | 20, 25 |
| `admin` | `/v1/admin/…` (projects, bots, `projects/{slug}/bot-signup-key`, capabilities, directory, events, accounts) | 22, 24 |
| `abuse` | `POST /v1/abuse/report` | 12 |
| `did`, `info`, `health` | `/.well-known/did/{did}`, `/v1/info`, `/healthz` | — |

### Schema

Migrations live in `infra/migrations/` (`001_initial.sql` … `026_project_bot_signup_keys.sql`, 26 files) and are
the authoritative schema. Internal `BIGINT` primary keys; the external API speaks DIDs and
device ids. Message and blob content columns are `bytea`.

### Auth

Two-step challenge-response: `POST /v1/auth/challenge` returns a single-use nonce (5-minute
TTL); `POST /v1/auth/token` takes the nonce signed with the device's identity key and returns
an opaque, revocable session token. **Token issuance is identity-scoped; membership is checked
when the token is used**, so a client can tell 401 (re-authenticate) from 403 (removed)
(docs/34). Superuser endpoints (`/v1/admin/*`) authorize by the caller's link to the adminbot
Project (docs/22).

### Delivery

- **WebSocket** (`/v1/ws?token=`, token in the query string because browsers can't set WS
  headers). Binary `WsFrame` protobuf (`core/proto/ws.proto`); either side may originate;
  `frame.id` correlates request and response. Variants: send request/response, deliver
  request/ack, keepalive, prekey-low, group-pseudonym subscribe, group deliver request/ack,
  account-joined event, storage-changed notification.
- **On connect** the server drains the device's queue; new messages are pushed inline; the
  client acks each and the server deletes the row.
- **HTTP fallback:** `POST/GET/DELETE /v1/messages` for clients without a live socket.
- **Offline:** the server calls the push relay with the recipient's pseudonym (`RELAY_URL`;
  docs/15).
- **Retention:** the 1:1 queue keeps undelivered rows for 30 days (`message_expiry_secs`).
  Group messages and attachments have their own expiry tasks.

### In-process state: the single-instance contract

`AppState` holds three in-memory maps (`state.rs:90-113`): `ws_connections` (device → socket),
`group_subscriptions` (group push pseudonym → socket), and `account_joined_subscribers`
(admin-event listeners). They are never persisted, which is what keeps live pseudonym↔account
links out of a seized database (docs/03 §3.7), but it also means **one process serves all
sockets**. Running two instances behind a load balancer is not supported.

### Background tasks (`tasks/mod.rs`)

| Task | Interval |
|---|---|
| 1:1 message expiry, group message expiry, attachment blob TTL GC | 60s |
| Auth challenge expiry, OAuth grant expiry | 60s |
| Session-token expiry, Project-token expiry | 5 min |
| Rate-limit counter cleanup (account and IP tables) | 1 h |
| Server-event retention (30-day admin catch-up window) | 1 h |
| Prekey vacuum (send `PrekeyLow` to connected devices under threshold) | 60s |

### Rate limiting

Table-backed sliding windows: per-account counters (`rate_limit_counters`) and per-IP
counters (`ip_rate_limit_counters`, used where there's no session, e.g. registration, auth,
and the anonymous group send). Limits per action are in `middleware/rate_limit.rs`.

### External dependencies

PostgreSQL; the PLC directory at `https://plc.directory` (hardcoded in `plc.rs` and
`routes/registration.rs`); the push relay. Attachment blobs use a local-filesystem store
(`LocalFs` in `blobstore.rs`) — the S3 backend described in docs/35 is not built.

## Known gaps

- **Single-instance only** (above).
- **No server-side WebSocket ping**, so half-open sockets linger and pushes for those devices
  are silently skipped until the socket times out.
- **IP addresses are persisted** in `ip_rate_limit_counters` for about an hour, including for
  the anonymous group send — see docs/09 and docs/03.
- **PLC calls have no timeout or rate limit before them** on some paths (`routes/registration.rs`
  uses a bare `reqwest::get`). See docs/50.
- **No S3 blob backend.**

## Planned

- Server-side WS ping/pong with dead-socket eviction.
- Configurable PLC directory URL with timeouts.

## Rationale and rejected alternatives

- **No libsignal session code on the server.** The server stores and relays opaque bytes, so a
  server bug can't touch plaintext and the server needs no session state.
- **Opaque session tokens, not JWT.** Revocable by deleting a row; no signing key to manage.
- **`&mut PgConnection` everywhere.** Lets tests run each case inside a rolled-back
  transaction, and lets callers compose multi-statement writes atomically.
- **One-time prekeys are consumed with `DELETE … RETURNING`,** which is atomic under
  concurrent fetches.
- **`.sqlx/` offline data is checked in** so builds don't need a live database.
- **Migrations only via `migrate`, never on startup,** so an upgrade migrates exactly once,
  under the operator's control (docs/42).
