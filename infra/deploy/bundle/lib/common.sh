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
# Project manifests adminbot installs non-interactively at startup (docs/22): one
# per installed web Project, published into the DB directory + OAuth registry.
MANIFESTS="$SHARED/manifests"
# Per-Project bot signup keys adminbot writes when it auto-installs a manifest
# (docs/24): <slug>.key, read by first-party Project bots such as testbot.
BOT_SIGNUP_KEYS="$SHARED/bot-signup-keys"

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

# The superuser bootstrap secret (docs/24, docs/09 S-01): the root credential
# adminbot uses, once, to claim superuser. Generated on this host and never
# shared -- it is not in the configure tool's output or the first-members invite.
# Reuses the value already in the server's or adminbot's env, so both agree.
superuser_secret() {
  local f v
  for f in "$ETC/avalanche.env" "$ETC/adminbot.env"; do
    [ -f "$f" ] || continue
    v="$(grep '^SUPERUSER_BOOTSTRAP_SECRET=' "$f" | head -1 | cut -d= -f2- || true)"
    if [ -n "$v" ]; then echo "$v"; return 0; fi
  done
  head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n'
}

# Append KEY=VALUE to an env file if KEY isn't set there yet. Env files are
# operator-owned; this only ever adds a missing line.
ensure_env_line() {
  local f="$1" key="$2" value="$3"
  [ -f "$f" ] || return 0
  grep -q "^$key=" "$f" || echo "$key=$value" >> "$f"
}

# Remove KEY from an env file (used to retire a secret a component no longer
# needs). No-op if absent.
drop_env_line() {
  local f="$1" key="$2"
  [ -f "$f" ] || return 0
  sed -i "/^$key=/d" "$f"
}

# Bring existing env files up to the current layout. Idempotent; run by both
# install.sh and the updater, so servers installed before a change pick it up.
#   - the server and adminbot carry the superuser bootstrap secret;
#   - adminbot and testbot no longer carry the registration shared secret
#     (docs/09 S-01) -- testbot registers with its bot signup key instead.
migrate_env_files() {
  local secret
  secret="$(superuser_secret)"
  ensure_env_line "$ETC/avalanche.env" SUPERUSER_BOOTSTRAP_SECRET "$secret"
  ensure_env_line "$ETC/adminbot.env"  SUPERUSER_BOOTSTRAP_SECRET "$secret"
  ensure_env_line "$ETC/adminbot.env"  ADMINBOT_BOT_SIGNUP_KEY_DIR "$BOT_SIGNUP_KEYS"
  drop_env_line   "$ETC/adminbot.env"  REGISTRATION_SHARED_SECRET
  ensure_env_line "$ETC/testbot.env"   TESTBOT_BOT_SIGNUP_KEY_FILE "$BOT_SIGNUP_KEYS/testbot.key"
  drop_env_line   "$ETC/testbot.env"   REGISTRATION_SHARED_SECRET
  install -d -o avalanche -g avalanche -m 700 "$BOT_SIGNUP_KEYS"
}

# Write a bot's env file (operator-owned; written once, never rewritten). This
# is the one spot with per-bot config knowledge -- the updater itself stays
# bot-agnostic. Requires SERVER_URL in scope. Secrets and key paths are added by
# migrate_env_files.
write_bot_env() {
  # Separate declarations: a single `local a=.. b=$a` expands $a before local
  # assigns it (word expansion precedes the builtin), which trips `set -u`.
  local name="$1"
  local f="$ETC/$name.env"
  local key
  if [ -f "$f" ]; then log "$f exists, leaving as-is"; return 0; fi
  case "$name" in
    adminbot)
      key="$(head -c 32 /dev/urandom | od -An -tx1 | tr -d ' \n')"
      cat > "$f" <<EOF
ADMINBOT_SERVER_URL=$SERVER_URL
ADMINBOT_STATE_DIR=$SHARED/adminbot-state
ADMINBOT_DB_KEY=$key
ADMINBOT_LOG=info
ADMINBOT_MANIFEST_DIR=$MANIFESTS
EOF
      install -d -o avalanche -g avalanche -m 750 "$SHARED/adminbot-state"
      ;;
    testbot)
      cat > "$f" <<EOF
