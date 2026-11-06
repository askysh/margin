# Parity: chat ("Recall" upstream, "Ask" in Margin)

This note compares upstream Minutes' Recall chat (desktop app, minutes-core v0.32.1 and
the current `~/dev/minutes-upstream/tauri`) with Margin's Ask. It says what Margin copies
and what it does not copy.

## What upstream does

Upstream has two Recall surfaces:

1. **Terminal Recall** (`pty.rs`, `context.rs`, `assistant_skills.rs`). It starts the
   user's agent CLI (claude, codex and others) in a PTY with xterm.js, in a workspace
   directory. It writes a `CLAUDE.md` and skill files into that workspace, and gives the
   agent the full tool set. This is a terminal, not a chat.
2. **Native chat** (`cmd_recall_chat_send` and its helpers in `commands.rs`,
   `src/scripts/recall-chat.js`, the contract in `src/contracts/recall-chat.json`).
   - Providers: Claude CLI (subscription or API key), Codex CLI, Ollama, or an
     OpenAI-compatible local server.
   - Claude runs as `claude -p --safe-mode` (or `--bare` with an API key),
     `--setting-sources ""`, `--no-session-persistence`, `--strict-mcp-config` with an
     empty config, **`--tools ""`** (no tools at all) and its own `--system-prompt`. The
     prompt goes on stdin. Output is `stream-json --verbose --include-partial-messages`.
   - The working directory is an empty "chat" folder. Claude cannot read any file.
   - Context is put in the prompt by the app: the focused meeting (first 6,000
     characters of the body) and up to 5 keyword search snippets from other meetings
     (1,200 characters each), 12,000 characters in all.
   - Follow-ups do not resume a session. The app keeps the last 6 turns in memory and
     prepends them to each new prompt.
   - History is in memory only. It is lost on restart.
   - Before each turn it checks `claude --help` for the isolation flags and
     `claude auth status --json` for `loggedIn`. If either check fails, it falls back to
     the terminal.
   - Stop kills the process group (SIGTERM, then SIGKILL).
   - It re-checks the sha256 of each source meeting before it sends context, and checks
     that the config did not change mid-turn. It caps the response at 8 MiB.
   - The frontend events are `recall-chat-chunk` (raw stream-json frames),
     `recall-chat-done` and `recall-chat-error`.

## What Margin copies

- The headless `claude -p` call with `--output-format stream-json --verbose
  --include-partial-messages`, the prompt on stdin, and a Margin system prompt.
- The isolation flags: `--safe-mode` (when this claude has it), `--setting-sources ""`,
  `--strict-mcp-config` with an empty MCP config. With these flags, the user's
  CLAUDE.md, hooks, plugins and MCP servers do not load.
- The readiness checks: `claude --help` for the flags Margin needs, and
  `claude auth status --json` for `loggedIn`.
- The stop method: a process group per turn, SIGTERM, then SIGKILL.
- The "a stream that did not finish is not a finished answer" rule. A stopped turn is
  saved as "Stopped." with the partial text. A turn without a `result` line shows an
  error.

## What Margin does differently, and why

| Topic | Upstream | Margin | Why |
|---|---|---|---|
| Tools | None (`--tools ""`) | `Read,Grep,Glob` with `--permission-mode dontAsk`, cwd = meetings folder | Questions across all meetings need search. A 12,000-character prompt cannot hold many transcripts. `dontAsk` denies every action that needs a prompt, and that includes reading outside the working folder. A real test confirmed that a read outside the folder is denied. |
| Context | 6,000 characters of the meeting, plus snippets | The whole meeting file (up to 200 KB) for one meeting. A list of all meetings (date, title, file) for all meetings. | Whole transcripts give better answers. Claude finds the rest with Grep. |
| Follow-ups | Replays the last 6 turns in each prompt | `--resume <session_id>` | The session keeps the whole conversation, with the files it read. If the session is lost, Margin starts a new one and replays the recent turns, as upstream does. |
| History | In memory | `state/chat/<key>.json`, one file per meeting plus `all.json` | It survives a restart. Reset wipes `state/`. Clear and Move to Trash delete one file. |
| Sources | Not shown | Files Claude read, files the answer links to, and the meeting in scope, shown as links | Granola shows sources. The links open the meeting. |
| Terminal Recall, skills, CLAUDE.md workspace | Yes | No | It is a power-user surface with full tools. That is out of scope for a calm notepad. |
| Codex, Ollama, OpenAI-compatible | Yes | No | Claude only in v1. Ollama has no tool use in this flow, so it would need the upstream context packing. |
| Source re-authorization (sha256, config changes mid-turn) | Yes | No | Upstream needs it for its "restricted meetings" policy. Margin has no restricted meetings. The meeting folder is the only boundary, and `dontAsk` enforces it. |
| Wire contract (JSON schema, generated `.d.ts`) | Yes | No | Margin parses stream-json in Rust and sends three small events: `chat-delta` (the answer so far), `chat-activity` ("Reading “Launch sync”…") and `chat-done` (the saved thread). |
| Session persistence | `--no-session-persistence` | Sessions are saved | `--resume` needs them. Claude Code keeps them under `~/.claude/projects/` for the meetings folder path. |

## Known gaps

- There is no Ollama chat.
- Claude can read every file in the meetings folder, not only meeting files. If the folder
  is an Obsidian vault folder, Claude can also read the other notes in that folder.
- Margin does not control the model. Claude Code uses the user's default model.
