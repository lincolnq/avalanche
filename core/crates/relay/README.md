# actnet push relay

Standalone HTTP service that mediates between homeservers and APNs/FCM.
Homeservers never see device push tokens — they POST wakeup requests
addressed to opaque per-(user, server) pseudonyms. The relay maps
pseudonyms to device tokens and fires content-free silent pushes.

## Endpoints

Client-facing (called by `app-core` when a device registers / rotates):

- `POST /v1/register`   `{ pseudonym, device_token, platform, environment }`
  - `environment` is `"sandbox"` (debug iOS builds) or `"production"`
    (TestFlight / App Store). The relay routes to the matching APNs
    endpoint at wakeup time.
- `POST /v1/unregister` `{ pseudonym }` — marks rotated, kept 7d

Homeserver-facing:

- `POST /v1/wakeup` `{ pseudonyms: [..] }` — sends silent push to each

## Running locally

```bash
# Logged-only mode (no APNs send, useful for end-to-end plumbing tests):
make relay

# Real APNs mode (serves both sandbox + production at once — clients pick
# which endpoint to use by passing `environment` at registration):
APNS_KEY_PATH=./AuthKey_XXXXXXXXXX.p8 \
APNS_KEY_ID=XXXXXXXXXX \
APNS_TEAM_ID=YYYYYYYYYY \
APNS_BUNDLE_ID=net.theavalanche.app \
make relay
```

If `APNS_KEY_PATH` is unset the relay still runs and logs the wakeup
intent, but does not contact Apple — convenient for testing the
server→relay→pseudonym-lookup chain without a `.p8` to hand.

## Env vars

| Var | Default | Purpose |
|---|---|---|
| `RELAY_BIND_ADDR` | `0.0.0.0:3002` | HTTP bind address |
| `DATA_DIR` | `.` | Directory holding `relay.db` |
| `APNS_KEY_PATH` | _(unset)_ | Path to `.p8` auth key. If unset, APNs is disabled. |
| `APNS_KEY_ID` | _(required if key set)_ | 10-char key ID |
| `APNS_TEAM_ID` | _(required if key set)_ | 10-char team ID |
| `APNS_BUNDLE_ID` | _(required if key set)_ | App bundle ID |

A single relay instance handles both sandbox and production tokens. The
client passes `environment` ("sandbox" or "production") at registration
based on its build flavor (`#if DEBUG`); the relay stores it and routes
each wakeup to the matching APNs endpoint. Sending a sandbox token to
the production endpoint (or vice versa) returns `BadDeviceToken`, which
is why the split matters.

## Smoke-testing APNs auth without the relay

```bash
APNS_KEY_PATH=... APNS_KEY_ID=... APNS_TEAM_ID=... APNS_BUNDLE_ID=... \
APNS_ENVIRONMENT=sandbox \
cargo run -p relay --example send_test_push -- <device_token_hex>
```

(`APNS_ENVIRONMENT` here is read by the standalone example only — the
relay itself ignores it and uses the per-registration value instead.)

A `code: 200` response means the key, bundle ID, entitlement, and
provisioning profile are all correctly aligned.
