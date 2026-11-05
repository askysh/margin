import { useEffect, useRef, useState } from "react";
import { api, type LiveLine, type RecorderView } from "../api";
import { clock, useDebounced } from "../util";
import { NotesEditor } from "./NotesEditor";

interface Props {
  recorder: RecorderView;
  lines: LiveLine[];
  level: number;
}

/** The latest notes and title typed, newer than the last debounced save. Stop sends them,
 * so the last few words before Stop are never lost. */
export const liveDraft: { session: string | null; notes: string | null; title: string | null } = { session: null, notes: null, title: null };

/** The notepad during a meeting: your notes on the left and the live transcript on the
 * right. The rule between them is the recording light and level meter. */
export function LiveView({ recorder, lines, level }: Props) {
  // Coming back to the live view mid-recording: what you typed lives in liveDraft, which is
  // newer than the recorder snapshot App holds.
  const same = liveDraft.session === recorder.started_at;
  const [notes, setNotes] = useState(same && liveDraft.notes !== null ? liveDraft.notes : recorder.notes);
  const [title, setTitle] = useState(same && liveDraft.title !== null ? liveDraft.title : recorder.title ?? "");
  liveDraft.session = recorder.started_at;
  useDebounced(notes, 350, (v) => api.setDraftNotes(v));
  useDebounced(title, 350, (v) => api.setRecordingTitle(v));
  liveDraft.notes = notes;
  liveDraft.title = title;

  const started = recorder.started_at ? new Date(recorder.started_at) : new Date();
  const kindLabel = recorder.kind === "call" ? "Call" : "In person";

  return (
    <div className="page page-live">
      <div className="notes-col">
        <input
          className="title-input"
          placeholder="Untitled meeting"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          aria-label="Meeting title"
        />
        <div className="meta-line">
          <span>{started.toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" })}</span>
          <span className="dot-sep" />
          <span>{kindLabel}</span>
        </div>
        <NotesEditor
          value={notes}
          onChange={setNotes}
          autoFocus
          placeholder={"Type notes as you go.\nThe transcript is taken for you, so write only what matters."}
        />
      </div>
      <Rule level={level} live />
      <LiveTranscript lines={lines} call={recorder.kind === "call"} />
    </div>
  );
}

export function Rule({ level, live }: { level: number; live: boolean }) {
  // audio_level is a 0–100 RMS figure; a square root makes quiet speech visible.
  const v = live ? Math.min(1, Math.sqrt(Math.max(0, level) / 60)) : 0;
  return (
    <div className={`rule ${live ? "rule-live" : ""}`} aria-hidden style={{ ["--level" as string]: v.toFixed(3) }}>
      <div className="rule-meter" />
    </div>
  );
}

function LiveTranscript({ lines, call }: { lines: LiveLine[]; call: boolean }) {
  const end = useRef<HTMLDivElement>(null);
  const box = useRef<HTMLDivElement>(null);
  const pinned = useRef(true);

  useEffect(() => {
    if (pinned.current) end.current?.scrollIntoView({ block: "end" });
  }, [lines.length]);

  const onScroll = () => {
    const el = box.current;
    if (el) pinned.current = el.scrollHeight - el.scrollTop - el.clientHeight < 40;
  };

  return (
    <aside className="transcript-col" ref={box} onScroll={onScroll} aria-label="Live transcript" aria-live="polite">
      <div className="col-label">
        Live transcript
        {call && <span className="hint" title="Guessed from which side was louder. Real speaker names come after the meeting.">You / Them are approximate</span>}
      </div>
      {lines.length === 0 ? (
        <p className="transcript-empty">Listening… Lines appear here a few seconds after they're spoken.</p>
      ) : (
        lines.map((l, i) => {
          const prev = lines[i - 1];
          const showSpeaker = call && l.speaker && l.speaker !== prev?.speaker;
          return (
            <div key={l.line} className={`live-line ${l.speaker ?? ""}`}>
              {showSpeaker && <div className="live-speaker">{l.speaker === "you" ? "You" : "Them"}</div>}
              <span className="ts mono">{clock(l.offset_ms / 1000)}</span>
              <span className="live-text">{l.text}</span>
            </div>
          );
        })
      )}
      <div ref={end} />
    </aside>
  );
}
