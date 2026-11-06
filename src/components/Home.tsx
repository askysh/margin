import type { Kind } from "../api";
import { copyText, tildify } from "../util";
import { DownloadProgress } from "./Models";

interface Props {
  count: number;
  outputDir: string;
  orphanedDraft: string | null;
  onDismissDraft: () => void;
  onStart: (k: Kind) => void;
  busy: boolean;
}

export function Home({ count, outputDir, orphanedDraft, onDismissDraft, onStart, busy }: Props) {
  return (
    <div className="page page-home">
      <div className="home-inner">
        {orphanedDraft && (
          <div className="callout">
            <p><strong>Notes from an interrupted recording.</strong> Margin closed before the meeting was saved. Here's what you typed:</p>
            <pre className="notes notes-static">{orphanedDraft}</pre>
            <div className="row-actions">
              <button className="btn btn-quiet" onClick={() => copyText(orphanedDraft)}>Copy</button>
              <button className="btn btn-quiet" onClick={onDismissDraft}>Discard</button>
            </div>
          </div>
        )}
        <h1 className="home-title">{count === 0 ? "Your first meeting starts here." : "Ready when you are."}</h1>
        <p className="home-body">
          Start recording when the meeting begins and type notes in your own words. Margin writes the transcript,
          tells speakers apart and saves everything to <span className="path">{tildify(outputDir)}</span>.
        </p>
        <DownloadProgress compact hideWhenReady pendingNote="Recording starts working when the speech models are downloaded." />
        <div className="home-choices">
          <button className="choice" onClick={() => onStart("call")} disabled={busy}>
            <span className="choice-title">Record a call</span>
            <span className="choice-body">Zoom, Meet, Teams, or any app. Captures your mic and the other people.</span>
          </button>
          <button className="choice" onClick={() => onStart("in-person")} disabled={busy}>
            <span className="choice-title">Record in person</span>
            <span className="choice-body">Mic only, for a meeting around a table.</span>
          </button>
        </div>
        <p className="quiet-note">You can also start and stop from the menu bar.</p>
      </div>
    </div>
  );
}
