import { createContext, useContext, useEffect, useState } from "react";

/** The window width under which the sidebar becomes a drawer: the notepad view (D51).
 * `NARROW_BELOW` in src-tauri/src/notepad.rs must match it (D57). */
export const COMPACT_QUERY = "(max-width: 759px)";

/** The page width a page needs for notes and transcript side by side. Under it, a page
 * shows one column at a time. With the sidebar docked, the page is 252 px narrower than the
 * window. */
export const oneColumnQuery = (sidebarDocked: boolean) => `(max-width: ${(sidebarDocked ? 252 : 0) + 819}px)`;

/** True when a page shows one column at a time. App provides it, so every view agrees. */
export const OneColumnContext = createContext(false);
export const useOneColumn = () => useContext(OneColumnContext);

export function useMediaQuery(query: string) {
  const [match, setMatch] = useState(() => window.matchMedia(query).matches);
  useEffect(() => {
    const mq = window.matchMedia(query);
    const on = () => setMatch(mq.matches);
    on();
    mq.addEventListener("change", on);
    return () => mq.removeEventListener("change", on);
  }, [query]);
  return match;
}

export type Side = "notes" | "transcript";

/** In a narrow page, picks the column a page shows: your notes or the transcript. */
export function SideSwitch({ side, onSide, transcriptLabel = "Transcript" }: { side: Side; onSide: (s: Side) => void; transcriptLabel?: string }) {
  return (
    <div className="pill-switch side-switch" role="radiogroup" aria-label="Show">
      <button role="radio" aria-checked={side === "notes"} onClick={() => onSide("notes")}>Notes</button>
      <button role="radio" aria-checked={side === "transcript"} onClick={() => onSide("transcript")}>{transcriptLabel}</button>
    </div>
  );
}
