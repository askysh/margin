import { useEffect, useMemo, useState } from "react";
import { open } from "@tauri-apps/plugin-dialog";
import { api, errorText, type Backends, type Permissions, type Settings } from "../api";
import { tildify } from "../util";
import { DownloadProgress, MODEL_CHOICES } from "./Models";
import { MicIcon, PermissionRow, SpeakerIcon } from "./PermissionRow";
import { SummaryChoice } from "./SummaryChoice";

const STEPS = ["You", "Setup", "Permissions", "Summaries", "Ready"] as const;
const SETUP = 1;
const READY = STEPS.length - 1;

/** First run. Every step has a sensible default, so Enter (or Continue) is enough. The
 * speech models start downloading when step 2 is done and keep going in the background. */
export function Onboarding({ initial, onDone }: { initial: Settings; onDone: () => void }) {
  const [step, setStep] = useState(0);
  const [s, setS] = useState<Settings>(initial);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);

  useEnterContinues();
  useEffect(() => {
    // The name field takes focus itself; later steps focus their Continue button.
    if (step > 0) document.querySelector<HTMLElement>(".ob-card [data-primary]")?.focus({ preventScroll: true });
  }, [step]);

  const next = async (patch: Partial<Settings>) => {
    setSaving(true);
    setError(null);
    try {
      setS(await api.saveSettings({ ...s, ...patch }));
      if (step === SETUP) api.downloadModels().catch((e) => setError(errorText(e)));
      setStep((n) => n + 1);
    } catch (e) {
      setError(errorText(e));
    } finally {
      setSaving(false);
    }
  };

  const finish = async () => {
    setSaving(true);
    try {
      await api.completeOnboarding();
      onDone();
    } catch (e) {
      setError(errorText(e));
      setSaving(false);
    }
  };

  return (
    <div className="onboarding" data-tauri-drag-region>
      <div className="ob-card">
        <header className="ob-head">
          <span className="ob-brand">Margin</span>
          <ol className="ob-steps" aria-label={`Setup, step ${step + 1} of ${STEPS.length}`}>
            {STEPS.map((label, i) => (
              <li key={label} className={i < step ? "done" : i === step ? "now" : ""} aria-current={i === step ? "step" : undefined}>{label}</li>
            ))}
          </ol>
          {step > 0 && <button type="button" className="ob-back" onClick={() => setStep((n) => n - 1)}>Back</button>}
        </header>
        <div className="ob-step" key={step}>
          {step === 0 && <StepYou s={s} saving={saving} onNext={(name) => next({ user_name: name })} />}
          {step === 1 && <StepSetup s={s} saving={saving} onNext={next} />}
          {step === 2 && <StepPermissions onNext={() => setStep(3)} />}
          {step === 3 && <StepSummaries s={s} saving={saving} onNext={next} />}
          {step === READY && <StepReady s={s} saving={saving} onFinish={finish} />}
        </div>
        {error && <p className="ob-error" role="alert">{error}</p>}
        {step > SETUP && step < READY && <div className="ob-foot"><DownloadProgress compact /></div>}
      </div>
    </div>
  );
}

/** Enter continues: in the name field, and on an option you just picked. Other buttons
 * (Allow, Back) keep their own Enter. */
function useEnterContinues() {
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || e.isComposing || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      const t = e.target as HTMLElement;
      if (t.closest("textarea, select, a") || (t.closest("button") && t.getAttribute("role") !== "radio")) return;
      e.preventDefault();
      document.querySelector<HTMLButtonElement>(".ob-card [data-primary]")?.click();
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, []);
}

function Continue({ label = "Continue", disabled, onClick }: { label?: string; disabled?: boolean; onClick: () => void }) {
  return (
    <div className="ob-actions">
      <button type="button" className="btn btn-primary btn-lg" data-primary disabled={disabled} onClick={onClick}>{label}</button>
      <span className="ob-key" aria-hidden>or press Return</span>
    </div>
  );
}

function StepYou({ s, onNext, saving }: { s: Settings; onNext: (name: string) => void; saving: boolean }) {
  const [name, setName] = useState(s.user_name);
  return (
    <>
      <h1 className="ob-title">Notes in your words.<br />Transcripts on the side.</h1>
      <p className="ob-body">
        Margin records your meetings and transcribes them on this Mac. Each one is a Markdown file you own.
        No account, no cloud, no bot in your calls.
      </p>
      <label className="field-label" htmlFor="ob-name">Your name, so transcripts can label you</label>
      <input id="ob-name" className="field" value={name} onChange={(e) => setName(e.target.value)} placeholder="Your name" autoFocus />
      <Continue disabled={saving} onClick={() => onNext(name)} />
    </>
  );
}

type FolderChoices = { default_dir: string; obsidian_vaults: { name: string; path: string }[] };

