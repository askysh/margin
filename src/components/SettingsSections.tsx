import { useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, type Settings, type SettingsOptions } from "../api";
import { tildify } from "../util";
import { DownloadProgress, MODEL_CHOICES } from "./Models";
import { SummaryChoice } from "./SummaryChoice";
import { Card, Row, SectionHead, SelectRow, TextRow, ToggleRow, type Choice } from "./SettingsRows";
import type { SettingsView } from "./Settings";

type Save = (patch: Partial<Settings>) => void;

export function GeneralSection({ s, save, options }: SettingsView) {
  const changeFolder = async () => {
    const chosen = await open({ directory: true, multiple: false, title: "Choose where to save meetings", defaultPath: s.output_dir });
    if (typeof chosen === "string") save({ output_dir: chosen });
  };
  return (
    <>
      <SectionHead title="General" about="Your name, where meetings are saved, privacy, and how Margin starts." />
      <Card>
        <NameRow s={s} save={save} />
      </Card>
      <Card title="Meetings" note="Changing the folder affects new meetings. Meetings already saved stay where they are.">
        <Row title="Meetings folder" help={<span className="path">{tildify(s.output_dir)}</span>}>
          <button className="btn btn-quiet" onClick={() => api.reveal(s.output_dir)}>Show in Finder</button>
          <button className="btn btn-quiet" onClick={changeFolder}>Change…</button>
        </Row>
        <SelectRow
          id="set-keep-audio"
          label="Keep audio"
          help="Transcripts and notes are always kept. Older audio files are deleted for good."
          value={s.keep_audio_days ?? 0}
          choices={(options?.keep_audio_days ?? [0]).map((d) => ({ value: d, label: d === 0 ? "Forever" : `For ${d} days` }))}
          onChange={(d) => save({ keep_audio_days: d })}
        />
      </Card>
      <Card title="Privacy">
        <ToggleRow
          title="Hide Margin from screen sharing"
          help="People on a call do not see Margin when you share your screen. Screenshots leave Margin out too."
          checked={s.hide_from_screen_share ?? true}
          onChange={(on) => save({ hide_from_screen_share: on })}
        />
      </Card>
      <Card title="Startup">
        <ToggleRow
          title="Open Margin when you log in"
          help="Margin waits in the menu bar, ready for your first meeting."
          checked={s.open_at_login ?? false}
          onChange={(on) => save({ open_at_login: on })}
        />
      </Card>
    </>
  );
}

/** Saved when the field loses focus or on Return, not on every key. */
function NameRow({ s, save }: { s: Settings; save: Save }) {
  const [draft, setDraft] = useState(s.user_name);
  return (
    <Row title="Name in transcripts" help="Labels what you say. Summaries use it too." htmlFor="set-name">
      <input
        id="set-name"
        className="field set-input"
        value={draft}
        aria-describedby="set-name-help"
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => e.key === "Enter" && e.currentTarget.blur()}
        onBlur={() => draft !== s.user_name && save({ user_name: draft })}
      />
    </Row>
  );
}

export function RecordingSection({ s, save, options, prefs, changePrefs }: SettingsView) {
  const device = s.input_device ?? "";
  const devices = options?.input_devices ?? [];
  const mics: Choice<string>[] = [
    { value: "", label: "System default" },
    ...devices.map((d) => ({ value: d, label: d })),
    ...(device && !devices.includes(device) ? [{ value: device, label: `${device} (not connected)` }] : []),
  ];
  const silence = (options?.silence_stop_minutes ?? [s.silence_stop_minutes ?? 30]).map((m) => ({ value: m, label: minutesLabel(m) }));
  return (
    <>
      <SectionHead title="Recording" about="The microphone, when a recording stops by itself, and ways to start one." />
      <Card title="Capture">
        <SelectRow
          id="set-mic"
          label="Microphone"
          help="Used for calls and in-person meetings. If it isn't connected, Margin uses the system default."
          value={device}
          choices={mics}
          onChange={(d) => save({ input_device: d })}
        />
        <SelectRow
          id="set-silence"
          label="Stop after silence"
          help="Stops and saves a recording after this long with no sound. A call that is still going keeps recording."
          value={s.silence_stop_minutes ?? 30}
          choices={silence}
          onChange={(m) => save({ silence_stop_minutes: m })}
        />
      </Card>
      <Card title="Starting and stopping">
        <ToggleRow
          title="Suggest recording when a call starts"
          help="Shows a banner when Zoom, Teams or another call app opens."
          checked={s.call_detection ?? true}
          onChange={(on) => save({ call_detection: on })}
        />
        <ToggleRow
          title="Keyboard shortcut in every app"
          help={<>Start and stop recording from any app with <kbd className="kbd">⌃⌥⌘R</kbd></>}
          checked={prefs.record_shortcut}
          onChange={(on) => changePrefs({ ...prefs, record_shortcut: on })}
        />
        <ToggleRow
          title="Sounds"
          help="Play a sound when a recording starts, stops and is ready."
          checked={prefs.sound_cues}
          onChange={(on) => changePrefs({ ...prefs, sound_cues: on })}
        />
      </Card>
      <Card title="During a recording">
        <ToggleRow
          title="Show the recording chip"
          help="When a recording starts, the Margin window hides and a small chip shows the time at the right edge of the screen. Click the chip to open your notes. Drag it up or down to move it."
          checked={prefs.recording_chip}
          onChange={(on) => changePrefs({ ...prefs, recording_chip: on })}
        />
      </Card>
    </>
  );
}

