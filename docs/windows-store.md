# Margin Notes on the Microsoft Store

This page holds everything a Store submission needs: the identity, the package, the
listing text, the images, the answers for Partner Center and the checks before you
submit. DECISIONS.md D61 gives the reasons.

The Windows app builds and records a meeting in the room (D62). It has no call recording
and no GPU transcription yet, and no Store package. Lines marked **Confirm** describe a
feature that the Windows build must have before the text goes to the Store. Store policy 10.1.1 requires
the listing to describe the product as it is.

## Identity

The product is **Margin Notes** in Partner Center, of the type "MSIX or PWA app". The name
"Margin" was taken.

| Field | Value |
|---|---|
| Store ID | `9P0G73M2LBN3` |
| Package/Identity/Name | `Askysh.MarginNotes` |
| Package/Identity/Publisher | `CN=BA1A2A4F-276F-4E64-8619-08871322A90C` |
| Package/Properties/PublisherDisplayName | `Askysh` |
| Package Family Name | `Askysh.MarginNotes_0f196q1bwh8q6` |

These values are in [`windows/msix/AppxManifest.xml`](../windows/msix/AppxManifest.xml).
Partner Center refuses a package where one of them differs. They are not secrets: every
installed copy carries them.

## The package

| Rule | What it means for Margin |
|---|---|
| The Store signs an MSIX. | No certificate is needed. Upload the package unsigned. |
| The version has four parts. The last part is 0. The first part cannot be 0. | Margin `0.0.10` goes to the Store as `1.0.10.0`. The first part is Margin's major version plus 1 (D61). |
| One package for each CPU. | x64 first. An arm64 package can come later with a lower or equal version. |
| The Store updates an MSIX. | The Store build has no update check of its own. |
| A package cannot write inside its own folder. | Margin keeps models and state in its data folder, never next to `margin.exe`. |
| Windows redirects writes under `%APPDATA%` and `%LOCALAPPDATA%` to a private store. | Settings and models (`%LOCALAPPDATA%\Margin`) go away on uninstall. The meetings folder is under Documents, so it stays. The saved voices are in `%USERPROFILE%\.minutes`, so they stay too. |

`windows/msix/Assets/` holds the package logos. `scripts/make-icons.swift` draws them, with
the rest of Margin's icons. Do not edit them by hand.

To make the package, run `scripts/build-windows.sh`. It builds `margin.exe` and puts five
DLLs beside it: `DirectML.dll`, `msvcp140.dll`, `msvcp140_1.dll`, `vcruntime140.dll` and
`vcruntime140_1.dll`. Windows does not include them, and without them the app exits with
no message on a clean PC. Then put the exe, the DLLs, the manifest and `Assets` in one
folder and pack it with Microsoft's `winapp` CLI (`winapp pack`). The pack step has no
script yet.

## Partner Center, page by page

### Pricing and availability

- Markets: all.
- Audience: public.
- Price: free.
- Free trial: none.

### Properties

- Category: **Productivity**.
- Privacy policy URL: `https://github.com/askysh/margin/blob/main/PRIVACY.md`. The link
  works after [PRIVACY.md](../PRIVACY.md) is on `main`.
- Website: `https://github.com/askysh/margin`
- Support contact: `https://github.com/askysh/margin/issues`
- System requirements, minimum: a microphone, 8 GB of memory. Recommended: 16 GB of
  memory and a dedicated graphics card. **Confirm** both on a real PC.

Product declarations:

- "This app accesses, collects or transmits personal information": **yes**. It records
  audio. That is why the privacy policy URL is required (policy 10.5.1).
- "Customers can install this app to alternate drives or removable storage": yes.
- "Windows can include this app's data in automatic backups to OneDrive": **no**. The
  models are large, and the audio is private.
- "This app uses live generative AI": **yes** (policy 11.16). Summaries and Ask show text
  from an AI model.

### Age ratings

