#!/usr/bin/env bash
#
# Build, sign, notarize, and publish Avalanche Desktop for macOS (docs/63), then
# attach it to the tag's GitHub Release next to the server artifacts. Runs on the
# maintainer's Mac, like `make android-release`: every secret comes from
# 1Password into a RAM disk / environment variables for this run only.
#
#   make desktop-release            # on the release tag (HEAD must be tagged vX.Y.Z)
#   desktop/release.sh --local-test # build + updater artifacts only: unsigned, throwaway
#                                   # updater key, no notarization, no upload
#
# Produces and uploads:
#   Avalanche-<ver>-macos-arm64.dmg                 first-time download (macOS)
#   Avalanche-macos-arm64.app.tar.gz (+ .sig)       macOS update package
#   *.sig for the Windows/Linux installers          CI builds those (release.yml
#                                                   desktop-build) unsigned; this
#                                                   script signs them locally
#   latest.json                                     update manifest, all platforms (docs/63)
#
# Prerequisites (release mode):
#   - 1Password CLI `op` (all secrets come from ONE `op run`: one approval), with:
#       "Avalanche Desktop updater signing key"   (desktop/updater-keygen.sh)
#       "Avalanche App Store Connect API key"     document holding AuthKey_XXXX.p8 (OP_ASC_KEY_FILE)
#       "Avalanche notarization"                  fields: key id, issuer id
#   - a "Developer ID Application" certificate in the login Keychain
#   - `gh` signed in; the tag's GitHub Release exists (the release workflow
#     creates it as a draft on tag push)
#   - the updater public key filled in to src-tauri/tauri.conf.json

set -euo pipefail

LOCAL_TEST=0
[ "${1:-}" = "--local-test" ] && LOCAL_TEST=1

OP_VAULT="${OP_VAULT:-Private}"
OP_UPDATER_ITEM="${OP_UPDATER_ITEM:-Avalanche Desktop updater signing key}"
OP_ASC_KEY_DOC="${OP_ASC_KEY_DOC:-Avalanche App Store Connect API key}"
OP_NOTARY_ITEM="${OP_NOTARY_ITEM:-Avalanche notarization}"
OP_ASC_KEY_FILE="${OP_ASC_KEY_FILE:-AuthKey_7U6GFQU793.p8}"   # the file inside that document
REPO="${REPO:-lincolnq/avalanche}"
TARGET="aarch64-apple-darwin"     # macOS: Apple Silicon only for now (docs/63)

# --- One 1Password round-trip --------------------------------------------------
# Every `op` invocation is its own macOS "iTerm2 would like to access data from
# other apps" prompt (op talks to the 1Password app), plus a 1Password approval.
# So resolve ALL secrets in one `op run`: re-launch this script under it with an
# env file of op:// REFERENCES (no secret values), and the child gets the values
# in its environment only.
if [ "$LOCAL_TEST" = 0 ] && [ -z "${AV_OP_RESOLVED:-}" ]; then
  command -v op >/dev/null || { echo "error: 1Password CLI (op) not found" >&2; exit 1; }
  REFS="$(mktemp -t avrelease-refs)"
  cat > "$REFS" <<EOF
AV_UPDATER_KEY="op://$OP_VAULT/$OP_UPDATER_ITEM/private key"
AV_ASC_P8="op://$OP_VAULT/$OP_ASC_KEY_DOC/$OP_ASC_KEY_FILE"
AV_ASC_KEY_ID="op://$OP_VAULT/$OP_NOTARY_ITEM/key id"
AV_ASC_ISSUER="op://$OP_VAULT/$OP_NOTARY_ITEM/issuer id"
EOF
  exec op run --env-file "$REFS" -- env AV_OP_RESOLVED=1 AV_REFS_FILE="$REFS" "$0" "$@"
fi
[ -n "${AV_REFS_FILE:-}" ] && rm -f "$AV_REFS_FILE"

cd "$(dirname "$0")"   # desktop/
CONF="src-tauri/tauri.conf.json"

# --- Version: the release tag ---------------------------------------------------
if [ "$LOCAL_TEST" = 1 ]; then
  TAG="${TAG:-$(git describe --tags --abbrev=0)}"
