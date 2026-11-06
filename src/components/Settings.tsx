import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type Backends, type Boot, type Permissions, type ResetPreview, type Settings, type SettingsOptions, type Voice } from "../api";
import { bytes, copyText, tildify } from "../util";
import { DownloadProgress, MODEL_CHOICES } from "./Models";
import { PermissionRow } from "./PermissionRow";
import { SummaryChoice } from "./SummaryChoice";
import { SelectRow, TextRow, ToggleRow, type Choice } from "./SettingsRows";

/** Settings stored outside config.toml. A save sends them only when they changed. */
const EXTRA_KEYS = ["vocabulary", "summary_instructions", "open_at_login"] as const;

type Save = (patch: Partial<Settings>) => void;
import { AppPrefsBlock } from "./AppPrefsBlock";

interface Props {
  boot: Boot;
  onClose: () => void;
  onSaved: (s: Settings) => void;
  onReset: () => void;
  notify: (t: string, tone?: "info" | "error") => void;
}

export function SettingsSheet({ boot, onClose, onSaved, onReset, notify }: Props) {
  const [s, setS] = useState<Settings>(boot.settings);
  const [backends, setBackends] = useState<Backends | null>(null);
  const [perms, setPerms] = useState<Permissions | null>(null);
  const [voices, setVoices] = useState<Voice[]>([]);
  const [mcp, setMcp] = useState("");
  const [resetOpen, setResetOpen] = useState(false);
  const [nameDraft, setNameDraft] = useState(boot.settings.user_name);
  const [options, setOptions] = useState<SettingsOptions | null>(null);

  const refreshPerms = () => api.permissions().then(setPerms).catch(() => {});
  const onCloseRef = useRef(onClose);
  onCloseRef.current = onClose;
  const resetOpenRef = useRef(resetOpen);
  resetOpenRef.current = resetOpen;
  useEffect(() => {
    api.summaryBackends().then(setBackends).catch(() => {});
    refreshPerms();
    api.listVoices().then(setVoices).catch(() => {});
    api.mcpSnippet().then(setMcp).catch(() => {});
    api.settingsOptions().then(setOptions).catch(() => setOptions({ languages: [], input_devices: [], silence_stop_minutes: [], keep_audio_days: [] }));
    const esc = (e: KeyboardEvent) => e.key === "Escape" && !resetOpenRef.current && onCloseRef.current();
    window.addEventListener("keydown", esc);
    window.addEventListener("focus", refreshPerms);
    return () => { window.removeEventListener("keydown", esc); window.removeEventListener("focus", refreshPerms); };
  }, []);

  // Each change applies on top of the latest settings, and saves go one at a time, so a
  // name edit and a model click in quick succession can't undo each other.
  const latest = useRef<Settings>(boot.settings);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const pendingSaves = useRef(0);
  const save = (patch: Partial<Settings>) => {
    latest.current = { ...latest.current, ...patch };
    setS(latest.current);
    pendingSaves.current++;
    queue.current = queue.current.then(async () => {
      try {
        const payload: Settings = { ...latest.current };
        for (const k of EXTRA_KEYS) if (!(k in patch)) delete payload[k];
        const saved = await api.saveSettings(payload);
        pendingSaves.current--;
        if (pendingSaves.current === 0) {
          latest.current = saved;
          setS(saved);
          onSaved(saved);
        }
        if (patch.model) {
          const files = await api.modelsStatus();
          if (files.some((f) => !f.installed)) await api.downloadModels();
        }
        if (patch.output_dir) api.mcpSnippet().then(setMcp);
      } catch (e) {
        pendingSaves.current--;
        notify(errorText(e), "error");
      }
    });
  };

  const changeFolder = async () => {
    const chosen = await open({ directory: true, multiple: false, title: "Choose where to save meetings", defaultPath: s.output_dir });
    if (typeof chosen === "string") save({ output_dir: chosen });
  };

  return (
    <div className="sheet-backdrop" onMouseDown={(e) => e.target === e.currentTarget && onClose()}>
      <div className="sheet" role="dialog" aria-modal="true" aria-label="Settings">
        <header className="sheet-head">
          <h1>Settings</h1>
          <button className="btn btn-quiet" onClick={onClose}>Done</button>
        </header>
        {/* The drop-downs need their choices, so the body waits for them (a few ms). */}
        {options && <div className="sheet-body">
          <section className="set">
            <h2 className="eyebrow">You</h2>
            <label className="field-label" htmlFor="set-name">Name in transcripts</label>
            <input id="set-name" className="field" value={nameDraft} onChange={(e) => setNameDraft(e.target.value)} onBlur={() => nameDraft !== s.user_name && save({ user_name: nameDraft })} />
          </section>

          <section className="set">
            <h2 className="eyebrow">Meetings folder</h2>
            <div className="set-row">
              <span className="path">{tildify(s.output_dir)}</span>
              <div className="row-actions">
                <button className="btn btn-quiet" onClick={() => api.reveal(s.output_dir)}>Show in Finder</button>
                <button className="btn btn-quiet" onClick={changeFolder}>Change…</button>
              </div>
            </div>
            <p className="set-help">New meetings are saved here. Meetings already saved stay where they are.</p>
            <SelectRow
              id="set-keep-audio"
              label="Keep audio"
              help="Transcripts and notes are always kept. Older audio files are deleted for good."
              value={s.keep_audio_days ?? 0}
              choices={(options?.keep_audio_days ?? [0]).map((d) => ({ value: d, label: d === 0 ? "Forever" : `For ${d} days` }))}
              onChange={(d) => save({ keep_audio_days: d })}
            />
          </section>

          <RecordingSection s={s} options={options} save={save} />

          <section className="set">
            <h2 className="eyebrow">Transcription</h2>
            <div className="options options-row" role="radiogroup">
              {MODEL_CHOICES.map((m) => (
                <button key={m.id} role="radio" aria-checked={s.model === m.id} className={`option ${s.model === m.id ? "on" : ""}`} onClick={() => save({ model: m.id })}>
                  <span className="option-title">{m.title} <span className="mono option-size">{m.size}</span></span>
                  <span className="option-body">{m.body}</span>
                </button>
              ))}
            </div>
            <DownloadProgress compact key={s.model} />
            <SelectRow
              id="set-language"
              label="Language spoken in meetings"
              help="Choose one only if auto-detect gets your meetings wrong."
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
          </section>

          <section className="set">
            <h2 className="eyebrow">Summaries</h2>
            <SummaryChoice backends={backends} value={s} onChange={(v) => save(v)} />
            {s.summary_engine !== "none" && <SummaryOptions s={s} options={options} save={save} />}
          </section>

          <section className="set">
            <h2 className="eyebrow">Permissions</h2>
            <PermissionRow
              title="Microphone"
              body="Needed for every recording."
              state={perms?.microphone}
              onRequest={async () => { await api.requestPermission("microphone"); refreshPerms(); }}
              onSettings={() => api.openPrivacySettings("microphone")}
            />
            <PermissionRow
              title="System audio"
              body="Needed to hear the other people on calls."
              state={perms?.system_audio}
              onRequest={async () => { await api.requestPermission("system-audio"); refreshPerms(); }}
              onSettings={() => api.openPrivacySettings("system-audio")}
            />
          </section>

          <section className="set">
            <h2 className="eyebrow">Voices Margin recognises</h2>
            {voices.length === 0 ? (
              <p className="set-help">None yet. Rename a speaker in a meeting and keep “Recognise this voice” checked.</p>
            ) : (
              <ul className="voices">
                {voices.map((v) => (
                  <li key={v.slug}>
                    <span>{v.name} <span className="quiet mono">{v.samples} sample{v.samples === 1 ? "" : "s"}</span></span>
                    <button className="link-btn danger" onClick={async () => { await api.forgetVoice(v.slug); setVoices(await api.listVoices()); }}>Forget</button>
                  </li>
                ))}
              </ul>
            )}
          </section>

          <section className="set">
            <h2 className="eyebrow">Use your meetings in Claude</h2>
            <p className="set-help">Add this to your Claude Desktop or Claude Code MCP config to search and read your meetings from Claude. It uses the open-source Minutes MCP server.</p>
            <pre className="code">{mcp}</pre>
            <button className="btn btn-quiet" onClick={async () => notify((await copyText(mcp)) ? "Copied." : "Couldn't copy. Select the text and press ⌘C.")}>Copy</button>
          </section>

          <AppPrefsBlock initial={boot.prefs} notify={notify} />

          <section className="set set-danger">
            <h2 className="eyebrow">Start over</h2>
            <div className="set-row">
              <p className="set-help">Reset Margin to a fresh install and run setup again. You choose what happens to your meetings and downloads.</p>
              <button className="btn btn-danger" onClick={() => setResetOpen(true)}>Reset Margin…</button>
            </div>
          </section>

          <footer className="sheet-foot">Margin {boot.version} · built on minutes-core {boot.upstream_tag}</footer>
        </div>}
      </div>
      {resetOpen && <ResetDialog onCancel={() => setResetOpen(false)} onDone={onReset} notify={notify} />}
    </div>
  );
}

