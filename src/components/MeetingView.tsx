import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, errorText, on, type Job, type MeetingDetail, type Turn } from "../api";
import { isRawSpeaker, speakerNames, useDebounced } from "../util";
import { readyText, whenParts } from "../times";
import { Markdown } from "./Markdown";
import { NotesEditor } from "./NotesEditor";
import { Rule } from "./LiveView";
import { ActionItems } from "./ActionItems";
import { AudioPlayer, type AudioHandle } from "./AudioPlayer";
import { MoreMenu } from "./MoreMenu";
import { CopyMenu } from "./CopyMenu";
import { SideSwitch, useOneColumn, type Side } from "./Compact";
import { sameNotes } from "../notes";
import { ACTION_HEADING, meetingMarkdown, summaryMarkdown, transcriptMarkdown, tsSeconds, withoutCoverage } from "../meetingMarkdown";

interface Props {
  path: string;
  notify: (t: string, tone?: "info" | "error") => void;
  onChanged: () => void;
  onDeleted: () => void;
  /** The job that is still finishing this file (separating speakers, saving), or null.
   * Notes typed meanwhile go into the job's draft; edits that rewrite the file wait. */
  job: Job | null;
  /** Show the meetings this person was in. */
  onPerson?: (name: string) => void;
}

type SaveState = "idle" | "saving" | "saved" | "error";

