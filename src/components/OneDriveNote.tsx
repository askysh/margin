import { useEffect, useState } from "react";
import { api } from "../api";
import { hasOneDriveNote } from "../platform";

/** Windows: a calm note for a meetings folder that is inside OneDrive. OneDrive backup
 * moves Documents into OneDrive, so the default folder is the usual case, and the audio
 * of each meeting then goes to OneDrive too. The note says so and lets the user choose
 * another folder. It shows nothing for any other folder, and nothing on macOS. */
export function OneDriveNote({ dir, onChoose, className }: { dir: string; onChoose: () => void; className: string }) {
  const [inside, setInside] = useState(false);
  useEffect(() => {
    if (!hasOneDriveNote) return;
    let current = true;
    api.folderInOneDrive(dir).then((yes) => current && setInside(yes)).catch(() => current && setInside(false));
    return () => {
      current = false;
    };
  }, [dir]);
  if (!inside) return null;
  return (
    <span className={`onedrive-note ${className}`} role="note">
      This folder is in OneDrive. OneDrive can upload your recordings.{" "}
      <button type="button" className="link-btn" onClick={onChoose}>Choose another folder</button>
    </span>
  );
}
