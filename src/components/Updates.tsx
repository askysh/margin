import { useEffect, useState } from "react";
import { api, errorText, on, type UpdateStatus } from "../api";

/** The updater's state, kept fresh by its "update-status" events. */
export function useUpdateStatus() {
  const [status, setStatus] = useState<UpdateStatus | null>(null);
  useEffect(() => {
    api.updateStatus().then(setStatus).catch(() => {});
    const sub = on<UpdateStatus>("update-status", setStatus);
    return () => { sub.then((u) => u()); };
  }, []);
  return [status, setStatus] as const;
}

async function restart(notify: (t: string, tone?: "info" | "error") => void) {
  try {
    await api.installUpdate();
  } catch (e) {
    notify(errorText(e), "error");
  }
}

/** Toolbar note when a downloaded update waits for a restart. */
export function UpdateBanner({ recording, notify }: { recording: boolean; notify: (t: string, tone?: "info" | "error") => void }) {
  const [status] = useUpdateStatus();
  const [dismissed, setDismissed] = useState<string | null>(null);
  if (!status?.ready || dismissed === status.ready) return null;
  return (
    <div className="call-banner" role="status">
      <span>Margin {status.ready} is ready.</span>
      {recording ? (
        <span className="quiet">It installs after this recording.</span>
      ) : (
        <button className="link-btn" onClick={() => restart(notify)}>Restart to update</button>
      )}
      <button className="icon-btn small" aria-label="Later" title="Later: it installs when you quit Margin" onClick={() => setDismissed(status.ready)}>×</button>
    </div>
  );
}

/** Settings → Updates: version, the automatic-check switch and Check now. */
export function UpdatesSection({ auto, onAuto, notify }: { auto: boolean; onAuto: (on: boolean) => void; notify: (t: string, tone?: "info" | "error") => void }) {
  const [status, setStatus] = useUpdateStatus();
  const [asked, setAsked] = useState(false);
  if (!status) return null;
  const check = async () => {
    setAsked(true);
    try {
      setStatus(await api.checkUpdates());
    } catch (e) {
      notify(errorText(e), "error");
    }
  };
  const line = !status.supported
    ? "Updates work only in the installed app."
    : status.checking
      ? "Checking…"
      : status.ready
        ? `Margin ${status.ready} is downloaded. It installs when you restart or quit Margin.`
        : status.error
          ? status.error
          : asked || status.checked_secs_ago !== null
            ? "Margin is up to date."
            : "";
  return (
    <section className="set">
      <h2 className="eyebrow">Updates</h2>
      <div className="set-row">
        <span>Margin <span className="mono">{status.current}</span></span>
        <div className="row-actions">
          {status.ready ? (
            <button className="btn btn-primary" onClick={() => restart(notify)}>Restart to update</button>
          ) : (
            <button className="btn btn-quiet" onClick={check} disabled={!status.supported || status.checking}>Check now</button>
          )}
        </div>
      </div>
      {line && <p className="quiet-note">{line}</p>}
      <label className="check">
        <input type="checkbox" checked={auto} onChange={(e) => onAuto(e.target.checked)} />
        Check for updates and download them in the background
      </label>
    </section>
  );
}
