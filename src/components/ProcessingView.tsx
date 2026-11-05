import type { Job } from "../api";

const STEPS: { state: Job["state"][]; label: string }[] = [
  { state: ["queued"], label: "Queued" },
  { state: ["transcribing", "transcript-only"], label: "Transcribing" },
  { state: ["diarizing"], label: "Telling speakers apart" },
  { state: ["summarizing"], label: "Summarizing" },
  { state: ["saving"], label: "Saving" },
];

interface Props {
  job: Job | null;
  notes: string;
  title: string | null;
  onOpen: (path: string) => void;
  onRetry: (id: string) => void;
}

export function ProcessingView({ job, notes, title, onOpen, onRetry }: Props) {
  const current = job ? STEPS.findIndex((s) => s.state.includes(job.state)) : 0;
  const done = job && ["complete", "needs-review"].includes(job.state);
  const failed = job?.state === "failed";

  return (
    <div className="page page-processing">
      <div className="notes-col">
        <h1 className="title">{title || job?.title || "New meeting"}</h1>
        {failed ? (
          <div className="callout callout-error">
            <p><strong>Processing failed.</strong> {job?.error ?? "The recording is kept, so nothing is lost."}</p>
            <button className="btn btn-primary" onClick={() => job && onRetry(job.id)}>Try again</button>
          </div>
        ) : done && job?.output_path ? (
          <div className="callout">
            <p>Your meeting is ready.</p>
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
          Everything runs on this Mac. You can close the window; Margin keeps working from the menu bar.
        </p>
        {notes.trim() && (
          <section className="your-notes">
            <h2 className="eyebrow">Your notes</h2>
            <pre className="notes notes-static">{notes}</pre>
            <p className="quiet-note">These go into the meeting file as soon as it's saved.</p>
          </section>
        )}
      </div>
    </div>
  );
}