// Memoised so a recording's clock, ticking in App, doesn't re-render a long transcript.
export const MeetingView = memo(function MeetingView({ path, notify, onChanged, onDeleted, job, onPerson }: Props) {
  const processing = !!job;
  const [m, setM] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notes, setNotes] = useState("");
  const [save, setSave] = useState<SaveState>("idle");
  const [showTranscript, setShowTranscript] = useState(true);
  // A narrow window shows one column. The audio player lives in the notes column, so that
  // column is hidden, not removed, while the transcript shows: a timestamp still plays.
  const oneColumn = useOneColumn();
  const [side, setSide] = useState<Side>("notes");
  const [summarizing, setSummarizing] = useState(false);
  // Margin writes the summary after processing (D40); this follows that run.
  const [writing, setWriting] = useState(false);
  const player = useRef<AudioHandle>(null);
  const [activeTurn, setActiveTurn] = useState(-1);
  // Every edit bumps `edits`; a save records the count it wrote. Unsaved = they differ.
  // Saves run one at a time, so an older save can never land after a newer one.
  const edits = useRef(0);
  const saved = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const unsaved = () => edits.current !== saved.current;

  // Focus and save refreshes usually read back the same file: keep the rendered one then.
  const lastRead = useRef("");
  const notesNow = useRef(notes);
  notesNow.current = notes;
  const load = useCallback(() => {
    api
      .readMeeting(path)
      .then((d) => {
        const json = JSON.stringify(d);
        if (json === lastRead.current) return;
        lastRead.current = json;
        setM(d);
        // The file has the trimmed copy of a save. Taking it would move the caret.
        if (edits.current === saved.current && !sameNotes(notesNow.current, d.notes)) setNotes(d.notes);
      })
      .catch((e) => setError(errorText(e)));
  }, [path]);

  // Each job stage can rewrite the file, and the last one fills in the speakers: read it
  // again in place. Unsaved notes and the scroll position stay as they are.
  const jobState = job?.state;
  useEffect(load, [load, jobState]);
  useEffect(() => { if (m) setWriting(m.summarizing); }, [m]);
  useEffect(() => {
    const sub = on<{ path: string; running: boolean; error?: string | null }>("summary-status", (e) => {
      if (e.path !== path) return;
      setWriting(e.running);
      if (!e.running) {
        load();
        onChanged();
        if (e.error) notify(`No summary: ${e.error}`, "error");
      }
    });
    return () => { sub.then((u) => u()); };
  }, [path, load, onChanged, notify]);
  useEffect(() => {
    // Pick up edits made in Obsidian or another editor, unless there are unsaved changes here.
    const f = () => !unsaved() && load();
    window.addEventListener("focus", f);
    return () => window.removeEventListener("focus", f);
  }, [load]);
  useDebounced(notes, 600, (v) => {
    if (!unsaved()) return;
    const upTo = edits.current;
    setSave("saving");
    queue.current = queue.current.then(() =>
      api
        .saveNotes(path, v)
        .then(() => {
          saved.current = Math.max(saved.current, upTo);
          if (!unsaved()) setSave("saved");
          onChanged();
        })
        .catch((e) => {
          setSave("error");
          notify(`Notes not saved: ${errorText(e)}`, "error");
        }),
    );
  });

  // One display name per label, used by chips and transcript rows alike: a name from
  // speaker_map wins, then "You" / "Them" / "Speaker n".
  const names = useMemo(() => {
    const base = speakerNames((m?.transcript ?? []).map((t) => t.speaker ?? "").filter(Boolean));
    for (const s of m?.speakers ?? []) if (s.name) base.set(s.label, s.name);
    return base;
  }, [m]);

  // Transcript start times in seconds, for following playback.
  const starts = useMemo(() => (m?.transcript ?? []).map((t) => tsSeconds(t.ts)), [m]);
  const onTime = useCallback(
    (secs: number) => {
      let i = -1;
      while (i + 1 < starts.length && starts[i + 1] <= secs) i++;
      setActiveTurn(i);
    },
    [starts],
  );

  if (error) return <div className="page page-empty"><p className="empty-title">Couldn't open this meeting</p><p className="empty-body">{error}</p></div>;
  if (!m) return <div className="page" />;

  const hasActions = m.action_items.length > 0;
  // The action items block replaces core's plain checklist section.
  const sections = m.sections.map((s) => ({ ...s, ...withoutCoverage(s.body) }));
  const summaryIncomplete = sections.some((s) => s.incomplete);
  const summary = sections.filter((s) => s.body.trim() && !(hasActions && s.heading === ACTION_HEADING));
  const hasSummary = summary.some((s) => s.heading === "Summary");
  const copyChoices = [
    hasSummary && { label: "Summary", hint: "The summary sections, as Markdown", done: "Summary copied.", text: () => summaryMarkdown(m) },
    notes.trim() && { label: "Your notes", hint: "What you typed", done: "Notes copied.", text: () => notes.trim() + "\n" },
    m.transcript.length > 0 && { label: "Transcript", hint: "Every line, with speakers and times", done: "Transcript copied.", text: () => transcriptMarkdown(m, names) + "\n" },
    { label: "Whole meeting", hint: "Title, summary, notes and transcript", done: "Meeting copied as Markdown.", text: () => meetingMarkdown(m, names, notes) },
  ].filter((c) => !!c) as Parameters<typeof CopyMenu>[0]["choices"];

  const summarize = async () => {
    if (hasSummary) {
      const sure = await ask("The current summary and action items are replaced. Your notes and the transcript stay as they are.", {
        title: "Summarize this meeting again?",
        kind: "info",
        okLabel: "Summarize again",
        cancelLabel: "Cancel",
      });
      if (!sure) return;
    }
    setSummarizing(true);
    try {
      await api.summarize(path);
      load();
      onChanged();
      notify(hasSummary ? "Summary updated." : "Summary added.");
    } catch (e) {
      notify(errorText(e), "error");
    } finally {
      // This run is over, so no summary is being written. A failed run leaves the file as
      // it was, so `load` reads nothing new and can't clear it.
      setSummarizing(false);
      setWriting(false);
    }
  };

  const trash = async () => {
    const sure = await ask(`“${m.title}” and its audio will be moved to the Trash. You can restore them from there.`, {
      title: "Move this meeting to the Trash?",
      kind: "warning",
      okLabel: "Move to Trash",
      cancelLabel: "Cancel",
    });
    if (!sure) return;
    try {
      await api.trashMeeting(path);
      notify("Moved to the Trash.");
      onDeleted();
    } catch (e) {
      notify(errorText(e), "error");
    }
  };

  const hasTranscript = m.transcript.length > 0;
  // Shown once the job is done. Until then the "Finishing" line says what is happening.
  const ready = job ? null : readyText(m.times);
  const when = [...whenParts(m.date, m.times), m.duration ?? ""].filter(Boolean);
  const split = oneColumn && hasTranscript;
  const transcriptShown = hasTranscript && (oneColumn ? side === "transcript" : showTranscript);

  return (
    <div className={`page page-meeting ${split || transcriptShown ? "" : "no-transcript"}${split ? " page-compact" : ""}`}>
      {split && <SideSwitch side={side} onSide={setSide} />}
      <div className={`notes-col${split && side !== "notes" ? " is-hidden" : ""}`}>
        <TitleField
          value={m.title}
          disabled={processing}
          onSave={async (t) => {
            try {
              await api.setMeetingTitle(path, t);
              load();
              onChanged();
            } catch (e) {
              notify(errorText(e), "error");
            }
          }}
        />
        <div className="meta">
          <div className="meta-line meta-parts">
            {/* Day · start – Stop · duration · people. Each part brings its own dot, and a
                dot that starts a line is clipped, so a wrap leaves no dot at a line end (D58). */}
            {when.map((part, i) => <span key={i}>{part}</span>)}
            {m.attendees.length > 0 && (
              <span className="people">
                {m.attendees.map((a, i) => (
                  <span key={a}>
                    {i > 0 && ", "}
                    {onPerson ? <button className="person-btn" title={`Show meetings with ${a}`} onClick={() => onPerson(a)}>{a}</button> : a}
                  </span>
                ))}
              </span>
            )}
          </div>
          {ready && (
            <p className="meta-line meta-parts meta-ready">
              {ready.split(" · ").map((part) => <span key={part}>{part}</span>)}
            </p>
          )}
        </div>
        <div className="actions">
          <CopyMenu choices={copyChoices} notify={notify} />
          {m.transcript.length > 0 && (
            <button className="link-btn" onClick={summarize} disabled={summarizing || writing || processing}>
              {summarizing || writing ? "Summarizing…" : hasSummary ? "Summarize again" : "Summarize"}
            </button>
          )}
          {hasTranscript && !oneColumn && (
            <button className="link-btn" onClick={() => setShowTranscript((s) => !s)}>{showTranscript ? "Hide transcript" : "Show transcript"}</button>
          )}
          <MoreMenu
            choices={[
              { label: "Show in Finder", onSelect: () => api.reveal(path) },
              ...(m.in_obsidian_vault ? [{ label: "Open in Obsidian", onSelect: () => api.openInObsidian(path).catch((e) => notify(errorText(e), "error")) }] : []),
              { label: "Move to Trash…", onSelect: trash, danger: true },
            ]}
          />
        </div>

        {job && (
          <p className="quiet-note finishing" role="status">
            <span className="spinner" aria-hidden />
            <span><strong>{finishingLabel(job.state)}</strong> You can read and add notes now. Renaming unlocks when it's done.</span>
          </p>
        )}

        {m.status === "no-speech" && (
          <div className="callout">
            <p><strong>No speech was detected in this recording.</strong> Check that the right microphone is selected in
              System Settings → Sound, and for calls, that Margin is allowed to record system audio.</p>
          </div>
        )}

        {m.has_audio && <AudioPlayer ref={player} path={path} onTime={onTime} />}

        {writing && !summarizing && (
          <p className="quiet-note writing-summary"><span className="spinner" aria-hidden /> Writing the summary. You can read and add notes meanwhile.</p>
        )}

        {summary.map((s, i) => (
          <div key={s.heading}>
            <section className="summary-block">
              <h2 className="eyebrow">{s.heading}</h2>
              <Markdown source={s.body} />
            </section>
            {/* Action items follow the summary, where core puts its checklist. */}
            {hasActions && i === 0 && (
              <ActionItems path={path} items={m.action_items} names={names} locked={processing || writing || summarizing} notify={notify} onChanged={() => { load(); onChanged(); }} />
            )}
          </div>
        ))}
        {summaryIncomplete && (
          <p className="quiet-note">Part of the meeting did not reach the summarizer, so this summary can miss things. Summarize again to try once more.</p>
        )}
        {hasActions && summary.length === 0 && (
          <ActionItems path={path} items={m.action_items} names={names} locked={processing || writing || summarizing} notify={notify} onChanged={() => { load(); onChanged(); }} />
        )}

        <section className="your-notes">
          <h2 className="eyebrow">
            Your notes
            <span className={`save-state ${save}`}>{save === "saving" ? "Saving…" : save === "saved" ? "Saved" : save === "error" ? "Not saved" : ""}</span>
          </h2>
          <NotesEditor
            value={notes}
            onChange={(v) => { edits.current++; setSave("idle"); setNotes(v); }}
            placeholder="Add notes. They're saved into the meeting file."
          />
        </section>
      </div>
      {transcriptShown && (
        <>
          {!oneColumn && <Rule level={0} live={false} />}
          <Transcript
            meeting={m}
            names={names}
            locked={processing}
            onRenamed={() => { load(); onChanged(); }}
            notify={notify}
            active={activeTurn}
            onSeek={m.has_audio ? (secs) => player.current?.playFrom(secs) : undefined}
          />
        </>
      )}
    </div>
  );
});

