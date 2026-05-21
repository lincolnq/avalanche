# Stage 2 Implementation Plan: Homeserver MVP

## 1. PostgreSQL Schema

All message content is `bytea` (the server stores ciphertext it cannot read). Devices are first-class from day one for multi-device support.

```sql
-- Accounts: one per DID. Internal bigint PK for efficient FK joins;
-- DID is the external identifier exposed via the API.
CREATE TABLE accounts (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    did             TEXT NOT NULL UNIQUE,
    profile_blob    BYTEA,          -- encrypted profile (display name, avatar, bio); client-owned, opaque to server; updated via messages
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- DID documents (local stub; full PLC directory sync is Stage 9)
CREATE TABLE did_documents (
    account_id  BIGINT PRIMARY KEY REFERENCES accounts(id),
    document    JSONB NOT NULL,
    updated_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Devices: each account has one or more devices. Internal bigint PK;
-- external identifier is (account DID, device_id).
CREATE TABLE devices (
    id              BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    account_id      BIGINT NOT NULL REFERENCES accounts(id),
    device_id       INTEGER NOT NULL,
    identity_key    BYTEA NOT NULL,
    registration_id INTEGER NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (account_id, device_id)
);

-- Session tokens: short-lived auth tokens tied to a device
CREATE TABLE session_tokens (
    token       TEXT PRIMARY KEY,
    device_pk   BIGINT NOT NULL REFERENCES devices(id),
    issued_at   TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at  TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_session_tokens_expires ON session_tokens (expires_at);

-- Signed prekeys: medium-term, one active per device
CREATE TABLE signed_prekeys (
    id          INTEGER NOT NULL,
    device_pk   BIGINT NOT NULL REFERENCES devices(id),
    public_key  BYTEA NOT NULL,
    signature   BYTEA NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_pk, id)
);

-- One-time EC prekeys: consumed one per session initiation
CREATE TABLE one_time_prekeys (
    id          INTEGER NOT NULL,
    device_pk   BIGINT NOT NULL REFERENCES devices(id),
    public_key  BYTEA NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_pk, id)
);

-- Kyber (post-quantum) prekeys
CREATE TABLE kyber_prekeys (
    id          INTEGER NOT NULL,
    device_pk   BIGINT NOT NULL REFERENCES devices(id),
    public_key  BYTEA NOT NULL,
    signature   BYTEA NOT NULL,
    uploaded_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (device_pk, id)
);

-- Encrypted message queue: store-and-forward. Uses bigint PK (not UUID)
-- since the server is the sole writer.
CREATE TABLE message_queue (
    id                  BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    recipient_device_pk BIGINT NOT NULL REFERENCES devices(id),
    sender_account_id   BIGINT,                -- nullable for future sealed-sender
    ciphertext          BYTEA NOT NULL,
    message_kind        SMALLINT NOT NULL,      -- 0 = PreKey, 1 = Whisper
    enqueued_at         TIMESTAMPTZ NOT NULL DEFAULT now(),
    expires_at          TIMESTAMPTZ NOT NULL
);
CREATE INDEX idx_message_queue_recipient ON message_queue (recipient_device_pk, enqueued_at);
CREATE INDEX idx_message_queue_expires ON message_queue (expires_at);

-- Push pseudonyms (stub for Stage 5)
CREATE TABLE push_pseudonyms (
    pseudonym       TEXT PRIMARY KEY,
    device_pk       BIGINT NOT NULL REFERENCES devices(id),
    registered_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

-- Rate-limit counters: per-account sliding window
CREATE TABLE rate_limit_counters (
    account_id   BIGINT NOT NULL REFERENCES accounts(id),
    action       TEXT NOT NULL,
    window_start TIMESTAMPTZ NOT NULL,
    count        INTEGER NOT NULL DEFAULT 1,
    PRIMARY KEY (account_id, action, window_start)
);
```