else
  TAG="${TAG:-$(git describe --tags --exact-match 2>/dev/null || true)}"
  [ -n "$TAG" ] || { echo "error: HEAD isn't tagged. Check out the release tag (vX.Y.Z) first." >&2; exit 1; }
  [ -z "$(git status --porcelain)" ] || { echo "error: working tree has changes; build releases from a clean tag." >&2; exit 1; }
fi
VERSION="${TAG#v}"
echo "Building Avalanche Desktop $VERSION ($TARGET)"

[ "$(uname -s)" = "Darwin" ] || { echo "error: macOS only" >&2; exit 1; }
rustup target list --installed | grep -qx "$TARGET" || { echo "error: rustup target add $TARGET" >&2; exit 1; }

# --- Volatile storage for key files ---------------------------------------------
RAM_DEV=""
cleanup() { [ -n "$RAM_DEV" ] && hdiutil detach "$RAM_DEV" >/dev/null 2>&1 || true; }
trap cleanup EXIT
RAM_DEV="$(hdiutil attach -nomount ram://16384 | awk '{print $1}')"
diskutil erasevolume HFS+ avrelease "$RAM_DEV" >/dev/null
SECRETS="/Volumes/avrelease"

# --- Updater signing key + Apple credentials --------------------------------------
CONFIG_OVERRIDE="{\"version\":\"$VERSION\"}"
if [ "$LOCAL_TEST" = 1 ]; then
  # Throwaway updater key so the update package can be signed; nothing is trusted.
  npx --no-install tauri signer generate --ci -p "" -w "$SECRETS/test.key" >/dev/null
  export TAURI_SIGNING_PRIVATE_KEY="$(cat "$SECRETS/test.key")"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
  PUB="$(cat "$SECRETS/test.key.pub")"
  CONFIG_OVERRIDE="{\"version\":\"$VERSION\",\"plugins\":{\"updater\":{\"pubkey\":\"$PUB\"}},\"bundle\":{\"macOS\":{\"signingIdentity\":\"-\"}}}"
