import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, on, type Backends, type ModelFile, type ModelProgress, type Permissions, type Settings } from "../api";
import { bytes, tildify } from "../util";
import { PermissionRow } from "./PermissionRow";
import { SummaryChoice } from "./SummaryChoice";

const STEPS = ["You", "Folder", "Transcription", "Permissions", "Summaries"] as const;

export const MODEL_CHOICES = [
  { id: "base", title: "Fast", size: "148 MB", body: "Quick and light. Fine for clear audio." },
  { id: "small", title: "Balanced", size: "488 MB", body: "Recommended. Handles accents and crosstalk well." },
  { id: "large-v3-turbo", title: "Best", size: "1.6 GB", body: "Most accurate. Takes longer to download." },
];

export function Onboarding({ initial, onDone }: { initial: Settings; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [s, setS] = useState<Settings>(initial);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  const save = async (next: Settings) => {
    setSaving(true);
    setError(null);
    try {
      const saved = await api.saveSettings(next);
      setS(saved);
      return true;
    } catch (e) {
      setError(errorText(e));
      return false;
    } finally {
      setSaving(false);
    }
  };

  const next = async (patch?: Partial<Settings>) => {
    const merged = { ...s, ...patch };
    if (await save(merged)) setStep((n) => n + 1);
  };

  return (
    <div className="onboarding" data-tauri-drag-region>
      <div className="ob-card">
        <ol className="ob-steps" aria-label="Setup steps">
          {STEPS.map((label, i) => (
            <li key={label} className={i < step ? "done" : i === step ? "now" : ""}>{label}</li>
          ))}
        </ol>
        {step === 0 && <StepYou s={s} onNext={(name) => next({ user_name: name })} saving={saving} />}
        {step === 1 && <StepFolder s={s} onNext={(dir) => next({ output_dir: dir })} saving={saving} />}
        {step === 2 && <StepModel s={s} onNext={(model) => next({ model })} saving={saving} />}
        {step === 3 && <StepPermissions onNext={() => setStep(4)} />}
        {step === 4 && (
          <StepSummaries
            s={s}
            saving={saving}
            onFinish={async (patch) => {
              if (await save({ ...s, ...patch })) {
                await api.completeOnboarding();
                onDone();
              }
            }}
          />
        )}
        {error && <p className="ob-error" role="alert">{error}</p>}
        {step > 0 && <button className="ob-back" onClick={() => setStep((n) => n - 1)}>Back</button>}
      </div>
    </div>
  );
}

function StepYou({ s, onNext, saving }: { s: Settings; onNext: (name: string) => void; saving: boolean }) {
  const [name, setName] = useState(s.user_name);
  return (
    <form className="ob-step" onSubmit={(e) => { e.preventDefault(); onNext(name); }}>
      <h1 className="ob-title">Notes in your words.<br />Transcripts on the side.</h1>
      <p className="ob-body">
        Margin records your meetings, transcribes them on this Mac and keeps each one as a Markdown file you own.
        No account, no cloud and no bot in your calls.
      </p>
      <label className="field-label" htmlFor="ob-name">Your name, so transcripts can label you</label>
      <input id="ob-name" className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder="Your name" autoFocus />
      <div className="ob-actions"><button className="btn btn-primary btn-lg" disabled={saving}>Continue</button></div>
    </form>
  );
}

function StepFolder({ s, onNext, saving }: { s: Settings; onNext: (dir: string) => void; saving: boolean }) {
  const [choices, setChoices] = useState<{ default_dir: string; obsidian_vaults: { name: string; path: string }[] } | null>(null);
  const [dir, setDir] = useState(s.output_dir);
  useEffect(() => { api.folderChoices().then(setChoices).catch(() => {}); }, []);

  const options = useMemo(() => {
    if (!choices) return [];
    const list = [{ key: "default", title: "Documents", path: choices.default_dir, body: "A Meetings folder in your Documents." }];
    for (const v of choices.obsidian_vaults) {
      list.push({ key: v.path, title: `Obsidian: ${v.name}`, path: `${v.path}/Meetings`, body: "Meetings show up in your vault as regular notes." });
    }
    return list;
  }, [choices]);

  const pick = async () => {
    const chosen = await open({ directory: true, multiple: false, title: "Choose where to save meetings" });
    if (typeof chosen === "string") setDir(chosen);
  };

  const custom = !options.some((o) => o.path === dir);
  return (
    <div className="ob-step">
      <h1 className="ob-title">Where should meetings go?</h1>
      <p className="ob-body">
        Each meeting is one Markdown file with its audio next to it. Pick a folder in iCloud Drive to back them up,
        or a folder in an Obsidian vault to read them there.
      </p>
      <div className="options" role="radiogroup">
        {options.map((o) => (
          <button key={o.key} role="radio" aria-checked={dir === o.path} className={`option ${dir === o.path ? "on" : ""}`} onClick={() => setDir(o.path)}>
            <span className="option-title">{o.title}</span>
            <span className="option-body">{o.body}</span>
            <span className="option-path path">{tildify(o.path)}</span>
          </button>
        ))}
        <button role="radio" aria-checked={custom} className={`option ${custom ? "on" : ""}`} onClick={pick}>
          <span className="option-title">Another folder…</span>
          <span className="option-body">{custom ? "Chosen:" : "Pick any folder, such as one in iCloud Drive."}</span>
          {custom && <span className="option-path path">{tildify(dir)}</span>}
        </button>
      </div>
      <div className="ob-actions"><button className="btn btn-primary btn-lg" onClick={() => onNext(dir)} disabled={saving}>Use this folder</button></div>
    </div>
  );
}

function StepModel({ s, onNext, saving }: { s: Settings; onNext: (model: string) => void; saving: boolean }) {
  const [model, setModel] = useState(s.model);
  return (
    <div className="ob-step">
      <h1 className="ob-title">How accurate should transcripts be?</h1>
      <p className="ob-body">Margin downloads a speech model once, then works offline. You can change this later in Settings.</p>
      <div className="options options-row" role="radiogroup">
        {MODEL_CHOICES.map((m) => (
          <button key={m.id} role="radio" aria-checked={model === m.id} className={`option ${model === m.id ? "on" : ""}`} onClick={() => setModel(m.id)}>
            <span className="option-title">{m.title} <span className="mono option-size">{m.size}</span></span>
            <span className="option-body">{m.body}</span>
          </button>
        ))}
      </div>
      <div className="ob-actions">
        <button className="btn btn-primary btn-lg" disabled={saving} onClick={() => { onNext(model); }}>
          Download and continue
        </button>
      </div>
    </div>
  );
}

function StepPermissions({ onNext }: { onNext: () => void }) {
  const [p, setP] = useState<Permissions | null>(null);
  const [downloadStarted, setDownloadStarted] = useState(false);
  const refresh = () => api.permissions().then(setP).catch(() => {});
  useEffect(() => {
    refresh();
    // Models download in the background while you read this.
    if (!downloadStarted) {
      api.downloadModels().catch(() => {});
      setDownloadStarted(true);
    }
    const f = () => refresh();
    window.addEventListener("focus", f);
    return () => window.removeEventListener("focus", f);
  }, [downloadStarted]);

  return (
    <div className="ob-step">
      <h1 className="ob-title">Two permissions, both explained.</h1>
      <p className="ob-body">macOS asks once for each. Audio is only recorded while you're recording a meeting, and never leaves this Mac.</p>
      <PermissionRow
        title="Microphone"
        body="Records your voice, and the whole room for in-person meetings."
        state={p?.microphone}
        onRequest={async () => { await api.requestPermission("microphone"); refresh(); }}
        onSettings={() => api.openPrivacySettings("microphone")}
      />
      <PermissionRow
        title="System audio"
        body="Records the other people on a call, straight from Zoom, Meet or Teams. macOS lists this under Screen & System Audio Recording, but Margin never records your screen."
        state={p?.system_audio}
        onRequest={async () => { await api.requestPermission("system-audio"); refresh(); }}
        onSettings={() => api.openPrivacySettings("system-audio")}
      />
      <DownloadProgress />
      <div className="ob-actions">
        <button className="btn btn-primary btn-lg" onClick={onNext}>
          {p?.microphone === "granted" ? "Continue" : "Continue without allowing"}
        </button>
      </div>
    </div>
  );
}

export function DownloadProgress({ compact }: { compact?: boolean }) {
  const [files, setFiles] = useState<ModelFile[]>([]);
  const [progress, setProgress] = useState<Record<string, ModelProgress>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    api.modelsStatus().then(setFiles).catch(() => {});
    const sub = on<ModelProgress>("model-progress", (p) => {
      if (p.error) setError(p.error);
      setProgress((prev) => ({ ...prev, [p.id]: p }));
      // Also catches a download for a model this view didn't know about yet.
      if (p.finished || p.all_done) api.modelsStatus().then(setFiles);
      else setFiles((f) => (f.some((x) => x.id === p.id) ? f : (api.modelsStatus().then(setFiles), f)));
    });
    return () => { sub.then((u) => u()); };
  }, []);

  if (!files.length) return null;
  const total = files.reduce((n, f) => n + (progress[f.id]?.total || f.approx_bytes), 0);
  const done = files.reduce((n, f) => n + (f.installed ? progress[f.id]?.total || f.approx_bytes : progress[f.id]?.downloaded ?? 0), 0);
  const all = files.every((f) => f.installed);
  const pct = all ? 100 : Math.min(99, Math.floor((done / Math.max(1, total)) * 100));

  return (
    <div className={`download ${compact ? "compact" : ""}`}>
      <div className="download-head">
        <span>{all ? "Speech models ready" : error ? "Download stopped" : "Downloading speech models"}</span>
        <span className="mono">{all ? bytes(total) : `${bytes(done)} of ${bytes(total)}`}</span>
      </div>
      <div className="bar" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <div className="bar-fill" style={{ width: `${pct}%` }} />
      </div>
      {error && (
        <p className="download-error">
          {error} <button className="link-btn" onClick={() => { setError(null); api.downloadModels(); }}>Try again</button>
        </p>
      )}
    </div>
  );
}

