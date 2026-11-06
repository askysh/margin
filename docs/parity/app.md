# App parity: Margin and the upstream Minutes desktop app

This file compares Margin with the upstream Minutes desktop app (`tauri/` at tag
v0.32.1). It covers the meeting view, the library, recording, the menu bar and
notifications. Settings, AI chat, onboarding and performance are in other documents.

Status values:
- **has**: Margin had it before this work.
- **added**: added by this work.
- **added (Margin only)**: upstream does not have it. Margin adds it because it fits a
  notepad.
- **skipped**: not in Margin. The reason is on the same line.

Upstream paths are relative to `~/dev/minutes-upstream/tauri/`.

## Meeting view

| Upstream feature | Where upstream | Margin |
|---|---|---|
| Meeting detail with summary sections | `src/index.html` `renderDetailSections` | has |
| Date, duration and attendee chips | `index.html` ~11786 | has (one meta line) |
| Decisions card | `index.html` ~11803 | has (the Decisions section of the summary) |
| Action items with checkboxes that write the file (`cmd_set_action_item_status` → `markdown::set_action_item_status`) | `index.html` ~11829, `commands.rs` 8731 | **added**. Owner and due date show under each item. A summary that has only a body checklist can also be ticked. |
| Copy action items | not upstream | **added (Margin only)**: a Copy link on the Action items block |
| Resummarize (`resummarize::resummarize_meeting`) | `index.html` ~10792 | **added** as "Summarize again", with a confirm step |
| Copy as Markdown (raw `.md`) | `palette_dispatch.rs` 678 | **added** as Copy → Whole meeting. It leaves out the frontmatter and uses the speaker names you see. |
| Copy summary, notes or transcript of one meeting | tray only, latest meeting | **added** to the Copy menu for each meeting |
| Rename the meeting | palette `rename-current-meeting` | has (edit the title in place) |
| Speakers card, confirm a speaker (overlay row only) | `index.html` ~11894 | has (rename everywhere in the Markdown, and remember the voice) |
| "Remember" a person in the vocabulary | `cmd_remember_vocabulary_person` | skipped: voice recognition already labels the person next time |
| Edit your notes in place | not upstream | has |
| Play the recording | not upstream | **added (Margin only)**: a slim player under the title |
| Click a transcript time to play from there | not upstream | **added (Margin only)**. The line that plays is marked. |
| Related people and topics chips (run a search) | `index.html` ~12926 | **added** as clickable attendee names that filter the library |
| Open commitments, nearby meetings, drafts | `index.html` ~12040 | skipped: graph and artifact features, not a notepad |
| Discuss with agent, Create draft (memo, follow-up email) | `index.html` ~19171, ~20281 | skipped: AI chat belongs to another workstream |
| Sensitive meeting debrief | `cmd_run_meeting_debrief` | skipped: Margin has no sensitive (no-audio) meetings |
| Open in editor | `cmd_open_file` | skipped: Show in Finder and Open in Obsidian cover it |
| Copy path | `cmd_copy_path` | skipped: rarely needed; Show in Finder shows it |
| Show in Folder | `cmd_reveal_path` | has (Show in Finder) |
| Archive with Undo | `cmd_archive_meeting` | skipped: Margin moves meetings to the Trash (DECISIONS.md D10), and the Trash is the undo |

## Library

