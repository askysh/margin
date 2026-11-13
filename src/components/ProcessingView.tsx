import { useEffect, useState } from "react";
import { api, type Job, type LiveLine } from "../api";
import { LiveTranscript, Rule } from "./LiveView";
import { SideSwitch, useOneColumn, type Side } from "./Compact";
import { readyText, stoppedText } from "../times";
import { DEVICE, TRAY } from "../platform";

const STEPS: { state: Job["state"][]; label: string }[] = [
  { state: ["queued"], label: "Queued" },
  { state: ["transcribing", "transcript-only"], label: "Transcribing" },
  { state: ["diarizing"], label: "Telling speakers apart" },
  { state: ["summarizing"], label: "Summarizing" },
  { state: ["saving"], label: "Saving" },
];

interface Props {
  jobId: string;
  job: Job | null;
  notes: string;
  title: string | null;
  onOpen: (path: string) => void;
  onRetry: (id: string) => void;
}

export function ProcessingView({ jobId, job, notes, title, onOpen, onRetry }: Props) {
  const current = job ? STEPS.findIndex((s) => s.state.includes(job.state)) : 0;
  const done = job && ["complete", "needs-review"].includes(job.state);
  const failed = job?.state === "failed";
  const lines = useLiveDraft(jobId);
  const oneColumn = useOneColumn();
  const [side, setSide] = useState<Side>("notes");
  const split = oneColumn && lines.length > 0;

  return (
    <div className={`page page-processing ${lines.length ? "has-draft" : ""}${split ? " page-compact" : ""}`}>
      {split && <SideSwitch side={side} onSide={setSide} transcriptLabel="Draft transcript" />}
      <div className={`notes-col${split && side !== "notes" ? " is-hidden" : ""}`}>
        <h1 className="title">{title || job?.title || "New meeting"}</h1>
        {job && <SinceStop job={job} />}
        {failed ? (
          <div className="callout callout-error">
            <p><strong>Processing failed.</strong> {job?.error ?? "The recording is kept, so nothing is lost."}</p>
            <button className="btn btn-primary" onClick={() => job && onRetry(job.id)}>Try again</button>
          </div>
        ) : done && job?.output_path ? (
          <div className="callout">
            <p>{job.state === "needs-review" ? "Saved, but something needs a look. Open the meeting to see what happened." : "Your meeting is ready."}</p>
            <button className="btn btn-primary" onClick={() => onOpen(job.output_path!)}>Open meeting</button>
          </div>
        ) : (
          <ol className="steps" aria-label="Processing progress">
            {STEPS.map((s, i) => (
              <li key={s.label} className={i < current ? "done" : i === current ? "now" : ""}>
                {s.label}
                {i === current && <span className="spinner" aria-hidden />}
              </li>
            ))}
          </ol>
        )}
        <p className="quiet-note">
          Everything runs on this {DEVICE}. The meeting opens here when its transcript is ready. You can close the window.
          Margin keeps working from the {TRAY}.
        </p>
        {notes.trim() && (
          <section className="your-notes">
            <h2 className="eyebrow">Your notes</h2>
            <pre className="notes notes-static">{notes}</pre>
            <p className="quiet-note">These go into the meeting file as soon as it's saved.</p>
          </section>
        )}
      </div>
      {lines.length > 0 && !oneColumn && <Rule level={0} live={false} />}
      {lines.length > 0 && (!oneColumn || side === "transcript") && (
        <LiveTranscript lines={lines} call={lines.some((l) => !!l.speaker)} draft />
      )}
    </div>
  );
}

/** When Stop happened and how long processing has run since, to the second (D58). When
 * the job is done: when it was ready. A failed job shows its error instead. Its own component, so only this line ticks. */
function SinceStop({ job }: { job: Job }) {
  const running = !["complete", "needs-review", "failed"].includes(job.state);
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (!running) return;
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, [running]);
  const text = running ? stoppedText(job.recording_finished_at, now) : readyText(job);
  if (!text) return null;
  return (
    <div className="meta since-stop">
      <p className="meta-line meta-parts" role="timer" aria-live="off">
        {text.split(" · ").map((part, i) => <span key={i}>{part}</span>)}
      </p>
    </div>
  );
}

/** The live lines kept with the job. They are on disk, so they survive a restart. */
function useLiveDraft(jobId: string) {
  const [lines, setLines] = useState<LiveLine[]>([]);
  useEffect(() => {
    let live = true;
    api.liveDraft(jobId).then((l) => live && setLines(l)).catch(() => {});
    return () => { live = false; };
  }, [jobId]);
  return lines;
}
