import { useEffect, useState } from "react";
import { api, on, type ModelFile, type ModelProgress } from "../api";
import { bytes } from "../util";

export const MODEL_CHOICES = [
  { id: "base", title: "Fast", size: "148 MB", body: "Quick and light. Fine for clear audio." },
  { id: "small", title: "Balanced", size: "488 MB", body: "Recommended. Handles accents and crosstalk well." },
  { id: "large-v3-turbo", title: "Best", size: "1.6 GB", body: "Most accurate. Takes longer to download." },
];

interface Props {
  compact?: boolean;
  /** Show nothing once every model is installed. */
  hideWhenReady?: boolean;
  /** A line shown under the bar until the download is done. */
  pendingNote?: string;
}

/** Progress of the speech-model download, and a retry when it fails. */
export function DownloadProgress({ compact, hideWhenReady, pendingNote }: Props) {
  const [files, setFiles] = useState<ModelFile[]>([]);
  const [progress, setProgress] = useState<Record<string, ModelProgress>>({});
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    const refresh = () => api.modelsStatus().then(setFiles).catch(() => {});
    refresh();
    const sub = on<ModelProgress>("model-progress", (p) => {
      if (p.error) setError(p.error);
      setProgress((prev) => ({ ...prev, [p.id]: p }));
      if (p.all_done && !p.error) {
        // A model chosen while another one was downloading is fetched now.
        api.modelsStatus().then((f) => {
          setFiles(f);
          if (f.some((x) => !x.installed)) api.downloadModels().catch(() => {});
        }).catch(() => {});
      } else if (p.finished) refresh();
      // Also catches a download for a model this view didn't know about yet.
      else setFiles((f) => (f.some((x) => x.id === p.id) ? f : (refresh(), f)));
    });
    return () => { sub.then((u) => u()); };
  }, []);

  if (!files.length) return null;
  const total = files.reduce((n, f) => n + (progress[f.id]?.total || f.approx_bytes), 0);
  const done = files.reduce((n, f) => n + (f.installed ? progress[f.id]?.total || f.approx_bytes : progress[f.id]?.downloaded ?? 0), 0);
  const all = files.every((f) => f.installed);
  if (all && hideWhenReady) return null;
  const pct = all ? 100 : Math.min(99, Math.floor((done / Math.max(1, total)) * 100));

  return (
    <div className={`download ${compact ? "compact" : ""} ${all ? "is-ready" : ""}`}>
      <div className="download-head">
        <span>{all ? "Speech models ready" : error ? "Download stopped" : "Downloading speech models"}</span>
        <span className="mono">{all ? bytes(total) : `${bytes(done)} of ${bytes(total)}`}</span>
      </div>
      <div className="bar" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}>
        <div className="bar-fill" style={{ width: `${pct}%` }} />
      </div>
      {!all && !error && pendingNote && <p className="download-note">{pendingNote}</p>}
      {error && (
        <p className="download-error">
          {error} <button className="link-btn" onClick={() => { setError(null); api.downloadModels(); }}>Try again</button>
        </p>
      )}
    </div>
  );
}
