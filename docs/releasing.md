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

## Cut a release

1. Set the same version in `package.json`, `src-tauri/tauri.conf.json` and
   `src-tauri/Cargo.toml`. The script refuses a version that does not match all three.
2. Add a `## X.Y.Z` section to [CHANGELOG.md](../CHANGELOG.md).
3. Commit, then push a tag:
   ```bash
   git tag v0.0.5
   git push origin v0.0.5
   ```
4. Wait for `.github/workflows/release.yml`. It runs the same script on a macOS 26 runner
   and drafts a GitHub release with generated notes.
5. Open the draft on the Releases page, check the files and publish it.

The secrets of each workflow are listed at the top of its file.

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
