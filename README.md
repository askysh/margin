# Margin

A meeting notepad for macOS. You type notes in your own words while Margin records the
call and transcribes it on your Mac. Afterwards it tells speakers apart and saves each
meeting as a Markdown file in a folder you choose. Obsidian, iCloud Drive and any text
editor can read those files.

Margin is a separate app built on [`minutes-core`](https://github.com/silverstein/minutes)
(MIT). minutes-core supplies the local whisper transcription, pyannote diarization, voice
recognition and Markdown output. Margin adds a new interface: notepad first, simple setup
and a working **Reset**.

- Mac only: Apple Silicon, macOS 14.4 or later.
- MIT licensed.
- Built against minutes-core `v0.32.1`.

## Build and run

You need Xcode 26 (for `swiftc`), cmake, bun and Rust from rustup.

```bash
brew install cmake
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
export PATH="$HOME/.cargo/bin:$PATH"

bun install
scripts/build-app.sh              # → src-tauri/target/release/bundle/macos/Margin.app
open src-tauri/target/release/bundle/macos/Margin.app
```

A cold build takes about 2 minutes for debug and 2 minutes for release on an M-series
Mac. The app is 38.5 MB and unsigned. On macOS 26 and later, cargo needs
`CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1"`; the scripts set it for you.

For development:

```bash
export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
bunx tauri dev                    # hot-reloading UI, debug Rust
(cd src-tauri && cargo test)      # unit tests
```

### Headless checks (`--dev`)

The binary has a small developer CLI that runs the core integration without the UI. Point
`MARGIN_HOME` (and `HOME`, because voices.db ignores the other variables) at a temporary
directory so your real data stays untouched:

```bash
export MARGIN_HOME=/tmp/margin-test HOME=/tmp/margin-test/home
M=src-tauri/target/debug/margin
$M --dev models                          # download speech models (~520 MB)
$M --dev process meeting.wav "Title"     # queue and process a WAV as a meeting
$M --dev list                            # meetings as JSON
$M --dev rename <file.md> SPEAKER_2 Priya
$M --dev reset [--meetings] [--models] [--voices]
$M --dev permissions
```

## What works

All of the items below were checked with the `--dev` harness and with UI screenshots in
headless WebKit, using a mocked bridge and real meeting data.

- **Onboarding.** It runs in this order:
  1. Your name.
  2. The output folder: `~/Documents/Meetings`, your Obsidian vaults (found through
     Obsidian's own vault list) or any folder you pick.
  3. Model choice: Fast / Balanced / Best. The download starts in the background with a
     progress bar.
  4. Microphone and System Audio permissions, each with a short explanation.
  5. Optional summaries.
- **Processing.** Transcription, diarization and saving run in a background worker process,
  so they continue if you close the window. A 39-second, two-speaker recording processed in
  10 seconds.
- **Library.**
  - The sidebar groups meetings into Today, Yesterday, This week, then by month.
  - Search covers titles, notes and transcripts.
  - Each meeting can be opened, shown in Finder, opened in Obsidian (when it's in a vault)
    or moved to the Trash.
- **Meeting view.**
  - The summary sections are rendered.
  - **Your notes** can be edited and are saved into the file's `## Notes` section. The
    frontmatter and all other sections are kept byte-for-byte.
  - The transcript shows speakers.
- **Renaming a speaker.** Click a speaker chip or label and choose *Rename everywhere*.
  - This rewrites the transcript labels and the summary mentions, and updates
    `speaker_map` and `attendees`. Your notes are left as you typed them.
  - It also records the confirmation in minutes-core's overlay database.
  - With "Recognise this voice" checked, it saves a voice sample. In testing, the next
    meeting with that voice was labelled automatically (`source: enrollment`).
- **Summaries are optional.** You can choose:
  - **None** (the default): nothing leaves the Mac.
  - **Claude**, through your own `claude` CLI (found even when the app is launched from
    Finder).
  - **Ollama**.

  Meetings without a summary have a *Summarize* button.
- **Settings.**
  - Name, folder, model (switching downloads the new model if needed), summaries,
    permission status, and the voices Margin recognises (each can be forgotten).
  - A copy-paste MCP snippet, so Claude can read your meetings through the
    `minutes-mcp` server.
- **Reset.** Choose **Reset Margin…** under *Start over*. The dialog says what will happen:
  - It always clears settings, the processing queue, logs and app state. Setup then runs
    again.
  - "Keep my meetings", "Keep downloaded speech models" and "Keep recognised voices" are
    each on by default.
  - If you choose to delete meetings, you must type `delete`. The files go to the
    **Trash**, and only files Margin recognises as meetings are moved, so a vault folder
    keeps its other notes.
  - It refuses to run while you're recording or a meeting is still processing.
- **Menu bar.** Record a call, record in person, stop, open, quit. Quitting during a
  recording stops it and saves it first. Closing the window keeps Margin running in the
  menu bar.
- **Call detection.** A small "Zoom looks active — Record this call" banner appears when a
  call app starts.
- **Crash safety.** Notes typed during a recording are saved to disk as you type. If Margin
  dies mid-meeting, the next launch shows them. Unfinished processing resumes on the next
  launch.

## What isn't verified, or doesn't work yet

- **Live recording has not been run end to end.** The recorder uses the same minutes-core
  sequence as upstream's `minutes record`: lock, capture thread, queue the job, spawn the
  worker. The headless harness processed real audio through the same queue. But the
  processes I could launch had not been granted the microphone, and starting a recording
  would have raised a macOS prompt that needs you. Things to try first:
  1. Record in person for 30 seconds.
  2. Then record a call while something is playing.
  3. Watch the live transcript and the red rule (it is the level meter).
- **"You / Them" live labels are a heuristic.** They compare the loudness of the mic stem
  and the system-audio stem for each line, and are labelled approximate in the UI. Upstream
  has no live speaker labels. Real speaker names come after processing. In-person meetings
  show no live labels.
- **Permissions reset on every rebuild.** The app is ad-hoc signed, so macOS treats each
  build as a new app. Sign it with a stable identity (even a self-signed one) to keep them.
- **The system-audio permission check uses private TCC calls.** minutes-core has no check
  for it. `TCCAccessPreflight` and `TCCAccessRequest` are loaded with dlopen, as in Apple's
  AudioCap sample. If Apple changes them, Margin shows "Open System Settings" instead of
  "Allow".
- **Diarization quality is minutes-core's.** On synthetic TTS voices it found 3 speakers
  for 2, and shifted one boundary.
- **Some features are not in v1:**
  - Calendar-based titles and attendees (deliberately off; see DECISIONS.md D9).
  - Editing a meeting's title in the app.
  - Watching the folder for changes made by other apps. The meeting view reloads when
    the window regains focus instead.
  - Live partial transcripts. You see finished lines only, a few seconds behind.
  - Signing, notarization, auto-update.
- **Voices are shared with the upstream Minutes app.** minutes-core hardcodes
  `~/.minutes/voices.db`. Everything else Margin stores lives in
  `~/Library/Application Support/Margin`, apart from the Minutes app's files
  (UPSTREAM-PATCHES.md #1).

## Where things live

| What | Where |
|---|---|
| Meetings (`.md` and `.wav`) | the folder you chose, default `~/Documents/Meetings` |
| Settings (minutes-core config) | `~/Library/Application Support/Margin/config.toml` |
| Models, jobs, logs, state | `~/Library/Application Support/Margin/state/` |
| Voice profiles (shared) | `~/.minutes/voices.db` |
| Logs | `…/Margin/state/logs/margin.log`, `margin-worker.log` |

Margin never reads or writes the upstream Minutes app's `~/.config/minutes` or
`~/.minutes`, except `voices.db`.

## Project docs

- `RESEARCH.md`: the survey of minutes-core, the choice of shell, v1 scope and UI sketch.
  The detailed surveys are in `docs/research/`.
- `DECISIONS.md`: product and technical decisions, each with the options considered.
- `UPSTREAM-PATCHES.md`: small minutes-core changes worth sending upstream.
- `PROGRESS.md`: milestone log.

### Updating minutes-core

```bash
scripts/bump-upstream.sh v0.33.0
```

This re-seeds `src-tauri/Cargo.lock` from upstream's lock at that tag. That matters because
downstream crates don't see upstream's lockfile, and minutes-core's loose `ort` range
otherwise resolves to a release that breaks `pyannote-rs`.

### Packaging (later)

The build produces one `Margin.app` with no installer, which is what a Homebrew cask
expects. Signing and notarization should follow benchbar's flow and upstream's
`.github/workflows/release-macos.yml`. Sign inside-out with `--options runtime`, staple,
then ship a DMG. The cask should use `depends_on macos: :sonoma` and
`uninstall quit: "com.akashmishra.margin"`, and its `zap` should cover
`~/Library/Application Support/Margin`.
