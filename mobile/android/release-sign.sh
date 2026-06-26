#!/usr/bin/env bash
#
# Build a signed Android release APK without any signing secret ever touching
# persistent disk. The keystore and its passwords are pulled from 1Password into
# volatile storage only:
#
#   - the keystore file -> a RAM disk (macOS) or tmpfs (Linux /dev/shm)
#   - the passwords     -> environment variables of this process / Gradle
#
# When the build finishes (or fails, via the EXIT trap) the RAM disk is detached
# and the bytes are gone. `--no-daemon` ensures no lingering Gradle JVM keeps the
# credentials resident in memory after the build.
#
# Prerequisites:
#   - 1Password CLI `op` installed and signed in (`op signin`)
#   - the items below present in your vault (override names via env if different)
#   - JAVA_HOME / ANDROID_HOME set (the Makefile target passes these through)
#
# Invoked by `make android-release`; can also be run directly from mobile/android.

set -euo pipefail

# --- 1Password locations (override via env if your vault/item names differ) ----
OP_VAULT="${OP_VAULT:-Private}"
OP_KEYSTORE_DOC="${OP_KEYSTORE_DOC:-Avalanche Android release keystore}"
OP_CREDS_ITEM="${OP_CREDS_ITEM:-Avalanche Android keystore credentials}"
OP_STORE_PW_REF="${OP_STORE_PW_REF:-op://$OP_VAULT/$OP_CREDS_ITEM/storePassword}"
OP_KEY_PW_REF="${OP_KEY_PW_REF:-op://$OP_VAULT/$OP_CREDS_ITEM/keyPassword}"
OP_KEY_ALIAS_REF="${OP_KEY_ALIAS_REF:-op://$OP_VAULT/$OP_CREDS_ITEM/keyAlias}"

command -v op >/dev/null 2>&1 || { echo "error: 1Password CLI (op) not found on PATH" >&2; exit 1; }
op account list >/dev/null 2>&1 || { echo "error: not signed in to 1Password — run: op signin" >&2; exit 1; }

cd "$(dirname "$0")"  # mobile/android (where ./gradlew lives)

# --- Allocate volatile storage for the keystore file --------------------------
RAM_DEV=""
cleanup() {
  # Detach the RAM disk / remove the tmpfs file. Runs on any exit.
  if [ -n "$RAM_DEV" ]; then
    hdiutil detach "$RAM_DEV" >/dev/null 2>&1 || true
  elif [ -n "${KEYSTORE:-}" ] && [ -f "$KEYSTORE" ]; then
    rm -f "$KEYSTORE"
  fi
}
trap cleanup EXIT

case "$(uname -s)" in
  Darwin)
    # 16384 * 512-byte sectors = 8 MB; ample for a keystore, tiny in RAM.
    # hdiutil pads the device node with trailing spaces — awk strips it, else
    # diskutil can't match the disk ("Unable to find disk for /dev/diskN").
    RAM_DEV="$(hdiutil attach -nomount ram://16384 | awk '{print $1}')"
    diskutil erasevolume HFS+ avsign "$RAM_DEV" >/dev/null
    KEYSTORE="/Volumes/avsign/release.jks"
    ;;
  Linux)
    [ -d /dev/shm ] || { echo "error: /dev/shm (tmpfs) not available" >&2; exit 1; }
    KEYSTORE="$(mktemp /dev/shm/release.XXXXXX.jks)"
    ;;
  *)
    echo "error: unsupported platform for in-memory signing ($(uname -s))" >&2
    exit 1
    ;;
esac

# --- Materialize secrets into RAM / environment only --------------------------
op document get "$OP_KEYSTORE_DOC" --vault "$OP_VAULT" --out-file "$KEYSTORE"
export RELEASE_KEYSTORE_FILE="$KEYSTORE"
export RELEASE_KEYSTORE_PASSWORD="$(op read "$OP_STORE_PW_REF")"
export RELEASE_KEY_PASSWORD="$(op read "$OP_KEY_PW_REF")"
export RELEASE_KEY_ALIAS="$(op read "$OP_KEY_ALIAS_REF")"

# --- Build (env vars consumed by app/build.gradle.kts signingConfig) ----------
# --no-daemon: don't leave a JVM holding the credentials in memory afterward.
./gradlew --no-daemon assembleRelease \
  -PMARKETING_VERSION="${MARKETING_VERSION:-}" \
  -PCURRENT_PROJECT_VERSION="${CURRENT_PROJECT_VERSION:-}"

echo
echo "Signed release APK:"
echo "  mobile/android/app/build/outputs/apk/release/app-release.apk"
