#!/usr/bin/env bash
#
# ONE-TIME: create the Desktop auto-update signing key (docs/63) directly in
# 1Password. The private key only ever exists on a RAM disk while this runs, and
# goes into 1Password via a template file on that RAM disk (never as a command
# argument, so it doesn't show up in `ps`). Prints the PUBLIC key, which goes in
# desktop/src-tauri/tauri.conf.json (plugins.updater.pubkey).
#
# The key has no password of its own: 1Password is what protects it, the same as
# the Android release keystore (mobile/android/release-sign.sh).
#
# Refuses to run if the item already exists — regenerating would orphan every
# installed copy (they only trust the public key they were built with).
#
# Prerequisites: 1Password CLI `op` signed in; run from anywhere.

set -euo pipefail

OP_VAULT="${OP_VAULT:-Private}"
OP_ITEM="${OP_ITEM:-Avalanche Desktop updater signing key}"

command -v op >/dev/null 2>&1 || { echo "error: 1Password CLI (op) not found on PATH" >&2; exit 1; }
op account list >/dev/null 2>&1 || { echo "error: not signed in to 1Password — run: op signin" >&2; exit 1; }

if op item get "$OP_ITEM" --vault "$OP_VAULT" >/dev/null 2>&1; then
  echo "error: '$OP_ITEM' already exists in vault '$OP_VAULT'. Not overwriting it:" >&2
  echo "installed copies trust its public key. Rotate deliberately (docs/63)." >&2
  exit 1
fi

cd "$(dirname "$0")"  # desktop/ (where the tauri CLI is installed)

RAM_DEV=""
cleanup() { [ -n "$RAM_DEV" ] && hdiutil detach "$RAM_DEV" >/dev/null 2>&1 || true; }
trap cleanup EXIT
[ "$(uname -s)" = "Darwin" ] || { echo "error: macOS only (uses a RAM disk)" >&2; exit 1; }
RAM_DEV="$(hdiutil attach -nomount ram://16384 | awk '{print $1}')"
diskutil erasevolume HFS+ avupdkey "$RAM_DEV" >/dev/null
DIR="/Volumes/avupdkey"

npx --no-install tauri signer generate --ci -p "" -w "$DIR/updater.key" >/dev/null

PRIVATE_KEY="$(cat "$DIR/updater.key")"
PUBLIC_KEY="$(cat "$DIR/updater.key.pub")"

# Item template on the RAM disk; fields: private key (concealed), public key (text).
python3 - "$DIR/item.json" "$OP_ITEM" "$PRIVATE_KEY" "$PUBLIC_KEY" <<'PY'
import json, sys
path, title, priv, pub = sys.argv[1:]
json.dump({
    "title": title,
    "category": "SECURE_NOTE",
    "notesPlain": "Avalanche Desktop auto-update signing key (minisign, via the Tauri "
                  "updater). Used by `make desktop-release`. See docs/63. Never "
                  "regenerate: installed copies only trust this public key.",
    "fields": [
        {"id": "private_key", "label": "private key", "type": "CONCEALED", "value": priv},
        {"id": "public_key", "label": "public key", "type": "STRING", "value": pub},
    ],
}, open(path, "w"))
PY
op item create --vault "$OP_VAULT" --template "$DIR/item.json" >/dev/null

echo "Stored '$OP_ITEM' in 1Password vault '$OP_VAULT'."
echo
echo "Public key (put this in desktop/src-tauri/tauri.conf.json → plugins.updater.pubkey):"
echo "$PUBLIC_KEY"