function RecordingSection({ s, options, save }: { s: Settings; options: SettingsOptions | null; save: Save }) {
  const device = s.input_device ?? "";
  const devices = options?.input_devices ?? [];
  const mics: Choice<string>[] = [
    { value: "", label: "System default" },
    ...devices.map((d) => ({ value: d, label: d })),
    ...(device && !devices.includes(device) ? [{ value: device, label: `${device} (not connected)` }] : []),
  ];
  const silence = (options?.silence_stop_minutes ?? [s.silence_stop_minutes ?? 30]).map((m) => ({ value: m, label: minutesLabel(m) }));
  return (
    <section className="set">
      <h2 className="eyebrow">Recording</h2>
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
      <ToggleRow
        title="Suggest recording when a call starts"
        help="Shows a banner when Zoom, Teams or another call app opens."
        checked={s.call_detection ?? true}
        onChange={(on) => save({ call_detection: on })}
      />
      <ToggleRow
        title="Open Margin when you log in"
        help="Margin waits in the menu bar, ready for your first meeting."
        checked={s.open_at_login ?? false}
        onChange={(on) => save({ open_at_login: on })}
      />
    </section>
  );
}

function SummaryOptions({ s, options, save }: { s: Settings; options: SettingsOptions | null; save: Save }) {
  return (
    <>
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
    </>
  );
}

