#!/bin/bash
# Sets the Margin version in every file that holds it, and opens a CHANGELOG section for it.
#
#   scripts/bump-version.sh 0.0.6
#
# The files: package.json, src-tauri/tauri.conf.json, src-tauri/Cargo.toml, and Margin's
# own entry in src-tauri/Cargo.lock (only that entry, as cargo itself would write it).
# CHANGELOG.md gets a "## [X.Y.Z] - Unreleased" section on top, if it has none. The section
# of the version before it, which is released now, gets today's date if it has none.
#
# Run it on next-release once per release (docs/releasing.md). When next-release merges
# into main, .github/workflows/release.yml tags the version and drafts the release.
# It runs on macOS and on Linux.
set -euo pipefail

cd "$(dirname "$0")/.."

version="${1:-}"
if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "usage: scripts/bump-version.sh X.Y.Z" >&2
  exit 2
fi

current="$(sed -nE 's/^  "version": "([^"]+)",$/\1/p' src-tauri/tauri.conf.json | head -1)"
if [[ -z "$current" ]]; then
  echo "Cannot read the version in src-tauri/tauri.conf.json." >&2
  exit 1
fi

# One "version" line at the top level of each JSON file, two spaces in. sed -i is not the
# same on macOS and Linux, so each edit writes a new file.
for f in package.json src-tauri/tauri.conf.json; do
  sed -E "s/^  \"version\": \"[^\"]+\",$/  \"version\": \"$version\",/" "$f" > "$f.new" && mv "$f.new" "$f"
done
# The [package] version is the first version line in Cargo.toml.
awk -v v="$version" '!done && /^version = "/ { print "version = \"" v "\""; done = 1; next } { print }' \
  src-tauri/Cargo.toml > src-tauri/Cargo.toml.new && mv src-tauri/Cargo.toml.new src-tauri/Cargo.toml
# In Cargo.lock, only the line after name = "margin".
awk -v v="$version" 'prev == "name = \"margin\"" && /^version = "/ { print "version = \"" v "\""; prev = ""; next } { print; prev = $0 }' \
  src-tauri/Cargo.lock > src-tauri/Cargo.lock.new && mv src-tauri/Cargo.lock.new src-tauri/Cargo.lock

for f in package.json src-tauri/tauri.conf.json src-tauri/Cargo.toml; do
  grep -q "\"$version\"" "$f" || { echo "$f does not have $version now. Check it." >&2; exit 1; }
done
grep -A1 '^name = "margin"$' src-tauri/Cargo.lock | grep -q "\"$version\"" \
  || { echo "src-tauri/Cargo.lock does not have $version for margin now. Check it." >&2; exit 1; }

today="$(date +%Y-%m-%d)"
sed "s/^## \[$current\] - Unreleased$/## [$current] - $today/" CHANGELOG.md > CHANGELOG.md.new && mv CHANGELOG.md.new CHANGELOG.md
if ! grep -q "^## \[$version\]" CHANGELOG.md; then
  awk -v v="$version" '!done && /^## \[/ { print "## [" v "] - Unreleased\n"; done = 1 } { print }' \
    CHANGELOG.md > CHANGELOG.md.new && mv CHANGELOG.md.new CHANGELOG.md
fi

echo "Version $current -> $version. Check the diff, then commit:"
echo "  git commit -am \"Version $version\""
