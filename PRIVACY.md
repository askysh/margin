# Privacy policy

This policy covers Margin for macOS and Margin Notes for Windows. Both are the same app,
and this page calls it Margin. Akash Mishra publishes it. The source code is public at
[github.com/askysh/margin](https://github.com/askysh/margin).

Last change: 13 November 2026.

## The short version

- Margin has no account, no analytics and no advertising.
- The publisher has no server that receives your data. The publisher cannot see your
  meetings.
- Audio, transcripts and notes stay on your computer, unless you turn on Summaries or Ask.

## What Margin accesses

| Data | Why | Where it is kept |
|---|---|---|
| Microphone audio | To record a meeting, only between Start and Stop. | A `.wav` file next to the meeting file, on your computer. |
| System audio (the other people on a call) | To record a call, only between Start and Stop. | The same `.wav` file. |
| The transcript and your notes | They are the meeting. | A Markdown file in the meetings folder that you choose. |
| Voice profiles | To put the same name on a speaker in later meetings. | A database on your computer. |
| Your name | To label your own lines in a transcript. | Margin's settings, on your computer. |

Margin makes the transcript, the speaker separation and the voice profiles on your
computer. It does not send audio to a server.

Margin shows a recording state for the whole time it records. On Windows, the microphone
icon of Windows also shows in the taskbar.

## When data leaves your computer

Nothing leaves your computer unless you turn on one of these. Both are off after setup
until you choose an engine in Settings.

- **Summaries.** With **Claude**, Margin gives the transcript and your notes to the
  `claude` program that you installed and signed in to. That program sends them to
  Anthropic under your own Anthropic account and Anthropic's terms. With **Ollama**,
  Margin sends them to the Ollama server at the address in Settings, which by default is
  on your own computer.
- **Ask.** Ask runs your own `claude` program. It can read the files in your meetings
  folder, and it sends what it reads to Anthropic in the same way. It cannot write files
  or run commands.

To stop this, set Summaries to None in Settings, and do not use Ask.

## Other connections to the internet

- **Models.** Margin downloads the speech and speaker models from Hugging Face and GitHub.
  These requests carry no meeting data.
- **Updates.** Margin for macOS asks GitHub for the latest version. Settings → Updates
  turns this off. Margin Notes for Windows gets its updates from the Microsoft Store and
  makes no update request of its own.

Those services see your IP address, as with any download. The Microsoft Store has its own
privacy terms for installs and updates.

## Other people in your meetings

A recording holds the voices of the other people in the meeting. You are responsible for
telling them and for getting their agreement where the law requires it. Margin does not
share a recording with anyone.

## Your controls

- **See your data.** Every meeting is a plain file in your meetings folder. You can open,
  copy and move it with any tool.
- **Delete a meeting.** Delete it in Margin. Margin asks first, then moves the files to
  the Trash (the Recycle Bin on Windows).
- **Delete old audio.** *Keep audio* in Settings deletes the audio of a meeting after 7
  days or 30 days. The transcript and your notes stay.
- **Delete everything.** *Reset Margin…* in Settings clears Margin's settings. You choose
  if it also removes the models, the saved voices and the meetings.
- **Take back a permission.** Turn off the microphone permission for Margin in the
  settings of macOS or Windows.
- **Uninstall.** Uninstalling removes the app. Your meetings folder and the saved voices
  stay, because the files are yours. *Reset Margin…* removes the voices.

## Security

Margin keeps your files with the permissions of your user account. It does not encrypt
them itself. Use the disk encryption of your computer (FileVault or BitLocker). Downloads
use HTTPS.

## Children

Margin is a work tool. It is not directed at children, and it collects no data from
anyone.

## Changes

A change to this policy is a change to this file. The
[history of the file](https://github.com/askysh/margin/commits/main/PRIVACY.md) shows
every version.

## Contact

Open an issue at
[github.com/askysh/margin/issues](https://github.com/askysh/margin/issues), or email
[mail@akashmishra.com](mailto:mail@akashmishra.com). To report a security problem, see
[SECURITY.md](SECURITY.md).