function minutesLabel(m: number): string {
  if (m === 0) return "Never";
  if (m % 60 === 0) return m === 60 ? "After 1 hour" : `After ${m / 60} hours`;
  return `After ${m} minutes`;
}

function ResetDialog({ onCancel, onDone, notify }: { onCancel: () => void; onDone: () => void; notify: Props["notify"] }) {
  const [preview, setPreview] = useState<ResetPreview | null>(null);
  const [deleteMeetings, setDeleteMeetings] = useState(false);
  const [deleteModels, setDeleteModels] = useState(false);
  const [forgetVoices, setForgetVoices] = useState(false);
  const [confirm, setConfirm] = useState("");
  const [busy, setBusy] = useState(false);
  useEffect(() => { api.resetPreview().then(setPreview).catch((e) => notify(errorText(e), "error")); }, [notify]);

  const needsTyping = deleteMeetings && (preview?.meeting_count ?? 0) > 0;
  const canReset = !busy && (!needsTyping || confirm.trim().toLowerCase() === "delete");

  const run = async () => {
    setBusy(true);
    try {
      const r = await api.reset({ delete_meetings: deleteMeetings, delete_models: deleteModels, forget_voices: forgetVoices });
      notify(r.meetings_trashed ? `Margin was reset. ${r.meetings_trashed} meetings moved to the Trash.` : "Margin was reset. Your meetings are untouched.");
      onDone();
    } catch (e) {
      notify(errorText(e), "error");
      setBusy(false);
    }
  };

  return (
    <div className="dialog-backdrop">
      <div className="dialog" role="alertdialog" aria-modal="true" aria-labelledby="reset-title">
        <h1 id="reset-title">Reset Margin?</h1>
        <p>Your settings are cleared and setup runs again. macOS keeps the permissions you already gave.</p>
        <ul className="reset-list">
          <li className="reset-always">Settings, the processing queue and logs are cleared.</li>
          <li>
            <label className="check">
              <input type="checkbox" checked={!deleteMeetings} onChange={(e) => setDeleteMeetings(!e.target.checked)} />
              <span>Keep my meetings <span className="quiet">({preview?.meeting_count ?? "…"} in {preview ? tildify(preview.output_dir) : "…"})</span></span>
            </label>
          </li>
          <li>
            <label className="check">
              <input type="checkbox" checked={!deleteModels} onChange={(e) => setDeleteModels(!e.target.checked)} />
              <span>Keep downloaded speech models <span className="quiet">({preview ? bytes(preview.models_bytes) : "…"}, saves re-downloading)</span></span>
            </label>
          </li>
          <li>
            <label className="check">
              <input type="checkbox" checked={!forgetVoices} onChange={(e) => setForgetVoices(!e.target.checked)} />
              <span>Keep recognised voices <span className="quiet">({preview?.voice_count ?? "…"}{preview?.upstream_app_installed ? ", shared with the Minutes app" : ""})</span></span>
            </label>
          </li>
        </ul>
        {needsTyping && (
          <div className="danger-confirm">
            <p><strong>{preview?.meeting_count} meetings and their audio will be moved to the Trash.</strong> You can restore them from the Trash until you empty it. Type <span className="mono">delete</span> to confirm.</p>
            <input className="field" value={confirm} onChange={(e) => setConfirm(e.target.value)} placeholder="delete" autoFocus />
          </div>
        )}
        <div className="dialog-actions">
          <button className="btn btn-quiet" onClick={onCancel} disabled={busy} autoFocus={!needsTyping}>Cancel</button>
          <button className="btn btn-danger" onClick={run} disabled={!canReset}>{busy ? "Resetting…" : "Reset Margin"}</button>
        </div>
      </div>
    </div>
  );
}