function StepSummaries({ s, onFinish, saving }: { s: Settings; onFinish: (patch: Partial<Settings>) => void; saving: boolean }) {
  const [backends, setBackends] = useState<Backends | null>(null);
  const [choice, setChoice] = useState<Partial<Settings>>({ summary_engine: s.summary_engine, ollama_model: s.ollama_model });
  const [ready, setReady] = useState(false);
  useEffect(() => { api.summaryBackends().then(setBackends).catch(() => {}); }, []);
  useEffect(() => {
    const check = () => api.modelsStatus().then((f) => setReady(f.every((x) => x.installed)));
    check();
    const sub = on<ModelProgress>("model-progress", (p) => (p.finished || p.all_done) && check());
    return () => { sub.then((u) => u()); };
  }, []);

  return (
    <div className="ob-step">
      <h1 className="ob-title">Want summaries too?</h1>
      <p className="ob-body">
        Optional. Margin can ask an AI you already have to summarize each meeting. Without one you still get
        the full transcript and your notes.
      </p>
      <SummaryChoice backends={backends} value={choice} onChange={setChoice} />
      <DownloadProgress compact />
      <div className="ob-actions">
        <button className="btn btn-primary btn-lg" disabled={saving || !ready} onClick={() => onFinish(choice)}>
          {ready ? "Start using Margin" : "Waiting for the download…"}
        </button>
      </div>
    </div>
  );
}