function finishingLabel(state: Job["state"]) {
  if (state === "summarizing") return "Finishing: summarizing…";
  if (state === "saving") return "Finishing: saving…";
  return "Finishing: separating speakers…";
}

/** The meeting title, editable in place. Enter or leaving the field saves; Escape undoes. */
function TitleField({ value, onSave, disabled }: { value: string; onSave: (t: string) => void; disabled?: boolean }) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  const commit = () => {
    const t = draft.trim();
    if (!t) setDraft(value);
    else if (t !== value) onSave(t);
  };
  return (
    <input
      className="title-input"
      value={draft}
      onChange={(e) => setDraft(e.target.value)}
      onBlur={commit}
      onKeyDown={(e) => {
        if (e.key === "Enter") e.currentTarget.blur();
        if (e.key === "Escape") {
          const el = e.currentTarget;
          setDraft(value);
          requestAnimationFrame(() => el.blur());
        }
      }}
      aria-label="Meeting title"
      title={disabled ? "You can rename it when Margin is done with it" : "Click to rename this meeting"}
      disabled={disabled}
    />
  );
}

function Transcript({ meeting, names, locked, onRenamed, notify, active, onSeek }: {
  meeting: MeetingDetail;
  names: Map<string, string>;
  locked: boolean;
  onRenamed: () => void;
  notify: Props["notify"];
  /** The turn the recording is playing, or -1. */
  active: number;
  onSeek?: (secs: number) => void;
}) {
  const [renaming, setRenaming] = useState<string | null>(null);
  const turns = meeting.transcript;
  const hasSpeakers = meeting.speakers.length > 0;

  return (
    <aside className="transcript-col" aria-label="Transcript">
      <div className="col-label">Transcript</div>
      {hasSpeakers && !locked && (
        <div className="speakers">
          {meeting.speakers.map((s) => (
            <SpeakerChip
              key={s.label}
              label={s.label}
              display={s.name ?? names.get(s.label) ?? s.label}
              unnamed={isRawSpeaker(s.label) && !s.name}
              open={renaming === s.label}
              onOpen={() => setRenaming(renaming === s.label ? null : s.label)}
              onClose={() => setRenaming(null)}
              path={meeting.path}
              onRenamed={(msg) => { setRenaming(null); onRenamed(); notify(msg); }}
              notify={notify}
            />
          ))}
        </div>
      )}
      {turns.map((t, i) => (
        <TurnRow
          key={i}
          turn={t}
          showSpeaker={!!t.speaker && t.speaker !== turns[i - 1]?.speaker}
          display={t.speaker ? names.get(t.speaker) ?? t.speaker : ""}
          onSpeaker={() => !locked && t.speaker && setRenaming(t.speaker)}
          active={i === active}
          onSeek={onSeek}
        />
      ))}
    </aside>
  );
}

