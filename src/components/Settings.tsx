import { useEffect, useRef, useState } from "react";
import { api, errorText, type AppPrefs, type Backends, type Boot, type Permissions, type Settings, type SettingsOptions, type Voice } from "../api";
import type { SectionId } from "./SettingsNav";
import { ResetDialog } from "./ResetDialog";
import { GeneralSection, RecordingSection, SummariesSection, TranscriptionSection } from "./SettingsSections";
import { AboutSection, IntegrationsSection, PermissionsSection, ResetSection, UpdatesSection, VoicesSection } from "./SettingsMore";

export { SettingsNav } from "./SettingsNav";

/** Settings stored outside config.toml. A save sends them only when they changed. */
const EXTRA_KEYS = ["vocabulary", "summary_instructions", "open_at_login"] as const;
const NO_OPTIONS: SettingsOptions = { languages: [], input_devices: [], silence_stop_minutes: [], keep_audio_days: [] };

export type Notify = (t: string, tone?: "info" | "error") => void;

interface Props {
  boot: Boot;
  section: SectionId;
  onBack: () => void;
  onSaved: (s: Settings) => void;
  onReset: () => void;
  notify: Notify;
}

/** The Settings page: one section at a time, all sharing one settings state and save queue. */
export function SettingsPage({ boot, section, onBack, onSaved, onReset, notify }: Props) {
  const ctx = useSettingsState(boot, onSaved, notify);
  const [resetOpen, setResetOpen] = useState(false);
  // WebKit doesn't focus a clicked button, so the opener comes from the click itself.
  const resetOpener = useRef<HTMLElement | null>(null);
  const cancelReset = () => {
    setResetOpen(false);
    resetOpener.current?.focus();
  };
  const scroller = useRef<HTMLDivElement>(null);
  useEscape(onBack, resetOpen);
  useEffect(() => scroller.current?.scrollTo(0, 0), [section]);

  const openReset = (from: HTMLElement) => {
    resetOpener.current = from;
    setResetOpen(true);
  };
  const view = { ...ctx, boot, notify, openReset };
  return (
    <div className="page page-settings" role="region" aria-label="Settings" ref={scroller}>
      {/* The drop-downs need their choices, so the body waits for them (a few ms). */}
      {ctx.options && (
        <div className="set-inner" key={section}>
          {section === "general" && <GeneralSection {...view} />}
          {section === "recording" && <RecordingSection {...view} />}
          {section === "transcription" && <TranscriptionSection {...view} />}
          {section === "summaries" && <SummariesSection {...view} />}
          {section === "voices" && <VoicesSection {...view} />}
          {section === "permissions" && <PermissionsSection {...view} />}
          {section === "integrations" && <IntegrationsSection {...view} />}
          {section === "updates" && <UpdatesSection {...view} />}
          {section === "about" && <AboutSection {...view} />}
          {section === "reset" && <ResetSection {...view} />}
        </div>
      )}
      {resetOpen && <ResetDialog onCancel={cancelReset} onDone={onReset} notify={notify} />}
    </div>
  );
}

export type SettingsView = ReturnType<typeof useSettingsState> & { boot: Boot; notify: Notify; openReset: (from: HTMLElement) => void };

/** Everything the sections read and change, loaded once when Settings opens. */
function useSettingsState(boot: Boot, onSaved: (s: Settings) => void, notify: Notify) {
  const [s, setS] = useState<Settings>(boot.settings);
  const [backends, setBackends] = useState<Backends | null>(null);
  const [perms, setPerms] = useState<Permissions | null>(null);
  const [voices, setVoices] = useState<Voice[]>([]);
  const [mcp, setMcp] = useState("");
  const [options, setOptions] = useState<SettingsOptions | null>(null);
  const [prefs, setPrefs] = useState<AppPrefs>(boot.prefs);

  const refreshPerms = () => api.permissions().then(setPerms).catch(() => {});
  useEffect(() => {
    api.summaryBackends().then(setBackends).catch(() => {});
    refreshPerms();
    api.listVoices().then(setVoices).catch(() => {});
    api.mcpSnippet().then(setMcp).catch(() => {});
    api.settingsOptions().then(setOptions).catch(() => setOptions(NO_OPTIONS));
    // Permissions change in System Settings, so look again whenever the window comes back.
    window.addEventListener("focus", refreshPerms);
    return () => window.removeEventListener("focus", refreshPerms);
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

  // Recording sounds, the global shortcut and updates live in prefs.json, saved on their own.
  const changePrefs = async (next: AppPrefs) => {
    const before = prefs;
    setPrefs(next);
    try {
      setPrefs(await api.saveAppPrefs(next));
    } catch (e) {
      setPrefs(before);
      notify(`Not saved: ${errorText(e)}`, "error");
    }
  };

  return { s, save, options, backends, perms, refreshPerms, voices, setVoices, mcp, prefs, changePrefs };
}

/** Esc leaves Settings. The Reset dialog takes Esc for itself while it is open. */
function useEscape(onBack: () => void, resetOpen: boolean) {
  const state = useRef({ onBack, resetOpen });
  state.current = { onBack, resetOpen };
  useEffect(() => {
    const esc = (e: KeyboardEvent) => {
      if (e.key !== "Escape" || e.defaultPrevented || state.current.resetOpen) return;
      e.preventDefault();
      state.current.onBack();
    };
    window.addEventListener("keydown", esc);
    return () => window.removeEventListener("keydown", esc);
  }, []);
}
