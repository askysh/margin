#!/usr/bin/env bash
#
# release.sh: build Margin.app with a Developer ID signature, notarize it with
# Apple, staple the tickets, and package a zip and a DMG for a GitHub release.
#
#   scripts/release.sh 0.0.1              sign, notarize, staple, package
#   scripts/release.sh 0.0.1 --sign-only  sign and package, no notarization
#   scripts/release.sh --check            check the tools and settings only
#
# Settings, from the environment or from $MARGIN_SIGNING_ENV
# (default ~/keys/margin-signing.env, a file of `export NAME=value` lines):
#   MARGIN_SIGN_IDENTITY  "Developer ID Application: Name (TEAMID)", in the keychain
#   NOTARY_KEY_PATH       App Store Connect API key (.p8)
#   NOTARY_KEY_ID         the key's ID
#   NOTARY_ISSUER_ID      the issuer ID from App Store Connect, Users and Access, Integrations
#
# Output in dist/: Margin-<version>.zip, Margin-<version>.dmg, SHA256SUMS.
# The version must match package.json, src-tauri/tauri.conf.json and
# src-tauri/Cargo.toml.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
DIST="${ROOT}/dist"
APP="${ROOT}/src-tauri/target/release/bundle/macos/Margin.app"
VERSION=""
NOTARIZE=1
CHECK_ONLY=0

die() { printf '[FAIL] %s\n' "$1" >&2; [[ -n "${2:-}" ]] && printf '  fix: %s\n' "$2" >&2; exit 1; }
step() { printf '\n==> %s\n' "$1"; }
ok() { printf '  [OK] %s\n' "$1"; }

for arg in "$@"; do
  case "$arg" in
    --sign-only) NOTARIZE=0 ;;
    --check) CHECK_ONLY=1 ;;
    -h|--help) sed -n '2,21p' "$0"; exit 0 ;;
    -*) die "unknown option: $arg" ;;
    *) VERSION="$arg" ;;
  esac
done

SIGNING_ENV="${MARGIN_SIGNING_ENV:-$HOME/keys/margin-signing.env}"
# shellcheck disable=SC1090
[[ -f "$SIGNING_ENV" ]] && source "$SIGNING_ENV"

# ------------------------------------------------------------------ checks
step "tools and settings"
for tool in xcrun codesign ditto hdiutil shasum bun; do
  command -v "$tool" >/dev/null || die "missing tool: $tool"
done
missing=()
need=(MARGIN_SIGN_IDENTITY)
[[ "$NOTARIZE" == "1" ]] && need+=(NOTARY_KEY_PATH NOTARY_KEY_ID NOTARY_ISSUER_ID)
for var in "${need[@]}"; do
  [[ -n "${!var:-}" ]] || missing+=("$var")
done
[[ "${#missing[@]}" == "0" ]] || die "missing settings: ${missing[*]}" "set them in $SIGNING_ENV (see the header of this script)"
security find-identity -v -p codesigning | grep -qF "$MARGIN_SIGN_IDENTITY" \
  || die "signing identity not in the keychain: $MARGIN_SIGN_IDENTITY" "import the Developer ID .p12 into the login keychain"
[[ "$NOTARIZE" == "0" || -f "$NOTARY_KEY_PATH" ]] || die "no API key at $NOTARY_KEY_PATH"
ok "signing identity present"

