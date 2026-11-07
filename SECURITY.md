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
- **Updates and releases.** A way to make the in-app updater install a file that Margin's
  updater key did not sign, or a way to make a release download or the Homebrew cask run
  code it should not.
- **Other programs.** A way for another local program or a web page to drive Margin's
  backend. `margin --dev serve`, the test bridge, listens on `127.0.0.1` only, and runs
  only when someone starts it from Terminal.

## What Margin does not protect

- **Meetings are plain files.** Meetings, transcripts and audio are Markdown and WAV files
  in the folder you chose, readable by any program that runs as your user. Margin does
  not encrypt them. A sync service such as iCloud Drive copies them where it is set to.
- **Shared voice profiles.** minutes-core keeps voice profiles in `~/.minutes/voices.db`,
  which the upstream Minutes app also uses.
- **The System Audio permission check** uses private macOS calls (`TCCAccessPreflight` and
  `TCCAccessRequest`). This affects only how Margin shows the permission state.

Not a vulnerability in Margin: a problem in minutes-core itself (report it to
[silverstein/minutes](https://github.com/silverstein/minutes/security)), in the `claude`
CLI or in Ollama.

## Supported versions

Only the latest release gets security fixes. Update first (Settings → Updates, or
`brew upgrade --cask margin`), then report.