HOMESERVER_URL=$SERVER_URL
TESTBOT_BIND_ADDR=127.0.0.1:3001
TESTBOT_BASE_PATH=/p/testbot/
TESTBOT_PUBLIC_URL=${SERVER_URL%/}/p/testbot
TESTBOT_LOG=info
EOF
      ;;
    *) die "unknown bot '$name' -- no env known for it" ;;
  esac
  chown root:avalanche "$f"
  chmod 640 "$f"
}

# Web descriptor for Projects that serve a webview: "PORT|NAME|DESCRIPTION".
# Empty for headless Projects (e.g. adminbot) and unknown names. This is the one
# spot that knows a Project's public face; the updater stays Project-agnostic.
project_web_meta() {
  case "$1" in
    testbot) echo "3001|Testbot|Chat with an AI bot" ;;
    *) echo "" ;;
  esac
}

# OAuth login client id for a Project that supports "Sign in with Avalanche"
# (docs/25); empty for Projects that don't. When set, write_project_manifests
# adds clientId + the <base>/login redirect to the Project's manifest so adminbot
# registers it as an OAuth login client on the `projects` row.
project_oauth_client() {
  case "$1" in
    testbot) echo "testbot" ;;
    *) echo "" ;;
  esac
}

# Write a Project manifest (docs/20) per installed web Project into MANIFESTS, so
# adminbot installs it non-interactively at startup (via ADMINBOT_MANIFEST_DIR).
# This is what publishes a web Project's client-directory entry + OAuth login
# registration into the DB (there is no PROJECTS env). Regenerated from the
# on-disk component set each run (a removed Project's manifest disappears); needs
# SERVER_URL in scope. No-op if SERVER_URL is unset.
write_project_manifests() {
  # SERVER_URL is in scope during install (from bootstrap.env) but not during
  # update.sh — fall back to avalanche.env there (as regenerate_project_routes does).
  local server_url="${SERVER_URL:-}"
  if [ -z "$server_url" ] && [ -f "$ETC/avalanche.env" ]; then
    server_url="$(grep '^SERVER_URL=' "$ETC/avalanche.env" | cut -d= -f2-)"
  fi
  [ -n "$server_url" ] || return 0
  server_url="${server_url%/}"
  install -d -o avalanche -g avalanche -m 750 "$MANIFESTS"
  rm -f "$MANIFESTS"/*.json 2>/dev/null || true
  local c meta port name desc base cid oauth
  while IFS= read -r c; do
    [ -n "$c" ] || continue
    meta="$(project_web_meta "$c")"
    [ -n "$meta" ] || continue
    IFS='|' read -r port name desc <<< "$meta"
    base="$server_url/p/$c"
    cid="$(project_oauth_client "$c")"
    oauth=""
    [ -n "$cid" ] && oauth=",\"clientId\":\"$cid\",\"redirectUris\":[\"$base/login\"]"
    cat > "$MANIFESTS/$c.json" <<EOF
{"slug":"$c","name":"$name","description":"$desc","url":"$base","permissions":[],"webEntries":[{"name":"$name","url":"$base/","description":"$desc"}]$oauth}
EOF
    chown avalanche:avalanche "$MANIFESTS/$c.json"
    chmod 640 "$MANIFESTS/$c.json"
  done < <(deployment_components "$CURRENT")
}

# Regenerate the Caddy reverse-proxy routes (/p/<slug>/*) for the web Projects
# installed in current/. Server host only (needs avalanche.env; callers reload
# caddy afterward). The client-facing directory (GET /v1/projects) and each
# Project's OAuth login registration are NOT managed here — they live in the DB,
# published by adminbot's /install-project manifest (docs/22, docs/25). This
# function only wires HTTP routing.
regenerate_project_routes() {
  [ -f "$ETC/avalanche.env" ] || return 0   # not a server host
  local snippet="/etc/caddy/avalanche-projects.caddy"
  local c meta port name desc
  : > "$snippet"
  while IFS= read -r c; do
    [ -n "$c" ] || continue
    meta="$(project_web_meta "$c")"
    [ -n "$meta" ] || continue
    IFS='|' read -r port name desc <<< "$meta"
    cat >> "$snippet" <<EOF
handle_path /p/$c/* {
    reverse_proxy 127.0.0.1:$port
}
EOF
  done < <(deployment_components "$CURRENT")
}
