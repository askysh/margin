import { memo, useEffect, useMemo, useState } from "react";
import { api, type Job, type MeetingSummary } from "../api";
import type { Selection } from "../App";
import { clock, groupLabel, shortTime } from "../util";

interface Props {
  meetings: MeetingSummary[];
  jobs: Job[];
  selection: Selection;
  recording: boolean;
  recordingTitle: string | null;
  elapsed: number;
  onSelect: (s: Selection) => void;
  /** Show only meetings with this attendee. */
  person: string | null;
  onPerson: (p: string | null) => void;
  askOpen: boolean;
  onAsk: () => void;
}

export function Sidebar({ meetings, jobs, selection, recording, recordingTitle, elapsed, onSelect, person, onPerson, askOpen, onAsk }: Props) {
  const [query, setQuery] = useState("");
  const [hits, setHits] = useState<Map<string, string> | null>(null);

  useEffect(() => {
    const q = query.trim();
    if (!q) {
      setHits(null);
      return;
    }
    let cancelled = false;
    const t = setTimeout(() => {
      api
        .search(q)
        .then((r) => !cancelled && setHits(new Map(r.map((h) => [h.path, h.excerpt]))))
        .catch(() => !cancelled && setHits(new Map()));
    }, 180);
    return () => {
      cancelled = true;
      clearTimeout(t);
    };
  }, [query]);

  const groups = useMemo(() => {
    const who = person?.toLowerCase();
    const shown = meetings.filter((m) => (!hits || hits.has(m.path)) && (!who || m.people.some((p) => p.toLowerCase() === who)));
    const out: Group[] = [];
    for (const m of shown) {
      const label = groupLabel(m.date);
      const last = out[out.length - 1];
      if (last?.label === label) last.items.push(m);
      else out.push({ label, items: [m] });
    }
    return out;
  }, [meetings, hits, person]);

  const selectedPath = selection.type === "meeting" ? selection.path : null;

  return (
    <aside className="sidebar">
      <div className="sidebar-top" data-tauri-drag-region />
      <label className="search">
        <svg width="13" height="13" viewBox="0 0 16 16" aria-hidden><circle cx="7" cy="7" r="5" stroke="currentColor" strokeWidth="1.6" fill="none" /><path d="M11 11l3.5 3.5" stroke="currentColor" strokeWidth="1.6" /></svg>
        <input
          placeholder="Search meetings"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          onKeyDown={(e) => e.key === "Escape" && setQuery("")}
          aria-label="Search meetings"
        />
      </label>
      <button className={`row row-ask ${askOpen ? "selected" : ""}`} onClick={onAsk} title="Ask Claude about your meetings (⌘J)">
        <span className="row-title">
          <svg width="13" height="13" viewBox="0 0 16 16" aria-hidden><path d="M2.5 3.5h11v7h-6l-3 2.5v-2.5h-2z" stroke="currentColor" strokeWidth="1.5" strokeLinejoin="round" fill="none" /></svg>
          Ask about your meetings
        </span>
        <kbd className="row-kbd">⌘J</kbd>
      </button>
      {person && (
        <div className="filter-pill" role="status">
          <span>With <strong>{person}</strong></span>
          <button className="icon-btn small" aria-label="Show all meetings" onClick={() => onPerson(null)}>×</button>
        </div>
      )}

      <nav className="list" aria-label="Meetings">
        {recording && (
          <button className={`row row-live ${selection.type === "live" ? "selected" : ""}`} onClick={() => onSelect({ type: "live" })}>
            <span className="row-title"><span className="rec-dot small" aria-hidden />{recordingTitle || "Recording now"}</span>
            <span className="row-meta mono">{clock(elapsed)}</span>
          </button>
        )}
        {jobs.map((j) => (
          <button
            key={j.id}
            className={`row row-job ${selection.type === "processing" && selection.jobId === j.id ? "selected" : ""}`}
            onClick={() => onSelect({ type: "processing", jobId: j.id, notes: "", title: j.title })}
          >
            <span className="row-title">{j.title || "New meeting"}</span>
            <span className="row-meta"><span className="spinner" aria-hidden />{j.stage ?? "Processing"}</span>
          </button>
        ))}

        <MeetingRows groups={groups} hits={hits} selectedPath={selectedPath} onSelect={onSelect} />
        {hits && groups.length === 0 && <p className="list-empty">No meetings mention “{query.trim()}”.</p>}
        {!hits && person && groups.length === 0 && <p className="list-empty">No meetings with {person}.</p>}
      </nav>
    </aside>
  );
}

type Group = { label: string; items: MeetingSummary[] };

/** The library rows. Memoised: a recording's clock ticks twice a second, and the rows
 * don't need to re-render for it. */
const MeetingRows = memo(function MeetingRows({ groups, hits, selectedPath, onSelect }: {
  groups: Group[];
  hits: Map<string, string> | null;
  selectedPath: string | null;
  onSelect: (s: Selection) => void;
}) {
  return (
    <>
      {groups.map((g) => (
        <div key={g.label} className="group">
          <div className="group-label">{g.label}</div>
          {g.items.map((m) => (
            <button
              key={m.path}
              className={`row ${selectedPath === m.path ? "selected" : ""}`}
              onClick={() => onSelect({ type: "meeting", path: m.path })}
            >
              <span className="row-title">{m.title}</span>
              <span className="row-meta">
                <span className="mono">{shortTime(m.date)}</span>
                {m.duration && <span className="mono"> · {m.duration}</span>}
                {m.kind === "memo" && <span> · memo</span>}
              </span>
              {hits?.get(m.path) && <span className="row-hit">{hits.get(m.path)}</span>}
            </button>
          ))}
        </div>
      ))}
    </>
  );
});