export function TranscriptionSection({ s, save, options }: SettingsView) {
  return (
    <>
      <SectionHead title="Transcription" about="Speech becomes text on this Mac. Nothing is uploaded." />
      <Card title="Speech model">
        <div className="set-pad">
          <div className="options options-row" role="radiogroup" aria-label="Speech model">
            {MODEL_CHOICES.map((m) => (
              <button key={m.id} role="radio" aria-checked={s.model === m.id} className={`option ${s.model === m.id ? "on" : ""}`} onClick={() => save({ model: m.id })}>
                <span className="option-title">{m.title} <span className="mono option-size">{m.size}</span></span>
                <span className="option-body">{m.body}</span>
              </button>
            ))}
          </div>
          <DownloadProgress compact key={s.model} />
        </div>
        <RollingToggle s={s} save={save} />
      </Card>
      <Card>
        <SelectRow
          id="set-language"
          label="Language spoken in meetings"
          help="Choose one only if auto-detect gets your meetings wrong. For meetings that mix two languages, such as Hindi and English, keep Auto-detect and use the Best model."
          value={s.transcription_language ?? "auto"}
          choices={[{ value: "auto", label: "Auto-detect" }, ...(options?.languages ?? []).map((l) => ({ value: l.code, label: l.name }))]}
          onChange={(code) => save({ transcription_language: code })}
        />
        <ToggleRow
          title="Tell speakers apart"
          help="Labels who said what. Turn it off for faster processing without speaker names."
          checked={s.speaker_separation ?? true}
          onChange={(on) => save({ speaker_separation: on })}
        />
        <TextRow
          id="set-vocabulary"
          label="Names and words to spell right"
          help="One per line: people, products, acronyms. The transcriber gets up to 8 of them as hints."
          placeholder={"Priya Raman\nKubernetes"}
          value={(s.vocabulary ?? []).join("\n")}
          onSave={(text) => save({ vocabulary: text.split("\n") })}
        />
      </Card>
    </>
  );
}

/** Rolling transcription (rolling.rs). It works with every model (DECISIONS.md D52). */
function RollingToggle({ s, save }: { s: Settings; save: Save }) {
  return (
    <ToggleRow
      title="Transcribe during the meeting"
      help="Most of the transcript is done by the time you press Stop, so a long meeting is ready in a few minutes, not an hour. Uses more battery while recording."
      checked={s.rolling_transcription ?? true}
      onChange={(on) => save({ rolling_transcription: on })}
    />
  );
}

export function SummariesSection({ s, save, options, backends }: SettingsView) {
  return (
    <>
      <SectionHead title="Summaries" about="Who writes a summary after each meeting, and how." />
      <Card title="Written by" plain>
        <SummaryChoice backends={backends} value={s} onChange={(v) => save(v)} />
      </Card>
      {s.summary_engine !== "none" && <SummaryOptions s={s} options={options} save={save} />}
    </>
  );
}

function SummaryOptions({ s, options, save }: { s: Settings; options: SettingsOptions | null; save: Save }) {
  return (
    <Card>
      <SelectRow
        id="set-summary-language"
        label="Summary language"
        value={s.summary_language ?? "auto"}
        choices={[{ value: "auto", label: "Same as the meeting" }, ...(options?.languages ?? []).map((l) => ({ value: l.name, label: l.name }))]}
        onChange={(name) => save({ summary_language: name })}
      />
      <TextRow
        id="set-summary-instructions"
        label="Instructions for summaries"
        help="Added to every new summary. Leave it empty for the standard summary."
        placeholder="For example: Start with decisions. Keep each point to one line."
        value={s.summary_instructions ?? ""}
        onSave={(text) => save({ summary_instructions: text })}
      />
    </Card>
  );
}

function minutesLabel(m: number): string {
  if (m === 0) return "Never";
  if (m % 60 === 0) return m === 60 ? "After 1 hour" : `After ${m / 60} hours`;
  return `After ${m} minutes`;
}
