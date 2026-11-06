#!/usr/bin/env bash
# Move Margin to a new minutes-core tag.
#   scripts/bump-upstream.sh v0.33.0 <fork-commit>   # the askysh/minutes fork, with Margin's patch
#   scripts/bump-upstream.sh v0.33.0                 # upstream itself, once it has the patch
#
# Margin uses minutes-core from askysh/minutes: the tag plus one patch (transcript
# checkpoints, for rolling transcription), pinned by commit while upstream reviews it
# (DECISIONS.md, "A minutes-core fork with one patch"). Before running this, rebase the
# patch onto the new tag in the fork and push it (README, "Updating minutes-core").
#
# Re-seeds src-tauri/Cargo.lock from upstream's lock at that tag. Downstream crates don't
# see upstream's lockfile, and minutes-core's caret ranges (ort, pyannote-rs) break when
# cargo resolves newer releases. The lock keeps native deps on the versions upstream ships.
# The patch adds no dependency, so upstream's lock fits the fork too.
set -euo pipefail
tag="${1:?usage: $0 <tag> [fork-commit]}"
rev="${2:-}"
root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/src-tauri/Cargo.toml"

if [[ -n "$rev" ]]; then
  # The fork commit must be the tag plus Margin's patch, not some other base.
  compare="$(curl -fsSL "https://api.github.com/repos/silverstein/minutes/compare/${tag}...askysh:minutes:${rev}")"
  status="$(sed -nE 's/^  "status": "([a-z]+)".*/\1/p' <<<"$compare")"
  behind="$(sed -nE 's/^  "behind_by": ([0-9]+).*/\1/p' <<<"$compare")"
  ahead="$(sed -nE 's/^  "ahead_by": ([0-9]+).*/\1/p' <<<"$compare")"
  if [[ "$status" != "ahead" || "$behind" != "0" ]]; then
    echo "askysh/minutes@${rev} is not ${tag} plus commits (status: ${status:-unknown}, behind: ${behind:-?})." >&2
    exit 1
  fi
  echo "askysh/minutes@${rev} is ${tag} plus ${ahead} commit(s)."
  source_spec="git = \"https://github.com/askysh/minutes\", rev = \"${rev}\""
else
  source_spec="git = \"https://github.com/silverstein/minutes\", tag = \"${tag}\""
fi

curl -fsSL "https://raw.githubusercontent.com/silverstein/minutes/${tag}/Cargo.lock" -o "$root/src-tauri/Cargo.lock"
sed -i '' -E "s#^minutes-core = \{ git = \"[^\"]+\", (tag|rev) = \"[^\"]+\"#minutes-core = { ${source_spec}#" "$manifest"
sed -i '' -E "s#minutes-core v[0-9][^ ]* plus one patch#minutes-core ${tag} plus one patch#" "$manifest"
sed -i '' -E "s#upstream_tag: \"[^\"]+\"#upstream_tag: \"${tag}\"#" "$root/src-tauri/src/commands.rs"
grep '^minutes-core' "$manifest"
grep -A1 'name = "ort"' "$root/src-tauri/Cargo.lock" | tail -1

export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
(cd "$root/src-tauri" && cargo build && cargo test)
echo "Now on ${tag}${rev:+ + patch (askysh/minutes@${rev})}. If ort's version above changed, update the ort pin in src-tauri/Cargo.toml."
if [[ -z "$rev" ]]; then
  echo "Back on upstream: update the minutes-core comment in src-tauri/Cargo.toml and DECISIONS.md."
fi