/** Folder and model on one page: both have good defaults, and the download can start sooner. */
function StepSetup({ s, onNext, saving }: { s: Settings; onNext: (patch: Partial<Settings>) => void; saving: boolean }) {
  const [choices, setChoices] = useState<FolderChoices | null>(null);
  const [dir, setDir] = useState(s.output_dir);
  const [model, setModel] = useState(s.model);
  useEffect(() => { api.folderChoices().then(setChoices).catch(() => {}); }, []);

  const options = useMemo(() => {
    if (!choices) return [];
    const list = [{ key: "default", title: "Documents", path: choices.default_dir }];
    for (const v of choices.obsidian_vaults) list.push({ key: v.path, title: `Obsidian: ${v.name}`, path: `${v.path}/Meetings` });
    return list;
  }, [choices]);

  const pick = async () => {
    const chosen = await open({ directory: true, multiple: false, title: "Choose where to save meetings" });
    if (typeof chosen === "string") setDir(chosen);
  };

  const custom = choices !== null && !options.some((o) => o.path === dir);
  const chosenModel = MODEL_CHOICES.find((m) => m.id === model) ?? MODEL_CHOICES[1];
  return (
    <>
      <h1 className="ob-title">Where should meetings go?</h1>
      <p className="ob-body">One Markdown file per meeting, with its audio beside it. Choose a folder in a vault to read them in Obsidian.</p>
      <div className="options options-tight" role="radiogroup" aria-label="Meetings folder">
        {options.map((o) => (
          <button type="button" key={o.key} role="radio" aria-checked={dir === o.path} className={`option ${dir === o.path ? "on" : ""}`} onClick={() => setDir(o.path)}>
            <span className="option-title">{o.title}</span>
            <span className="option-path path">{tildify(o.path)}</span>
          </button>
        ))}
        <button type="button" role="radio" aria-checked={custom} className={`option ${custom ? "on" : ""}`} onClick={pick}>
          <span className="option-title">Another folder…</span>
          {custom ? <span className="option-path path">{tildify(dir)}</span> : <span className="option-body">iCloud Drive, or anywhere else.</span>}
        </button>
      </div>

      <h2 className="ob-sub" id="ob-model">Transcript quality</h2>
      <div className="segmented" role="radiogroup" aria-labelledby="ob-model">
        {MODEL_CHOICES.map((m) => (
          <button type="button" key={m.id} role="radio" aria-checked={model === m.id} className={model === m.id ? "on" : ""} onClick={() => setModel(m.id)}>
            {m.title} <span className="mono">{m.size}</span>
          </button>
        ))}
      </div>
      <p className="ob-hint">{chosenModel.body} It downloads once, in the background, then works offline.</p>
      <Continue disabled={saving} onClick={() => onNext({ output_dir: dir, model })} />
    </>
  );
}

function StepPermissions({ onNext }: { onNext: () => void }) {
  const [p, setP] = useState<Permissions | null>(null);
  useEffect(() => {
    const refresh = () => api.permissions().then(setP).catch(() => {});
    refresh();
    window.addEventListener("focus", refresh);
    return () => window.removeEventListener("focus", refresh);
  }, []);
  const ask = async (which: "microphone" | "system-audio") => {
    await api.requestPermission(which).catch(() => {});
    setP(await api.permissions());
  };
  const allGranted = p?.microphone === "granted" && p?.system_audio === "granted";

  return (
    <>
      <h1 className="ob-title">Let Margin hear the meeting.</h1>
      <p className="ob-body">macOS asks once for each. Margin listens only while you record, and the audio stays on this Mac.</p>
      <div className="perms">
        <PermissionRow
          icon={<MicIcon />}
          title="Microphone"
          body="Your voice, and the whole room for in-person meetings."
          state={p?.microphone}
          onRequest={() => ask("microphone")}
          onSettings={() => api.openPrivacySettings("microphone")}
        />
        <PermissionRow
          icon={<SpeakerIcon />}
          title="System audio"
          body="The other people on a call, straight from Zoom, Meet or Teams. macOS lists this as Screen & System Audio Recording, but Margin never records your screen."
          state={p?.system_audio}
          onRequest={() => ask("system-audio")}
          onSettings={() => api.openPrivacySettings("system-audio")}
        />
      </div>
      {p && !allGranted && <p className="ob-hint">You can also allow these later, in Settings or on your first recording.</p>}
      <Continue onClick={onNext} />
    </>
  );
}

function StepSummaries({ s, onNext, saving }: { s: Settings; onNext: (patch: Partial<Settings>) => void; saving: boolean }) {
  const [backends, setBackends] = useState<Backends | null>(null);
  const [choice, setChoice] = useState<Partial<Settings>>({ summary_engine: s.summary_engine, ollama_model: s.ollama_model, claude_path: s.claude_path });
  useEffect(() => { api.summaryBackends().then(setBackends).catch(() => {}); }, []);
  return (
    <>
      <h1 className="ob-title">Want summaries too?</h1>
      <p className="ob-body">Optional. Margin can ask an AI you already use to summarize each meeting. You always get the transcript and your notes.</p>
      <SummaryChoice backends={backends} value={choice} onChange={setChoice} />
      <Continue disabled={saving} onClick={() => onNext(choice)} />
    </>
  );
}

function StepReady({ s, saving, onFinish }: { s: Settings; saving: boolean; onFinish: () => void }) {
  return (
    <>
      <h1 className="ob-title">You're ready.</h1>
      <p className="ob-body">
        When a meeting starts, start a recording and type notes in your own words. Margin writes the transcript and saves
        it to <span className="path">{tildify(s.output_dir)}</span>.
      </p>
      <ul className="ways" aria-label="Ways to start a recording">
        <li>
          <span className="way-demo"><span className="btn btn-primary demo-btn">Record call</span></span>
          <span className="way-text"><strong>In the window.</strong> Top right. Its arrow also offers In person.</span>
        </li>
        <li>
          <span className="way-demo"><span className="demo-menubar" aria-hidden><span className="demo-glyph" /></span></span>
          <span className="way-text"><strong>From the menu bar.</strong> Margin stays there when you close the window.</span>
        </li>
        <li>
          <span className="way-demo"><kbd>⌘</kbd><kbd>⇧</kbd><kbd>R</kbd></span>
          <span className="way-text"><strong>From the keyboard.</strong> <span className="mono">⌘⇧R</span> records a call, <span className="mono">⌘⇧I</span> a meeting in the room.</span>
        </li>
      </ul>
      <DownloadProgress pendingNote="Recording works once this finishes. You can look around meanwhile." />
      <Continue label="Start using Margin" disabled={saving} onClick={onFinish} />
    </>
  );
}
