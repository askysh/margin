import { useEffect, useRef, type KeyboardEvent } from "react";
import { SettingsIcons } from "./SettingsIcons";
import { hasOwnUpdater } from "../platform";

/** Sections in nav order. Groups are separated by space; Start over sits apart at the bottom. */
export const SECTIONS = [
  { id: "general", label: "General", group: 0 },
  { id: "recording", label: "Recording", group: 0 },
  { id: "transcription", label: "Transcription", group: 0 },
  { id: "summaries", label: "Summaries", group: 0 },
  { id: "voices", label: "Voices", group: 0 },
  { id: "permissions", label: "Permissions", group: 1 },
  { id: "integrations", label: "Integrations", group: 1 },
  { id: "updates", label: "Updates", group: 1 },
  { id: "about", label: "About", group: 1 },
  { id: "reset", label: "Start over", group: 2 },
] as const;

export type SectionId = (typeof SECTIONS)[number]["id"];

/** The sections this platform has. The Store updates the Windows app, so it has no Updates. */
const SHOWN = SECTIONS.filter((s) => hasOwnUpdater || s.id !== "updates");

interface Props {
  section: SectionId;
  onSection: (s: SectionId) => void;
  onBack: () => void;
}

/** Replaces the meeting list while Settings is open, the way T3 Code's settings route does. */
export function SettingsNav({ section, onSection, onBack }: Props) {
  const list = useRef<HTMLUListElement>(null);
  // Opening Settings puts focus in the nav, so the arrow keys work straight away.
  useEffect(() => list.current?.querySelector<HTMLButtonElement>("[aria-current]")?.focus({ preventScroll: true }), []);

  return (
    <aside className="sidebar set-nav">
      <div className="sidebar-top" data-tauri-drag-region />
      <button className="set-nav-back" onClick={onBack} title="Back (Esc)" aria-label="Back" aria-keyshortcuts="Escape">
        <span className="set-nav-icon">{SettingsIcons.back()}</span>
        <span className="set-nav-label">Back</span>
        <kbd className="row-kbd" aria-hidden>esc</kbd>
      </button>
      <nav aria-label="Settings sections" className="set-nav-list">
        <ul ref={list} onKeyDown={arrowKeys}>
          {SHOWN.map((s, i) => (
            <li key={s.id} className={i > 0 && SHOWN[i - 1].group !== s.group ? `set-nav-gap${s.group === 2 ? " set-nav-end" : ""}` : undefined}>
              <button
                className={`set-nav-item${s.id === "reset" ? " danger" : ""}`}
                aria-current={s.id === section ? "page" : undefined}
                tabIndex={s.id === section ? 0 : -1}
                // A narrow window hides the label and shows only the icon.
                aria-label={s.label}
                title={s.label}
                onClick={() => onSection(s.id)}
              >
                <span className="set-nav-icon">{SettingsIcons[s.id]()}</span>
                <span className="set-nav-label">{s.label}</span>
              </button>
            </li>
          ))}
        </ul>
      </nav>
    </aside>
  );
}

/** One Tab stop for the list; Up, Down, Home and End move focus without opening a section. */
function arrowKeys(e: KeyboardEvent<HTMLUListElement>) {
  const items = Array.from(e.currentTarget.querySelectorAll<HTMLButtonElement>("button"));
  const at = items.indexOf(e.target as HTMLButtonElement);
  if (at < 0) return;
  const moves: Record<string, number> = { ArrowDown: Math.min(at + 1, items.length - 1), ArrowUp: Math.max(at - 1, 0), Home: 0, End: items.length - 1 };
  const to = moves[e.key];
  if (to === undefined) return;
  e.preventDefault();
  items[to].focus();
}
