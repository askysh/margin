import { useEffect, useState } from "react";
import { copyText } from "../util";

export interface CopyChoice {
  label: string;
  hint: string;
  /** Toast text after copying, such as "Summary copied." */
  done: string;
  text: () => string;
}

/** "Copy ▾" in a meeting's action line: each choice puts Markdown on the clipboard. */
export function CopyMenu({ choices, notify }: { choices: CopyChoice[]; notify: (t: string, tone?: "info" | "error") => void }) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    const esc = (e: KeyboardEvent) => e.key === "Escape" && close();
    window.addEventListener("click", close);
    window.addEventListener("keydown", esc);
    return () => {
      window.removeEventListener("click", close);
      window.removeEventListener("keydown", esc);
    };
  }, [open]);
  if (choices.length === 0) return null;

  const copy = async (c: CopyChoice) => {
    setOpen(false);
    notify((await copyText(c.text())) ? c.done : "Couldn't copy to the clipboard.", "info");
  };

  return (
    <div className="copy-wrap">
      <button
        className="link-btn"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={(e) => { e.stopPropagation(); setOpen((o) => !o); }}
      >
        Copy
      </button>
      {open && (
        <div className="menu menu-left" role="menu" onClick={(e) => e.stopPropagation()}>
          {choices.map((c) => (
            <button key={c.label} role="menuitem" onClick={() => copy(c)}>
              <strong>{c.label}</strong>
              <span>{c.hint}</span>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
