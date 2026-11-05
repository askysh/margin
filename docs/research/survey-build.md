# Survey: how upstream Minutes builds and packages its Tauri app, and what native deps a downstream app inherits

Upstream: `~/dev/minutes-upstream` at `2e64cd49` (`v0.32.1-3`). All paths are relative to that root unless they are absolute. Nothing was built.

---

## 1. `tauri/src-tauri`: manifest, build script, config, plists

### Cargo.toml (`tauri/src-tauri/Cargo.toml`)
- Package `minutes-app` (`:2`). Cargo uses this name for the binary, and it matters (see the decode-worker gotcha in section 4).
- The feature flags pass straight through to core (`:7-20`): `parakeet`, `engine-sherpa`, `pocketstation-capture`, `vad-ort`, `voice-live`, `coreml`, `cuda`, `hipblas`, `metal`, `vulkan`.
- `minutes-core = { path = "../../crates/core", features = ["streaming", "diarize"] }` (`:32`). Default features are on, so `whisper` is included.
- tauri 2 features: `tray-icon`, `image-png`, `devtools`, `macos-private-api` (`:23`).
- Plugins: shell, global-shortcut, dialog, notification, pty, autostart, updater, window-state (`:24-45`). Other crates: `window-vibrancy`, `portable-pty`, `reqwest` (rustls), `minisign-verify`, and `security-framework` on macOS (`:53`).

### build.rs (`tauri/src-tauri/build.rs`)
`main` (`:5-14`) runs these steps in order:
1. `link_swift_runtime_rpath` (`:20-24`) adds `-Wl,-rpath,/usr/lib/swift` to the `minutes-app` bin. **Required downstream.** Core links a static Swift library, and core's own `rustc-link-arg` does not reach downstream binaries.
2. `compile_mic_check_helper` (`:341-376`) runs `swiftc -target <arch>-apple-macos11.0 src/mic_check.swift` and writes `bin/mic_check-<triple>`.
3. `compile_system_audio_helper` (`:378-421`) runs `swiftc -parse-as-library -target …macos14.0` over `src/system_audio_record.swift` and `src/system_audio_tap.swift` (ScreenCaptureKit plus Core Audio tap) and writes `bin/system_audio_record-<triple>`. **These Swift sources live in the Tauri crate, not in core.**
4. `compile_calendar_helper` (`:423-485`) runs `swiftc -O -target …macos14.0` with `-sectcreate __TEXT __info_plist scripts/calendar-helper-Info.plist` over `scripts/calendar-events.swift` and writes `bin/calendar-events-<triple>`.
5. `stage_rust_sidecars` (`:254-339`) copies `target[/<triple>]/release/{minutes, minutes-graph-worker, minutes-apple-speech-worker}` to `bin/<name>-<triple>`. If they are missing, it writes a 52-byte shell placeholder. `MINUTES_REQUIRE_REAL_CLI_SIDECAR=1` turns the placeholder into a hard failure (`:263-317`). Because of issue #324, the CLI **must be built before** `cargo tauri build`.
6. `stage_assistant_skill_bundle` (`:487-526`) copies `.agents/skills/minutes`, `.opencode/skills` and `.opencode/commands` into `resources/assistant-skill-bundle`.
7. `stage_msvc_runtime` runs on Windows only.

