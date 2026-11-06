# Security policy

## Reporting a vulnerability

Please report it privately, never in a public issue, discussion or pull request: open a
report at
[github.com/askysh/margin/security/advisories/new](https://github.com/askysh/margin/security/advisories/new)
(Security, then Report a vulnerability). Only the maintainers can read it. If you cannot
use GitHub, email [mail@akashmishra.com](mailto:mail@akashmishra.com). You get an answer
within a week, and a fix or a plan for one before anything is disclosed. Say whether you
want credit in the advisory.

Include what you ran, the macOS version, Margin's version (About Margin), and what an
attacker gains. Use test recordings, never someone else's meetings.

## What counts

Margin records meetings and keeps them on your Mac. Report anything in these areas, even
when you are not sure it is exploitable:

- **Recording.** Anything that records the microphone or system audio without a visible
  recording state (the red dot and the menu bar timer), or keeps recording after Stop.
- **Your meetings.** Anything that reads, changes or deletes files outside the meetings
  folder and Margin's own folder (`~/Library/Application Support/Margin`), or deletes
  meetings without the confirmation the app promises.
- **Ask.** Anything that lets Claude, through Margin, write files, run commands, or read
  files outside the meetings folder.
- **Data leaving the Mac.** Margin sends meeting text only to the summary engine and the
  Ask engine you chose (the `claude` CLI or a local Ollama). Anything else that sends
  audio, transcripts or notes off the Mac counts.
