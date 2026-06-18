#!/bin/bash
# In-place upgrade to a release tag. Invoked as `avalanche-update [TAG]`
# (a symlink to current/deploy/update.sh). Forward path only:
#   reconcile -> build alongside -> migrate -> flip -> restart -> prune.
# Rollback and pre-migrate dumps are deferred (docs/42 "Deferred").
set -euo pipefail
SELF="$(readlink -f "$0")"; HERE="$(dirname "$SELF")"
# shellcheck source=lib/common.sh
source "$HERE/lib/common.sh"

PRUNE_KEEP=3   # deployments to retain (including current)

# ---- activate phase: driven by the NEW bundle after build (re-exec target) ----
activate() {
  local tag="$1" dep="$DEPLOYMENTS/$tag" comps=() c u
  while IFS= read -r c; do [ -n "$c" ] && comps+=("$c"); done < <(deployment_components "$dep")
  [ "${#comps[@]}" -gt 0 ] || die "no components staged in $dep"
  has_server() { printf '%s\n' "${comps[@]}" | grep -qx server; }

  # Migrate before flipping: a failure leaves `current` (and the running
  # service) untouched.
  if has_server; then
    local dburl
    dburl="$(grep '^DATABASE_URL=' "$ETC/avalanche.env" | cut -d= -f2-)"
    log "running migrations"
    sudo -u avalanche env DATABASE_URL="$dburl" "$dep/server/avalanche-server" migrate
  fi

  # Refresh units from the new bundle, then flip the symlink (the atomic switch).
  for c in "${comps[@]}"; do
    u="$(component_unit "$c")"
    install -m 644 "$dep/deploy/systemd/$u" "/etc/systemd/system/$u"
  done
  systemctl daemon-reload
  ln -sfn "$tag" "$CURRENT"
  log "switched current -> $tag"

  # Restart server first, then the bots (Restart=always absorbs the brief skew).
  if has_server; then
    systemctl restart avalanche
    systemctl reload caddy 2>/dev/null || systemctl restart caddy || true
  fi
  for c in "${comps[@]}"; do
    [ "$c" = "server" ] && continue
    systemctl restart "avalanche-$c"
  done

  if has_server; then
    sleep 2
    if curl -fsS -o /dev/null http://127.0.0.1:3000/healthz; then
      log "healthz OK"
    else
      warn "healthz FAILED after upgrade to $tag -- investigate."
      warn "Rollback is manual for now; the prior deployment is retained under $DEPLOYMENTS."
    fi
  fi

  prune_old "$tag"
  log "upgrade to $tag complete"
}

# Keep the newest PRUNE_KEEP deployments (by mtime), never removing current.
# `-type d` excludes the `current` symlink itself from the candidate list.
prune_old() {
  local cur old
  cur="$(readlink "$CURRENT" 2>/dev/null || true)"
  find "$DEPLOYMENTS" -mindepth 1 -maxdepth 1 -type d -printf '%T@ %f\n' 2>/dev/null \
    | sort -rn | awk '{print $2}' \
    | grep -vx "$cur" \
    | tail -n +"$PRUNE_KEEP" \
    | while read -r old; do
        [ -n "$old" ] || continue
        log "pruning old deployment $old"
        rm -rf "${DEPLOYMENTS:?}/$old"
      done
}

# ---- entry ----
if [ "${1:-}" = "--activate" ]; then
  activate "$2"
  exit 0
fi

reconcile_or_halt

TAG="${1:-$(latest_release_tag || true)}"
[ -n "$TAG" ] || die "could not determine the target tag -- pass one explicitly"
CUR="$(readlink "$CURRENT" 2>/dev/null || true)"
if [ "$TAG" = "$CUR" ]; then log "already on $TAG"; exit 0; fi

log "upgrading $CUR -> $TAG"
DEP="$DEPLOYMENTS/$TAG"

# Build the new deployment alongside current. The live service is untouched.
install -d -m 755 "$DEP/deploy"
tmp="$(mktemp)"
log "downloading av-deploy ($TAG)"
curl -fsSL "$(release_base_url "$TAG")/av-deploy-$TAG.tar.gz" -o "$tmp"
tar xzf "$tmp" -C "$DEP/deploy" --strip-components=1
rm -f "$tmp"
echo "$TAG" > "$DEP/VERSION"

# Replicate current's component set at the new tag.
while IFS= read -r c; do
  [ -n "$c" ] && fetch_component "$TAG" "$c" "$DEP/$c"
done < <(deployment_components "$CURRENT")

# Hand off to the NEW bundle's logic for migrate/flip/restart/prune.
exec "$DEP/deploy/update.sh" --activate "$TAG"
