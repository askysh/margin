import { useEffect, useState } from "react";
import { api, errorText, type ResetPreview } from "../api";
import { bytes, tildify } from "../util";

type Notify = (t: string, tone?: "info" | "error") => void;

/** Settings → Start over. Keeps meetings, models and voices unless you untick them. */
export function ResetDialog({ onCancel, onDone, notify }: { onCancel: () => void; onDone: () => void; notify: Notify }) {
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
    <div className="dialog-backdrop" onKeyDown={(e) => {
      if (e.key !== "Escape" || busy) return;
      e.preventDefault(); // so Settings, under the dialog, stays open
      onCancel();
    }}>
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
