import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, on, type Boot, type Job, type Kind, type LiveLine, type MeetingSummary, type RecorderView } from "./api";
import { Onboarding } from "./components/Onboarding";
import { Sidebar } from "./components/Sidebar";
import { LiveView, liveDraft } from "./components/LiveView";
import { MeetingView } from "./components/MeetingView";
import { ProcessingView } from "./components/ProcessingView";
import { Home } from "./components/Home";
import { SettingsSheet } from "./components/Settings";
import { clock } from "./util";

export type Selection =
  | { type: "home" }
  | { type: "live" }
  | { type: "processing"; jobId: string; notes: string; title: string | null }
  | { type: "meeting"; path: string };

export default function App() {
  const [boot, setBoot] = useState<Boot | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [meetings, setMeetings] = useState<MeetingSummary[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [selection, setSelection] = useState<Selection>({ type: "home" });
  const [recorder, setRecorder] = useState<RecorderView | null>(null);
  const [lines, setLines] = useState<LiveLine[]>([]);
  const [tick, setTick] = useState({ elapsed_secs: 0, level: 0 });
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [toast, setToast] = useState<{ text: string; tone: "info" | "error" } | null>(null);
  const [busy, setBusy] = useState(false);
  const [callApp, setCallApp] = useState<string | null>(null);
  const selectionRef = useRef(selection);
  selectionRef.current = selection;

  const notify = useCallback((text: string, tone: "info" | "error" = "info") => {
    setToast({ text, tone });
    window.setTimeout(() => setToast((t) => (t?.text === text ? null : t)), tone === "error" ? 9000 : 4000);
  }, []);

  const refreshMeetings = useCallback(() => api.listMeetings().then(setMeetings).catch(() => {}), []);

  const reboot = useCallback(() => {
    api
      .boot()
      .then((b) => {
        setBoot(b);
        setRecorder(b.recorder);
        setLines(b.recorder.lines);
        if (b.recorder.recording) setSelection({ type: "live" });
      })
      .catch((e) => setBootError(errorText(e)));
  }, []);

  useEffect(() => {
    reboot();
    refreshMeetings();
    api.listJobs().then(setJobs).catch(() => {});
    const subs = [
      on<RecorderView>("recorder", (v) => {
        setRecorder(v);
        if (v.recording) {
          setLines(v.lines);
          setSelection({ type: "live" });
        }
      }),
      on<LiveLine[]>("live-lines", (l) => setLines((prev) => [...prev, ...l.filter((x) => !prev.some((p) => p.line === x.line))])),
      on<{ elapsed_secs: number; level: number }>("recorder-tick", setTick),
      on<Job[]>("jobs", setJobs),
      on<{ job_id: string; path: string }>("meeting-ready", ({ job_id, path }) => {
        refreshMeetings();
        const sel = selectionRef.current;
        if (sel.type === "processing" && sel.jobId === job_id) setSelection({ type: "meeting", path });
      }),
      on<string>("error-toast", (t) => notify(t, "error")),
      on<string>("recording-ended", (t) => notify(t)),
      on<string | null>("call-detected", (a) => setCallApp(a)),
    ];
    const onFocus = () => refreshMeetings();
    window.addEventListener("focus", onFocus);
    return () => {
      subs.forEach((p) => p.then((u) => u()));
      window.removeEventListener("focus", onFocus);
    };
  }, [reboot, refreshMeetings, notify]);

  // When a recording ends from the menu bar, follow it to its processing view.
  const lastRecording = useRef<RecorderView | null>(null);
  useEffect(() => {
    const prev = lastRecording.current;
    if (prev?.recording && recorder && !recorder.recording && selectionRef.current.type === "live") {
      const job = jobs.find((j) => !["complete", "failed", "needs-review"].includes(j.state));
      setSelection(job ? { type: "processing", jobId: job.id, notes: prev.notes, title: prev.title } : { type: "home" });
    }
    lastRecording.current = recorder;
  }, [recorder, jobs]);

  const start = async (kind: Kind) => {
    setBusy(true);
    setCallApp(null);
    try {
      const v = await api.startRecording(kind);
      setRecorder(v);
      setLines([]);
      setTick({ elapsed_secs: 0, level: 0 });
      setSelection({ type: "live" });
    } catch (e) {
      notify(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    const notes = liveDraft.notes ?? recorder?.notes ?? "";
    try {
      const { job_id, title } = await api.stopRecording(liveDraft.notes, liveDraft.title);
      lastRecording.current = null;
      liveDraft.notes = liveDraft.title = null;
      setRecorder((r) => (r ? { ...r, recording: false, stopping: false } : r));
      setSelection({ type: "processing", jobId: job_id, notes, title });
    } catch (e) {
      notify(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  if (bootError) return <div className="fatal">Margin couldn't start: {bootError}</div>;
  if (!boot) return <div className="splash" />;
  if (!boot.onboarded) {
    return <Onboarding initial={boot.settings} onDone={() => { reboot(); refreshMeetings(); }} />;
  }

  const recording = !!recorder?.recording;
  const active = jobs.filter((j) => !["complete", "failed", "needs-review"].includes(j.state));

  return (
    <div className={`shell ${recording ? "is-recording" : ""}`}>
      <Sidebar
        meetings={meetings}
        jobs={active}
        selection={selection}
        recording={recording}
        recordingTitle={recorder?.title ?? null}
        elapsed={tick.elapsed_secs}
        onSelect={setSelection}
      />
      <main className="pane">
        <header className="toolbar" data-tauri-drag-region>
          <div className="toolbar-spacer" data-tauri-drag-region />
          {callApp && !recording && (
            <div className="call-banner" role="status">
              <span>{callApp} looks active.</span>
              <button className="link-btn" onClick={() => start("call")}>Record this call</button>
              <button className="icon-btn small" aria-label="Dismiss" onClick={() => setCallApp(null)}>×</button>
            </div>
          )}
          {recording ? (
            <div className="rec-control">
              <span className="rec-dot" aria-hidden />
              <span className="rec-time mono">{clock(tick.elapsed_secs)}</span>
              <button className="btn btn-stop" onClick={stop} disabled={busy || recorder?.stopping}>
                {busy || recorder?.stopping ? "Saving…" : "Stop"}
              </button>
            </div>
          ) : (
            <RecordButton busy={busy} onStart={start} />
          )}
          <button className="icon-btn" title="Settings (⌘,)" aria-label="Settings" onClick={() => setSettingsOpen(true)}>
            <svg width="17" height="17" viewBox="0 0 20 20" aria-hidden><path fill="currentColor" d="M11.4 1.5l.5 2.3c.5.2 1 .5 1.4.8l2.2-.8 1.4 2.4-1.7 1.6c.1.5.1 1.1 0 1.6l1.7 1.6-1.4 2.4-2.2-.8c-.4.3-.9.6-1.4.8l-.5 2.3H8.6l-.5-2.3c-.5-.2-1-.5-1.4-.8l-2.2.8-1.4-2.4 1.7-1.6a4 4 0 010-1.6L3.1 6.2l1.4-2.4 2.2.8c.4-.3.9-.6 1.4-.8l.5-2.3h2.8zM10 7a3 3 0 100 6 3 3 0 000-6z" /></svg>
          </button>
        </header>
        <section className="content">
          {selection.type === "live" && recorder?.recording && (
            <LiveView recorder={recorder} lines={lines} level={tick.level} />
          )}
          {selection.type === "processing" && (
            <ProcessingView
              job={jobs.find((j) => j.id === selection.jobId) ?? null}
              notes={selection.notes}
              title={selection.title}
              onOpen={(path) => setSelection({ type: "meeting", path })}
              onRetry={(id) => api.retryJob(id).catch((e) => notify(errorText(e), "error"))}
            />
          )}
          {selection.type === "meeting" && (
            <MeetingView
              key={selection.path}
              path={selection.path}
              notify={notify}
              onChanged={refreshMeetings}
              onDeleted={() => { setSelection({ type: "home" }); refreshMeetings(); }}
            />
          )}
          {(selection.type === "home" || (selection.type === "live" && !recorder?.recording)) && (
            <Home
              count={meetings.length}
              outputDir={boot.settings.output_dir}
              orphanedDraft={boot.orphaned_draft}
              onDismissDraft={() => { api.discardOrphanedDraft(); setBoot({ ...boot, orphaned_draft: null }); }}
              onStart={start}
              busy={busy}
            />
          )}
        </section>
      </main>
      {settingsOpen && (
        <SettingsSheet
          boot={boot}
          onClose={() => setSettingsOpen(false)}
          onSaved={(s) => { setBoot({ ...boot, settings: s }); refreshMeetings(); }}
          onReset={() => { setSettingsOpen(false); setSelection({ type: "home" }); setMeetings([]); reboot(); }}
          notify={notify}
        />
      )}
      {toast && (
        <div className={`toast toast-${toast.tone}`} role="status" onClick={() => setToast(null)}>{toast.text}</div>
      )}
      <KeyboardShortcuts onSettings={() => setSettingsOpen(true)} />
    </div>
  );
}

function RecordButton({ busy, onStart }: { busy: boolean; onStart: (k: Kind) => void }) {
  const [open, setOpen] = useState(false);
  useEffect(() => {
    if (!open) return;
    const close = () => setOpen(false);
    window.addEventListener("click", close);
    return () => window.removeEventListener("click", close);
  }, [open]);
  return (
    <div className="split">
      <button className="btn btn-primary split-main" disabled={busy} onClick={() => onStart("call")} title="Records your mic and the other people on the call">
        Record call
      </button>
      <button
        className="btn btn-primary split-more"
        aria-label="More recording options"
        aria-expanded={open}
        disabled={busy}
        onClick={(e) => { e.stopPropagation(); setOpen((o) => !o); }}
      >
        <svg width="10" height="10" viewBox="0 0 10 10" aria-hidden><path d="M2 3.5l3 3 3-3" stroke="currentColor" strokeWidth="1.5" fill="none" /></svg>
      </button>
      {open && (
        <div className="menu" role="menu">
          <button role="menuitem" onClick={() => onStart("call")}>
            <strong>Call</strong>
            <span>Zoom, Meet, Teams: your mic and the other people</span>
          </button>
          <button role="menuitem" onClick={() => onStart("in-person")}>
            <strong>In person</strong>
            <span>Mic only, for a meeting in the room</span>
          </button>
        </div>
      )}
    </div>
  );
}

function KeyboardShortcuts({ onSettings }: { onSettings: () => void }) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (e.metaKey && e.key === ",") {
        e.preventDefault();
        onSettings();
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onSettings]);
  return null;
}
