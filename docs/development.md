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
updater of its own. Whisper runs on the CPU. A build with Whisper on the GPU is an option
([Whisper on the GPU](#whisper-on-the-gpu)).

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

The script adds CMake and LLVM to the PATH, sets `LIBCLANG_PATH` and the whisper.cpp
variables below, builds, and puts the DLLs that `margin.exe` needs beside it. For a plain
`cargo build` or `cargo test`, set the same variables yourself:

```bash
export PATH="/c/Program Files/CMake/bin:/c/Program Files/LLVM/bin:$PATH"
export LIBCLANG_PATH="C:/Program Files/LLVM/bin"
export CMAKE_C_FLAGS_RELEASE="-MD -O2 -Ob2 -DNDEBUG" CMAKE_CXX_FLAGS_RELEASE="-MD -O2 -Ob2 -DNDEBUG"
export GGML_NATIVE=OFF
```

A release build takes about 15 minutes from nothing on an 8-core laptop, and 9 minutes
when only Margin's own code changed. Most of that is the link step (LTO).

### The whisper.cpp variables

Without the two `CMAKE_` variables, a release build compiles whisper.cpp with no
optimization. The app works, and Whisper is about 13 times slower: 190 s for 46 s of
audio, in place of 13 s. The `cmake` crate replaces CMake's flags for the Release
configuration, and the new flags have no `/O2` (UPSTREAM-PATCHES.md #21). The build of
`whisper-rs-sys` passes each `CMAKE_` and `GGML_` variable of the environment to CMake,
so the variables put the flags back. A debug build is not affected.

The build script does not see a change of these variables. After you change one, delete
`src-tauri/target/release/build/whisper-rs-sys` and build again.

`GGML_NATIVE=OFF` makes whisper.cpp use AVX2, FMA and F16C on every build PC. The
default, `ON`, uses what the CPU of the build PC has. A build PC with AVX-512 then gives
an app that stops on a PC without it. A CPU with no AVX2 cannot run either build.

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

Under `cargo test`, cargo puts every link folder of the build on the PATH. whisper.cpp
has many, and the PATH goes past the length that `cmd.exe` searches. A test that starts
a program through `cmd.exe` must give the full path of the program.

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

### The browser keys

WebView2 is a browser. `win.rs` `tame_webview` turns off its keys (Ctrl+R, F5, Ctrl+P,
F12) and takes the browser items out of the context menu. A text field keeps Emoji, Cut,
Copy, Paste and Select all. To see the items that WebView2 offers, start the app with
`MARGIN_LOG=info,margin=debug`. Each right click writes a "context menu" line to
`margin.log`.

### The Store package

```powershell
powershell -ExecutionPolicy Bypass -File scripts\package-msix.ps1
```

Run `scripts/build-windows.sh` first. The script needs the `winapp` CLI
(`winget install Microsoft.WinAppCli`). It writes two things into `release\windows`,
which git ignores:

- `layout`: the package folder. It has `margin.exe`, the five DLLs, `Assets`,
  `AppxManifest.xml` with the version and the CPU filled in, and `resources.pri`.
- `MarginNotes_<version>_x64.msix`: the package for Partner Center, not signed. The
  Store signs it. Margin 0.0.10 is package 1.0.10.0 (DECISIONS.md D61). The package is
  28 MB.

`resources.pri` is the index that lets Windows pick a logo file by size and scale. The
script makes it with MakePri, and keeps every scale in the one file.

Windows does not install a package that is not signed. To test the app with its package
identity, register the package folder:

```powershell
winapp run release\windows\layout          # register the folder and start the app
winapp unregister release\windows\layout   # remove it, with its app data
```

This needs Developer Mode (Settings, System, For developers). Without it, `winapp run`
stops with "Developer Mode is not enabled on this machine". Developer Mode needs an
administrator.

A packaged run does not follow the sandbox variables, because Windows starts the app. It
writes to the real places:

| What | Where |
|---|---|
| Settings, state, models, logs | `%LOCALAPPDATA%\Packages\Askysh.MarginNotes_0f196q1bwh8q6\LocalCache\Local\Margin` |
| The WebView2 data | the same `LocalCache\Local`, in `com.akashmishra.margin` |
| Meetings | the folder that setup shows. The default is `Meetings` in Documents. |
| Voices | `%USERPROFILE%\.minutes\voices.db` |

These places follow the rules of Windows for a package. They are not confirmed by a
packaged run yet. So before a packaged test: check that the PC has no real Margin data,
pick a temporary meetings folder in setup, and keep the `.minutes` link from "Tests and
the sandbox" in place. Afterwards, unregister, and delete the meetings folder.

The Windows App Certification Kit (`appcert.exe`, with the Windows SDK) needs an
administrator, and a package that Windows can install. So it needs a test certificate
that the PC trusts.

To check the package folder without identity, start `release\windows\layout\margin.exe`
in the sandbox. The app must write nothing into that folder.

### Whisper on the GPU

minutes-core passes a `vulkan` feature to whisper.cpp. Margin has the same feature, off
by default.

```bash
export VULKAN_SDK="C:/Users/<you>/VulkanSDK/<version>"
scripts/build-windows.sh --vulkan    # src-tauri/target/vulkan/release/margin.exe
```

The build needs the Vulkan SDK from LunarG, for the headers, `vulkan-1.lib` and `glslc`.
The installer takes `copy_only=1`, which needs no administrator and changes no system
setting:

```powershell
vulkansdk-windows-X64-<version>.exe --root C:\Users\<you>\VulkanSDK\<version> --accept-licenses --default-answer --confirm-command install copy_only=1
```

The script gives the build folder a drive letter (`V:`, or `MARGIN_VULKAN_DRIVE`) while
it builds, and removes the letter at the end. whisper.cpp builds its shader tool in a
project inside the project, and from a normal folder those paths go past the 260
characters that MSBuild allows.

Measured on 2026-11-13: a Ryzen 7 6800H (8 cores), an RTX 3070 Ti Laptop GPU (8 GB), 231 s
of English speech, `margin --dev process`. The time is the transcription only. Speaker
separation adds 17 s to each run, on the CPU in both builds.

| Model | CPU | Vulkan, RTX 3070 Ti | Times real time, CPU | Times real time, Vulkan |
|---|---|---|---|---|
| `base` (Fast) | 14.3 s | 4.5 to 5.4 s | 16 | 43 to 51 |
| `small` (Balanced) | 45.0 s | 7.5 to 8.6 s | 5.1 | 27 to 31 |
| `large-v3-turbo` (Best) | 219.7 s | 10.9 to 11.3 s | 1.05 | 20 to 21 |

On the integrated Radeon graphics of the same laptop, `large-v3-turbo` took 16.0 s for
46 s of audio. The CPU took 65.7 s and the RTX 6.1 s.

What to know about a `vulkan` build:

- **Which GPU.** whisper.cpp uses the dedicated GPUs first. On the test laptop, device 0
  is the RTX and device 1 is the Radeon, and Whisper uses device 0. `MARGIN_LOG=debug`
  writes the list to `margin.log` ("ggml_vulkan: 0 = ..."). To force a device, set
  `GGML_VK_VISIBLE_DEVICES` to its number in that list.
- **No usable GPU.** The build falls back to the CPU. Tested with
  `VK_LOADER_DRIVERS_DISABLE=*`, which hides every driver, and with a device number that
  does not exist: the log says "no GPU found", and the meeting is written.
- **No `vulkan-1.dll`.** `margin.exe` imports `vulkan-1.dll`. The graphics driver
  installs it. On a PC that has none, such as a virtual machine with the basic display
  driver, the app does not start. A package for every PC must ship the DLL beside
  `margin.exe`. It is the Vulkan loader from Khronos (Apache 2.0, 1.7 MB). Not done yet,
  and not tested on such a PC.
- **The first run.** The first transcription after an install takes about 15 s more. The
  driver compiles the shaders and keeps them.
- **The size.** `margin.exe` is 108 MB in place of 52 MB, and the package is 44 MB in
  place of 28 MB. The shaders are inside the exe.
- **Live transcript.** Not measured. The timing is for the transcript after Stop.

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
