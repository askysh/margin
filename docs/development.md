# Development

How to build Margin from source, run it, test it and move it to a new minutes-core. For
releases, see [releasing.md](releasing.md).

## Requirements

The app is for macOS. The Windows build has its own section below, [Windows](#windows).

- An Apple Silicon Mac with macOS 14.4 or later.
- Xcode 26 or later, for `swiftc` and the macOS 26 Speech APIs that minutes-core uses.
  `build.rs` also compiles `src-tauri/speech/margin-speech.swift`, the Apple Speech live
  transcript helper, into `src-tauri/binaries/` (D56).
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
bun test                          # UI unit tests (src/*.test.ts)
bunx tsc --noEmit                 # UI type check (CI runs this and `bun run build`)
```

The UI tests use bun's own test runner. `tsconfig.json` leaves them out of the type
check, because `bun:test` has no types in this repository.

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

`scripts/e2e/make-fixtures.sh <out-dir>` makes the test audio with the macOS voices (`say`
and `afconvert`). The same command makes the same files on each Mac:

- `meeting.wav`: 29 s, two voices. It says "legal review", which the search check finds.
- `call.wav`: a 65 s call with `call.voice.wav`, `call.system.wav` and `call.live.jsonl`.
  The speech stops after 25 s. The silence after it lets the test press Stop.
- `models/`: links to `ggml-small.bin`, `ggml-silero-v6.2.0.bin` and `diarization/` in
  your Margin install, if they exist. Links use no disk space. Give this folder to
  `--seed-models` to skip the download.

```bash
bun run build && (cd src-tauri && cargo build)     # build what the e2e run uses
scripts/e2e/make-fixtures.sh /tmp/margin-fixtures
F=/tmp/margin-fixtures
node scripts/e2e/run.mjs --audio $F/meeting.wav --seed-models $F/models \
  --summaries fake-ollama --simulate $F/call.wav [--download]
```

Add `--dark` to run the pass in dark mode. `scripts/e2e/fake-ollama.py` and
`scripts/e2e/fake-claude.py` stand in for Ollama and the `claude` CLI. `$E2E_OUT` is
`/tmp/margin-e2e/out` by default. `E2E_BRIDGE_PORT`, `E2E_UI_PORT`, `E2E_OLLAMA_PORT` and
`CARGO_TARGET_DIR` move a run off the default ports and build folder. The comment at the
top of `run.mjs` lists every option.

### README screenshots

`scripts/e2e/screenshots.mjs` makes the images in `docs/images/`. It uses the same sandbox
and bridge as the end-to-end pass, adds a demo library (`scripts/e2e/demo-meetings.mjs`)
and takes each image at the default window size (1180 × 760, at 2×), in light and dark:

- `screenshot-light.png` and `screenshot-dark.png`: an open meeting with notes and a
  transcript with two named speakers.
- `settings-light.png` and `settings-dark.png`: Settings → Recording.

```bash
node scripts/e2e/screenshots.mjs --seed-models /tmp/margin-fixtures/models
```

If `pngquant` is installed, the script makes the images smaller.

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

## Windows

The Windows build is on the `windows-port` branch (DECISIONS.md D61). It records a meeting
in the room from the microphone, transcribes it and writes the meeting file. It does not
record a call, and it has no recording chip, no Open at login, no Apple Speech and no
updater of its own. Whisper runs on the CPU.

### Requirements

- Windows 10 22H2 or later, x64.
- Rust from rustup, with the `stable-x86_64-pc-windows-msvc` toolchain.
- Visual Studio with the "Desktop development with C++" workload.
- CMake and LLVM. whisper.cpp needs CMake, and bindgen needs `libclang.dll`.
- bun, and Git for Windows. The scripts run in Git Bash.

### Build

```bash
scripts/build-windows.sh            # src-tauri/target/release/margin.exe
scripts/build-windows.sh --debug    # a debug build with the UI built in
```

The script adds CMake and LLVM to the PATH, sets `LIBCLANG_PATH`, builds, and puts the
DLLs that `margin.exe` needs beside it. For a plain `cargo build` or `cargo test`, set
the same variables yourself:

```bash
export PATH="/c/Program Files/CMake/bin:/c/Program Files/LLVM/bin:$PATH"
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
```

A debug build from `cargo build` loads the UI from `http://localhost:1420`. Run
`bun run dev` beside it, or use `scripts/build-windows.sh --debug`.

### The ort-sys note

`ort-sys` 2.0.0-rc.10 copies `DirectML.dll` to the third folder above its `OUT_DIR`, and
to `examples` and `deps` in that folder. Before cargo 1.100 that folder was
`target/<profile>`, next to `margin.exe`. Cargo 1.100 changed the layout of the build
folder, and now it is `target/<profile>/build`. Two things follow:

- When Windows lets the build script make links (Developer Mode, or a shell with
  administrator rights), the script stops with an error at `build.rs:126`: the two
  folders do not exist. Make them first:
  `mkdir -p src-tauri/target/debug/build/examples src-tauri/target/debug/build/deps`.
- In every case `DirectML.dll` is in `target/<profile>/build`, not next to `margin.exe`.
  A bare `margin.exe` then loads the older `DirectML.dll` of Windows.

`scripts/build-windows.sh` does both steps. The `ort` pin stays (AGENTS.md).

### The DLLs beside margin.exe

| DLL | From | Why |
|---|---|---|
| `DirectML.dll` | the ONNX Runtime download of `ort-sys` | ONNX Runtime is linked into `margin.exe` and imports it. |
| `msvcp140.dll`, `msvcp140_1.dll`, `vcruntime140.dll`, `vcruntime140_1.dll` | Visual Studio, `VC\Redist\MSVC\<version>\x64\Microsoft.VC*.CRT` | whisper.cpp and ONNX Runtime use the Visual C++ runtime. A PC without it gives error 0xC0000135 and no window. |

ONNX Runtime and the WebView2 loader are linked into `margin.exe`. They need no DLL.

### Tests and the sandbox

On Windows the profile folder does not follow `HOME`. Set `USERPROFILE` too, and
`APPDATA` and `LOCALAPPDATA`, so that every folder Windows gives is in the sandbox:

```bash
S="$(cygpath -w /tmp/margin-test)"
export MARGIN_HOME="$S\\margin" HOME="$S\\home" USERPROFILE="$S\\home"
export APPDATA="$S\\home\\AppData\\Roaming" LOCALAPPDATA="$S\\home\\AppData\\Local"
export CARGO_HOME="C:\\Users\\<you>\\.cargo" RUSTUP_HOME="C:\\Users\\<you>\\.rustup"   # for cargo itself
```

One file still leaves the sandbox. minutes-core keeps `voices.db` in
`%USERPROFILE%\.minutes`, and it asks Windows for that folder, so no variable moves it
(UPSTREAM-PATCHES.md #1). Processing a meeting opens that file. Before a test run on a PC
that has real voices, link the folder into the sandbox, and remove the link afterwards:

```bash
cmd //c mklink //J "C:\\Users\\<you>\\.minutes" "$S\\dot-minutes"    # only if .minutes does not exist
cmd //c rmdir "C:\\Users\\<you>\\.minutes"                           # removes the link only
```

The end-to-end pass does this itself. It stops if a real `.minutes` folder exists.

The end-to-end pass runs on Windows as far as the build goes. The comment at the top of
`scripts/e2e/run.mjs` says what differs. `scripts/e2e/make-fixtures.sh` needs the macOS
voices, so make the test audio another way: the Windows voices (`System.Speech`) can
write the same lines to 16 kHz mono WAV files.

### Where the files are

| What | Where |
|---|---|
| Settings, state, models, logs | `%LOCALAPPDATA%\Margin` |
| Meetings, by default | `Meetings` in the Documents folder that Windows gives. With OneDrive backup on, that folder is in OneDrive. |
| Voices | `%USERPROFILE%\.minutes\voices.db` |

Nothing is written next to `margin.exe`.

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
