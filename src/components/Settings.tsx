import { useEffect, useRef, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type Backends, type Boot, type Permissions, type ResetPreview, type Settings, type Voice } from "../api";
import { bytes, copyText, tildify } from "../util";
import { DownloadProgress, MODEL_CHOICES } from "./Onboarding";
import { PermissionRow } from "./PermissionRow";
import { SummaryChoice } from "./SummaryChoice";

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
        const saved = await api.saveSettings(latest.current);
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
        <div className="sheet-body">
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
          </section>

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
          </section>

          <section className="set">
            <h2 className="eyebrow">Summaries</h2>
            <SummaryChoice backends={backends} value={s} onChange={(v) => save(v)} />
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

          <section className="set set-danger">
            <h2 className="eyebrow">Start over</h2>
            <div className="set-row">
              <p className="set-help">Reset Margin to a fresh install and run setup again. You choose what happens to your meetings and downloads.</p>
              <button className="btn btn-danger" onClick={() => setResetOpen(true)}>Reset Margin…</button>
            </div>
          </section>

          <footer className="sheet-foot">Margin {boot.version} · built on minutes-core {boot.upstream_tag}</footer>
        </div>
      </div>
      {resetOpen && <ResetDialog onCancel={() => setResetOpen(false)} onDone={onReset} notify={notify} />}
    </div>
  );
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
