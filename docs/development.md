# Development

How to build Margin from source, run it, test it and move it to a new minutes-core. For
releases, see [releasing.md](releasing.md).

## Requirements

- An Apple Silicon Mac with macOS 14.4 or later.
- Xcode 26 or later, for `swiftc` and the macOS 26 Speech APIs that minutes-core uses.
- cmake, bun and Rust from rustup.

```bash
brew install cmake
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
export PATH="$HOME/.cargo/bin:$PATH"
bun install
```

### The CXXFLAGS note

On macOS 26 and later, cargo cannot find the C++ standard library headers for
whisper.cpp. Set these two variables in every shell that runs cargo or `bunx tauri`:

```bash
export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
```

`scripts/build-app.sh` and `scripts/release.sh` set them for you.

## Build the app

```bash
scripts/build-app.sh              # → src-tauri/target/release/bundle/macos/Margin.app
open src-tauri/target/release/bundle/macos/Margin.app
```

A cold build takes about 2 minutes for debug on an M-series Mac. A release build takes
longer, because the release profile uses whole-program LTO (DECISIONS.md D36). The app is
35 MB.

`scripts/build-app.sh` does not sign the app with a stable identity. macOS treats each new
build as a new app, so it asks for the Microphone and System Audio permissions again
after each rebuild. To keep the permissions, sign your builds with one identity (a
self-signed one is enough), or use [releasing.md](releasing.md) with `--sign-only`.

## Run in development

```bash
bunx tauri dev                    # hot-reloading UI, debug Rust
```

The UI is React and TypeScript in `src/`, built by Vite. The backend is Rust in
`src-tauri/src/`. It calls minutes-core in-process ([research.md](research.md) §2).

## Tests

```bash
(cd src-tauri && cargo test)      # Rust unit tests
bunx tsc --noEmit                 # UI type check (CI runs this and `bun run build`)
```

CI (`.github/workflows/ci.yml`) type-checks and builds the UI on each push and pull
request. The Rust side needs a macOS runner and a long build, so CI does not build it.
Run the Rust tests and the end-to-end pass locally.

### The end-to-end pass

`scripts/e2e/run.mjs` is a QA pass that acts as a user:

- It runs the built UI in headless WebKit. Each click goes to the real backend through
  `margin --dev serve`. This is a localhost bridge in front of the same functions the app
  calls.
- It uses a sandbox `HOME` and `MARGIN_HOME`. Your settings, meetings and voices are never
  touched.
- The bridge refuses permission requests, so no macOS prompt can appear.
- In debug builds, `--simulate` plays a WAV in place of the microphone. It also plays
  `<wav>.voice.wav`, `<wav>.system.wav` and `<wav>.live.jsonl` when they exist. So the whole
  flow (record, live transcript, stop, process) runs without permissions.
- It writes screenshots and `report.json` to `$E2E_OUT`.

```bash
bun run build && (cd src-tauri && cargo build)     # build what the e2e run uses
node scripts/e2e/run.mjs --audio meeting.wav \
  --seed-models ~/Library/Application\ Support/Margin/state/models \
  --summaries fake-ollama --simulate call.wav [--download]
```

Add `--dark` to run the pass in dark mode. `scripts/e2e/fake-ollama.py` and
`scripts/e2e/fake-claude.py` stand in for Ollama and the `claude` CLI. `$E2E_OUT` is
`/tmp/margin-e2e/out` by default. `E2E_BRIDGE_PORT`, `E2E_UI_PORT`, `E2E_OLLAMA_PORT` and
`CARGO_TARGET_DIR` move a run off the default ports and build folder. The comment at the
top of `run.mjs` lists every option.

## The developer CLI

The binary has a small developer CLI for the core integration. Before you use it, point
`MARGIN_HOME` and `HOME` at a temporary directory. `HOME` is necessary because minutes-core
always keeps `voices.db` in `~/.minutes`.

```bash
export MARGIN_HOME=/tmp/margin-test HOME=/tmp/margin-test/home
M=src-tauri/target/debug/margin
$M --dev models                          # download speech models (~520 MB)
$M --dev preflight call                  # the device checks Start runs, without recording
$M --dev permissions                     # the permission state, as JSON
$M --dev process meeting.wav "Title"     # process a WAV as a meeting, in-process
$M --dev queue meeting.wav "Title" notes.md   # what Stop does: queue + worker child
$M --dev list | read <file.md>
$M --dev rename <file.md> SPEAKER_2 Priya
$M --dev summarize <file.md> claude|ollama
$M --dev ask [--meeting <file.md>] "question" ["follow-up" ...]   # Ask, with timings
$M --dev live-replay <wav> [speed] [model]    # the live transcript, from a WAV
$M --dev rolling <wav> [segment-secs] [out.md]  # rolling transcription, one source
$M --dev rolling-job <wav> [speed] [--off]      # a whole job, with and without rolling
$M --dev reset [--meetings] [--models] [--voices]
$M --dev serve [port]                    # QA bridge for scripts/e2e
```

[perf.md](perf.md) uses the same CLI to measure latency, startup and idle cost.

## Updating minutes-core

Margin uses minutes-core from upstream,
[silverstein/minutes](https://github.com/silverstein/minutes). Rolling transcription needs
the transcript checkpoint patch, which upstream merged in
[PR #1316](https://github.com/silverstein/minutes/pull/1316). No upstream release contains
it yet, so `src-tauri/Cargo.toml` pins minutes-core by commit `209265f` (upstream `main`
with the patch). When upstream tags a release that contains the patch, move to that tag.

```bash
scripts/bump-upstream.sh v0.33.0      # an upstream tag (preferred)
scripts/bump-upstream.sh <full-sha>   # or a full 40-character commit SHA
```

The script does these steps:

1. It sets the `minutes-core` line in `src-tauri/Cargo.toml` to that tag or commit.
2. It re-seeds `src-tauri/Cargo.lock` from upstream's lock at the same ref.
3. It builds Margin and runs the tests.

Step 2 is necessary. Downstream crates do not see upstream's lockfile. Without it,
minutes-core's loose `ort` range resolves to a release that breaks `pyannote-rs`.

A change that Margin needs in minutes-core goes upstream as a pull request. Each
candidate is in [UPSTREAM-PATCHES.md](../UPSTREAM-PATCHES.md): what, why and where.
