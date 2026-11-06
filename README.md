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

### Testing

```bash
(cd src-tauri && cargo test)                       # unit tests
bun run build && (cd src-tauri && cargo build)     # build what the e2e run uses
node scripts/e2e/run.mjs --audio meeting.wav \
  --seed-models ~/Library/Application\ Support/Margin/state/models \
  --summaries fake-ollama --simulate call.wav [--download]
```

`scripts/e2e/run.mjs` is a QA pass run as a user.
- It runs the built UI in headless WebKit. Every click goes to the real backend through
  `margin --dev serve`, a localhost bridge in front of the same functions the app calls.
- It uses a sandbox `HOME` and `MARGIN_HOME`, so your settings, meetings and voices are
  never touched.
- The bridge refuses permission requests, so no macOS prompt can appear.
- In debug builds, `--simulate` plays a WAV instead of the microphone, including
  `<wav>.voice.wav`, `<wav>.system.wav` and `<wav>.live.jsonl` when present. So the whole
  record → live transcript → stop → process flow runs without permissions.
- It writes screenshots and `report.json` to `$E2E_OUT`.

The binary also has a small developer CLI for the core integration. Point `MARGIN_HOME`
(and `HOME`, because voices.db ignores the other variables) at a temporary directory:

```bash
export MARGIN_HOME=/tmp/margin-test HOME=/tmp/margin-test/home
M=src-tauri/target/debug/margin
$M --dev models                          # download speech models (~520 MB)
$M --dev preflight call                  # the device checks Start runs, without recording
$M --dev process meeting.wav "Title"     # process a WAV as a meeting, in-process
$M --dev queue meeting.wav "Title" notes.md   # what Stop does: queue + worker child
$M --dev list | read <file.md>
$M --dev rename <file.md> SPEAKER_2 Priya
$M --dev summarize <file.md> claude|ollama
$M --dev ask [--meeting <file.md>] "question" ["follow-up" ...]   # Ask, with timings
$M --dev reset [--meetings] [--models] [--voices]
$M --dev serve [port]                    # QA bridge for scripts/e2e
```

## What works

Everything below was checked by the end-to-end pass, 40 of 40 (see PROGRESS.md), unless
it says otherwise.

