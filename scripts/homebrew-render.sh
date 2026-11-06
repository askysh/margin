#!/usr/bin/env bash
#
# homebrew-render.sh: write Casks/margin.rb into a clone of askysh/homebrew-tap.
#
#   scripts/homebrew-render.sh --version 0.0.1 --dmg release/Margin-0.0.1.dmg ~/dev/homebrew-tap
#
# The cask is packaging/homebrew/margin.rb with the version and the DMG's sha256.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
die() { printf '[FAIL] %s\n' "$1" >&2; exit 1; }

VERSION="" DMG="" TAP=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --version) VERSION="$2"; shift 2 ;;
    --dmg) DMG="$2"; shift 2 ;;
    -h|--help) sed -n '2,8p' "$0"; exit 0 ;;
    -*) die "unknown option: $1" ;;
    *) TAP="$1"; shift ;;
  esac
done
[[ "$VERSION" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]] || die "--version X.Y.Z is required"
[[ -f "$DMG" ]] || die "no DMG at ${DMG:-(none given)}"
[[ -d "$TAP" ]] || die "no tap folder at ${TAP:-(none given)}"

sha="$(shasum -a 256 "$DMG" | awk '{print $1}')"
mkdir -p "$TAP/Casks"
sed -e "s/@VERSION@/${VERSION}/" -e "s/@SHA256@/${sha}/" "$ROOT/packaging/homebrew/margin.rb" > "$TAP/Casks/margin.rb"
printf '[OK] %s: margin %s, sha256 %s\n' "$TAP/Casks/margin.rb" "$VERSION" "$sha"
