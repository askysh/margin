import { useEffect, useState } from "react";

export interface MoreChoice {
  label: string;
  onSelect: () => void;
  danger?: boolean;
}

/** "More ▾" at the end of a meeting's action line, for the actions used less often. */
export function MoreMenu({ choices }: { choices: MoreChoice[] }) {
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

  return (
    <div className="copy-wrap">
      <button
        className="link-btn"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={(e) => { e.stopPropagation(); setOpen((o) => !o); }}
      >
        More
      </button>
      {open && (
        <div className="menu menu-left menu-compact" role="menu" onClick={(e) => e.stopPropagation()}>
          {choices.map((c) => (
            <button key={c.label} role="menuitem" className={c.danger ? "danger" : ""} onClick={() => { setOpen(false); c.onSelect(); }}>
              <strong>{c.label}</strong>
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