The questionnaire is from IARC. Margin is a utility. The answer to each content question
is **no**: no violence, fear, sexual content, bad language, drugs, gambling or crude
humour. For the questions about features:

| Question | Answer |
|---|---|
| Can users interact or exchange content with other users? | No |
| Does the app share the user's location with other users? | No |
| Can users buy digital goods? | No |
| Does the app give unrestricted access to the internet? | No |
| Does the app contain or show content that an AI model makes? | Yes, summaries and answers about the user's own meetings |

The wording in Partner Center can differ. Answer for what the app does, not for what a
meeting could contain. The expected rating is for all ages.

### Store listing (English, United States)

**Product name**

```
Margin Notes
```

**Short description**

```
A calm meeting notepad. You write the notes. Margin records, transcribes on your PC and saves each meeting as a Markdown file.
```

**Description**

```
Margin Notes is a meeting notepad. You type notes in your own words while Margin records the meeting. After the meeting, you get your notes and a full transcript in one Markdown file.

Summaries and Ask are optional. They need software that is not part of Margin: the Claude command line program from Anthropic, or Ollama. Everything else works without them. Summaries and Ask show text that an AI model writes.

THE NOTEPAD COMES FIRST
Your notes are the meeting. The live transcript stays beside your notes, not in their place.

EVERYTHING RUNS ON YOUR PC
Margin makes the transcript on your PC with the open Whisper speech model. Your audio does not go to a server. There is no account and no analytics.

YOUR MEETINGS ARE PLAIN FILES
Each meeting is a Markdown file in a folder that you choose. Obsidian and any text editor can read it. If you stop using Margin, your meetings stay.

WHAT IT DOES
• Records the microphone for an in-person meeting.
• Records a call: your microphone and the other people.
• Shows a live transcript while you write.
• Puts a name on each speaker and remembers voices for later meetings.
• Plays the recording from any line of the transcript.
• Finds a word in every meeting.
• Writes an optional summary with Claude or a local Ollama model.
• Answers questions about one meeting or all of them with Ask.

GOOD TO KNOW
• Margin downloads a speech model on first use. The size is 150 MB to 1.6 GB, by the quality that you choose.
• A dedicated graphics card makes the transcript faster.
• You must tell the people in a meeting that you record it, and get their agreement where the law requires it.

Margin is open source under the MIT license: github.com/askysh/margin
```

**Confirm** each line under WHAT IT DOES against the Windows build, most of all the call
recording, the speaker names and the graphics card line. Remove a line that is not true
on Windows.

**What's new in this version**

```
The first version of Margin Notes for Windows.
```

**Product features** (each one is a line in the listing)

```
Write your own notes while the meeting records
Transcription on your PC, with no account
Each meeting is one Markdown file in your folder
Speaker names, and voices it remembers
Optional summaries with Claude or Ollama
Ask a question about one meeting or all of them
Works with Obsidian
```

**Search terms** (the limit is 7 terms)

```
meeting notes
transcription
meeting recorder
markdown
obsidian
whisper
local ai
```

**Copyright and trademark info**

```
© 2026 Akash Mishra. MIT license.
```

**Additional license terms**

```
Margin Notes is open source under the MIT license: https://github.com/askysh/margin/blob/main/LICENSE
```

**Developed by**

```
Akash Mishra
```

### Images

| Image | File | State |
|---|---|---|
| App tile icon, 1:1, 300 x 300 | `windows/store/app-tile-300.png` | Ready |
| Box art, 1:1, 2160 x 2160 | `windows/store/box-art-2160.png` | Ready |
| Poster art, 1440 x 2160 | `windows/store/poster-1440x2160.png` | Ready |
| Screenshots, at least 1, 1366 x 768 or larger, PNG | none | Take them from the Windows build |

Take four screenshots at 1920 x 1080, in this order: a meeting with notes and the
transcript, a recording with the live transcript, the meeting list, and Settings. Use a
made-up meeting, never a real one. A screenshot must show the Windows app, not the Mac
app (policy 10.1.1).