`swift_deployment_target` (`:209-234`) documents why it passes an explicit `-target`: swiftc ignores `MACOSX_DEPLOYMENT_TARGET`. The 0.22.0 helper was stamped with minos 26 and crashed on macOS 15 (issue #494).

**Yes, the `minutes` CLI ships inside the .app as an externalBin.** It lands at `Contents/MacOS/minutes`. The in-app "Set up CLI" action links `~/.local/bin/minutes` to it (`tauri/src-tauri/src/cli_setup.rs:1-3`).

### tauri.conf.json
- `productName` "Minutes", `identifier` **`com.useminutes.desktop`**, `version` 0.32.1 (`:3-5`). The dev config uses `com.useminutes.desktop.dev` (`tauri.dev.conf.json:3`).
- `frontendDist: "../src"` (`:7`). There is no dev server and no bundler.
- `withGlobalTauri: true`, `macOSPrivateApi: true`, `windows: []` (windows are created in Rust), `csp: null` (`:10-15`).
- `bundle.createUpdaterArtifacts: true`, `resources: assistant-skill-bundle/**`, and a custom DMG background and layout (`:17-35`).
- `plugins.updater` has a minisign pubkey and the endpoint `https://github.com/silverstein/minutes/releases/latest/download/latest.json` (`:40-45`).
- **No `minimumSystemVersion` is set** in either config, so Tauri falls back to its default LSMinimumSystemVersion of 10.13. The archive app sets 13.0 (`archive/src-tauri/tauri.conf.json:37`). Our app should set `bundle.macOS.minimumSystemVersion: "14.0"`, or 14.4 (see the Core Audio tap note below).

### tauri.macos.conf.json
- `externalBin` (`:3-10`): `bin/mic_check`, `bin/system_audio_record`, `bin/calendar-events`, `bin/minutes`, `bin/minutes-graph-worker`, `bin/minutes-apple-speech-worker`.
- `macOS.entitlements: entitlements.plist` and `macOS.infoPlist: Info.plist` (`:14-17`). Tauri merges this Info.plist into the generated one.

### Info.plist (`tauri/src-tauri/Info.plist`)
| Key | Line | Purpose |
|---|---|---|
| `NSMicrophoneUsageDescription` | `:5` | mic (cpal) |
| `NSAudioCaptureUsageDescription` | `:7` | Core Audio process taps (system audio, macOS 14.4+) |
| `NSScreenCaptureUsageDescription` | `:9` | ScreenCaptureKit system audio and screenshots |
| `NSCalendarsFullAccessUsageDescription` | `:11` | EventKit (the calendar helper embeds its own copy as well) |

### entitlements.plist (`tauri/src-tauri/entitlements.plist`)
- `com.apple.security.device.audio-input` = true (`:5`)
- `com.apple.security.files.user-selected.read-write` = true (`:7`). This only matters under the sandbox. The app is **not** sandboxed: it uses Hardened Runtime with Developer ID.
- There are **no** `cs.allow-jit`, `cs.disable-library-validation` or screen-capture entitlements.

The other entitlement files:
- `minutes-cli.entitlements` contains `device.audio-input` = true and `cs.allow-jit` = false. It is applied to the CLI sidecar.
- `minutes-graph-worker.entitlements` and `minutes-apple-speech-worker.entitlements` each contain only `app-sandbox` = true. They apply to the XPC services.

**What mic and system audio capture need:**
- **Entitlement:** only `com.apple.security.device.audio-input`. Hardened Runtime blocks the mic without it. ScreenCaptureKit and Core Audio taps have no entitlement; they are gated only by TCC.
- **Info.plist:** `NSMicrophoneUsageDescription`, `NSAudioCaptureUsageDescription` (the tap prompt) and `NSScreenCaptureUsageDescription`. Add `NSCalendarsFullAccessUsageDescription` if we read the calendar.
- **Child helpers:** TCC attributes them to the responsible app, so their prompts use the app's strings. The `minutes` sidecar is signed with `audio-input` too, because a CLI that the user runs directly from the terminal is its own responsible process.

---

## 2. Frontend (`tauri/src`)

- **Framework:** vanilla HTML, CSS and JS with no build step. `tauri/package.json` does not exist.
- **Size:** `index.html` is **20,611 lines**, a single file with inline CSS and JS. The other windows are `dictation-overlay.html` (1,217 lines), `copilot-hud.html` (915), `meeting-prompt.html` (467), `note.html` (392), `terminal.html` (331) and `palette/index.html`. There are about 3.3k lines in `scripts/*.js` and `locales/*.js`, plus vendored `marked.min.js` and `xterm`.
- **Calling Rust:** `const { invoke } = window.__TAURI__.core` (`index.html:8256`) together with `window.__TAURI__.event.listen(...)` (`:10436`, `:10538`). index.html invokes about 128 distinct commands. `main.rs:3350-3545` registers about 184.
- **Main screens:**
  - meeting list as the main window
  - Recall panel with weekly view and chat or terminal (`:6706-6738`)
  - detail overlay (`:6802`)
  - Readiness Center (`:6879`)
  - Recording consent (`:6894`)
  - Recovery and Jobs overlays (`:6914`, `:6935`)
  - About (`:6952`)
  - Settings overlay with 6 tabs: General, Capture, Dictation, Transcription, AI & Privacy, Advanced (`:7019-7033`)
  - Coach onboarding (`:8066`)
- **Pain points relevant to our app:**
  - **Onboarding** is the empty state of the meeting list (`renderSetupEmptyState`, `index.html:13535`). It has a "Welcome to Minutes" step that downloads a Small or Base whisper model, then asks for one short recording. There is no folder choice and no permissions walkthrough; permissions are handled reactively by `spawn_permission_monitor` and the Readiness Center.
  - **File management:** there is **no UI to choose `output_dir`**. It defaults to `~/meetings` (`crates/core/src/config.rs:1733`), and the Advanced > Storage panel is read-only stats (`index.html:8015-8020`, `:14757-14763`). "Set Up Vault" (`index.html:7969-7976`) calls `cmd_vault_setup`. That command keeps meetings in `~/meetings` and creates a symlink `<vault>/<meetings_subdir>` -> `output_dir`, falling back to a "copy" strategy (`tauri/src-tauri/src/commands.rs:6336-6379`). This indirection is the confusing part.
  - **Settings** are written through a stringly typed `cmd_set_setting(section, key, value)` (`commands.rs:14335`). They are partly shadowed in localStorage (`index.html:9597-9753`).
  - There is **no reset or factory-reset command** (no `cmd_*reset*`).
- The main window is 1120x720 and transparent (`main.rs:503-520`). The app shows in the Dock: no `LSUIElement` and no accessory activation policy.

---

## 3. `main.rs` (4,501 lines; `commands.rs` is 26,170 lines)

### Before Tauri starts (`main.rs:1839-1900`)
- Calls `minutes_core::graph_worker::maybe_run_policy_projection_worker()` and `minutes_core::audio_decode_worker::maybe_run_audio_decode_worker()`, and exits if either returns `Some`. These are **self-exec worker entry points**, and **any downstream binary must call them first** (see section 4).
- If `Contents/XPCServices/com.useminutes.graph-worker.xpc` or `…apple-speech-worker.xpc` exists, it calls `install_policy_projection_worker_executable` or `install_apple_speech_worker_service`.
- Handles the acceptance-test argv hooks.

### Builder
- Plugins: customize_theme, shell, dialog, notification, autostart, updater, window-state (`:2228-2246`).
- Managed state:
  - `commands::AppState` (`:2247-2314`) is one large struct of `Arc<AtomicBool>` and `Arc<Mutex<…>>` flags. It covers recording, starting, stop_flag, processing, live transcript, dictation, copilot, voice, hotkeys, palette, updater, call-detect countdown, recall chat and the PTY manager.
  - `Arc<Mutex<ShortcutManager>>` (`:2315`).
  - In setup, `TrayMenuHandles` (`:3118`) and `TrayAppearanceState` (`:3154`).

### `setup` (`:2318-3253`) and what it starts
- `install_macos_terminate_hook`
- `spawn_meetings_refresh_watcher`: a `notify` thread on `output_dir` (`:1758`)
- workspace cleanup
- an optional retention-cleanup thread (`:2345`)
- the **updater thread**, which checks every 6 h and polls every 30 s for deferred updates (`:2405`)
- an optional dictation model preload thread (`:2427`)
- `show_main_window`
- `commands::spawn_permission_monitor` (`:2437`)
- `spawn_processing_worker` if jobs are queued (`:2440`)
- shortcut restore
- the tray menu and `TrayIconBuilder::with_id("minutes-tray")` (`:2677-2760`). Menu items: Open, Record, Quick thought, Sensitive, Stop, Coach, Mic mute, Note, dictation copy/paste/reprocess, Assistant, List, paste summary/transcript, screen-share hide, Check update, Quit.
- the **call detection** loop, `call_detect::CallDetector::start` (`:3163`)
- the **desktop-control** thread (`:3183-3210`). Every 2 s it writes `desktop_control::write_desktop_app_status` and claims pending requests, which is a file-queue IPC that the CLI uses to drive the app.
- the **calendar refresh** thread with backoff (`:3214-3249`).

### Other handlers
- `on_window_event` (`:3255`) persists window state.
- `RunEvent::ExitRequested`, `Reopen` and `Exit` (`:3545-3558`).

### How recording runs
- The mic and the in-process path run **in-process** through `minutes_core::capture::record_to_wav_with_lifecycle` (`commands.rs:7043`).
- The "native call" path spawns the Swift `system_audio_record` helper, found beside the exe (`call_capture.rs:1316-1331`).
- Core also has an in-process Core Audio tap backend through `cidre` (`crates/core/src/system_audio_backend.rs`), built with cidre feature `macos_14_4` (`crates/core/Cargo.toml:118`).

---

## 4. Native deps and build requirements

### What a downstream crate gets by depending on `minutes-core`
With default `whisper`, plus `diarize` and `streaming` the way the app uses it:

| Dep | Version (locked) | Build mechanism | Needs |
|---|---|---|---|
| whisper-rs / whisper-rs-sys | 0.16.0 / 0.15.0 | **cmake** build of whisper.cpp plus **bindgen** | `cmake` (`brew install cmake`, CONTRIBUTING.md:28), libclang from Xcode |
| `metal` feature | — | `GGML_METAL=ON`, `GGML_METAL_EMBED_LIBRARY=ON` (whisper-rs-sys build.rs:258-265) | the shader is embedded, so no .metallib resource. Without the feature, Metal is explicitly OFF. Links Accelerate always and Metal/MetalKit with `metal`. |
| `coreml` feature | — | `WHISPER_COREML=ON` | requires the `-encoder.mlmodelc` next to the model at runtime |
| ort / ort-sys (`diarize`, `vad-ort`) | **2.0.0-rc.10** | default features incl. **`download-binaries`** and `copy-dylibs`. At build time it downloads prebuilt ORT 1.22.0 from `cdn.pyke.io` (ort-sys `dist.txt`, `aarch64-apple-darwin.tgz`) and links it **statically** | network at build time, or `ORT_LIB_LOCATION` / `ORT_SKIP_DOWNLOAD` / `ORT_PREFER_DYNAMIC_LINK` (ort-sys build.rs:10-14). No dylib to bundle. |
| pyannote-rs | 0.3.4 | depends on `ort ^2.0.0-rc.10` with `ndarray`, plus knf-rs | — |
| knf-rs / knf-rs-sys | 0.3.2 | **cmake** C++ (kaldi-native-fbank) plus bindgen | cmake |
| cidre | 0.22.1 | Rust plus ObjC, features `macos_14_4` | macOS SDK |
| rusqlite | `bundled` | cc (SQLite amalgamation) | — |
| **core build.rs Swift** | — | see below | `swiftc` from **Xcode 26 SDK** |

### `crates/core/build.rs`
Runs unconditionally on a macOS target; it is not feature-gated.
- `compile_macos_graph_xpc_authority_bridge` (`:13-84`) runs `swiftc -parse-as-library -O -target <arch>-apple-macos11.0 -emit-library -static` over `src/macos_graph_xpc_authority.swift` and `src/macos_apple_speech_bridge.swift`, producing `libminutes_macos_graph_xpc_authority.a`. It links swiftCore, Foundation, Security, AVFAudio, CoreMedia and **Speech**. The Apple Speech bridge uses `SpeechTranscriber` / `SpeechAnalyzer` (macOS 26 API, `macos_apple_speech_bridge.swift:310-317`), so **the build needs the macOS 26 SDK (Xcode 26+)** even if the app targets macOS 14/15. This is documented at `docs/install.md:340-347`.
  - Its `cargo:rustc-link-arg=-Wl,-rpath,/usr/lib/swift` (`:63`) applies only to core's own targets. The comment at `:56-62` says so. **The downstream app's build.rs must emit `cargo:rustc-link-arg-bin=<our-bin>=-Wl,-rpath,/usr/lib/swift`**, as `tauri/src-tauri/build.rs:20-24` does.
- `stage_calendar_helper` (`:86-135`) compiles `<repo_root>/scripts/calendar-events.swift`, with an embedded plist, into `$OUT_DIR/calendar-events`. It does **not** pass `-target`, so the helper is stamped with the host SDK minos. Core uses it as the last fallback (`crates/core/src/calendar.rs:829`). Lookup order (`calendar.rs:799-830`): beside the current exe, then `tauri/src-tauri/bin/…` (resolved from `CARGO_MANIFEST_DIR`, meaning the git checkout in `~/.cargo/git/checkouts/...`), then `$OUT_DIR`. If the sources are missing, as in the **crates.io tarball**, it writes an empty placeholder and prints a warning (`:105-111`), and embedded calendar support is effectively disabled. We confirmed that the 0.32.1 .crate has no `scripts/`.

### `crates/cli/build.rs`
- Embeds a versioned Info.plist into `minutes-graph-worker` and `minutes-apple-speech-worker` with `-sectcreate` (`:25-46`).
- Adds `-rpath /usr/lib/swift` to the Apple Speech worker (`:58`).
- The `minutes` bin itself gets no special flags on macOS.

### sherpa plugin (`crates/sherpa-plugin`)
- It is outside the workspace (`Cargo.toml:16-19` of the root) and is a cdylib `libminutes_sherpa`.
- Its build.rs downloads `sherpa-onnx-v1.13.8-osx-universal2-shared` from GitHub, checks the sha256, runs bindgen, and links `libsherpa-onnx-c-api.dylib` and `libonnxruntime.dylib` with `@loader_path` (`crates/sherpa-plugin/build.rs:33-80`). `MINUTES_SHERPA_ARCHIVE` allows offline builds.
- Core's `engine-sherpa` feature is only a `libloading` loader (`crates/core/Cargo.toml:35-49`). The three dylibs must be copied by hand into `Contents/MacOS` and signed (`release-macos.yml:230-265`).
- **Skip this for v1.** `parakeet` is dormant too and resolves to whisper (`crates/core/Cargo.toml:15-17`).

### Env vars and flags upstream relies on
- `MACOSX_DEPLOYMENT_TARGET=11.0`
  - Set in `.cargo/config.toml:5` with force, `scripts/build.sh:12` and `release-macos.yml:19`.
  - Reason: whisper.cpp's `std::filesystem` fails at cc's default of 10.13 (issue #14).
  - **`.cargo/config.toml` of a dependency is NOT applied to downstream builds.** Put it in our own `.cargo/config.toml`, and use 14.0 to match our target.
- `CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1"`
  - Set in `release-macos.yml:93-94`, `scripts/build.sh:11` and `docs/install.md:353`.
  - Needed for whisper.cpp on Xcode 26 / macOS 15+.
  - Note: `release-macos.yml:20` sets it literally in `env:`, where `$(...)` is not expanded, and line 94 then fixes it through `GITHUB_ENV`.
- `CMAKE_OSX_DEPLOYMENT_TARGET=11.0` and `GGML_CCACHE=OFF`
  - Used in the Homebrew formula (`docs/release/pre-release-checklist.md:203-206`).
  - whisper-rs-sys forwards every `WHISPER_*`, `GGML_*` and `CMAKE_*` env var to cmake as `-D` (whisper-rs-sys build.rs:279-287).
  - `GGML_CCACHE=OFF` avoids ccache breakage.
- `DEVELOPER_DIR` pointing at Xcode 26, if multiple Xcodes are installed (`docs/install.md:343-347`).
- Stale `ort-sys` clang_rt path after an Xcode upgrade: `rm -rf target/*/build/ort-sys-*` (`scripts/build.sh:59-70`).
- Toolchain is pinned to **Rust 1.95.0** (`rust-toolchain.toml`). This file does not apply downstream either, so pin our own to 1.95 or newer.

### Gotchas specific to depending on core
1. **ort version drift.**
   - Core declares `ort = "2.0.0-rc.10"`, which is caret (root `Cargo.toml:115` and the published manifest). That allows later 2.0.0-rc.N releases, and rc.15 already exists in `~/.cargo/registry`.
   - Later rc versions moved to ndarray 0.17, which conflicts with pyannote-rs and core's ndarray 0.16.
   - Our build does not use upstream's `Cargo.lock`.
   - **Add `ort = { version = "=2.0.0-rc.10" }` to our Cargo.toml**, exactly as `crates/cli/Cargo.toml:64-68` does and for the same reason, or seed our lockfile from upstream's.
2. **Decode worker self-exec.**
   - `audio_decode_worker` re-execs the current exe only if its file stem is `minutes` or `minutes-app` (`crates/core/src/audio_decode_worker.rs:266`). Otherwise it falls back to a `minutes` binary **in the same directory** (`:291-320`).
   - So either name our bin `minutes-app` and call `maybe_run_audio_decode_worker()` first in `main`, or (simplest) bundle the `minutes` CLI as an externalBin, which we likely want anyway.
   - Either way, call both `maybe_run_*` functions at the top of `main`, as `main.rs:1839-1850` does.
3. **Graph worker XPC is pinned to upstream's identity.**
   - It requires team `63TMLKT8HN` and identifier `com.useminutes.desktop[.dev]` (`crates/core/src/macos_graph_xpc_authority.swift:4-7`, `macos_graph_xpc.rs:1091`).
   - Our app, under any other team or ad-hoc signing, fails the "trusted distribution" check. Core then picks `InProcessFallback` (`graph_worker.rs:89-105`, `:208-248`).
   - So **do not ship the graph or Apple Speech XPC services.** The in-process fallback works.
4. The `tests/integration` and `include_str!("../../…")` references in core are test-only, so they are harmless.

---

## 5. `.github/workflows/release-macos.yml`

- **Trigger:** `v*` tag or a dispatch with a tag (`:3-12`).
- **Global env:** `MACOSX_DEPLOYMENT_TARGET=11.0`, `CXXFLAGS`, `TAURI_CLI_VERSION=2.10.1` (`:17-22`).
- **Readiness job:** `node scripts/check_version_sync.mjs --release` (`:49-50`).
- **Main job:** `macos-latest` with environment `production-release` (`:52-62`).

Steps:
1. Check out the tag and verify that HEAD equals the tag (`:75-91`). Set CXXFLAGS through GITHUB_ENV (`:93-94`). Install the Rust stable action; rust-toolchain.toml pins 1.95 (`:96-97`). Cache cargo keyed by runner image (`:99-114`). Install tauri-cli 2.10.1 (`:116-117`).
2. Validate the secrets (`:119-148`): `APPLE_CERTIFICATE` (base64 p12), `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY`, `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_PRIVATE_KEY` (.p8 contents), `TAURI_SIGNING_PRIVATE_KEY` (updater minisign).
3. Write the .p8 to `$RUNNER_TEMP` and set `APPLE_API_KEY_PATH` (`:150-158`). Create an ephemeral keychain and import the p12 (`:160-190`).
4. `cargo build --release --locked` for `crates/sherpa-plugin` and `crates/embed-worker` (`:192-198`).
5. `cargo build --release -p minutes-cli --features parakeet,metal,engine-sherpa` (`:203-204`). build.rs stages its output as the sidecar.
6. `cargo tauri build --features parakeet,metal,engine-sherpa --bundles app`, run from `tauri/src-tauri` with `MINUTES_REQUIRE_REAL_CLI_SIDECAR=1` (`:206-217`). Running from that directory avoids feature resolution across the virtual workspace. Tauri signs with `APPLE_SIGNING_IDENTITY`. Notarization env is deliberately not passed, so Tauri does not notarize.
7. Manual inside-out signing (`:219-288`):
   - Copy `libminutes_sherpa.dylib`, the 2 sherpa dylibs and `minutes-embed-worker` into `Contents/MacOS`.
   - Run sanity checks: the CLI must be at least 10 MB and Mach-O.
   - Run `codesign --force --options runtime --timestamp` on each dylib and worker, and on the CLI with `minutes-cli.entitlements`.
   - Run `scripts/package-graph-xpc.sh` and `scripts/package-apple-speech-xpc.sh`. They move the workers into `Contents/XPCServices/*.xpc`, sign them sandboxed, and bind the cdhash into `Contents/Resources/*.cdhash`.
   - Re-sign the outer app with `entitlements.plist`.
8. Verify with `codesign --verify --deep --strict` and check the entitlements and identity of the XPC workers (`:289-336`).
9. Notarize: `ditto -c -k --keepParent` the app, run `xcrun notarytool submit --key/--key-id/--issuer --wait`, then `stapler staple` and `validate`, and `spctl --assess --type execute` (`:338-348`).
10. Updater artifact: `tar czf Minutes.app.tar.gz` and `cargo tauri signer sign`, producing a `.sig` (`:350-354`).
11. DMG (`:356-383`):
    - `scripts/create-branded-dmg.sh` produces `Minutes_${VERSION}_aarch64.dmg`.
    - **codesign the DMG itself**, then notarize, staple and run `spctl --type open`.
12. Upload these artifacts: the `.app`, the `.dmg`, `Minutes.app.tar.gz` plus `.sig`, and `latest.json` (darwin-aarch64 only), all to the existing GitHub release (`:400-465`).

**Artifact:** a signed, notarized, stapled **.dmg**, `Minutes_<ver>_aarch64.dmg`, plus `.app.tar.gz` for the Tauri updater. The release is Apple Silicon only.

### `.github/workflows/homebrew-tap.yml`
- Runs on `release: published` (not on tag push, because the DMG must already exist) or on dispatch with a version and dry_run (`:11-23`).
- Runs `scripts/bump_homebrew_tap.py`: `--self-test`, then `--check-access`, then `--version`, then `--wait-current` (`:59-145`).
- Uses a fine-grained PAT `HOMEBREW_TAP_TOKEN` and skips prereleases.
- The script rewrites only `version` and `sha256` in `Casks/minutes.rb` (plus the formula's sherpa resource). It computes the sha from the release asset `Minutes_{v}_aarch64.dmg` (`scripts/bump_homebrew_tap.py:47`, `:108-121`, `:353-355`).
- Cask shape (fixture at `:537-543`): `version`, `sha256`, `url ".../v#{version}/Minutes_#{version}_aarch64.dmg"`, `app "Minutes.app"`.

### What a cask needs from the build
- A stable versioned asset URL for a DMG (or zip), with a stable filename pattern.
- An outer signature on the DMG, plus notarization and stapling.
- An app name that is stable across versions.
- The bundle id, used for `uninstall quit:` and `zap`.

---

## 6. crates.io vs git dependency

- **`minutes-core` IS on crates.io.** `max_version` is 0.32.1, published 2026-11-05 (crates.io API). Publishing was revived in #79 (`docs/release/pre-release-checklist.md:208-219`). whisper-guard 0.3.2 is also on crates.io.
- The crates.io tarball has 236 files and **omits `scripts/calendar-events.swift`**. The embedded calendar helper is therefore disabled when building from crates.io (`crates/core/build.rs:105-111`).
- A **git dependency pinned to a tag** (for example `tag = "v0.32.1"`) gives the full repo: the calendar helper builds, and the Swift sources in `tauri/src-tauri/src/*.swift` sit in the checkout for reference.
- Git-dep compatibility:
  - `version.workspace = true` and the other `*.workspace` keys resolve against the upstream root `Cargo.toml` `[workspace.package]` and `[workspace.dependencies]` inside the git checkout.
  - Path deps: `whisper-guard = { path = "../whisper-guard", version = "0.3.2" }` (`crates/core/Cargo.toml:88`) and `minutes-core = { path = "../core" }` in the CLI are in-repo, so they work.
  - The root manifest has no `[patch]`, no `git =` deps and no `path =` outside the repo (checked with grep).
  - Cargo parses all workspace members in the checkout, and all of them are in-repo.
- **Not inherited downstream:** upstream's `Cargo.lock`, `.cargo/config.toml`, `rust-toolchain.toml` and `[profile.release]` (`strip = true`, `lto = "thin"`, `Cargo.toml:30-32`). Re-declare the ones we want.
- `~/.cargo/git/checkouts/minutes-b09dfdb9df1ed76b/b92ef34` already exists, so a git checkout of upstream has been fetched before.

---

## 7. Benchbar packaging (`~/dev/benchbar/packaging/homebrew`) and `~/dev/homebrew-tap`

- **Cask template:** `Casks/benchbar-app.rb.tmpl`. `scripts/homebrew-render.sh` fills in `{{VERSION}}` and `{{SHA256}}` of the DMG. The template has:
  - `url ".../releases/download/v#{version}/BenchBar-#{version}.dmg"`, `name`, `desc`, `homepage`
  - `livecheck { url :url; strategy :github_latest }`
  - `auto_updates true`, `depends_on arch: :arm64`, `depends_on macos: :sonoma`, `depends_on formula: "askysh/tap/benchbar"`
  - `app "BenchBar.app"`, `uninstall quit: "<bundle id>"`, `zap trash: [Application Support, Caches, HTTPStorages, Preferences plist]`, `caveats`
- **Formula template:** `Formula/benchbar.rb.tmpl` installs the CLI tarball into libexec and writes an exec script from `opt_libexec`.
- **Tap repo** `askysh/homebrew-tap`: `Casks/` and `Formula/` are empty except `.gitkeep`. CI (`.github/workflows/tests.yml`) runs `brew style`, then `brew audit --strict --online`, install, test and uninstall on macos-latest. Benchbar's `homebrew-tap.yml` opens an auto-merging PR into the tap after it renders and tests the files.
- The benchbar release (`scripts/macos-release.sh`) is xcodebuild-based. Steps: `notarytool submit --key`, `stapler staple` the app, `hdiutil create -format UDZO` the DMG, `codesign` the DMG, then notarize and staple it.

**What a cask for our app needs:** a `cask "<app>"` with `version`, `sha256`, a GitHub-release DMG `url`, `depends_on arch: :arm64` and `macos: :sonoma`, `auto_updates true` (only if we ship the Tauri updater), `app "<Name>.app"`, `uninstall quit: "<our.bundle.id>"`, and `zap trash:` for `~/Library/Application Support/<bundle id>`, Caches, WebKit and Preferences. Be careful about zapping `~/.minutes` or `~/.config/minutes`, because those are shared with the `minutes` CLI. A separate formula is not needed if we embed the CLI. The render, audit and PR workflow can be copied from benchbar.