- **Onboarding.** It runs in this order:
  1. Your name.
  2. The meetings folder: `~/Documents/Meetings`, your Obsidian vaults (found through
     Obsidian's own vault list) or any folder you pick.
  3. Model choice: Fast / Balanced / Best. The download starts in the background with a
     progress bar.
  4. Microphone and System Audio permissions, each with a one-line reason.
  5. Optional summaries.
- **Recording (simulated mic).**
  - *Record call* opens the notepad with a live transcript beside it. In calls, lines are
    marked You / Them by comparing your mic and the call audio.
  - Notes typed right up to *Stop* are kept.
  - Processing (transcribe, tell speakers apart, save) runs in a background worker, and
    the window then opens the finished meeting.
  - Closing the window keeps Margin in the menu bar. Quitting mid-recording saves the
    meeting first.
- **Meeting view.**
  - The summary sections are rendered.
  - **Your notes** can be edited and are saved into the file's `## Notes` section.
    The frontmatter and the other sections stay byte-for-byte.
  - Titles can be edited. The file name stays, so Obsidian links keep working.
  - Recordings with no speech say what to check.
  - While core is still finishing a meeting (speakers, summary), it's shown read-only.
- **Renaming a speaker.** Click a speaker chip or a transcript label, then *Rename
  everywhere*.
  - This updates the transcript labels, mentions in the summary, action-item assignees,
    commitments, `attendees` and `speaker_map`. Your notes are left as you typed them.
  - It also records the confirmation in minutes-core's overlay database, which the CLI
    and MCP read.
  - With "Recognise this voice" checked, it saves a voice sample, and the next meeting
    labels that person automatically.
- **Library.** Meetings are grouped Today / Yesterday / This week / by month. Search covers
  titles, notes and transcripts. Each meeting can be shown in Finder, opened in Obsidian
  (when it's in a vault) or moved to the Trash.
- **Summaries** are optional:
  - **None** (the default): nothing leaves the Mac.
  - **Claude**, through your own `claude` CLI. Checked headlessly with a real login: 10 s.
  - **Ollama**. The flow is checked against a stand-in server. See the limits below.
- **Ask (chat with Claude).** Press ⌘J, click *Ask about your meetings* in the sidebar,
  or click *Ask about this meeting*.
  - It uses your own `claude` CLI. Claude can only read files in your meetings folder:
    it gets Read, Grep and Glob and nothing else. It cannot write files or run commands.
  - Answers stream in. Follow-up questions continue the same conversation. **Stop** ends
    an answer. The meetings an answer used are shown as links that open them.
  - Each meeting has its own conversation, and there is one across all meetings. They are
    kept after a restart. **Clear** deletes one, and Reset deletes all of them.
  - If Claude Code is not installed or not signed in, the panel says how to fix it.
  - Checked with a real login (`margin --dev ask`): about 6 s for a question about one
    meeting, about 14 s for a question across five meetings. See docs/parity/chat.md.
- **Settings.**
  - Name, folder, model (switching downloads it with progress), summaries, permission
    status, and recognised voices (each can be forgotten).
  - An MCP snippet, so Claude can read your meetings.
- **Reset.** Choose **Reset Margin…** under *Start over*.
  - It always clears settings, the processing queue, logs and state, then runs setup
    again.
  - "Keep my meetings", "Keep downloaded speech models" and "Keep recognised voices" are
    each on by default.
  - Unprocessed audio is saved to `failed-captures/` and never wiped.
  - If you choose to delete meetings, you must type `delete`. The files go to the
    **Trash**, and only files Margin recognises as meetings are moved, so a vault folder
    keeps its other notes.
  - It refuses to run while recording or processing.
- **Call detection.** A "Zoom looks active — Record this call" banner appears when a call
  app starts.
- **Crash safety.** Notes typed during a recording are saved to disk as you type, and
  shown on the next launch if Margin dies. Unfinished processing resumes on the next
  launch. Audio left over from a crash is moved aside instead of blocking the next
  recording.

## What isn't verified, or doesn't work yet

- **Real microphone and system-audio capture has not been run.** Everything after the
  capture thread is tested: the simulated mic runs the same code from the live tail
  onwards. Recording preflight passes on this Mac for both modes, and the call route is
  the built-in mic plus the Core Audio process tap. But actually capturing needs you to
  answer the macOS permission prompts. To try:
  1. Open the app, allow both permissions in onboarding.
  2. Record in person for 30 seconds.
  3. Record a call while a video plays.
- **The heuristic "You / Them" live labels** compare mic and call loudness for each line.
  They're labelled approximate in the UI. Upstream has no live speaker labels; real
  speaker names come after processing. In-person meetings show no live labels.
- **Ollama models that think first, such as qwen3, time out.** minutes-core gives LLM
  calls a fixed 2 minutes and doesn't turn thinking off (UPSTREAM-PATCHES.md #3). Use a
  non-thinking model (llama3.2, gemma3) or Claude. Margin's error message says this.
- **Permissions reset on every rebuild.** The app is ad-hoc signed, so macOS treats each
  build as a new app. Sign it with a stable identity (even a self-signed one) to keep them.
- **The system-audio permission check uses private TCC calls.** minutes-core has no check
  for it. `TCCAccessPreflight` and `TCCAccessRequest` are loaded with dlopen, as in Apple's
  AudioCap sample. If Apple changes them, Margin shows "Open System Settings" instead of
  "Allow".
- **On call recordings core labels lines by source,** `MICROPHONE` (you) and `SYSTEM` (the
  call). Margin shows them as You / Them, and you can rename them. All voices on the call
  side share one label unless diarization separates them.
- **Diarization quality is minutes-core's.** On synthetic TTS voices it found 3 speakers
  for 2.
- **Not in v1:**
  - Calendar-based titles and attendees (deliberately off; DECISIONS.md D9).
  - Watching the folder for changes made by other apps. The meeting view reloads when
    the window regains focus instead.
  - Live partial transcripts. You see finished lines only, a few seconds behind.
  - Signing, notarization, auto-update.
- **Voices are shared with the upstream Minutes app.** minutes-core hardcodes
  `~/.minutes/voices.db` (UPSTREAM-PATCHES.md #1).

## Where things live

| What | Where |
|---|---|
| Meetings (`.md` and `.wav`) | the folder you chose, default `~/Documents/Meetings` |
| Settings (minutes-core config) | `~/Library/Application Support/Margin/config.toml` |
| Models, jobs, logs, state | `~/Library/Application Support/Margin/state/` |
| Ask conversations | `…/Margin/state/chat/` |
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