### Packages

Upload the `.msix` or `.msixupload` file. Partner Center reads the identity, the version
and the capabilities from it.

### Submission options

**Restricted capability, `runFullTrust`.** Partner Center asks why the app needs it. Paste:

```
Margin Notes is a desktop (Win32) program built with Tauri and WebView2. It cannot run in an app container. It records audio with WASAPI, runs the Whisper speech model in its own process, reads and writes the Markdown files in the folder that the user chooses, and can start the user's own Claude command line program for optional summaries.
```

**Notes for certification.** Paste, then **confirm** each step on the Windows build:

```
Margin Notes needs no account and no sign-in. It needs a microphone.

To test:
1. Start Margin Notes. Finish setup: press Enter on each step to take the default. Allow the microphone when Windows asks.
2. Setup downloads a speech model from Hugging Face (about 490 MB with the default choice). Wait for the download to finish. The app shows the progress.
3. Select "In person", then Start. Say a few sentences. Type some notes. Select Stop.
4. Margin processes the recording. This takes about as long as the recording on a PC with no dedicated graphics card. The meeting then opens with the transcript and the notes.
5. The meeting is a Markdown file in Documents\Meetings.

Summaries and Ask are optional and off by default. They use live generative AI. They need software that the user installs: the Claude command line program from Anthropic, or Ollama. The app works in full without them. To report AI text that is not appropriate, a user can open an issue at https://github.com/askysh/margin/issues or email mail@akashmishra.com. The same link is in the app, in Settings, About.

The app downloads speech models only. These are data files. It does not download or run code.
The app does not change any Windows setting. It needs no administrator rights.
Uninstall removes the app, its settings and its models. The meetings folder and the saved voices stay, because they belong to the user.
```

## What the Windows build must have for the Store

These come from the Store policies (version 7.20). Each one is a requirement on the port.

- [x] **No update check of its own** in the Store build. The Store updates the package.
- [x] **No administrator rights**, for any feature. A product that needs elevation is not
      accepted.
- [ ] **A way to report AI text** (policy 11.16): a link in Settings → About to the issue
      tracker.
- [ ] **A link to the privacy policy** in Settings → About.
- [ ] **A clear first run** (policy 10.1.1): setup says what Margin does before it asks
      for the microphone.
- [x] **A message when the PC cannot run it** (policy 10.4.1), for example when there is
      no microphone.
- [x] **The name "Margin Notes"** in the window title, the Start menu and the notification
      area, so the app matches its listing.
- [x] **No writes next to `margin.exe`.** The package folder is read-only.
- [ ] **Summaries and Ask find `claude` on Windows** without a shell, or the listing does
      not mention Claude.

## Before each submission

1. Run the Windows App Certification Kit on the package. Fix every failure.
2. Install the package on a PC that never had Margin. Do the steps from the notes for
   certification.
3. Check that the version is higher than the one in the Store.
4. Check the listing against the build. Remove what is not true.
5. Uninstall. Check that the Start menu entry and the app data are gone, and that the
   meetings folder is still there.

Two things here are from memory and not confirmed in Microsoft's docs: certification takes
a few business days, and a reserved name with no submission can expire. Do not leave the
product in draft for many months.

The field names on the Properties page and the image sizes are also from memory. Partner
Center shows the exact ones.

## Sources

- [Microsoft Store Policies 7.20](https://learn.microsoft.com/en-us/windows/apps/publish/store-policies)
- [App package requirements for an MSIX app](https://learn.microsoft.com/en-us/windows/apps/publish/publish-your-app/msix/app-package-requirements)
- [Using the winapp CLI with Tauri](https://learn.microsoft.com/en-us/windows/apps/dev-tools/winapp-cli/guides/tauri)
- [Tauri: Microsoft Store](https://v2.tauri.app/distribute/microsoft-store/)
