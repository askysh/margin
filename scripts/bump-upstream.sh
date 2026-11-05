#!/usr/bin/env bash
# Move Margin to a new minutes-core tag.
#   scripts/bump-upstream.sh v0.33.0
# Re-seeds src-tauri/Cargo.lock from upstream's lock at that tag. Downstream crates don't
# see upstream's lockfile, and minutes-core's caret ranges (ort, pyannote-rs) break when
# cargo resolves newer releases. The lock keeps native deps on the versions upstream ships.
set -euo pipefail
tag="${1:?usage: $0 <tag>}"
root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/src-tauri/Cargo.toml"

curl -fsSL "https://raw.githubusercontent.com/silverstein/minutes/${tag}/Cargo.lock" -o "$root/src-tauri/Cargo.lock"
sed -i '' -E "s#(minutes-core = \{ git = \"https://github.com/silverstein/minutes\", tag = \")[^\"]+#\1${tag}#" "$manifest"
sed -i '' -E "s#upstream_tag: \"[^\"]+\"#upstream_tag: \"${tag}\"#" "$root/src-tauri/src/commands.rs"
grep -A1 'name = "ort"' "$root/src-tauri/Cargo.lock" | tail -1

export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
(cd "$root/src-tauri" && cargo build && cargo test)
echo "Now bumped to ${tag}. If ort's version above changed, update the ort pin in src-tauri/Cargo.toml."
