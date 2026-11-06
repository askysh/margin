import { useState } from "react";
import { api, errorText, type AppPrefs } from "../api";

/** Settings rows for recording sounds and the global shortcut (prefs.json). */
export function AppPrefsBlock({ initial, notify }: { initial: AppPrefs; notify: (t: string, tone?: "info" | "error") => void }) {
  const [prefs, setPrefs] = useState(initial);
  const change = async (next: AppPrefs) => {
    setPrefs(next);
    try {
      setPrefs(await api.saveAppPrefs(next));
    } catch (e) {
      setPrefs(prefs);
      notify(`Not saved: ${errorText(e)}`, "error");
    }
  };
  return (
    <section className="set">
      <h2 className="eyebrow">Sounds and shortcut</h2>
      <label className="check">
        <input type="checkbox" checked={prefs.sound_cues} onChange={(e) => change({ ...prefs, sound_cues: e.target.checked })} />
        Play a sound when a recording starts, stops and is ready
      </label>
      <label className="check">
        <input type="checkbox" checked={prefs.record_shortcut} onChange={(e) => change({ ...prefs, record_shortcut: e.target.checked })} />
        Start and stop recording from any app with <kbd className="kbd">⌃⌥⌘R</kbd>
      </label>
    </section>
  );
}
