# Settings parity with the upstream Minutes app

This table lists every user-facing setting in the upstream Minutes desktop app at
v0.32.1 and compares it with Margin. DECISIONS.md D54 adds the core options at commit
209265f.

Sources:
- The upstream settings panel: `tauri/src/index.html`.
- Its backend: `cmd_get_settings` and `cmd_set_setting` in `tauri/src-tauri/src/commands.rs`.
- The core config: `crates/core/src/config.rs` at tag v0.32.1.

Status:
- **has**: Margin had it before this work.
- **added**: added by this work. The e2e pass checks it.
- **skipped**: not in Margin. The last column gives the reason.

## General and identity

| Upstream setting | What it does | Margin | Reason for a skip / notes |
|---|---|---|---|
| Your name (`identity.name`) | Labels you in transcripts and summaries. | has | Settings → General. |
| Your email addresses (`identity.emails`) | Folds calendar attendees onto you. | skipped | Margin has no calendar (D9). |
| Name variants (`identity.aliases`) | Other forms of your name, for hints and matching. | skipped | "Names and words to spell right" covers the transcription use. |
| Display language (`ui.language`) | Language of the app's menus and text. | skipped | Margin is English only in v1. |
| Theme, accent colors (`customize.toml`) | Light/dark and accent color. | skipped | Margin follows the system appearance. Few choices by design. |
| Show completion alerts (`notifications.completion_enabled`) | A macOS notification when a meeting is ready. | added (no setting) | The app-parity work sends it when the window is not in front (DECISIONS.md D22). macOS System Settings turns it off. |
| Critical Coach alerts (`notifications.copilot_critical_enabled`) | Coach notifications while its HUD is hidden. | skipped | No Coach in Margin. |
| Sound cues | Sounds for start, stop and complete. | skipped | Low value. One more choice. |
| Open Minutes at login (autostart plugin) | Starts the app when you log in. | added | "Open Margin when you log in". A LaunchAgent opens Margin in the menu bar only. |
| Quick Thought shortcut (`global_hotkey`) | A global key to record a voice memo. | skipped | Margin has no voice memos. |

## Capture and recording

| Upstream setting | What it does | Margin | Reason for a skip / notes |
|---|---|---|---|
| Microphone picker (`recording.device`) | The input device. Upstream shows it in the main window. | added | Settings → Recording → Microphone. Calls use it too. A missing device falls back to the system default. |
| Meeting audio source (`recording.desktop_audio_source`) | How system audio is captured. | skipped | Margin always uses the Core Audio process tap (D5). |
| Live transcript backend (`live_transcript.backend`) | Engine for the live transcript. | skipped | Developer feature. Only whisper works at v0.32.1. |
| Live transcript shortcut (`live_transcript.shortcut`) | A global key for a standalone live transcript. | skipped | Margin has no standalone live transcript. |
| Command palette shortcut (`palette`) | A global key for the palette. | skipped | Margin has no command palette. |
| Silence auto-stop (`recording.silence_auto_stop_secs`) | Stops a recording after a period with no sound. Config file only upstream. | added | "Stop after silence": Never, 5, 15, 30 (default) or 60 minutes. |
| Silence reminder, max duration, minimum disk space (`recording.*`) | Other recording safety limits. Config file only. | skipped | Core defaults are good. Few choices. |
| Recording shortcut | Margin's ⌘⇧R / ⌘⇧I work in the window. Upstream has no global record key. | added | ⌃⌥⌘R starts and stops a recording from any app. It is off by default (DECISIONS.md D23). |

## Transcription

| Upstream setting | What it does | Margin | Reason for a skip / notes |
|---|---|---|---|
| Engine (`transcription.engine`) | Whisper, Parakeet, Apple Speech. | skipped | Parakeet and Apple Speech resolve to whisper at v0.32.1. At 209265f, Parakeet v3 through sherpa needs a plugin library and a build feature that Margin does not ship (D54). Margin pins whisper. |
| Whisper model (`transcription.model`) | Model size. | has | Fast / Balanced / Best (base, small, large-v3-turbo). |
| Parakeet model, sidecar, install | Parakeet engine options. | skipped | Developer feature. Gated off in core. |
| Transcription language (`transcription.language`) | Forces a language, or auto-detects. | added | "Language spoken in meetings": Auto-detect or one of whisper's 100 languages. |
| Speaker diarization (`diarization.engine`) | Tells speakers apart. | added | "Tell speakers apart". Off writes `engine = "none"`. |
| Personal dictionary (`vocabulary.toml`) | Names and terms to spell right. Upstream adds them from speaker confirmations and dictation. | added | "Names and words to spell right". Core passes up to 8 to whisper as hints. |

## Dictation