Key decisions:
- **Internal bigint PKs, natural key unique indexes.** External API uses DIDs and device_id integers; the routes layer translates to internal bigints. FKs and joins are on compact integer columns for performance at scale, but internal IDs never leak outside the homeserver.
- `message_queue.expires_at` is computed at enqueue time (e.g. `now() + 30 days`); background task deletes expired rows.
- One-time prekeys are consumed via `DELETE ... RETURNING` in a single atomic statement.
- `push_pseudonyms` is a stub table — no code writes to it until Stage 5.
- `sender_account_id` is nullable to support future sealed-sender.

---

## 2. HTTP API Endpoints

All under `/v1/`. Authenticated endpoints require `Authorization: Bearer <token>`.

### Account Registration
```
POST /v1/accounts
Body: { identity_key, signed_prekey, one_time_prekeys, kyber_prekey, registration_id, device_id }
Response: 201 { did, device_pk, session_token, expires_at }
```
Creates account, generates `did:plc` locally, stores DID document, creates device, stores prekeys, issues session token.

### Device Authentication
```
POST /v1/auth/challenge
Body: { did, device_id }
Response: 200 { nonce }   # 32-byte random, base64url, 5-minute TTL, single-use
```
```
POST /v1/auth/token
Body: { did, device_id, nonce, signature }   # signature = Ed25519(nonce_bytes, identity_key)
Response: 200 { session_token, expires_at }
```
Two-step challenge-response. Client decodes the nonce to bytes, signs with its Ed25519 identity key, and sends the base64url signature. Server consumes the nonce atomically and verifies the signature against the stored public key before issuing a token.

### Prekey Upload
```
PUT /v1/prekeys
Auth: required
Body: { signed_prekey, one_time_prekeys, kyber_prekey }
Response: 200
```

### Prekey Fetch (for session initiation)
```
GET /v1/prekeys/:account_did/:device_id
Auth: required
Response: 200 { identity_key, registration_id, signed_prekey, one_time_prekey?, kyber_prekey }
```
Atomically consumes one one-time prekey. Returns bundle without it if pool is empty.

### Prekey Status
```
GET /v1/prekeys/status
Auth: required
Response: 200 { one_time_remaining, kyber_remaining }
```

### Message Send
```
POST /v1/messages
Auth: required
Body: { messages: [{ recipient_did, recipient_device_id, ciphertext, message_kind }] }
Response: 200 { sent: [...ids] }
```
Enqueues per recipient device. Notifies via WebSocket if connected.

### Message Fetch
```
GET /v1/messages
Auth: required
Response: 200 { messages: [{ id, ciphertext, message_kind, enqueued_at }] }
```

### Message Acknowledge
```
DELETE /v1/messages
Auth: required
Body: { message_ids: [...] }
```

### DID Document Resolution
```
GET /.well-known/did/:did
No auth (public)
Response: 200 DID document JSON, or 404
```

---

## 3. WebSocket Protocol

### Connection
```
GET /v1/ws?token=<session_token>
```
Upgrades to WebSocket. Server associates connection with (account, device).

### Server → Client
- `{ "type": "message", "id": 123, "ciphertext": "<base64>", "message_kind": 0, "enqueued_at": "..." }` — encrypted message delivered inline (no separate HTTP fetch needed)
- `{ "type": "prekey_low", "one_time_remaining": 2, "kyber_remaining": 1 }` — refill needed
- `{ "type": "pong" }` — keepalive response

### Client → Server
- `{ "type": "ack", "message_ids": [123, 456] }` — acknowledge delivery (server deletes from queue)
- `{ "type": "ping" }` — keepalive

### Connection Management
- In-memory `HashMap<DevicePk, Sender>` behind `Arc<RwLock<...>>`.
- Message enqueue pushes the full ciphertext over the WebSocket immediately if the device is connected. If not connected, the message stays in the queue and is delivered via `GET /v1/messages` on reconnect (or pushed over WebSocket when the device reconnects).
- Connection dropped on token expiry.

