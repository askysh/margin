# Brief: build v1 of a polished Mac meeting-notes app on top of `minutes-core`

> The original brief from 2026-11-06, kept as written. `RESEARCH.md` and `PROGRESS.md` are
> now [docs/research.md](research.md) and [docs/progress.md](progress.md).

## Who / why
Akash wants the functionality of Minutes (https://github.com/silverstein/minutes, MIT, Rust core + Tauri shell)
with the UI polish and ease of use of Granola. He tried Minutes in July 2026 and hated its UI, in particular
the file-management setup and the lack of any way to reset configuration. The core (local whisper/parakeet
transcription, pyannote diarization, Markdown output into an Obsidian vault, MCP tools) is worth keeping.

## Strategy (decided, do not re-litigate)
- Do NOT fork the Minutes repo. Build a separate app that depends on the upstream `minutes-core` crate
  (and, where simpler, shells out to the `minutes` CLI) pinned to a git tag. Upstream moves daily under one
  maintainer; carrying his UI code means endless rebases.
- A read-only clone of upstream lives at `~/dev/minutes-upstream`. Read it freely. Never commit there.
  If the core API is missing something the UI needs, write the change as a small, upstream-able patch and
  record it in `UPSTREAM-PATCHES.md` (what, why, where) so Akash can open a PR later.
- Open source, MIT, Mac-first (Apple Silicon, macOS 14+). Windows/Linux are out of scope for v1.
- Packaging later follows Akash's benchbar project (`~/dev/benchbar`, tap at `~/dev/homebrew-tap`):
  signed + notarized app distributed via Homebrew cask. Do not set up signing in v1; just keep the build
  cask-friendly (single .app bundle, no installer). Upstream's `.github/workflows/release-macos.yml` is a
  working reference for notarized Tauri builds.
- No in-app cloud sync in v1. Markdown output to a user-chosen folder (iCloud Drive / Obsidian vault) is the
  backup story.

## Phase 1: research (write findings to `RESEARCH.md` before writing app code)
1. Survey `minutes-core` and `minutes-cli` in `~/dev/minutes-upstream/crates/{core,cli}`: public API for
   recording (mic + system audio, bot-free), live/streaming transcription, diarization + speaker renaming +
   voice fingerprints, meeting storage format (Markdown + frontmatter, SQLite index), config loading, vault
   output, MCP. Note what is `pub` vs internal, feature flags, native deps (whisper.cpp, sherpa-onnx, pyannote
   sidecar in `sidecars/`), build time, and how the existing Tauri app in `tauri/src-tauri` calls the core.
2. Decide the shell: Tauri v2 with a real frontend framework (React/Svelte) vs native SwiftUI calling the Rust
   core via FFI / the CLI. Pick whichever gets a polished v1 fastest given what the core exposes. State the
   trade-off in one paragraph. Default lean: Tauri v2, because the core is Rust and upstream already proves
   the integration.
3. Define a ~1-page v1 scope and a UI sketch (text/ASCII is fine) modelled on Granola: a notepad-first main
   window, live transcript side panel, meeting library, minimal settings.

## Phase 2: build v1
Must-haves:
- First-run onboarding: pick output folder (default `~/Documents/Meetings`, offer to pick an Obsidian vault),
  download models with progress, request mic + screen-recording permission with clear explanations.
- Record a meeting (mic + system audio) and a mic-only in-person meeting. Start/stop from main window and
  menu bar.
- Live transcript with speaker turns; after the meeting, rename speakers (click label, rename everywhere,
  persist so the voice is recognised next time if the core supports it).
- Meeting library: list, search, open; each meeting is a Markdown file with YAML frontmatter that Obsidian
  reads directly. Edit notes in-app and have the file update.
- Settings with a visible, working **Reset** (wipe config, keep or delete meetings, re-run onboarding).
  This is the pain point that drove the project; make it obvious and safe.
- Summaries via the user's own `claude` CLI or Ollama, both optional; app must be fully usable with neither.
Nice-to-have if cheap: calendar-based auto-detect of Zoom/Meet/Teams calls (core already has call detection).

Working style:
- Commit early and often in this repo with clear messages. Keep a `PROGRESS.md` updated after each milestone.
- Prefer small vertical slices that run end to end over broad scaffolding.
- Rust toolchain is NOT installed yet; install with `rustup` (https://rustup.rs) before building. Node 22,
  bun and Xcode are present. Upstream CONTRIBUTING.md notes macOS 26+ needs
  `export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1"` for cargo builds.
- When blocked on a product decision, write the options and your pick into `DECISIONS.md` and continue.
  Ask Akash only when a decision would waste more than an hour if wrong.
- Finish with a `README.md` that says how to build, run, and what works. Report honestly what does not.
