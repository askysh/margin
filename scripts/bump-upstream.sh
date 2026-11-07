#!/usr/bin/env bash
# Move Margin to another minutes-core from upstream (silverstein/minutes).
#   scripts/bump-upstream.sh v0.33.0      # a tag: the usual case
#   scripts/bump-upstream.sh <full-sha>   # a commit on upstream main that no tag has yet
#
# A tag pins with `tag = ...`. A commit pins with `rev = ...`, only after a check that it is on
# main and comes after the latest tag (DECISIONS.md, "Back on upstream minutes-core").
#
# Re-seeds src-tauri/Cargo.lock from upstream's lock at that ref. Downstream crates don't
# see upstream's lockfile, and minutes-core's caret ranges (ort, pyannote-rs) break when
# cargo resolves newer releases. The lock keeps native deps on the versions upstream ships.
set -euo pipefail
ref="${1:?usage: $0 <tag | full commit sha>}"
repo="https://github.com/silverstein/minutes"
api="https://api.github.com/repos/silverstein/minutes"
root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/src-tauri/Cargo.toml"
lock="$root/src-tauri/Cargo.lock"

# Prints "<status> <ahead_by> <behind_by>" for GitHub's compare of base...head.
compare() {
  curl -fsSL "${api}/compare/$1...$2" | sed -nE \
    -e 's/^  "status": "([a-z]+)".*/\1/p' \
    -e 's/^  "ahead_by": ([0-9]+).*/\1/p' \
    -e 's/^  "behind_by": ([0-9]+).*/\1/p' | paste -sd' ' -
}

if git ls-remote --exit-code --tags --refs "$repo" "refs/tags/${ref}" >/dev/null; then
  source_spec="git = \"${repo}\", tag = \"${ref}\""
  label="$ref"
elif [[ "$ref" =~ ^[0-9a-f]{40}$ ]]; then
  # The newest release tag; pre-releases such as v0.33.0-rc1 don't count.
  latest="$(git ls-remote --tags --refs --sort=-v:refname "$repo" 'v*' | sed 's#.*refs/tags/##' | grep -E '^v[0-9]+\.[0-9]+\.[0-9]+$' | head -1)"
  # GitHub also serves commits from forks and open pull requests: insist on main.
  read -r status _ _ <<<"$(compare "$ref" main)"
  if [[ "$status" != "ahead" && "$status" != "identical" ]]; then
    echo "${ref} is not on silverstein/minutes main (status: ${status:-unknown})." >&2
    exit 1
  fi
  read -r status ahead behind <<<"$(compare "$latest" "$ref")"
  if [[ "$status" == "identical" ]]; then
    echo "${ref} is ${latest}. Use the tag: $0 ${latest}" >&2
    exit 1
  fi
  if [[ "$status" != "ahead" ]]; then
    echo "${ref} is not ${latest} plus commits (status: ${status:-unknown}, behind: ${behind:-?})." >&2
    exit 1
  fi
  echo "silverstein/minutes@${ref} is ${latest} plus ${ahead} commit(s)."
  source_spec="git = \"${repo}\", rev = \"${ref}\""
  label="${latest} + ${ref:0:7}"
else
  echo "${ref} is not a tag on silverstein/minutes or a full 40-character commit SHA." >&2
  exit 1
fi

curl -fsSL "https://raw.githubusercontent.com/silverstein/minutes/${ref}/Cargo.lock" -o "$lock"
sed -i '' -E "s#^minutes-core = \{ git = \"[^\"]+\", (tag|rev) = \"[^\"]+\"#minutes-core = { ${source_spec}#" "$manifest"
sed -i '' -E "s#^\# Now: .*#\# Now: ${label}.#" "$manifest"
sed -i '' -E "s#upstream_tag: \"[^\"]+\"#upstream_tag: \"${label}\"#" "$root/src-tauri/src/commands.rs"
grep '^minutes-core' "$manifest"

# The ort pin in Cargo.toml must match the version in upstream's lock.
ort="$(grep -A1 '^name = "ort"$' "$lock" | sed -nE 's/^version = "([^"]+)"/\1/p' || true)"
if [[ -z "$ort" ]]; then
  echo "Upstream's lock has no ort crate. Check the ort pin in src-tauri/Cargo.toml, then build." >&2
  exit 1
fi
if ! grep -q "^ort = { version = \"=${ort}\"" "$manifest"; then
  echo "Upstream's lock has ort ${ort}. Update the ort pin in src-tauri/Cargo.toml to \"=${ort}\", then build." >&2
  exit 1
fi

export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
(cd "$root/src-tauri" && cargo build && cargo test)
echo "Now on minutes-core ${label}."
