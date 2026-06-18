#!/bin/bash
# Shared helpers for the Avalanche deploy bundle (install.sh, update.sh, bin/*).
# Sourced, not executed. See docs/42-server-upgrades.md.

set -euo pipefail

REPO="lincolnq/avalanche"
AV_ROOT="/opt/avalanche"
DEPLOYMENTS="$AV_ROOT/deployments"
CURRENT="$DEPLOYMENTS/current"
SHARED="$AV_ROOT/shared"
ETC="/etc/avalanche"

log()  { echo "[avalanche] $*"; }
warn() { echo "[avalanche] $*" >&2; }
die()  { echo "[avalanche] error: $*" >&2; exit 1; }

# This machine's arch -> release target triple.
detect_target() {
  case "$(uname -m)" in
    x86_64)  echo "x86_64-unknown-linux-gnu" ;;
    aarch64) echo "aarch64-unknown-linux-gnu" ;;
    *) die "unsupported arch: $(uname -m)" ;;
  esac
}

release_base_url() { echo "https://github.com/$REPO/releases/download/$1"; }

# Latest published release tag (newest first; prereleases included, drafts not
# visible to anonymous callers). Best-effort grep so we don't depend on jq.
latest_release_tag() {
  curl -fsSL "https://api.github.com/repos/$REPO/releases?per_page=10" \
    | grep -m1 '"tag_name"' \
    | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/'
}

# systemd unit name for a component (server -> avalanche, else avalanche-<name>).
component_unit() {
  if [ "$1" = "server" ]; then echo "avalanche.service"; else echo "avalanche-$1.service"; fi
}

# Components physically present in a deployment tree (subdirs minus deploy/).
deployment_components() {
  local dir="$1" path name
  [ -d "$dir" ] || return 0
  for path in "$dir"/*/; do
    [ -d "$path" ] || continue
    name="$(basename "$path")"
    [ "$name" = "deploy" ] && continue
    echo "$name"
  done
}

# Components whose systemd unit is installed on this host.
installed_unit_components() {
  local f base
  for f in /etc/systemd/system/avalanche.service /etc/systemd/system/avalanche-*.service; do
    [ -e "$f" ] || continue
    base="$(basename "$f" .service)"
    if [ "$base" = "avalanche" ]; then echo "server"; else echo "${base#avalanche-}"; fi
  done
}

# Cross-check: the component set on disk (current/) must match the installed
# units. Halt on any mismatch so an upgrade never starts/stops the wrong
# service (docs/42 "reconcile, don't guess").
reconcile_or_halt() {
  local on_disk units
  on_disk="$(deployment_components "$CURRENT" | sort | tr '\n' ' ')"
  units="$(installed_unit_components | sort -u | tr '\n' ' ')"
  if [ "$on_disk" != "$units" ]; then
    warn "component mismatch -- refusing to proceed:"
    warn "  on disk (deployments/current): ${on_disk:-(none)}"
    warn "  systemd units installed:       ${units:-(none)}"
    warn "Resolve with avalanche-install-project / avalanche-remove-project,"
    warn "then re-run. An upgrade never adds or removes a service on its own."
    exit 1
  fi
}

# Download av-<name>-<target>.tar.gz for a tag and unpack into dest, stripping
# the leading av-<name>/ directory.
fetch_component() {
  local tag="$1" name="$2" dest="$3" target tmp
  target="$(detect_target)"
  tmp="$(mktemp)"
  log "downloading av-$name-$target ($tag)"
  curl -fsSL "$(release_base_url "$tag")/av-$name-$target.tar.gz" -o "$tmp"
  mkdir -p "$dest"
  tar xzf "$tmp" -C "$dest" --strip-components=1
  rm -f "$tmp"
}

# Write a bot's env file (operator-owned; written once, never rewritten). This
# is the one spot with per-bot config knowledge -- the updater itself stays
# bot-agnostic. Requires SERVER_URL and REGISTRATION_SHARED_SECRET in scope.
write_bot_env() {
  local name="$1" f="$ETC/$name.env" key
  if [ -f "$f" ]; then log "$f exists, leaving as-is"; return 0; fi
  case "$name" in
    adminbot)
      key="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
      cat > "$f" <<EOF
ADMINBOT_SERVER_URL=$SERVER_URL
REGISTRATION_SHARED_SECRET=$REGISTRATION_SHARED_SECRET
ADMINBOT_STATE_DIR=$SHARED/adminbot-state
ADMINBOT_DB_KEY=$key
ADMINBOT_LOG=info
EOF
      install -d -o avalanche -g avalanche -m 750 "$SHARED/adminbot-state"
      ;;
    testbot)
      cat > "$f" <<EOF
HOMESERVER_URL=$SERVER_URL
REGISTRATION_SHARED_SECRET=$REGISTRATION_SHARED_SECRET
TESTBOT_BIND_ADDR=127.0.0.1:3001
TESTBOT_LOG=info
EOF
      ;;
    *) die "unknown bot '$name' -- no env known for it" ;;
  esac
  chown root:avalanche "$f"
  chmod 640 "$f"
}
