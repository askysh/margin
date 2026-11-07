# Releasing Margin

Maintainers cut releases. A release is a signed, notarized and stapled `Margin.app` in a
DMG and a zip, plus the files for the in-app update.

## What the release script makes

`scripts/release.sh` builds `Margin.app` and signs it with the Developer ID (hardened
runtime and `src-tauri/entitlements.plist`). Then it notarizes and staples the app, and
writes these files to `release/`:

- `Margin-<version>.dmg` and `Margin-<version>.zip`
- the in-app update: `Margin.app.tar.gz`, its `.sig`, and `latest.json`
- `SHA256SUMS`

```bash
scripts/release.sh --check            # tools, signing identity, versions
scripts/release.sh 0.0.2 --sign-only  # signed, not sent to Apple
scripts/release.sh 0.0.2              # signed, notarized, stapled
```

`RELEASE_NOTES` sets the text that the app shows with an update. Without it, the text is a
link to the GitHub release. `CARGO_TARGET_DIR` builds in another folder, for example to
keep a running copy of the app.

## Settings

The script reads its settings from the environment, or from `~/keys/margin-signing.env`
(`$MARGIN_SIGNING_ENV` changes the path):

- `MARGIN_SIGN_IDENTITY`, `NOTARY_KEY_PATH`, `NOTARY_KEY_ID` and `NOTARY_ISSUER_ID`. These
  are the same Developer ID and App Store Connect API key that BenchBar uses.
- `MARGIN_UPDATER_KEY_PATH` and `MARGIN_UPDATER_KEY_PASSWORD_PATH`: the updater's own key,
  `~/keys/margin-updater.key`. If this key is lost, installed copies cannot verify new
  updates. Keep a copy in your password manager.

## Branches

- `next-release` is the next release. Every change goes into this branch, Dependabot's
  pull requests too.
- `main` holds the released code. Only `next-release` merges into it.
- The pull request from `next-release` into `main` stays open. Merging it releases the
  version that `next-release` has.

## Cut a release

1. Set the version on `next-release`, once per release, before or with the first change:
   ```bash
   git switch next-release
   scripts/bump-version.sh 0.0.8
   git commit -am "Version 0.0.8"
   git push
   ```
   `scripts/bump-version.sh` sets the version in `package.json`,
   `src-tauri/tauri.conf.json`, `src-tauri/Cargo.toml` and Margin's entry in
   `src-tauri/Cargo.lock`. It adds a `## [0.0.8] - Unreleased` section to
   [CHANGELOG.md](../CHANGELOG.md), and dates the section of the version before it.
2. Each change a user can see gets a line in that CHANGELOG section. The section becomes
   the release notes.
3. When the release is ready, put the date in place of `Unreleased`, and merge the pull
   request from `next-release` into `main`. Use a merge commit, not squash, so that
   `next-release` and `main` keep the same history. CI blocks the merge when the version
   already has a tag, the version files do not agree, or the CHANGELOG section is empty.
4. The merge starts `.github/workflows/release.yml`. Its plan job tags the merge commit
   `v0.0.8`. Then it builds, signs and notarizes the app on a macOS 26 runner, and drafts
   a GitHub release on that tag with all its files and the CHANGELOG section as notes.
5. Open the draft on the Releases page, check the files and publish it.
6. Set the next version on `next-release` (step 1).

The secrets of each workflow are listed at the top of its file.

### Other ways to start a release

- **Push a tag** `vX.Y.Z` by hand. The workflow builds that tag.
- **Run workflow** on the Actions page, with a version.
- **Build on your Mac** with `scripts/release.sh X.Y.Z`, and upload the files to a release
  yourself. The workflow then skips that version, because it has a release.

## After you publish

- **The in-app update.** The app reads `releases/latest/download/latest.json`. A draft is
  not "latest", so installed copies see the update only after you publish.
- **The Homebrew cask.** Publishing starts `.github/workflows/homebrew-tap.yml`. It renders
  the cask from `packaging/homebrew/margin.rb` with the DMG's sha256, installs it on a
  runner, and opens a pull request on `askysh/homebrew-tap`. That pull request merges
  itself after the tap's tests pass. Check that it merged.

## Test an update locally

1. Build two versions, an older one and a newer one.
2. Serve the newer build's update files (`Margin.app.tar.gz`, `.sig`, `latest.json`) on
   localhost.
3. Run the older build with `MARGIN_UPDATE_ENDPOINT=http://127.0.0.1:<port>/latest.json`.

The download must still verify against the key built into the app.
`MARGIN_UPDATE_AUTO_INSTALL=1` installs the update and restarts as soon as the download
finishes.