function TurnRow({ turn, showSpeaker, display, onSpeaker, active, onSeek }: {
  turn: Turn;
  showSpeaker: boolean;
  display: string;
  onSpeaker: () => void;
  active: boolean;
  onSeek?: (secs: number) => void;
}) {
  const row = useRef<HTMLDivElement>(null);
  useEffect(() => {
    if (active) row.current?.scrollIntoView({ block: "nearest", behavior: "smooth" });
  }, [active]);
  return (
    <div className={`turn ${active ? "playing" : ""}`} ref={row}>
      {showSpeaker && (
        <button className={`turn-speaker ${isRawSpeaker(turn.speaker) ? "unnamed" : ""}`} onClick={onSpeaker} title="Rename this speaker">
          {display}
        </button>
      )}
      {onSeek ? (
        <button className="ts ts-btn mono" onClick={() => onSeek(tsSeconds(turn.ts))} title="Play from here">{turn.ts}</button>
      ) : (
        <span className="ts mono">{turn.ts}</span>
      )}
      <span className="turn-text">{turn.text}</span>
    </div>
  );
}

function SpeakerChip(props: {
  label: string;
  display: string;
  unnamed: boolean;
  open: boolean;
  onOpen: () => void;
  onClose: () => void;
  path: string;
  onRenamed: (msg: string) => void;
  notify: Props["notify"];
}) {
  const [name, setName] = useState(props.unnamed ? "" : props.display);
  const [remember, setRemember] = useState(true);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    if (props.open) setName(props.unnamed ? "" : props.display);
  }, [props.open, props.unnamed, props.display]);

  const submit = async (e: React.FormEvent) => {
    e.preventDefault();
    if (!name.trim()) return;
    setBusy(true);
    try {
      const r = await api.renameSpeaker(props.path, props.label, name.trim(), remember);
      props.onRenamed(r.note ?? (r.voice_saved ? `Renamed to ${name.trim()}. Margin will recognise this voice next time.` : `Renamed to ${name.trim()}.`));
    } catch (err) {
      props.notify(errorText(err), "error");
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="chip-wrap">
      <button className={`chip ${props.unnamed ? "unnamed" : ""}`} onClick={props.onOpen} aria-expanded={props.open}>
        {props.display}
      </button>
      {props.open && (
        <form className="popover" onSubmit={submit} onKeyDown={(e) => e.key === "Escape" && props.onClose()}>
          <label className="field-label" htmlFor={`rn-${props.label}`}>
            {props.label === "SYSTEM" ? "Who was on the other end of the call?" : props.label === "MICROPHONE" ? "Who was speaking on your mic?" : `Who is ${props.display}?`}
          </label>
          <input id={`rn-${props.label}`} autoFocus value={name} onChange={(e) => setName(e.target.value)} placeholder="Name" />
          <label className="check">
            <input type="checkbox" checked={remember} onChange={(e) => setRemember(e.target.checked)} />
            Recognise this voice in future meetings
          </label>
          <div className="popover-actions">
            <button type="button" className="btn btn-quiet" onClick={props.onClose}>Cancel</button>
            <button type="submit" className="btn btn-primary" disabled={busy || !name.trim()}>Rename everywhere</button>
          </div>
        </form>
      )}
    </div>
  );
}
