| Model | `--cpu` build | Vulkan, RTX 3070 Ti || `vulkan-1.dll`, with `VulkanRT-License.txt` | the Vulkan runtime files from LunarG, `<VULKAN_SDK>\runtime` | The Store build imports the Vulkan loader. A PC has it only when a graphics driver installed it. Not in a `--cpu` build. |
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
record a call, and it has no recording chip, no Open at login and no Apple Speech. The
copy from the installer updates itself, and the Store updates the Store package
([Updates on Windows](#updates-on-windows)). Whisper runs on the graphics card through Vulkan, and on the CPU when
the PC has no usable one ([Whisper on the GPU](#whisper-on-the-gpu)).

### Requirements

- Windows 11, x64, on a CPU with AVX2. The same is true for a PC that runs the app.
- Rust from rustup, with the `stable-x86_64-pc-windows-msvc` toolchain.
- Visual Studio with the "Desktop development with C++" workload.
- CMake and LLVM. whisper.cpp needs CMake, and bindgen needs `libclang.dll`.
- bun, and Git for Windows. The scripts run in Git Bash.
- The Vulkan SDK and the Vulkan runtime files, for the Store build
  ([Whisper on the GPU](#whisper-on-the-gpu)). A `--cpu` build and a debug build do not
  need them.
- The `winapp` CLI, for the Store package.

### Build

```bash
export VULKAN_SDK="C:/Users/<you>/VulkanSDK/<version>"
scripts/build-windows.sh            # the Store build: src-tauri/target/vulkan/release/margin.exe
scripts/build-windows.sh --cpu      # Whisper on the CPU only: src-tauri/target/release/margin.exe
scripts/build-windows.sh --debug    # a debug build, CPU only, with the UI built in
scripts/build-windows.sh --installer  # the Store build and an installer ([The installer](#the-installer))
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

- `layout`: the package folder. It has `margin.exe`, the DLLs beside it, `Assets`,
  `AppxManifest.xml` with the version and the CPU filled in, and `resources.pri`.
- `MarginNotes_<version>_x64.msix`: the package for Partner Center, not signed. The
  Store signs it. Margin 0.0.10 is package 1.0.10.0 (DECISIONS.md D61). The package is
  45 MB.

`-Cpu` packs the `--cpu` build in place of the Store build. That package is 28 MB.

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

A packaged run on 2026-11-17 confirmed the first three rows. `%LOCALAPPDATA%Margin` did
not appear, and the app wrote nothing into its own folder. That run gave no voice a
name, so it did not write `voices.db`. The file is not under `AppData`, so Windows does
not move it into the package.

`winapp unregister` removed the package data (207 MB with the Fast model) and the Start
menu entry. The meetings folder and `%USERPROFILE%.minutes` stayed.

So before a packaged test: check that the PC has no real Margin data, pick a temporary
meetings folder in setup, and keep the `.minutes` link from "Tests and the sandbox" in
place. Afterwards, unregister, and delete the meetings folder.

The microphone is different in a package. Windows asks the user once, with its own
prompt, and shows the package no microphone before the answer is yes. Margin asks for
the prompt at the first Record, and from the Allow button in setup and in Settings
(`win.rs` `request_microphone`). The registry has no entry for the package: the answer
comes from `AppCapability`. A new registration of the package folder asks again.

To see an error of the page, start the build outside the package with
`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222`, and read the
exceptions over that port. A release build has no developer tools.

The Windows App Certification Kit (`appcert.exe`, with the Windows SDK) needs an
administrator, and a package that Windows can install. So it needs a test certificate
that the PC trusts.

To check the package folder without identity, start `release\windows\layout\margin.exe`
in the sandbox. The app must write nothing into that folder.

### The installer

For a PC without the Store, the script also makes a normal installer:

```bash
export VULKAN_SDK="C:/Users/<you>/VulkanSDK/<version>"
scripts/build-windows.sh --installer         # the GPU build
scripts/build-windows.sh --cpu --installer   # the CPU build
```

It builds as above, then runs `tauri bundle --bundles nsis`. The first run downloads
NSIS into `%LOCALAPPDATA%\tauri`. The result is
`release\windows\MarginNotes-setup-<version>.exe`, about 27 MB for the GPU build. It is
not signed, so SmartScreen warns before it runs.

`tauri.windows.conf.json` has the settings:

- `installMode` is `currentUser`. The installer asks for no administrator rights. It
  installs into `%LOCALAPPDATA%\Margin Notes`, and adds "Margin Notes" to the Start menu,
  to the desktop and to Apps (installed apps), with the publisher Askysh.
- `webviewInstallMode` is `downloadBootstrapper`. It adds nothing to the installer.
  Windows 11 has WebView2, so the step does nothing there. On a PC where WebView2 is
  missing, the installer downloads Microsoft's bootstrapper and installs it.
- The script gives the DLLs from [The DLLs beside margin.exe](#the-dlls-beside-marginexe)
  and the Vulkan loader as resources, so they go beside `margin.exe`, as in the Store
  package.
- With `MARGIN_UPDATER_KEY_PATH` and `MARGIN_UPDATER_KEY_PASSWORD_PATH`, the same
  variables as `scripts/release.sh`, the script signs the setup exe and writes
  `MarginNotes-setup-<version>.exe.sig` beside it. The in-app update needs it
  ([Updates on Windows](#updates-on-windows)). Without them it makes no `.sig`.

`tauri bundle` writes the bundle type into `margin.exe` in the build folder. The updater
reads it.

The installed app is not packaged, so it writes to the places in
[Where the files are](#where-the-files-are). WebView2 keeps its data in
`%LOCALAPPDATA%\com.akashmishra.margin`. Windows asks once for the microphone, with its
own prompt.

The uninstaller (Apps, Margin Notes, Uninstall) removes the install folder, the
shortcuts and the Apps entry. With "Delete the application data" checked, it also
removes `%LOCALAPPDATA%\com.akashmishra.margin` and the key
`HKCU\Software\Askysh\Margin Notes`. It never removes `%LOCALAPPDATA%\Margin` (settings
and models), the meetings folder or `%USERPROFILE%\.minutes`.

### Updates on Windows

The copy from the installer updates itself, as the Mac app does (D38). The Store package
does not: the Store updates it, and a Store build must not update itself (D61). One
`margin.exe` serves both, so `win.rs` `installed_by_setup` decides at start:

- With package identity, the app runs from the Store package. No updater.
- With no bundle type in the exe (a plain `cargo build`), no updater.
- With no `uninstall.exe` beside the exe (the build folder, a layout folder, a copy
  elsewhere), no updater. There is no installer to update.

`margin.log` says `updater supported=true` or `false` at start. When the updater is off,
Settings has no Updates section on Windows.

The update file is the setup exe itself, with its `.sig`. The app reads the same
`latest.json` as the Mac app. `scripts/latest-json-windows.mjs` adds the Windows entry
to it:

```bash
bun scripts/latest-json-windows.mjs release/latest.json release/windows/MarginNotes-setup-<version>.exe.sig
```

The key is `windows-x86_64`. `tauri-plugin-updater` looks for `windows-x86_64-nsis`
first and then for `windows-x86_64` (`get_urls` in its `updater.rs`), so one entry is
enough. The URL is `https://github.com/askysh/margin/releases/download/v<version>/MarginNotes-setup-<version>.exe`.

How the plugin installs it on Windows:

- It checks the signature with the public key in `tauri.conf.json` before it saves the
  file.
- It writes the setup exe to `%TEMP%\Margin Notes-<version>-updater-<random>`, starts it
  with `/P /UPDATE`, and ends Margin Notes at once. Before that, Margin removes its tray
  icon (`on_before_exit`). The copy in `%TEMP%` stays after the update.
- The setup exe from the app has no Mark of the Web, so SmartScreen does not stop it.
- `/P` shows only a progress bar. The installer is for the current user, so Windows
  shows no UAC prompt. It asks the user nothing.
- "Restart to update" adds `/R`, so the installer starts the new version when it is done.
  Quit with an update downloaded installs it without `/R`, so the app stays closed.
- During a recording neither path installs, as on the Mac.
- `/UPDATE` keeps the shortcuts. Settings, models and meetings are not in the install
  folder, so they stay.

To test an update on one PC, make a test key and two installers with its public key:

```bash
T="$(cygpath -m "$TEMP/margin-update-test")"; mkdir -p "$T"
bunx tauri signer generate --ci -w "$T/test.key" -p test
printf test > "$T/key.pass"
# The public key from test.key.pub. An http endpoint needs dangerousInsecureTransportProtocol.
echo '{"plugins":{"updater":{"pubkey":"<test.key.pub>","dangerousInsecureTransportProtocol":true}}}' > "$T/low.json"
echo '{"version":"<a higher version>","plugins":{"updater":{"pubkey":"<test.key.pub>","dangerousInsecureTransportProtocol":true}}}' > "$T/high.json"
export MARGIN_UPDATER_KEY_PATH="$T/test.key" MARGIN_UPDATER_KEY_PASSWORD_PATH="$T/key.pass"
MARGIN_TAURI_CONFIG="$T/high.json" scripts/build-windows.sh --installer   # copy the exe and .sig away
MARGIN_TAURI_CONFIG="$T/low.json" scripts/build-windows.sh --installer
```

Install the lower one. Serve the higher exe, its `.sig` and a `latest.json` (made with
`MARGIN_UPDATE_BASE_URL=http://127.0.0.1:<port>`) from a local folder. Then start the
installed app with `MARGIN_UPDATE_ENDPOINT=http://127.0.0.1:<port>/latest.json`. The
version that the app shows comes from `Cargo.toml`, so a test build with only a higher
version in its config still shows the old number. The Apps entry and the file version of
`margin.exe` show the new one.

Tested on 2026-11-17 with 0.0.11 and a 0.0.12 test build, both signed with a test key:

- The installed 0.0.11 logged `updater supported=true`, found 0.0.12, and downloaded and
  verified it. The build-folder exe logged `updater supported=false`.
- During a recording, the banner said "It installs after this recording", and "Restart
  to update" was refused.
- "Restart to update": the setup ran with `/P /UPDATE /R /ARGS`, with no UAC prompt. The
  new version was running 7 s after the click. The Apps entry and the file version said
  0.0.12. `config.toml`, `prefs.json`, the model and the test meeting were the same byte
  for byte. The Start menu and desktop shortcuts stayed. A new check found no update.
- Quit from the notification area with an update downloaded: the setup ran with
  `/P /UPDATE`, and the app stayed closed. The next start was 0.0.12.
- A `latest.json` with the signature of another file: the app did not install it. The
  log and Settings, Updates said "The update's signature didn't match, so Margin Notes
  didn't install it."

### Whisper on the GPU

minutes-core passes a `vulkan` feature to whisper.cpp. Margin has the same feature. It is
off in `Cargo.toml`, so the macOS build and a plain `cargo build` do not get it.
`scripts/build-windows.sh` turns it on for the Store build (DECISIONS.md D63).

The build needs the Vulkan SDK from LunarG, for the headers, `vulkan-1.lib` and `glslc`.
The installer takes `copy_only=1`, which needs no administrator and changes no system
setting:

```powershell
vulkansdk-windows-X64-<version>.exe --root C:\Users\<you>\VulkanSDK\<version> --accept-licenses --default-answer --confirm-command install copy_only=1
```

The build also needs the Vulkan loader, `vulkan-1.dll`, to put beside `margin.exe`. It is
not in the SDK. LunarG has it in the "Runtime components" zip of the same release,
`VulkanRT-X64-<version>-Components.zip`, on the SDK download page. Unzip it, and copy
`VulkanRT-License.txt` and `x64\vulkan-1.dll` into `<VULKAN_SDK>\runtime`, with the `x64`
folder. Or set `VULKAN_RUNTIME` to another folder with the same two files. The loader is
open source (Apache 2.0 and MIT), and the license file goes into the package with it.
The laptop build used release 1.4.363.0.

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
- **No `vulkan-1.dll` on the PC.** `margin.exe` imports `vulkan-1.dll`, and a PC has it
  only when a graphics driver installed it. So the build puts its own copy beside
  `margin.exe`. Windows looks in the folder of the exe first: the running app shows
  that copy in its list of modules, also on a PC that has one in `System32`. Not tested
  on a PC with no copy in `System32`.
- **The switch.** Settings, Transcription, "Use the graphics card" is on by default.
  Off sends Whisper to the CPU from the next start of the app: Margin sets
  `VK_LOADER_DRIVERS_DISABLE=*` for itself, and the log says "no GPU found"
  (UPSTREAM-PATCHES.md #23).
- **The first run.** The first transcription after an install takes about 15 s more. The
  driver compiles the shaders and keeps them.
- **The size.** `margin.exe` is 108 MB in place of 52 MB, and the package is 45 MB in
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

`--no-build` stops the script after the file edits, before step 3. The workflow below uses
it.

### The upstream-bump workflow

[`.github/workflows/upstream-bump.yml`](../.github/workflows/upstream-bump.yml) does the
bump for you when upstream publishes a release (DECISIONS.md D64). Every Monday it
compares the newest upstream release tag with the pin. Pre-releases do not count, the same
rule as the script. When the tag is newer, the workflow does these steps:

1. It runs `scripts/bump-upstream.sh --no-build <tag>` on a `macos-26` runner, then
   `cargo fetch` to add Margin's own crates to the lock.
2. It runs the UI type check and build, `cargo test --locked` and `cargo build --locked`.
3. It runs the end-to-end pass with fixtures from `scripts/e2e/make-fixtures.sh` and
   models that `margin --dev models` downloads. The report is the `e2e-report` artifact.
4. It pushes the branch `upstream/<tag>` and opens a pull request into `next-release`.
   The pull request has the old and new ref, links to the upstream changes, the result of
   each check and a checklist.

A failed check makes the pull request a draft. A broken bump is still useful to see early.
The workflow opens no second pull request for a tag. After you close one, a scheduled run
does not open it again.

To run it by hand, open **Actions**, then **upstream-bump**, then **Run workflow**. Leave
`ref` empty for the newest release tag. Or type a tag, or a full commit SHA on upstream
`main` that no tag has yet. From the terminal:

```bash
gh workflow run upstream-bump.yml                                # the newest release tag
gh workflow run upstream-bump.yml -f ref=<full-sha>              # a commit on upstream main
```

GitHub runs scheduled and manual workflows only from the default branch. So the workflow
starts to work after `next-release` merges into `main`.

Before you merge the pull request:

- Select **Approve workflows to run** on the pull request. The workflow token opened it,
  so GitHub does not start `ci.yml` without a person.
- Do the checklist in the pull request. The runner has no microphone, no system audio and
  no Apple Speech. Record one real call on a Mac before the release.

The repository setting **Settings > Actions > General > Allow GitHub Actions to create and
approve pull requests** must be on. Without it, the last job fails.

A Windows check goes in this workflow as a job on `windows-latest`, next to the macOS job,
when the Windows port is on `next-release`.

A change that Margin needs in minutes-core goes upstream as a pull request. Each
candidate is in [UPSTREAM-PATCHES.md](../UPSTREAM-PATCHES.md): what, why and where.
