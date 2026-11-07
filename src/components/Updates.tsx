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

export async function restart(notify: (t: string, tone?: "info" | "error") => void) {
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
