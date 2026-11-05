import { useState } from "react";
import type { PermState } from "../api";

interface Props {
  title: string;
  body: string;
  state: PermState | undefined;
  onRequest: () => Promise<void>;
  onSettings: () => void;
}

export function PermissionRow({ title, body, state, onRequest, onSettings }: Props) {
  const [asking, setAsking] = useState(false);
  return (
    <div className="perm">
      <div className="perm-text">
        <div className="perm-title">
          {title}
          {state === "granted" && <span className="badge badge-ok">Allowed</span>}
          {state === "denied" && <span className="badge badge-warn">Not allowed</span>}
        </div>
        <p className="perm-body">{body}</p>
      </div>
      {state === "granted" ? null : state === "denied" || state === "unknown" ? (
        <button className="btn btn-quiet" onClick={onSettings}>Open System Settings</button>
      ) : (
        <button
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
