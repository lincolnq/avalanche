#!/usr/bin/env bash
# First-boot bootstrap for an actnet homeserver droplet.
#
# Paste this entire file into DigitalOcean's "Add Initial Scripts (user
# data)" field when creating a droplet. It runs as root on first boot.
#
# What it does:
#   - Installs Caddy (for HTTPS) and a few helpers.
#   - Creates the `actnet` system user and config/data directories.
#   - Drops in the systemd unit and Caddyfile templates.
#   - Downloads the latest server binary release.
#
# What it does NOT do (the operator does these by hand after SSHing in):
#   - Fill in /etc/actnet/actnet.env with their DATABASE_URL + domain.
#   - Edit /etc/caddy/Caddyfile to set the actual hostname.
#   - Run database migrations against the managed Postgres cluster.
#   - Start the actnet service.
#
# See docs/40-deployment.md for the full walkthrough.

set -euo pipefail

# ── 1. Base packages ────────────────────────────────────────────────────────
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq \
    ca-certificates \
    curl \
    gnupg \
    postgresql-client \
    ufw

# Caddy (Let's Encrypt + reverse proxy) — official APT repo.
curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/gpg.key \
    | gpg --dearmor -o /usr/share/keyrings/caddy-stable-archive-keyring.gpg
curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/debian.deb.txt \
    > /etc/apt/sources.list.d/caddy-stable.list
apt-get update -qq
apt-get install -y -qq caddy

# ── 2. Firewall ─────────────────────────────────────────────────────────────
# Only ports 22 (SSH), 80 (Let's Encrypt HTTP challenge), and 443 (TLS) are
# exposed. The server binary listens on 127.0.0.1:3000 and is reached via
# Caddy.
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
ufw allow 80/tcp
ufw allow 443/tcp
ufw --force enable

# ── 3. User + directories ───────────────────────────────────────────────────
useradd --system --home /var/lib/actnet --shell /usr/sbin/nologin actnet || true
install -d -o actnet -g actnet /var/lib/actnet
install -d -o root   -g root   /etc/actnet
install -d -o root   -g root   /usr/local/lib/actnet

# ── 4. Server binary ────────────────────────────────────────────────────────
# TODO(deploy): once GitHub Releases publishes actnet-server-linux-x86_64,
# replace the placeholder below with a real download. For now this leaves
# a marker file so the operator knows what's missing.
BINARY_URL="${ACTNET_BINARY_URL:-}"
if [[ -n "$BINARY_URL" ]]; then
    curl -fsSL -o /usr/local/lib/actnet/actnet-server "$BINARY_URL"
    chmod +x /usr/local/lib/actnet/actnet-server
else
    cat > /usr/local/lib/actnet/MISSING_BINARY <<'EOF'
The server binary has not been downloaded yet. Either:
  - Pass ACTNET_BINARY_URL in the cloud-init env, or
  - Manually copy a built binary to /usr/local/lib/actnet/actnet-server
    (chmod +x it), then `systemctl start actnet`.
EOF
fi

# ── 5. Config templates ─────────────────────────────────────────────────────
# Embedded so the script is fully self-contained — operator doesn't need
# to clone the repo on the droplet.

cat > /etc/actnet/actnet.env <<'EOF'
# actnet homeserver configuration. Fill in the values marked CHANGEME.
# After editing, run: systemctl restart actnet

# Public HTTPS URL of this homeserver. Must match your DNS and Caddyfile.
SERVER_URL=https://CHANGEME.example.com

# Managed Postgres connection string from DigitalOcean's panel.
DATABASE_URL=postgresql://CHANGEME

# Human-readable name shown during invite onboarding.
SERVER_NAME=CHANGEME

# Bind to localhost only — Caddy reverse-proxies from :443.
BIND_ADDR=127.0.0.1:3000

# Shared push relay (operated by the actnet project). Leave as-is unless
# you're running your own.
RELAY_URL=https://relay.actnet.example
EOF
chmod 640 /etc/actnet/actnet.env
chown root:actnet /etc/actnet/actnet.env

cat > /etc/caddy/Caddyfile <<'EOF'
# Change `your-domain.example.com` to your actual hostname, then:
#   systemctl restart caddy
#
# Caddy automatically obtains and renews a Let's Encrypt certificate for
# any hostname listed here, as long as DNS points at this droplet.

your-domain.example.com {
    reverse_proxy 127.0.0.1:3000
    encode gzip
}
EOF

cat > /etc/systemd/system/actnet.service <<'EOF'
[Unit]
Description=actnet homeserver
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=actnet
Group=actnet
EnvironmentFile=/etc/actnet/actnet.env
ExecStart=/usr/local/lib/actnet/actnet-server
WorkingDirectory=/var/lib/actnet
Restart=always
RestartSec=5
# Hardening
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
PrivateTmp=true
ReadWritePaths=/var/lib/actnet

[Install]
WantedBy=multi-user.target
EOF

systemctl daemon-reload
systemctl enable actnet
# Note: not starting actnet here — it would fail until the operator fills
# in /etc/actnet/actnet.env. The systemctl start happens at the end of
# Step 4 in the deployment guide.

# ── 6. Done ─────────────────────────────────────────────────────────────────
cat > /etc/motd <<'EOF'

  actnet homeserver — first-boot setup complete.

  Next steps (see docs/40-deployment.md):
    1. nano /etc/actnet/actnet.env      # fill in CHANGEME values
    2. nano /etc/caddy/Caddyfile        # set your real domain
    3. systemctl restart caddy actnet
    4. curl https://YOUR_DOMAIN/healthz # should print: ok

EOF

echo "[setup.sh] bootstrap complete — operator must finish config and start actnet"
