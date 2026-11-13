# Guidance for AI coding agents

Margin is a meeting notepad for macOS: Tauri v2, a React UI in `src/` and Rust in
`src-tauri/`, built on minutes-core from
[silverstein/minutes](https://github.com/silverstein/minutes). This file tells you where
things are, how to build and test, and what not to touch. People who contribute follow the
same rules ([CONTRIBUTING.md](CONTRIBUTING.md)).

## Repository map

| Path | What it holds |
|---|---|
| `src/` | The React UI. `App.tsx` is the shell, `api.ts` the backend commands and their types, `components/` one file per view. |
| `src-tauri/src/` | The Rust backend, one module per area. `main.rs` registers the commands. `commands.rs` is the command surface the UI calls. |
| `src-tauri/src/recorder.rs`, `rolling.rs`, `processing.rs` | Recording, rolling transcription, and the processing worker. |
| `src-tauri/src/chip.rs`, `src/chip/` | The recording chip: its window, and its page (`chip.html`). |
| `src-tauri/src/applespeech.rs`, `src-tauri/speech/` | The Apple Speech live transcript and its Swift helper, `margin-speech` (D56). |
| `src-tauri/src/library.rs`, `speakers.rs` | Meeting files: read, edit notes and titles, rename speakers. |
| `src-tauri/src/chat.rs` | Ask: runs the `claude` CLI with read-only tools. |
| `src-tauri/src/reset.rs`, `paths.rs`, `corecfg.rs` | Reset, every path Margin uses, and the minutes-core config. |
| `src-tauri/src/devcli.rs`, `devserve.rs` | The `margin --dev` CLI and the QA bridge for the e2e pass. |
| `src-tauri/Cargo.toml`, `Cargo.lock` | The minutes-core pin and the lock seeded from upstream. |
| `windows/` | The Microsoft Store package for Windows: the MSIX manifest, its logos and the listing images (D61). |
| `scripts/` | `build-app.sh`, `release.sh`, `bump-version.sh`, `bump-upstream.sh`, the Homebrew renderer and `e2e/`. |
| `docs/` | Development, releasing, research and parity notes. [docs/README.md](docs/README.md) is the map. |
| `DECISIONS.md` | Every product and technical decision, numbered `D1`, `D2` and so on. |
| `UPSTREAM-PATCHES.md` | minutes-core changes that Margin needs, sent or to send upstream. |

## Build and test

Every shell that runs cargo or `bunx tauri` on macOS 26 needs this line. Without it,
whisper.cpp does not find the C++ headers and the build fails:

```bash
export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
```

```bash
bun install
(cd src-tauri && cargo test)                       # Rust unit tests
bunx tsc --noEmit && bun run build                 # UI type check and build
bun run build && (cd src-tauri && cargo build)     # build for the e2e pass
node scripts/e2e/run.mjs --audio meeting.wav \
  --seed-models ~/Library/Application\ Support/Margin/state/models \
  --summaries fake-ollama --simulate call.wav      # end-to-end, in a sandbox
scripts/build-app.sh                               # Margin.app, not signed
```

A cold Rust build takes minutes. Run the e2e pass after a change to the UI or to a backend
command. [docs/development.md](docs/development.md) explains each option and the developer
CLI.

## Never touch the user's real data

The owner uses Margin every day. Their meetings, settings and voices are real.

- For the developer CLI, set `MARGIN_HOME` and `HOME` to a temporary folder first.
  minutes-core always uses `~/.minutes/voices.db`, so `HOME` must change too.
- Do not read or write `~/Library/Application Support/Margin`, `~/Documents/Meetings`, an
  Obsidian vault, `~/.minutes` or `~/.config/minutes`, except to read the models folder
  for `--seed-models`.
- The e2e pass makes its own sandbox. Use it in place of the real app for QA.

## Conventions

- **Your data first.** A meeting that the user deletes goes to the Trash, and only after
  the confirmation the app promises (DECISIONS.md D10). An edit to one section keeps the
  frontmatter and the other sections byte for byte.
- **Match the code around you.** A Rust module starts with a `//!` comment that says what
  it does and why. Comments explain the reason, not the code.
- **Commits** are small and logical, with an imperative subject and a body that says why.
- **Branches.** Every change goes into `next-release`. Only `next-release` merges into
  `main`, and that merge tags and drafts a release (docs/releasing.md).
- **CHANGELOG.md** gets a line in its top section, the one marked Unreleased, for each
  change a user can see.

## Docs

Write every doc, commit message and code comment in Simplified Technical English:

- Short sentences, active voice, one instruction per sentence.
- One name for one thing. Use the names the app shows.
- No marketing words, no semicolons, no exclamation points.

Match the tone of [README.md](README.md) and [DECISIONS.md](DECISIONS.md).

## Decisions

A choice that a reviewer could question goes into [DECISIONS.md](DECISIONS.md) as the next
`D<number>`: the options, the pick and the reason. Do not reopen a decision that is
already there without a new fact. Say what the new fact is.

## minutes-core

- minutes-core comes from upstream `silverstein/minutes`, pinned by tag or commit in
  `src-tauri/Cargo.toml`. Margin keeps no fork, and minutes-core code is never copied into
  this repository.
- Move to a new ref only with `scripts/bump-upstream.sh <tag-or-full-sha>`. It re-seeds
  `src-tauri/Cargo.lock` from upstream's lock, then builds and tests.
- A gap in minutes-core goes into [UPSTREAM-PATCHES.md](UPSTREAM-PATCHES.md) as a patch
  that upstream can take. Work around it in Margin until upstream ships it.

## Do not change without asking

- `src-tauri/Cargo.lock` by hand, and the `ort = "=2.0.0-rc.10"` pin. Later `ort` releases
  break `pyannote-rs`.
- The updater `pubkey` in `src-tauri/tauri.conf.json`. Installed copies verify updates
  with it. A new key breaks updates for everyone.
- The bundle identifier `com.akashmishra.margin`. macOS ties the permissions to it.
- The identity in `windows/msix/AppxManifest.xml` (Name, Publisher, PublisherDisplayName).
  It comes from Partner Center, and the Store refuses a package where it differs.
- The release and Homebrew workflows in `.github/workflows/`, and their secrets.
- The meeting file format. Obsidian, the minutes CLI and MCP read the same files.
- `tsconfig.tsbuildinfo`. The build changes it. Do not commit those changes.
