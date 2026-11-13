# Margin documentation

A map of the docs in this repository. The [README](../README.md) is for people who use
Margin. Everything here is for people who work on it.

## Work on Margin

| File | Covers |
|---|---|
| [development.md](development.md) | Build, run and test Margin. The developer CLI. Move to a new minutes-core. |
| [releasing.md](releasing.md) | Sign, notarize and publish a release. The in-app update and the Homebrew cask. |
| [windows-store.md](windows-store.md) | Margin Notes on the Microsoft Store: the identity, the package, the listing text and the checks before a submission. |
| [perf.md](perf.md) | Performance measurements, and the scripts in [perf-scripts/](perf-scripts/). |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | How to send a change. |
| [../AGENTS.md](../AGENTS.md) | Guidance for AI coding agents. |

## Why Margin is the way it is

| File | Covers |
|---|---|
| [../DECISIONS.md](../DECISIONS.md) | Product and technical decisions, each with the options considered. |
| [../UPSTREAM-PATCHES.md](../UPSTREAM-PATCHES.md) | minutes-core changes that Margin needs or would like, sent or to send upstream. |
| [research.md](research.md) | The first survey of minutes-core, the choice of shell, the v1 scope and UI sketch. |
| [research/](research/) | The detailed surveys, and [live-transcription.md](research/live-transcription.md): the research behind rolling transcription. |
| [parity/](parity/) | Margin compared with the upstream Minutes app: [app.md](parity/app.md), [settings.md](parity/settings.md) and [chat.md](parity/chat.md). |

## History

| File | Covers |
|---|---|
| [../CHANGELOG.md](../CHANGELOG.md) | What changed in each release. |
| [progress.md](progress.md) | The milestone log of the first builds, up to 0.0.3. |
| [brief.md](brief.md) | The original brief for v1, kept as written. |

## Images

[images/](images/) holds the app icon and the screenshots that the README uses.

## Writing docs

Write in Simplified Technical English: short sentences, active voice, one name for one
thing, no marketing words. Match the tone of [../DECISIONS.md](../DECISIONS.md).
Link to other files with relative paths.
