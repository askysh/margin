# Contributing to Margin

Thanks for helping. Issues, pull requests and ideas are welcome. This page says how to set
up the repository, how to test a change, and what a pull request needs.

By taking part you agree to the [Code of Conduct](CODE_OF_CONDUCT.md). Security problems go
through a private report, never a public issue: see [SECURITY.md](SECURITY.md).

## Before you start

- **A bug:** open an issue with the bug form. Attach the end of
  `~/Library/Application Support/Margin/state/logs/margin.log`. Remove meeting titles,
  names and other text you do not want to share.
- **A small fix:** open a pull request directly.
- **A large change** (a new feature, a change to the meeting file format, anything that
  needs a minutes-core change): open an issue first and say what you want to build. A
  short agreement on the shape saves you a rewrite.

## Set up

You need an Apple Silicon Mac with macOS 14.4 or later, Xcode 26, cmake, bun and Rust from
rustup. [docs/development.md](docs/development.md) has the full steps.

```bash
brew install cmake
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
bun install
export CXXFLAGS="-I$(xcrun --show-sdk-path)/usr/include/c++/v1" MACOSX_DEPLOYMENT_TARGET=14.4
bunx tauri dev                    # hot-reloading UI, debug Rust
```

Set `CXXFLAGS` in each shell that runs cargo. Without it, the whisper.cpp build cannot
find the C++ headers on macOS 26.

## Test a change

```bash
(cd src-tauri && cargo test)                       # Rust unit tests
bunx tsc --noEmit && bun run build                 # UI type check and build (CI runs these)
bun run build && (cd src-tauri && cargo build)     # then the end-to-end pass:
node scripts/e2e/run.mjs --audio meeting.wav \
  --seed-models ~/Library/Application\ Support/Margin/state/models \
  --summaries fake-ollama --simulate call.wav
```

The end-to-end pass runs the real UI against the real backend in a sandbox `HOME`. It
never touches your settings, meetings or voices. Run it for a change to the UI or to the
backend commands. Add `--dark` to check dark mode.
[docs/development.md](docs/development.md#the-end-to-end-pass) explains each option.

CI type-checks and builds the UI on each push. It does not build the Rust side, so run
`cargo test` yourself.

## How the code is written

- **Rust** in `src-tauri/src/` has one module per area: `recorder.rs`, `library.rs`,
  `speakers.rs`, `chat.rs` and so on. A module starts with a `//!` comment that says what
  it does and why. Match the style of the code around your change.
- **TypeScript** in `src/` is React with strict types. `src/api.ts` holds the backend
  commands and their types.
- **Your data first.** A meeting that the user deletes goes to the Trash
  ([DECISIONS.md](DECISIONS.md) D10), and only after the confirmation the app promises.
  When you change one section of a meeting, keep the frontmatter and the other sections
  byte for byte.
- **minutes-core is upstream code.** Do not copy it into this repository. A change that
  Margin needs in minutes-core goes upstream (see below).

## Write docs

All docs use Simplified Technical English, in the tone of [README.md](README.md) and
[DECISIONS.md](DECISIONS.md):

- Short sentences, active voice, one instruction per sentence.
- One name for one thing. If the app says "Record call", the docs say "Record call".
- No marketing words, no semicolons, no exclamation points.
- Numbered lists for steps, with one action per step.

## Record decisions

A choice that a reviewer could question goes into [DECISIONS.md](DECISIONS.md) as the next
`D<number>`. Write the options you considered, the pick, and the reason. If a reviewer
would ask "why not the obvious way?", the answer is a decision.

## What a pull request needs

- **The right branch.** Open the pull request against `next-release`, not `main`. A merge
  into `main` releases the app ([docs/releasing.md](docs/releasing.md)).

- **One topic.** A fix and a refactor are two pull requests.
- **A line in [CHANGELOG.md](CHANGELOG.md)**, in the top section, the one marked
  Unreleased, for a change that people who use Margin can see.
- **A decision in DECISIONS.md** for each choice that is not obvious, or none.
- **Tested on:** the macOS version, and whether you ran it in the real app or only in
  the end-to-end pass.
- **Commits** have a short subject in the imperative ("Add", "Fix", "Move") and a body
  that says why. Keep commits small and logical.

The pull request template asks for all of this.

## Changes to minutes-core

Margin uses minutes-core from upstream,
[silverstein/minutes](https://github.com/silverstein/minutes), pinned by tag or commit.
Margin does not keep a fork.

1. Write the change as a small patch that upstream can take.
2. Add it to [UPSTREAM-PATCHES.md](UPSTREAM-PATCHES.md): what, why and where.
3. Open the pull request on `silverstein/minutes`.
4. After upstream merges it, move Margin to the new ref with `scripts/bump-upstream.sh`
   ([docs/development.md](docs/development.md#updating-minutes-core)).

Until then, Margin works around the gap, and the workaround says so in a comment.

## AI assisted contributions

Margin is itself developed with AI coding agents. [AGENTS.md](AGENTS.md) is the guide
they read. AI assisted pull requests are welcome on these terms:

- **Disclose it.** One line in the pull request: the tool and how much it did, or "None".
- **Own every line.** You must be able to explain each line you submit, and why it is
  there.
- **Run it.** For a change to recording, processing or files on disk, run it in the real
  app, not only in the end-to-end pass.
- **Write issues in your own words.** Do not paste generated text into an issue.

## Releases

Maintainers cut releases. [docs/releasing.md](docs/releasing.md) has the steps.
