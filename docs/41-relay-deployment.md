# 41 — Deploying the push relay

> **Status:** Built — one relay (`https://relay.theavalanche.net`) is deployed and serves all
> environments. It is deployed by hand, outside the `av-deploy` bundle.
> **Last verified against code:** 2026-10-03

## Summary

The push relay (`core/crates/relay`) maps opaque per-(user, server) pseudonyms to APNs, FCM,
and UnifiedPush targets and fires content-free pushes when homeservers report waiting messages
(design: docs/15, docs/16). It is tiny: a small SQLite file (pseudonym → device token, 7-day
TTL), roughly 10 MB of RAM, and a `s-1vcpu-512mb-10gb` droplet ($4/mo) handles hundreds of
thousands of devices. This doc is the runbook for the one relay the project operates.

## Current design

### Build

Either take `av-relay-<target>.tar.gz` from a GitHub release (built by `release.yml` on every
`v*` tag), or build locally with `make relay-release`, which runs
`cargo build --release -p relay` inside a `rust:1-bookworm` container and writes `dist/relay`
(dynamically linked against glibc + libssl; any modern Debian/Ubuntu has them).

### Droplet setup

Ubuntu 24.04, `s-1vcpu-512mb-10gb`, SSH-key auth, and a DNS A record for
`relay.theavalanche.net`. The running relay uses the legacy `actnet-relay` names for its
user and paths:

```bash
adduser --system --group --home /var/lib/actnet-relay actnet-relay
mkdir -p /opt/actnet-relay /etc/actnet-relay
chown actnet-relay:actnet-relay /var/lib/actnet-relay
# Caddy from the official apt repo, for TLS
```

Copy the binary to `/opt/actnet-relay/relay` (mode 755) and the APNs `.p8` key and FCM
service-account JSON to `/etc/actnet-relay/` (mode 600, owned by `actnet-relay`).

### Configuration (`/etc/actnet-relay/env`, mode 600)

| Variable | Meaning |
|---|---|
| `RELAY_BIND_ADDR` | e.g. `127.0.0.1:3002` (Caddy terminates TLS in front) |
| `DATA_DIR` | `/var/lib/actnet-relay` — holds `relay.db` |
| `APNS_KEY_PATH`, `APNS_KEY_ID`, `APNS_TEAM_ID`, `APNS_BUNDLE_ID` | APNs token auth. Without a key path, APNs wakeups are logged only. |
| `APNS_PUSH_MODE` | `silent` (default: content-free background wakeup) or `alert` (content-free alert + `mutable-content`, which invokes the iOS Notification Service Extension — docs/16) |
| `FCM_SA_PATH`, `FCM_PROJECT_ID` | FCM HTTP v1 service account (project id defaults to the JSON's). Omit to disable FCM. |
| `RUST_LOG` | e.g. `relay=info,tower_http=info` |

One relay serves both sandbox and production APNs: it builds a client per environment from
the same key and routes each wakeup by the `environment` the client registered with
(`sandbox` for debug builds, `production` for TestFlight/App Store). UnifiedPush needs no
config — wakeups are HTTPS POSTs to the client-supplied endpoint, SSRF-guarded (https only,
global addresses only).

### Service

A systemd unit `actnet-relay.service` runs `/opt/actnet-relay/relay` as `actnet-relay` with
`EnvironmentFile=/etc/actnet-relay/env`, `Restart=on-failure`, `NoNewPrivileges`,
`ProtectSystem=strict`, `ProtectHome`, `PrivateTmp`, and `ReadWritePaths=/var/lib/actnet-relay`.
Caddy reverse-proxies `relay.theavalanche.net` to `127.0.0.1:3002`.

Smoke test:

```bash
curl -i https://relay.theavalanche.net/v1/wakeup -X POST \
  -H 'content-type: application/json' -d '{"pseudonyms":["bogus"]}'
# → 200 {"woken":[],"unknown":["bogus"]}
```

### Pointing homeservers at it

`RELAY_URL=https://relay.theavalanche.net` in each homeserver's env (the configure page sets
it by default).

### Updating

Copy the new binary to `/opt/actnet-relay/relay.new`, `mv` it over the old one, and
`systemctl restart actnet-relay`. In-flight requests drop, but homeservers retry and APNs
accepts duplicate wakeups (at-least-once is fine).

### Backup and observability

The only state is `/var/lib/actnet-relay/relay.db`. Losing it forces devices to re-register
their pseudonyms, which clients already do periodically, so backups are optional (droplet
snapshots suffice). Logs: `journalctl -u actnet-relay`. APNs rejections show as
`APNs send failed` (expired key, wrong environment, or wrong bundle id).

## Known gaps

- **Not in the release/deploy pipeline.** The binary is released, but install and upgrade are
  manual `scp` + `systemctl`, unlike the homeserver's `av-deploy` bundle (docs/42).
- **One relay for dev and production**, and no relay tests in CI.
- **Registration is unauthenticated** — `POST /v1/register` is an `INSERT OR REPLACE` keyed by
  pseudonym, so anyone who knows a pseudonym can redirect its wakeups. Combined with group
  members being able to see each other's group pseudonyms, this is a real attack; see docs/09
  and docs/03 §3.7.
- **The relay sees every device's full pseudonym set** (DM plus all group pseudonyms,
  registered as one batch), so whoever holds the relay plus one homeserver's database can
  re-link group membership. See docs/09.
- Legacy `actnet-*` names on the box.

## Planned

- Fold the relay into the `av-deploy` model (deployment dir, `current` symlink, same
  tag-based `avalanche-update`), and rename paths to `avalanche-relay`.
- Separate dev and production relays.
- Authenticate pseudonym registration (e.g. pseudonyms carry a secret whose hash the server
  and relay store; registration must present the preimage).

## Rationale and rejected alternatives

- **A separate, minimal service** so homeservers never hold device tokens and Apple/Google see
  only "app pinged" (docs/15).
- **All external transports go through the relay**, including UnifiedPush, so a homeserver
  never makes outbound push requests or stores per-device endpoints (docs/15).
- **Cheap by design:** the relay is stateless enough that losing it costs only a
  re-registration round.
