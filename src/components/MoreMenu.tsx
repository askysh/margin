import { useMenu } from "../menu";

export interface MoreChoice {
  label: string;
  onSelect: () => void;
  danger?: boolean;
}

/** "More ▾" at the end of a meeting's action line, for the actions used less often. */
export function MoreMenu({ choices }: { choices: MoreChoice[] }) {
  const [open, setOpen] = useMenu();

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
