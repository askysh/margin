import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { ask } from "@tauri-apps/plugin-dialog";
import { api, errorText, type MeetingDetail, type Turn } from "../api";
import { isRawSpeaker, meetingDate, speakerNames, useDebounced } from "../util";
import { Markdown } from "./Markdown";
import { NotesEditor } from "./NotesEditor";
import { Rule } from "./LiveView";
import { ActionItems } from "./ActionItems";
import { AudioPlayer, type AudioHandle } from "./AudioPlayer";
import { CopyMenu } from "./CopyMenu";
import { ACTION_HEADING, meetingMarkdown, summaryMarkdown, transcriptMarkdown, tsSeconds } from "../meetingMarkdown";

interface Props {
  path: string;
  notify: (t: string, tone?: "info" | "error") => void;
  onChanged: () => void;
  onDeleted: () => void;
  /** Core is still writing this file (diarizing or summarizing); edits would make it fail. */
  processing: boolean;
  /** Show the meetings this person was in. */
  onPerson?: (name: string) => void;
  /** Open Ask about this meeting. */
  onAsk?: () => void;
}

type SaveState = "idle" | "saving" | "saved" | "error";

export function MeetingView({ path, notify, onChanged, onDeleted, processing, onPerson, onAsk }: Props) {
  const [m, setM] = useState<MeetingDetail | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [notes, setNotes] = useState("");
  const [save, setSave] = useState<SaveState>("idle");
  const [showTranscript, setShowTranscript] = useState(true);
  const [summarizing, setSummarizing] = useState(false);
  const player = useRef<AudioHandle>(null);
  const [activeTurn, setActiveTurn] = useState(-1);
  // Every edit bumps `edits`; a save records the count it wrote. Unsaved = they differ.
  // Saves run one at a time, so an older save can never land after a newer one.
  const edits = useRef(0);
  const saved = useRef(0);
  const queue = useRef<Promise<void>>(Promise.resolve());
  const unsaved = () => edits.current !== saved.current;

  const load = useCallback(() => {
    api
      .readMeeting(path)
      .then((d) => {
        setM(d);
        if (edits.current === saved.current) setNotes(d.notes);
      })
      .catch((e) => setError(errorText(e)));
  }, [path]);

  useEffect(load, [load]);
  useEffect(() => {
    // Pick up edits made in Obsidian or another editor, unless there are unsaved changes here.
    const f = () => !unsaved() && load();
    window.addEventListener("focus", f);
    return () => window.removeEventListener("focus", f);
  }, [load]);
  useEffect(() => {
    // Processing just finished: show the enriched file.
    if (!processing) load();
  }, [processing, load]);

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
  const summary = m.sections.filter((s) => s.body.trim() && !(hasActions && s.heading === ACTION_HEADING));
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
      setSummarizing(false);
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

  return (
    <div className={`page page-meeting ${showTranscript && m.transcript.length ? "" : "no-transcript"}`}>
      <div className="notes-col">
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
        <div className="meta-line">
          <span>{meetingDate(m.date)}</span>
          {m.duration && (<><span className="dot-sep" /><span className="mono">{m.duration}</span></>)}
          {m.attendees.length > 0 && (
            <>
              <span className="dot-sep" />
              <span className="people">
                {m.attendees.map((a, i) => (
                  <span key={a}>
                    {i > 0 && ", "}
                    {onPerson ? <button className="person-btn" title={`Show meetings with ${a}`} onClick={() => onPerson(a)}>{a}</button> : a}
                  </span>
                ))}
              </span>
            </>
          )}
        </div>
        <div className="actions">
          {onAsk && <button className="link-btn" onClick={onAsk} title="Ask Claude about this meeting (⌘J)">Ask about this meeting</button>}
          <button className="link-btn" onClick={() => api.reveal(path)}>Show in Finder</button>
          {m.in_obsidian_vault && <button className="link-btn" onClick={() => api.openInObsidian(path).catch((e) => notify(errorText(e), "error"))}>Open in Obsidian</button>}
          <CopyMenu choices={copyChoices} notify={notify} />
          {m.transcript.length > 0 && (
            <button className="link-btn" onClick={summarize} disabled={summarizing || processing}>
              {summarizing ? "Summarizing…" : hasSummary ? "Summarize again" : "Summarize"}
            </button>
          )}
          {m.transcript.length > 0 && (
            <button className="link-btn" onClick={() => setShowTranscript((s) => !s)}>{showTranscript ? "Hide transcript" : "Show transcript"}</button>
          )}
          <button className="link-btn danger" onClick={trash}>Move to Trash</button>
        </div>

        {processing && (
          <div className="callout">
            <p><strong>Still processing.</strong> Margin is separating speakers and finishing this meeting. You can
              read it now; editing unlocks when it's done.</p>
          </div>
        )}

        {m.status === "no-speech" && (
          <div className="callout">
            <p><strong>No speech was detected in this recording.</strong> Check that the right microphone is selected in
              System Settings → Sound, and for calls, that Margin is allowed to record system audio.</p>
          </div>
        )}

        {m.has_audio && <AudioPlayer ref={player} path={path} onTime={onTime} />}

        {summary.map((s, i) => (
          <div key={s.heading}>
            <section className="summary-block">
              <h2 className="eyebrow">{s.heading}</h2>
              <Markdown source={s.body} />
            </section>
            {/* Action items follow the summary, where core puts its checklist. */}
            {hasActions && i === 0 && (
              <ActionItems path={path} items={m.action_items} names={names} locked={processing} notify={notify} onChanged={() => { load(); onChanged(); }} />
            )}
          </div>
        ))}
        {hasActions && summary.length === 0 && (
          <ActionItems path={path} items={m.action_items} names={names} locked={processing} notify={notify} onChanged={() => { load(); onChanged(); }} />
        )}

        <section className="your-notes">
          <h2 className="eyebrow">
            Your notes
            <span className={`save-state ${save}`}>{save === "saving" ? "Saving…" : save === "saved" ? "Saved" : save === "error" ? "Not saved" : ""}</span>
          </h2>
          <NotesEditor
            value={notes}
            onChange={(v) => { edits.current++; setSave("idle"); setNotes(v); }}
            readOnly={processing}
            placeholder="Add notes. They're saved into the meeting file."
          />
        </section>
      </div>
      {showTranscript && m.transcript.length > 0 && (
        <>
          <Rule level={0} live={false} />
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
      title="Click to rename this meeting"
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