| Upstream feature | Where upstream | Margin |
|---|---|---|
| Group by date | `groupByDate` ~11508 | has (Today, Yesterday, This week, months) |
| "· Memo" label on rows | ~12727 | has |
| Search with snippets (`search::search`) | ~13195 | has (Margin's own lock-free search) |
| Semantic search sections | `cmd_semantic_search` | skipped: needs the semantic index; plain search finds names and words |
| ⌘F to search | ~12655 | has |
| Arrow keys between rows | ~12655 | skipped: low value; the performance workstream owns the list |
| Rows rendered 80 at a time | `MEETING_ROWS_CHUNK` | skipped: performance workstream |
| Filter by a person | only via a person chip that runs a search | **added**: click an attendee name; a "With Priya" pill filters the list |
| Sort options, tag or type filters | not upstream | skipped: upstream has none, and newest first is enough |
| Folded dictation rows, Documents toggle | ~12541, ~12441 | skipped: Margin has no dictation or documents |
| File watcher reloads the list | `main.rs` ~1720 | skipped: Margin reloads when the window gets focus (README) |

## Recording

| Upstream feature | Where upstream | Margin |
|---|---|---|
| Start and stop, call or in person | `cmd_start_recording`, `cmd_stop_recording` | has |
| Elapsed time and level meter | recording bar ~6383 | has (the red rule and the clock) |
| Audio cues on start, stop, ready and failure | `index.html` 8489, 8682; `src/audio/` | **added**: the same four sounds, with a Settings switch (on by default) |
| Pause and resume | not upstream, not in core | skipped: core v0.32.1 has no pause (UPSTREAM-PATCHES.md #7) |
| Mute my mic (call recordings only) | tray, `streaming::toggle_mic_mute_with_sentinel` | skipped: the sentinel is in `~/.minutes`, so it would mute the upstream app too (UPSTREAM-PATCHES.md #6) |
| Add Note window and inline note field | `note.html`, `cmd_add_note` | skipped: the live notepad is the note field |
| Memo (quick thought) mode | `CaptureMode::QuickThought` | skipped: not a meeting notepad feature |
| Live-transcript-only mode | `cmd_start_live_transcript` | skipped: the live notepad already shows live lines |
| Sensitive meeting (markers, no audio) | `cmd_sensitive_start` | skipped: few users, more choices |
| Consent disclosure modal (`consent.mode`, core default `remind`) | `consentRequired` ~10513 | skipped: a modal before every recording is friction; telling people is left to you. Candidate for later as a one-line reminder. |
| Extend recording | `cmd_extend_recording` | skipped: upstream has no UI for it either |
| Microphone picker | `cmd_list_devices` | skipped here: Settings workstream |
| Output notice with Open and Retry | ~6440 | has (processing view with Retry, then the meeting opens) |
| Jobs and recovery centres | `cmd_processing_jobs`, `cmd_recovery_items` | has in part: jobs show in the sidebar; interrupted audio goes to `failed-captures/` |
| Global shortcut | only a memo hotkey, ⌘⇧M, off by default | **added (Margin only)**: ⌃⌥⌘R starts and stops a meeting recording from any app. Off by default, switch in Settings. |

## Menu bar

| Upstream feature | Where upstream | Margin |
|---|---|---|
| Open, Start recording, Stop, Quit | `main.rs` ~2555 | has |
| Icon changes while recording; tooltip | `apply_tray_activity` ~1030 | has |
| Elapsed time next to the icon | not upstream | **added (Margin only)** |
| "Recording: <title>" line | not upstream | **added (Margin only)** |
| Recent meetings | not upstream | **added (Margin only)**: the five newest, in a submenu |
| Copy latest summary or transcript | `paste_latest_artifact` | skipped: Copy is in the meeting view, and one place is enough |
| Open meetings folder | tray `list` | skipped: Show in Finder in each meeting |
| Calendar block (Now, Today, Tomorrow) | `tray_schedule.rs` | skipped: calendar is off (D9) |
| Quick thought, Sensitive, Coach, Voice, dictation items, Recall | `main.rs` ~2570–2660 | skipped: features Margin does not have |
| Hide from screen share, Check for updates | `main.rs` ~2655 | skipped: Settings and updates are other workstreams |

## Notifications

| Upstream feature | Where upstream | Margin |
|---|---|---|
| Processing complete, when the window is not in front | `maybe_show_completion_notification` 5455 | **added**: "Meeting ready". A click opens the meeting. No setting; macOS System Settings can turn it off. |
| Processing failed | same | **added**: "A meeting couldn't be processed" |
| Call detected (notification plus in-window banner) | `call_detect.rs` | **added**: the notification, when the window is not in front, has a **Record call** button. The banner was there already. |
| Reminder every 20 s while a call runs unrecorded | `call_detect.rs` 682 | skipped: it nags; one notice is calm |
| Bring the window forward on first detection | `cmd_show_main_window` | skipped: it takes focus from the call |
| Auto-record per app, stop when the call ends | `call_auto_record.rs` | skipped: Margin's detector sees only process names (it cannot tell an open Zoom from a call), so automatic start or stop would be wrong too often |
| Capture health notices (remote silence, call audio lost) | `commands.rs` ~3780–4520 | skipped: they come from upstream's own call capture code in the Tauri crate, not from core |
| Update, coach and dictation notices | various | skipped: features Margin does not have |

## Other upstream surfaces (all skipped)

- Dictation: overlay window, history, text insertion, its own cues.
- Coach / copilot HUD.
- Voice conversation mode.
- Command palette (`palette/`, ⌘⇧K).
- Terminal and PTY.
- Recall assistant chat, weekly summary, assistant skills, context bundle. AI chat is
  another workstream.
- Calendar: upcoming-meeting banner, meeting prompt window (`meeting-prompt.html`), tray
  schedule. Calendar is off in Margin (D9).
- Updates and What's New; themes; UI language; UI zoom; window-state plugin.
- Readiness and permission centre, onboarding nudges, Parakeet invite, CLI setup. These
  belong to the onboarding and Settings workstreams.

## How the added features work

- **Playback.** `meeting_audio` adds only that meeting's `.wav` to Tauri's asset scope
  (`assetProtocol` is on with an empty scope). The page loads it with `convertFileSrc`.
  The QA bridge serves the same file at `/asset?path=…` with byte ranges, and only when
  a meeting in the meetings folder owns the `.wav`.
- **Action items.** Core's `action_items` frontmatter is the source when it is present.
  Otherwise Margin reads the `- [ ]` lines under `## Action Items`. Ticking a structured
  item calls core's `set_action_item_status`, which also changes the matching body line.
- **Notifications.** Backend code sends an `events::NOTIFY` event. The app shows it with
  `tauri-plugin-notification` when the window is not visible and focused. The QA bridge
  only queues it, so the tests can check it.
- **Global shortcut.** `tauri-plugin-global-shortcut`, registered from Rust. It is
  registered only when the pref is on and setup is complete, and Reset drops it.
- **Cues.** The four `cue-*.wav` files from upstream (MIT) are in `public/audio/`. The
  page plays them on the `recorder`, `recording-stopped`, `meeting-ready` and `jobs`
  events.
