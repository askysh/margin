import { useState, type ReactNode } from "react";
import type { PermState } from "../api";

interface Props {
  title: string;
  body: string;
  state: PermState | undefined;
  onRequest: () => Promise<void>;
  onSettings: () => void;
  icon?: ReactNode;
}

export function PermissionRow({ title, body, state, onRequest, onSettings, icon }: Props) {
  const [asking, setAsking] = useState(false);
  return (
    <div className={`perm ${state === "granted" ? "is-granted" : ""}`}>
      {icon && <span className="perm-icon" aria-hidden>{icon}</span>}
      <div className="perm-text">
        <div className="perm-title">
          {title}
          {state === "granted" && <span className="badge badge-ok">Allowed</span>}
          {state === "denied" && <span className="badge badge-warn">Not allowed</span>}
        </div>
        <p className="perm-body">{asking ? "macOS is asking now. Choose Allow in its dialog." : body}</p>
      </div>
      {state === "granted" ? null : state === "denied" || state === "unknown" ? (
        <button type="button" className="btn btn-quiet" onClick={onSettings}>Open System Settings</button>
      ) : (
        <button
          type="button"
          className="btn btn-primary"
          disabled={asking}
          onClick={async () => { setAsking(true); try { await onRequest(); } finally { setAsking(false); } }}
        >
          {asking ? "Waiting for macOS…" : "Allow"}
        </button>
      )}
    </div>
  );
}

export const MicIcon = () => (
  <svg width="18" height="18" viewBox="0 0 20 20"><rect x="7" y="2.5" width="6" height="10" rx="3" fill="none" stroke="currentColor" strokeWidth="1.6" /><path d="M4.5 9.5a5.5 5.5 0 0011 0M10 15v2.5" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>
);

export const SpeakerIcon = () => (
  <svg width="18" height="18" viewBox="0 0 20 20"><path d="M3 7.5h3l4-3.5v12l-4-3.5H3z" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" /><path d="M13.5 7a4 4 0 010 6M15.8 4.8a7 7 0 010 10.4" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" /></svg>
);