pkg_version="$(sed -n 's/^  "version": "\([^"]*\)".*/\1/p' "$ROOT/package.json" | head -n 1)"
conf_version="$(sed -n 's/^  "version": "\([^"]*\)".*/\1/p' "$ROOT/src-tauri/tauri.conf.json" | head -n 1)"
cargo_version="$(sed -n 's/^version = "\([^"]*\)"$/\1/p' "$ROOT/src-tauri/Cargo.toml" | head -n 1)"
[[ -n "$VERSION" ]] || VERSION="$conf_version"
for v in "$pkg_version" "$conf_version" "$cargo_version"; do
  [[ "$v" == "$VERSION" ]] || die "version mismatch: asked for $VERSION; package.json $pkg_version, tauri.conf.json $conf_version, Cargo.toml $cargo_version" \
    "set the same version in all three files and commit"
done
ok "version $VERSION"
[[ "$CHECK_ONLY" == "1" ]] && { printf '\n[OK] ready to release %s\n' "$VERSION"; exit 0; }

# notarytool exits 0 when Apple answers Invalid, so the status line decides.
notarize() {
  local out id
  out="$(xcrun notarytool submit "$1" --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" \
    --issuer "$NOTARY_ISSUER_ID" --wait --timeout "${NOTARY_TIMEOUT:-50m}" 2>&1 | tee /dev/stderr)" \
    || die "notarytool failed for $1" "if it timed out: xcrun notarytool wait ID, then run this again"
  if ! grep -q '^[[:space:]]*status: Accepted' <<<"$out"; then
    id="$(sed -n 's/^[[:space:]]*id: //p' <<<"$out" | head -n 1)"
    [[ -n "$id" ]] && xcrun notarytool log "$id" --key "$NOTARY_KEY_PATH" --key-id "$NOTARY_KEY_ID" --issuer "$NOTARY_ISSUER_ID" || true
    die "Apple did not accept $1" "read the log above; each issue names a binary"
  fi
}

# ------------------------------------------------------------------- build
step "build and sign (hardened runtime)"
export PATH="$HOME/.cargo/bin:$PATH"
sdk="$(xcrun --show-sdk-path)"
export CXXFLAGS="-I${sdk}/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
# The Tauri bundler signs every binary with the hardened runtime and
# src-tauri/entitlements.plist when APPLE_SIGNING_IDENTITY is set. The notary
# variables are left unset here so that this script, not the bundler, notarizes.
(
  cd "$ROOT"
  unset APPLE_API_KEY APPLE_API_ISSUER APPLE_API_KEY_PATH APPLE_ID APPLE_PASSWORD
  bun install --frozen-lockfile
  APPLE_SIGNING_IDENTITY="$MARGIN_SIGN_IDENTITY" bunx tauri build --bundles app
)
codesign --verify --deep --strict --verbose=2 "$APP"
codesign -d --entitlements - "$APP" 2>/dev/null | grep -q "com.apple.security.device.audio-input" \
  || die "the app is signed without the microphone entitlement"
codesign -d --verbose=2 "$APP" 2>&1 | grep -q "flags=.*runtime" || die "the app is signed without the hardened runtime"
ok "signed: $(codesign -d --verbose=2 "$APP" 2>&1 | sed -n 's/^Authority=//p' | head -n 1)"

rm -rf "$DIST"
mkdir -p "$DIST"
ZIP="${DIST}/Margin-${VERSION}.zip"
DMG="${DIST}/Margin-${VERSION}.dmg"

# --------------------------------------------------------- notarize the app
if [[ "$NOTARIZE" == "1" ]]; then
  step "notarize the app"
  ditto -c -k --keepParent "$APP" "$ZIP"
  notarize "$ZIP"
  xcrun stapler staple "$APP"
  rm -f "$ZIP"
  spctl --assess --type execute --verbose=2 "$APP"
  ok "app notarized and stapled"
fi
ditto -c -k --keepParent "$APP" "$ZIP"

# ------------------------------------------------------------------- DMG
step "disk image"
stage="$(mktemp -d)"
trap 'rm -rf "$stage"' EXIT
ditto "$APP" "${stage}/Margin.app"
ln -s /Applications "${stage}/Applications"
hdiutil create -volname "Margin ${VERSION}" -srcfolder "$stage" -ov -format UDZO "$DMG" >/dev/null
codesign --sign "$MARGIN_SIGN_IDENTITY" --timestamp "$DMG"
if [[ "$NOTARIZE" == "1" ]]; then
  notarize "$DMG"
  xcrun stapler staple "$DMG"
fi
ok "dmg ready"

(cd "$DIST" && shasum -a 256 "Margin-${VERSION}.zip" "Margin-${VERSION}.dmg" > SHA256SUMS)
step "done"
ls -lh "$DIST"
cat "$DIST/SHA256SUMS"
