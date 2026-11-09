import { memo, useEffect, useMemo, useRef, useState } from "react";
import { api, type LiveLine, type RecorderView } from "../api";
import { clock, useDebounced } from "../util";
import { NotesEditor } from "./NotesEditor";
import { SideSwitch, useOneColumn, type Side } from "./Compact";

interface Props {
  recorder: RecorderView;
  lines: LiveLine[];
  level: number;
}

/** The latest notes and title typed, newer than the last debounced save. Stop sends them,
 * so the last few words before Stop are never lost. */
export const liveDraft: { session: string | null; notes: string | null; title: string | null } = { session: null, notes: null, title: null };

/** The notepad during a meeting: your notes on the left and the live transcript on the
 * right. The rule between them is the recording light and level meter. A narrow window
 * shows one of the two, picked with a switch. */
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

  const startedLabel = useMemo(
    () => (recorder.started_at ? new Date(recorder.started_at) : new Date()).toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" }),
    [recorder.started_at],
  );
  const kindLabel = recorder.kind === "call" ? "Call" : "In person";
  const oneColumn = useOneColumn();
  const [side, setSide] = useState<Side>("notes");
  const notesHidden = oneColumn && side !== "notes";

  return (
    <div className={`page page-live${oneColumn ? " page-compact" : ""}`}>
      {oneColumn && <SideSwitch side={side} onSide={setSide} transcriptLabel="Live transcript" />}
      {/* Hidden, not removed, so the caret and the scroll stay where you left them. */}
      <div className={`notes-col${notesHidden ? " is-hidden" : ""}`}>
        <input
          className="title-input"
          placeholder="Untitled meeting"
          value={title}
          onChange={(e) => setTitle(e.target.value)}
          aria-label="Meeting title"
        />
        <div className="meta-line">
          <span>{startedLabel}</span>
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
      {!oneColumn && <Rule level={level} live />}
      {(!oneColumn || side === "transcript") && <LiveTranscript lines={lines} call={recorder.kind === "call"} />}
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

/** Memoised: the level meter updates twice a second, the lines only when one arrives.
 * With `draft`, the same lines after Stop, until the final transcript replaces them. */
export const LiveTranscript = memo(function LiveTranscript({ lines, call, draft }: { lines: LiveLine[]; call: boolean; draft?: boolean }) {
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
    <aside className={`transcript-col ${draft ? "draft-transcript" : ""}`} ref={box} onScroll={onScroll} aria-label={draft ? "Draft transcript" : "Live transcript"} aria-live={draft ? undefined : "polite"}>
      <div className="col-label">
        {draft ? "Draft transcript" : "Live transcript"}
        {call && <span className="hint" title="Guessed from which side was louder. Real speaker names come after the meeting.">You / Them are approximate</span>}
      </div>
      {draft && <p className="draft-note">Draft from the live transcript. The final transcript replaces it.</p>}
      {lines.length === 0 ? (
        <p className="transcript-empty">Listening… Lines appear here a few seconds after they're spoken.</p>
      ) : (
        lines.map((l, i) => <LiveLineRow key={l.line} line={l} showSpeaker={call && !!l.speaker && l.speaker !== lines[i - 1]?.speaker} />)
      )}
      <div ref={end} />
    </aside>
  );
});

/** One live line. Memoised, so a new line doesn't re-render the hundreds before it. */
const LiveLineRow = memo(function LiveLineRow({ line, showSpeaker }: { line: LiveLine; showSpeaker: boolean }) {
  return (
    <div className={`live-line ${line.speaker ?? ""}`}>
      {showSpeaker && <div className="live-speaker">{line.speaker === "you" ? "You" : "Them"}</div>}
      <span className="ts mono">{clock(line.offset_ms / 1000)}</span>
      <span className="live-text">{line.text}</span>
    </div>
  );
});
