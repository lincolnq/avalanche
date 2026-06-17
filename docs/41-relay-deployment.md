# Deploying the push relay

One push relay serves all environments (dev + production). It maps opaque
per-(user, server) pseudonyms to APNs/FCM device tokens and fires
content-free silent pushes when homeservers report offline messages.

This guide walks through running it on a $5 DigitalOcean droplet.

**Why so cheap:** the relay has very little state, just a small SQLite file
(pseudonym → device token, with 7-day TTL). RAM use is ~10 MB, disk grows
linearly with active devices. A `s-1vcpu-512mb-10gb` droplet handles
hundreds of thousands of devices comfortably.

**What you'll need:**
- A DigitalOcean account.
- Docker on your dev Mac (for the cross-compile).
- Your APNs `.p8` key, Key ID, Team ID, and app bundle ID (see
  `core/crates/relay/README.md`).
- A domain you control (e.g. `relay.theavalanche.net`) — required so the
  homeserver can reach the relay over HTTPS.

---

## 1. Build the binary

From the repo root on your Mac:

```bash
make relay-release
```

This runs `cargo build --release -p relay` inside a `rust:1-bookworm`
Docker container and drops the binary at `dist/relay`. It links
dynamically against glibc + libssl/libcrypto, which any modern Debian or
Ubuntu droplet already has.

Rebuilds are incremental (cargo target dir is mounted at
`dist/cargo-target/`), so subsequent builds take ~30s.

---

## 2. Create the droplet

DigitalOcean → Create → Droplets:

- **Image:** Ubuntu 24.04 LTS
- **Size:** Basic → Regular → $4/mo (`s-1vcpu-512mb-10gb`)
- **Region:** anywhere; latency to APNs/FCM doesn't matter much
- **Auth:** SSH key (paste your `~/.ssh/id_ed25519.pub`)

Once it's up, point a DNS A record (`relay.theavalanche.net`) at the droplet's
IPv4 address.

---

## 3. Bootstrap the droplet

SSH in as root and create an unprivileged user for the relay:

```bash
ssh root@<droplet-ip>

adduser --system --group --home /var/lib/actnet-relay actnet-relay
mkdir -p /opt/actnet-relay /etc/actnet-relay
chown actnet-relay:actnet-relay /var/lib/actnet-relay
```

Install Caddy for TLS termination (free, auto-renews Let's Encrypt certs):

```bash
apt update
apt install -y debian-keyring debian-archive-keyring apt-transport-https curl
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/gpg.key' | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -1sLf 'https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt' | tee /etc/apt/sources.list.d/caddy-stable.list
apt update && apt install -y caddy
```

---

## 4. Copy the binary and APNs key

From your Mac:

```bash
scp dist/relay root@<droplet-ip>:/opt/actnet-relay/relay
scp AuthKey_3WMG978DSL.p8 root@<droplet-ip>:/etc/actnet-relay/
```

Back on the droplet:

```bash
chmod 755 /opt/actnet-relay/relay
chmod 600 /etc/actnet-relay/AuthKey_*.p8
chown actnet-relay:actnet-relay /etc/actnet-relay/AuthKey_*.p8
```

---

## 5. Configure the environment

```bash
cat > /etc/actnet-relay/env <<'EOF'
RELAY_BIND_ADDR=127.0.0.1:3002
DATA_DIR=/var/lib/actnet-relay
APNS_KEY_PATH=/etc/actnet-relay/AuthKey_3WMG978DSL.p8
APNS_KEY_ID=3WMG978DSL
APNS_TEAM_ID=7FVK3RR3TV
APNS_BUNDLE_ID=net.theavalanche.app
RUST_LOG=relay=info,tower_http=info
EOF
chmod 600 /etc/actnet-relay/env
```

A single relay handles both sandbox and production APNs endpoints — it
builds one client per environment from the same `.p8` and routes each
wakeup based on the `environment` field clients pass at registration
(`sandbox` for Xcode/debug builds, `production` for TestFlight/App
Store).

---

## 6. Create the systemd unit

```bash
cat > /etc/systemd/system/actnet-relay.service <<'EOF'
[Unit]
Description=avalanche push notification relay
After=network.target

[Service]
Type=simple
User=actnet-relay
Group=actnet-relay
EnvironmentFile=/etc/actnet-relay/env
ExecStart=/opt/actnet-relay/relay
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/actnet-relay
PrivateTmp=true

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable --now actnet-relay
systemctl status actnet-relay
journalctl -u actnet-relay -f
```

You should see `APNs client configured` and `starting push relay`.

---

## 7. Caddy TLS reverse proxy

```bash
cat > /etc/caddy/Caddyfile <<'EOF'
relay.theavalanche.net {
    reverse_proxy 127.0.0.1:3002
}
EOF
systemctl reload caddy
```

Caddy obtains a Let's Encrypt cert automatically on first request. Verify:

```bash
curl -i https://relay.theavalanche.net/v1/wakeup -X POST \
  -H 'content-type: application/json' \
  -d '{"pseudonyms":["bogus"]}'
# → HTTP/2 200, {"woken":[],"unknown":["bogus"]}
```

---

## 8. Point homeservers at it

On every homeserver's `.env`:

```
RELAY_URL=https://relay.theavalanche.net
```

Restart the homeserver. Send a DM to a backgrounded device; you should
see the device wake and present a banner, and `journalctl -u
actnet-relay` on the droplet should log `sent APNs wakeup`.

---

## Updating

```bash
# On Mac
make relay-release
scp dist/relay root@<droplet-ip>:/opt/actnet-relay/relay.new

# On droplet
mv /opt/actnet-relay/relay.new /opt/actnet-relay/relay
systemctl restart actnet-relay
```

Restarts drop in-flight HTTP requests but the homeserver retries, and
APNs accepts the same wakeup again — at-least-once is fine here.

---

## Backup

The only state is `/var/lib/actnet-relay/relay.db`. Losing it forces all
devices to re-register their pseudonym on next app launch (the client
already handles this — pseudonyms are re-uploaded periodically). So
backups are nice-to-have, not required. If you want them, DO's weekly
droplet snapshots ($1/mo) are enough.

---

## Observability

```bash
journalctl -u actnet-relay -f                  # live logs
journalctl -u actnet-relay --since "1h ago"    # last hour
ls -lh /var/lib/actnet-relay/relay.db          # DB size
```

If APNs starts rejecting tokens, look for `APNs send failed` lines —
typical causes are an expired/revoked `.p8`, the wrong
`APNS_ENVIRONMENT`, or device tokens for a different bundle ID.
