#!/bin/bash
# First-time provision. Run by cloud-init from the unpacked deploy bundle:
#   /opt/avalanche/deployments/<tag>/deploy/install.sh
# Inputs come from /etc/avalanche/bootstrap.env (written by cloud-init):
#   RELEASE_TAG, SERVER_URL, SERVER_NAME, RELAY_URL, REGISTRATION_SHARED_SECRET,
#   INVITE_URL, INSTALL_SERVER, INSTALL_ADMINBOT, INSTALL_TESTBOT
# See docs/42-server-upgrades.md.
set -euo pipefail
SELF="$(readlink -f "$0")"; HERE="$(dirname "$SELF")"
# shellcheck source=lib/common.sh
source "$HERE/lib/common.sh"
# shellcheck disable=SC1091
source "$ETC/bootstrap.env"

TAG="$RELEASE_TAG"
DEP="$DEPLOYMENTS/$TAG"

components=()
[ "${INSTALL_SERVER:-0}" = "1" ]   && components+=(server)
[ "${INSTALL_ADMINBOT:-0}" = "1" ] && components+=(adminbot)
[ "${INSTALL_TESTBOT:-0}" = "1" ]  && components+=(testbot)
[ "${#components[@]}" -gt 0 ] || die "nothing to install (set INSTALL_SERVER/INSTALL_ADMINBOT/INSTALL_TESTBOT)"

has() { local x; for x in "${components[@]}"; do [ "$x" = "$1" ] && return 0; done; return 1; }
want_node=0; { has adminbot || has testbot; } && want_node=1

log "installing $TAG: ${components[*]}"

# ---- host-level prep (always) ----
if [ ! -f /swapfile ]; then
  fallocate -l 1G /swapfile && chmod 600 /swapfile && mkswap /swapfile && swapon /swapfile
  echo '/swapfile none swap sw 0 0' >> /etc/fstab
fi

cat > /etc/sysctl.d/99-avalanche.conf <<EOF
vm.overcommit_memory = 2
vm.overcommit_ratio = 80
vm.swappiness = 10
EOF
sysctl --system >/dev/null

# Persistent journald so journalctl works and survives reboots (some minimal
# images default to volatile/no storage). Size-capped for the small disk.
mkdir -p /etc/systemd/journald.conf.d
cat > /etc/systemd/journald.conf.d/persistent.conf <<EOF
[Journal]
Storage=persistent
SystemMaxUse=200M
EOF
mkdir -p /var/log/journal
systemctl restart systemd-journald

export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq ca-certificates curl gnupg ufw tar

if [ "$want_node" = "1" ]; then
  log "installing Node.js 26 (bot runtime)"
  curl -fsSL https://deb.nodesource.com/setup_26.x | bash -
  apt-get install -y -qq nodejs
fi

if has server; then
  apt-get install -y -qq postgresql-16 postgresql-client-16 qrencode
  curl -fsSL https://dl.cloudsmith.io/public/caddy/stable/gpg.key \
    | gpg --dearmor -o /usr/share/keyrings/caddy-stable.gpg
  echo "deb [signed-by=/usr/share/keyrings/caddy-stable.gpg] https://dl.cloudsmith.io/public/caddy/stable/deb/debian any-version main" \
    > /etc/apt/sources.list.d/caddy-stable.list
  apt-get update -qq
  apt-get install -y -qq caddy
fi

ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp
if has server; then ufw allow 80/tcp; ufw allow 443/tcp; fi
ufw --force enable

# ---- users + dirs ----
useradd --system --home /var/lib/avalanche --shell /usr/sbin/nologin avalanche || true
install -d -o avalanche -g avalanche -m 750 /var/lib/avalanche
install -d -o root      -g root      -m 755 "$AV_ROOT" "$DEPLOYMENTS"
install -d -o avalanche -g avalanche -m 750 "$SHARED"
install -d -o root      -g avalanche -m 750 "$ETC"

# ---- build the deployment tree (deploy/ is already here) ----
has server   && fetch_component "$TAG" server   "$DEP/server"
has adminbot && fetch_component "$TAG" adminbot "$DEP/adminbot"
has testbot  && fetch_component "$TAG" testbot  "$DEP/testbot"
echo "$TAG" > "$DEP/VERSION"

# ---- postgres (server) ----
if has server; then
  mkdir -p /etc/postgresql/16/main/conf.d
  cat > /etc/postgresql/16/main/conf.d/avalanche.conf <<EOF
shared_buffers = 128MB
effective_cache_size = 512MB
work_mem = 4MB
maintenance_work_mem = 32MB
max_connections = 20
wal_compression = on
listen_addresses = ''
EOF
  systemctl restart postgresql
  sudo -u postgres createuser avalanche || true
  sudo -u postgres createdb -O avalanche avalanche || true
fi

# ---- config files (written once; operator-owned thereafter) ----
if has server; then
  HOST="$(echo "$SERVER_URL" | sed -E 's#^https?://##; s#/.*##')"
  cat > /etc/caddy/Caddyfile <<EOF
$HOST {
    reverse_proxy 127.0.0.1:3000
    encode gzip
    log {
        output stdout
        format console
    }
}
EOF
  if [ ! -f "$ETC/avalanche.env" ]; then
    cat > "$ETC/avalanche.env" <<EOF
SERVER_URL=$SERVER_URL
SERVER_NAME=$SERVER_NAME
RELAY_URL=$RELAY_URL
BIND_ADDR=127.0.0.1:3000
DATABASE_URL=postgresql:///avalanche?host=/var/run/postgresql
REGISTRATION_SHARED_SECRET=$REGISTRATION_SHARED_SECRET
EOF
    chown root:avalanche "$ETC/avalanche.env"
    chmod 640 "$ETC/avalanche.env"
  fi
fi
has adminbot && write_bot_env adminbot
has testbot  && write_bot_env testbot

# ---- systemd units + operator commands ----
for c in "${components[@]}"; do
  u="$(component_unit "$c")"
  install -m 644 "$HERE/systemd/$u" "/etc/systemd/system/$u"
done
# Commands resolve through `current`, so they track the active deployment.
ln -sfn "$CURRENT/deploy/update.sh"                     /usr/local/sbin/avalanche-update
ln -sfn "$CURRENT/deploy/bin/avalanche-status"          /usr/local/bin/avalanche-status
ln -sfn "$CURRENT/deploy/bin/avalanche-backup"          /usr/local/sbin/avalanche-backup
ln -sfn "$CURRENT/deploy/bin/avalanche-install-project" /usr/local/sbin/avalanche-install-project
ln -sfn "$CURRENT/deploy/bin/avalanche-remove-project"  /usr/local/sbin/avalanche-remove-project
if has server; then
  echo '17 3 * * * root /usr/local/sbin/avalanche-backup' > /etc/cron.d/avalanche-backup
fi

# ---- activate ----
ln -sfn "$TAG" "$CURRENT"
systemctl daemon-reload

if has server; then
  systemctl enable caddy
  systemctl restart caddy   # apt already started it on the stock config
  sudo -u avalanche env DATABASE_URL='postgresql:///avalanche?host=/var/run/postgresql' \
    "$DEP/server/avalanche-server" migrate
  systemctl enable --now avalanche
fi
has adminbot && systemctl enable --now avalanche-adminbot
has testbot  && systemctl enable --now avalanche-testbot

if has server && [ -n "${INVITE_URL:-}" ]; then
  echo "$INVITE_URL" > /var/lib/avalanche/first-invite.txt
  chown avalanche:avalanche /var/lib/avalanche/first-invite.txt
fi

log "install complete: $TAG (${components[*]})"
