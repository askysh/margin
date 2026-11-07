# Performance

Measured on 2026-11-06 on an Apple Silicon Mac. Other builds ran on the same Mac at the
same time, so the load average was between 5 and 40. To make a comparison fair, the
"before" and "after" runs were interleaved, and each figure is a median. Where the load
changed between two tables, compare only figures in the same table.

"Before" is commit `64649b4`. "After" is this branch.

## How to measure

All runs use a sandbox `HOME` and `MARGIN_HOME`. No run opens the app window or asks for a
permission. The scripts are in `docs/perf-scripts/`.

| Script | What it measures |
|---|---|
| `genlib.py <dir> 600` | Writes 600 one-hour meetings (48 MB) with summaries, notes and speaker maps. |
| `bench.mjs <port>` | Latency of the main commands through `margin --dev serve` (POST /invoke). |
| `startup.mjs <bin> <dist> …` | Process start to bridge ready; page start to the first `boot` call; to the library rows; to the notepad font. |
| `idle.sh <bin> <port> <s>` | CPU time and memory of an idle backend: the jobs watcher and the call detector. |
| `rec.sh <debug-bin> …` | The same during a simulated call recording (debug builds only). |
| `ticks.mjs <bin> <dist> …` | UI main-thread time for one live tick and one new live line, with 600 meetings in the sidebar and a 700-line live transcript. |

For the idle test, the sandbox also holds 600 archived job files. A real library has one
for each processed meeting, because minutes-core never removes them.

The app's own startup (process start to the first `boot` call in a real window) was not
measured: that needs the app window. The bridge figures stand in for it.

## Results

### Size

| | Before | After |
|---|---|---|
| Margin.app | 39.27 MiB | 34.98 MiB (−11%) |
| Binary in the app | 41,055,232 B | 36,551,200 B |
| Startup JS chunk | 194.05 KB (61.6 KB gzip) | 178.6 KB (58.4 KB gzip) |
| Settings / Onboarding chunks | in the startup chunk | 8.4 KB / 8.7 KB, loaded on first use |
| CSS | 21.0 KB | 24.5 KB (new onboarding styles) |
| Fonts | 682 KB in 4 subsets | unchanged |

In 0.0.5, Settings became a page with a section nav, and its chunk grew to about 22 KB
(7.5 KB gzip). It is still loaded on first use.

Note: with the old profile, the new code gave a 39.37 MiB app. The smaller app comes from
the release profile (DECISIONS.md D36).

### Command latency (release build, 600 meetings)

| Command | Before | After |
|---|---|---|
| `boot` | 0.5–2.3 ms | 0.5–1.0 ms |
| `list_meetings` | 32–39 ms | 3.5–6 ms |
| `read_meeting` (one hour) | 3.2–3.8 ms | 1.9–3.1 ms |
| `search_meetings`, a hit | 55 ms | 8–13 ms |
| `search_meetings`, no hit | 85 ms | 10–20 ms |
| `list_jobs` (600 archived) | 7–11 ms | 7–14 ms (unchanged; called once at startup) |
| `permissions_state` | 20–43 ms | unchanged (two TCC round trips) |

### Idle backend (release build)

| | Before | After |
|---|---|---|
| CPU, no archived jobs | 0.05% | 0.03% |
| CPU, 600 archived jobs | **2.1%** | **0.03%** |
| Memory | 14.0–17.2 MB | 14.4 MB |

### Recording (debug build, simulated call, 600 archived jobs)

| | Before | After |
|---|---|---|
| Backend CPU | 5.4–8.1% | 0.9% |
| Memory | 29.6–34.0 MB | 28.5 MB |

Most of the difference is the jobs watcher. The 500 ms ticker itself is cheap.

### UI during a recording

| Per event | Before | After |
|---|---|---|
| Live tick (twice a second) | 7.8–16 ms | 0.6–1.4 ms |
| New live line | 12–18 ms | 5–8 ms |

These figures are React work only (layout and paint are not included), measured under load.
The rest of the "new line" cost is the scroll to the newest line.

### Startup through the bridge (onboarded, 600 meetings)

| Median | Before | After |
|---|---|---|
| Backend ready | 17–23 ms | 17–24 ms |
| Page start to first `boot` call | 52–58 ms | 54–56 ms |
| Page start to the library rows | 275–315 ms | 295–311 ms |
| Page start to the notepad font | 96–108 ms | 102–111 ms |

No measurable change. A font preload was tried and removed, because it did not change the
font time (DECISIONS.md D37).

### Processing the 39 s test file (release build)

| | Before | After |
|---|---|---|
| Warm, quiet Mac | 2.9–3.4 s | not re-measured quiet |
| Warm, same load, interleaved | 7.3 s | 7.4 s |

Processing is all minutes-core (whisper, diarization), and Margin's changes do not touch
it. The first run of a new binary takes 10–29 s once, because Metal compiles its shaders.

### Command calls in one e2e pass

| | Before | After |
|---|---|---|
| All commands | 117 | 114 |
| `list_meetings` | 18 | 18 |
| `read_meeting` | 23 | 23 |

The number of refreshes is the same. Each one is cheaper, and a refresh that returns the
same data no longer re-renders the sidebar or the meeting.

## What changed

- **Jobs watcher** (`processing.rs`). Each second it reads only the active job files and
  the timestamps of the archive and drafts folders. It lists the archive only when one of
  those changes. DECISIONS.md D32.
- **Library list cache** (`library.rs`). Entries are kept while a file's size and
  modification time stay the same. DECISIONS.md D33.
- **Search** (`library.rs`). Files are read on up to 8 threads. For an ASCII query, each
  file is lowercased once instead of once per line.
- **Lazy views** (`App.tsx`). Settings and Onboarding load on first use.
- **Fewer re-renders.**
  - `App.tsx`: a list refresh that returns the same data does not update state. Live
    lines are merged by line number.
  - `Sidebar.tsx`: the meeting rows are memoised, so a tick re-renders only the live row.
  - `LiveView.tsx`: the transcript and each line are memoised. The date label is computed
    once.
  - `MeetingView.tsx`: memoised, and a reload that returns the same file does not
    re-render the transcript.
- **Release profile** (`Cargo.toml`): `lto = "fat"`, `codegen-units = 1`. DECISIONS.md D36.

## Not done

- **Fewer refresh calls.** The calls are now cheap, and the e2e checks rely on the focus
  refresh (edits made in other apps). Not worth the risk.
- **Smaller fonts.** The notepad uses the `CASL` and `MONO` axes, which only the "full"
  Recursive file has. An instanced font without the unused `slnt` and `CRSV` axes would
  be smaller, but that needs fonttools, which this Mac does not have.
- **Pruning the job archive.** It would also make `list_jobs` faster, but it changes data
  that minutes-core's CLI reads.