| Upstream setting | What it does | Margin | Reason for a skip / notes |
|---|---|---|---|
| Dictation shortcut, destination, writing style, backend, model, voice commands, daily note, silence timeout, auto paste, snippets, history, recovery shortcuts, post-dictation hook, dictation microphone | Speak into any app. | skipped | Dictation is not a meeting notepad feature. |

## AI and privacy

| Upstream setting | What it does | Margin | Reason for a skip / notes |
|---|---|---|---|
| Summarization engine (`summarization.engine`) | Off, agent CLI, Ollama, OpenAI-compatible, Claude API, OpenAI API. | has | None, Claude (agent CLI) and Ollama. The API-key engines are skipped: they need a key store, and Claude through the CLI covers the same need. |
| Agent command (`summarization.agent_command`) | Which agent CLI to run. | has | Margin finds `claude` and shows its path. |
| Ollama URL (`summarization.ollama_url`) | Address of the Ollama server. | skipped | The default `localhost:11434` fits nearly everyone. |
| Ollama model (`summarization.ollama_model`) | Which local model. | has | A list of the installed models. |
| Summary language (`summarization.language`) | The language summaries are written in. Config file only upstream. | added | "Summary language": Same as the meeting, or a named language. |
| Summary instructions (template `additional_instructions`) | Extra prompt text. Upstream needs a template file. | added | "Instructions for summaries". Margin writes a project template (see DECISIONS.md). |
| Search by meaning (semantic index) | Embedding search over meetings. | skipped | Power feature. A 310 MB extra download. |
| Coach (`copilot.*`) | Live coaching during meetings. | skipped | Power feature. Out of scope. |
| Pipeline hook (`hooks.post_record`) | A shell command after each meeting. | skipped | Developer feature. |
| Calendar filters (`calendar.*`) | Which events are suggested and matched. | skipped | Calendar is off in v1 (D9). |
| Call detection (`call_detection.enabled`) | Notices call apps. | added | "Suggest recording when a call starts". |
| Call detection interval, cooldown, Meet and Teams web detection, auto-record, stop when call ends | Tuning and automation for call detection. | skipped | Browser detection needs an automation permission. Auto-record is a large product decision. Few choices. |
| Hide windows from screen sharing (`privacy.hide_from_screen_share`) | Keeps the app out of shared screens. | added | "Hide Margin from screen sharing", on by default as in core. Margin applies it to the main window and the recording chip (DECISIONS.md D54). |
| Recording consent reminder (`consent.*`) | A disclosure before recording. | skipped | Margin leaves this to the user. Candidate for later. |
| Desktop context, screen context (`desktop_context.*`, `screen_context.*`) | Window titles and screenshots during meetings. | skipped | Needs Accessibility and Screen Recording permissions. |
| Vault sync (`vault.*`) | Symlinks or copies meetings into Obsidian. | skipped | Margin writes to the chosen folder directly (D4). |
| Recall agent (`assistant.*`) | The AI chat panel's agent and flags. | skipped | Another branch owns AI chat. |

## Advanced

| Upstream setting | What it does | Margin | Reason for a skip / notes |
|---|---|---|---|
| Storage stats | Disk use of meetings and audio. | skipped | Reset shows the model size. Candidate for later. |
| Audio retention (`retention.*`) | Deletes old audio. Config file only upstream. | added | "Keep audio": Forever (default), 30 days or 7 days. Transcripts and notes stay. |
| Diagnostics, recovery | Readiness checks and retry of failed captures. | skipped | Margin retries jobs in the processing view and keeps failed audio in `failed-captures/`. |
| Edit config.toml, config reference | Opens the file and the docs. | skipped | Margin pins some keys on every save (`corecfg.rs`). Hand edits are for developers. |
| Terminal for the agent | A terminal instead of chat. | skipped | Developer feature. |

## Where each added setting is stored

| Setting | Stored in |
|---|---|
| Keep audio | `config.toml` `[retention]`: `auto_cleanup`, `successful_audio_days`, `failed_audio_days` |
| Microphone | `config.toml` `recording.device` |
| Stop after silence | `config.toml` `recording.silence_auto_stop_secs` |
| Suggest recording when a call starts | `config.toml` `call_detection.enabled` |
| Hide Margin from screen sharing | `config.toml` `privacy.hide_from_screen_share` |
| Open Margin when you log in | `~/Library/LaunchAgents/com.akashmishra.margin.login.plist` |
| Language spoken in meetings | `config.toml` `transcription.language` |
| Tell speakers apart | `config.toml` `diarization.engine` |
| Names and words to spell right | `state/vocabulary.toml` (entries with `source = "manual"`) |
| Summary language | `config.toml` `summarization.language` |
| Instructions for summaries | `state/.minutes/templates/meeting.md` |