---

## 4. Background Tasks

- **Message expiry** (every 60s): `DELETE FROM message_queue WHERE expires_at < now()`
- **Session token expiry** (every 5m): `DELETE FROM session_tokens WHERE expires_at < now()`
- **Prekey vacuum** (every 5m): check pool counts per device, send `prekey_low` over WebSocket
- **Rate limit cleanup** (every 1h): `DELETE FROM rate_limit_counters WHERE window_start < now() - interval '1 hour'`

---

## 5. did:plc Stub

On registration: hash identity key + server URL + timestamp with SHA-256, base32-encode, prefix with `did:plc:`. Store a minimal DID document as JSONB with the verification method and service endpoint. No PLC directory interaction until Stage 9.

---

## 6. New Files

### Server crate (`core/crates/server/src/`)
| File | Purpose |
|---|---|
| `main.rs` | Binary: load config, connect Postgres, run migrations, start Axum, spawn background tasks |
| `config.rs` | Server configuration struct |
| `state.rs` | `AppState` (PgPool, config, WebSocket connection map) |
| `error.rs` | `ServerError` enum with `IntoResponse` |
| `db/mod.rs` | Database module root |
| `db/accounts.rs` | Account CRUD |
| `db/devices.rs` | Device registration and lookup |
| `db/prekeys.rs` | Prekey store/fetch/consume |
| `db/messages.rs` | Message queue operations |
| `db/sessions.rs` | Session token CRUD |
| `db/did.rs` | DID document storage |
| `db/rate_limits.rs` | Rate-limit counters |
| `routes/mod.rs` | Router construction |
| `routes/registration.rs` | `POST /v1/accounts` |
| `routes/auth.rs` | `POST /v1/auth/token` |
| `routes/prekeys.rs` | Prekey upload/fetch/status |
| `routes/messages.rs` | Message send/fetch/ack |
| `routes/websocket.rs` | WebSocket upgrade |
| `routes/did.rs` | DID document resolution |
| `middleware/auth.rs` | Session token extractor |
| `middleware/rate_limit.rs` | Rate limiting |
| `tasks/mod.rs` | Background task root |
| `tasks/expiry.rs` | Message + token expiry |
| `tasks/prekey_vacuum.rs` | Prekey pool monitoring |

### Infrastructure
| File | Purpose |
|---|---|
| `infra/migrations/001_initial.sql` | PostgreSQL schema |
| `infra/docker-compose.yml` | Homeserver + PostgreSQL |
| `infra/docker/Dockerfile.server` | Server build |

---

## 7. Implementation Order

**Phase 2.1 — Foundation:** migration, Docker Compose, Cargo.toml deps, config/state/error, db module, main.rs

**Phase 2.2 — Account & Auth:** registration endpoint, DID stub, token issuance, auth middleware, DID resolution endpoint

**Phase 2.3 — Prekeys:** upload, fetch (with atomic one-time consumption), status

**Phase 2.4 — Messaging:** send/fetch/ack endpoints, WebSocket connection manager, real-time notification on enqueue

**Phase 2.5 — Background Tasks + Docker:** expiry tasks, prekey vacuum, Dockerfile, finalize docker-compose, load test

---

## 8. Design Notes

- **Server never depends on libsignal.** It stores and relays opaque `bytea` blobs.
- **Opaque session tokens** (not JWTs) — simpler, revocable, good enough for single-instance.
- **WebSocket delivers full message payloads inline** (ciphertext pushed directly). `GET /v1/messages` is the reconnect/drain fallback, not the primary delivery path.
- **sqlx offline mode:** use `cargo sqlx prepare` and check in `.sqlx/` so CI doesn't need a live database at compile time.
- **One-time prekey consumption** uses `DELETE ... RETURNING` for atomicity under concurrent requests.