else
  grep -q "REPLACE_WITH_UPDATER_PUBLIC_KEY" "$CONF" && {
    echo "error: set plugins.updater.pubkey in $CONF (run desktop/updater-keygen.sh)" >&2; exit 1; }
  command -v gh >/dev/null && gh release view "$TAG" -R "$REPO" >/dev/null 2>&1 || {
    echo "error: no GitHub Release for $TAG (push the tag; the release workflow creates it)" >&2; exit 1; }
  IDENTITY="$(security find-identity -v -p codesigning | grep -o '"Developer ID Application:[^"]*"' | head -1 | tr -d '"')"
  [ -n "$IDENTITY" ] || { echo "error: no 'Developer ID Application' certificate in the Keychain" >&2; exit 1; }

  # Values resolved by the single `op run` above (AV_*).
  [ -n "${AV_UPDATER_KEY:-}" ] && [ -n "${AV_ASC_P8:-}" ] || {
    echo "error: secrets didn't resolve from 1Password (check the item names above)" >&2; exit 1; }
  export TAURI_SIGNING_PRIVATE_KEY="$AV_UPDATER_KEY"
  export TAURI_SIGNING_PRIVATE_KEY_PASSWORD=""
  # Tauri signs with APPLE_SIGNING_IDENTITY and notarizes with the App Store
  # Connect API key (APPLE_API_KEY / APPLE_API_ISSUER / APPLE_API_KEY_PATH).
  export APPLE_SIGNING_IDENTITY="$IDENTITY"
  printf '%s\n' "$AV_ASC_P8" > "$SECRETS/AuthKey.p8"
  export APPLE_API_KEY_PATH="$SECRETS/AuthKey.p8"
  export APPLE_API_KEY="$AV_ASC_KEY_ID"
  export APPLE_API_ISSUER="$AV_ASC_ISSUER"
  unset AV_UPDATER_KEY AV_ASC_P8
fi

# --- Build ----------------------------------------------------------------------
npx --no-install tauri build --target "$TARGET" --bundles app,dmg --config "$CONFIG_OVERRIDE"

BUNDLE="src-tauri/target/$TARGET/release/bundle"
APP_TGZ="$BUNDLE/macos/Avalanche.app.tar.gz"
DMG="$(ls "$BUNDLE"/dmg/*.dmg | head -1)"
[ -f "$APP_TGZ" ] && [ -f "$APP_TGZ.sig" ] && [ -f "$DMG" ] || {
  echo "error: expected build outputs missing under $BUNDLE" >&2; exit 1; }

# --- Stage assets with stable names ----------------------------------------------
OUT="../dist/desktop-$VERSION"
rm -rf "$OUT" && mkdir -p "$OUT"
cp "$DMG" "$OUT/Avalanche-$VERSION-macos-arm64.dmg"
cp "$APP_TGZ" "$OUT/Avalanche-macos-arm64.app.tar.gz"
cp "$APP_TGZ.sig" "$OUT/Avalanche-macos-arm64.app.tar.gz.sig"

# --- Sign CI's Windows/Linux installers with the same key --------------------------
# platform id → installer the updater downloads on that platform.
WIN="Avalanche-$VERSION-windows-x64-setup.exe"
LINUX="Avalanche-$VERSION-linux-x86_64.AppImage"
if [ "$LOCAL_TEST" = 0 ]; then
  for f in "$WIN" "$LINUX"; do
    if gh release download "$TAG" -R "$REPO" -p "$f" -D "$OUT" --clobber 2>/dev/null; then
      # Key and (empty) password come from the environment, never argv. Note
      # `tauri signer sign` reads the older TAURI_PRIVATE_KEY* names, unlike
      # `tauri build` (TAURI_SIGNING_PRIVATE_KEY*).
      TAURI_PRIVATE_KEY="$TAURI_SIGNING_PRIVATE_KEY" TAURI_PRIVATE_KEY_PASSWORD="" \
        npx --no-install tauri signer sign "$OUT/$f" >/dev/null
      echo "signed $f"
    else
      echo "warning: $f isn't on the $TAG release (did CI's desktop-build fail?); leaving that platform out of latest.json" >&2
    fi
  done
fi

# --- Update manifest ------------------------------------------------------------
python3 - "$OUT" "$VERSION" "$TAG" "$REPO" "$WIN" "$LINUX" <<'PY'
import json, os, sys, datetime
out, version, tag, repo, win, linux = sys.argv[1:]
url = lambda name: f"https://github.com/{repo}/releases/download/{tag}/{name}"
candidates = {
    "darwin-aarch64": "Avalanche-macos-arm64.app.tar.gz",
    "windows-x86_64": win,
    "linux-x86_64": linux,
}
platforms = {}
for platform, name in candidates.items():
    sig = os.path.join(out, name + ".sig")
    if os.path.exists(sig):
        platforms[platform] = {"signature": open(sig).read().strip(), "url": url(name)}
json.dump({
    "version": version,
    "notes": f"Avalanche {version}",
    "pub_date": datetime.datetime.now(datetime.timezone.utc).strftime("%Y-%m-%dT%H:%M:%SZ"),
    "platforms": platforms,
}, open(os.path.join(out, "latest.json"), "w"), indent=2)
print("latest.json platforms:", ", ".join(platforms))
PY
echo
echo "Staged in dist/desktop-$VERSION:"
ls -1 "$OUT"

if [ "$LOCAL_TEST" = 1 ]; then
  echo "(local test: unsigned, throwaway updater key — not uploaded)"
  exit 0
fi

# --- Upload to the tag's GitHub Release -------------------------------------------
# The macOS files, every .sig (incl. the Windows/Linux ones), and the manifest.
# The Windows/Linux installers themselves are already on the release (from CI).
gh release upload "$TAG" -R "$REPO" --clobber \
  "$OUT/Avalanche-$VERSION-macos-arm64.dmg" \
  "$OUT/Avalanche-macos-arm64.app.tar.gz" \
  "$OUT"/*.sig \
  "$OUT/latest.json"
echo
echo "Uploaded to $TAG. Installed copies see it once the release is published (docs/63)."
