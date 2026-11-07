import { Suspense, lazy, useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, on, type AppPrefs, type Boot, type Job, type Kind, type LiveLine, type MeetingSummary, type RecorderView } from "./api";
import { Sidebar } from "./components/Sidebar";
import { LiveView, liveDraft } from "./components/LiveView";
import { MeetingView } from "./components/MeetingView";
import { ProcessingView } from "./components/ProcessingView";
import { Home } from "./components/Home";
import { AskPanel, type AskScope } from "./components/AskPanel";
import { UpdateBanner } from "./components/Updates";
import { clock, meetingFileReady } from "./util";
import { useCues } from "./cues";
import type { SectionId } from "./components/SettingsNav";

// Rarely shown, so loaded on first use instead of at startup (docs/perf.md).
const Onboarding = lazy(() => import("./components/Onboarding").then((m) => ({ default: m.Onboarding })));
const SettingsPage = lazy(() => import("./components/Settings").then((m) => ({ default: m.SettingsPage })));
const SettingsNav = lazy(() => import("./components/Settings").then((m) => ({ default: m.SettingsNav })));

export type Selection =
  | { type: "home" }
  | { type: "live" }
  | { type: "processing"; jobId: string; notes: string; title: string | null }
  | { type: "meeting"; path: string; jobId?: string };

const DONE: Job["state"][] = ["complete", "failed", "needs-review"];

export default function App() {
  const [boot, setBoot] = useState<Boot | null>(null);
  const [bootError, setBootError] = useState<string | null>(null);
  const [meetings, setMeetings] = useState<MeetingSummary[]>([]);
  const [jobs, setJobs] = useState<Job[]>([]);
  const [selection, setSelection] = useState<Selection>({ type: "home" });
  const [recorder, setRecorder] = useState<RecorderView | null>(null);
  const [lines, setLines] = useState<LiveLine[]>([]);
  const [tick, setTick] = useState({ elapsed_secs: 0, level: 0 });
  // Settings is a page over the current view. The selection stays as it was, so Back returns to it.
  const [settings, setSettings] = useState<SectionId | null>(null);
  const lastSection = useRef<SectionId>("general");
  if (settings) lastSection.current = settings;
  // While Reset runs, nothing may leave Settings or start a recording: reset deletes the
  // state a new recording writes (drafts/current.md) and then runs setup again.
  const [resetting, setResetting] = useState(false);
  const resettingRef = useRef(false);
  resettingRef.current = resetting;
  const onboardedRef = useRef(false);
  onboardedRef.current = !!boot?.onboarded;
  const [toast, setToast] = useState<{ text: string; tone: "info" | "error" } | null>(null);
  const [busy, setBusy] = useState(false);
  const [callApp, setCallApp] = useState<string | null>(null);
  const [person, setPerson] = useState<string | null>(null);
  const [ask, setAsk] = useState<AskScope | null>(null);
  useCues(boot?.prefs.sound_cues ?? false);
  const selectionRef = useRef(selection);
  selectionRef.current = selection;
  const recorderRef = useRef<RecorderView | null>(null);

  const notify = useCallback((text: string, tone: "info" | "error" = "info") => {
    setToast({ text, tone });
    window.setTimeout(() => setToast((t) => (t?.text === text ? null : t)), tone === "error" ? 9000 : 4000);
  }, []);

  // Focus, saves and finished jobs all refresh the list. Most refreshes change nothing, and
  // then the sidebar is left alone.
  const lastList = useRef("");
  const refreshMeetings = useCallback(
    () =>
      api
        .listMeetings()
        .then((list) => {
          const json = JSON.stringify(list);
          if (json === lastList.current) return;
          lastList.current = json;
          setMeetings(list);
        })
        .catch(() => {}),
    [],
  );

  const reboot = useCallback(() => {
    api
      .boot()
      .then((b) => {
        setBoot(b);
        recorderRef.current = b.recorder;
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
        // A new recording (or one started from the menu bar) takes over the window. Later
        // updates, such as a title change, must not pull you back from another meeting.
        if (v.recording && !recorderRef.current?.recording) {
          setLines(v.lines);
          setSelection({ type: "live" });
          if (!resettingRef.current) {
            blurSettingsField();
            setSettings(null);
          }
        }
        recorderRef.current = v;
        setRecorder(v);
      }),
      on<LiveLine[]>("live-lines", (l) =>
        setLines((prev) => {
          // Line numbers only grow, so anything at or below the last one is a repeat.
          const last = prev.length ? prev[prev.length - 1].line : 0;
          const fresh = l.filter((x) => x.line > last);
          return fresh.length ? [...prev, ...fresh] : prev;
        }),
      ),
      on<{ elapsed_secs: number; level: number }>("recorder-tick", setTick),
      on<Job[]>("jobs", setJobs),
      on<{ job_id: string; path: string }>("meeting-ready", ({ job_id, path }) => {
        refreshMeetings();
        const sel = selectionRef.current;
        const following = (sel.type === "processing" || sel.type === "meeting") && sel.jobId === job_id;
        if (following && !(sel.type === "meeting" && sel.path === path)) setSelection({ type: "meeting", path, jobId: job_id });
      }),
      on<string>("error-toast", (t) => notify(t, "error")),
      on<string>("recording-ended", (t) => notify(t)),
      on<string | null>("call-detected", (a) => setCallApp(a)),
      // The menu bar's recent meetings and a click on a "meeting ready" notification.
      on<string>("open-meeting", (path) => {
        if (resettingRef.current) return;
        setSelection({ type: "meeting", path });
        blurSettingsField();
        setSettings(null);
      }),
      // The menu bar's "Settings…": the same as ⌘,.
      // Setup has no Settings yet, so the item waits until it is done.
      on<null>("open-settings", () => onboardedRef.current && setSettings((now) => now ?? lastSection.current)),
      on<AppPrefs>("app-prefs", (prefs) => setBoot((b) => b && { ...b, prefs })),
      // However a recording ended (Stop here, the menu bar, auto-stop), follow it to its
      // processing view if you were looking at it.
      on<{ job_id: string; title: string | null; notes: string }>("recording-stopped", ({ job_id, title, notes }) => {
        liveDraft.session = liveDraft.notes = liveDraft.title = null;
        if (selectionRef.current.type === "live") setSelection({ type: "processing", jobId: job_id, notes, title });
      }),
    ];
    const onFocus = () => refreshMeetings();
    window.addEventListener("focus", onFocus);
    return () => {
      subs.forEach((p) => p.then((u) => u()));
      window.removeEventListener("focus", onFocus);
    };
  }, [reboot, refreshMeetings, notify]);

  // A processing view becomes the meeting as soon as core has written the file (D42).
  useEffect(() => {
    if (selection.type !== "processing") return;
    const job = jobs.find((j) => j.id === selection.jobId);
    if (job && meetingFileReady(job)) {
      setSelection({ type: "meeting", path: job.output_path, jobId: job.id });
      refreshMeetings();
    }
  }, [jobs, selection, refreshMeetings]);

  const goHome = useCallback(() => {
    setSelection({ type: "home" });
    refreshMeetings();
  }, [refreshMeetings]);
  // Stable, because Sidebar and MeetingView are memoised against the ticking clock.
  const askAll = useCallback(() => setAsk((a) => (a === "all" ? null : "all")), []);
  const askMeeting = useCallback(() => setAsk("meeting"), []);
  // ⌘, opens the section you were on last, as System Settings does; a deep link (a
  // permission error, say) picks one.
  const openSettings = useCallback((section?: SectionId) => setSettings((now) => section ?? now ?? lastSection.current), []);
  const closeSettings = useCallback(() => {
    if (resettingRef.current) return;
    blurSettingsField();
    setSettings(null);
  }, []);

  const start = async (kind: Kind) => {
    setBusy(true);
    setCallApp(null);
    try {
      const v = await api.startRecording(kind);
      recorderRef.current = v;
      setRecorder(v);
      setLines([]);
      setTick({ elapsed_secs: 0, level: 0 });
      setSelection({ type: "live" });
      blurSettingsField();
      setSettings(null);
    } catch (e) {
      notify(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  const stop = async () => {
    setBusy(true);
    // Only this recording's draft: liveDraft could be from a view that never mounted.
    const mine = liveDraft.session === recorder?.started_at;
    try {
      // Navigation happens on the "recording-stopped" event.
      await api.stopRecording(mine ? liveDraft.notes : null, mine ? liveDraft.title : null);
    } catch (e) {
      // Auto-stop got there first; its event takes over.
      if (errorText(e) !== "Already stopping.") notify(errorText(e), "error");
    } finally {
      setBusy(false);
    }
  };

  if (bootError) return <div className="fatal">Margin couldn't start: {bootError}</div>;
  if (!boot) return <div className="splash" />;
  if (!boot.onboarded) {
    return (
      <Suspense fallback={<div className="splash" />}>
        <Onboarding initial={boot.settings} onDone={() => { reboot(); refreshMeetings(); }} />
      </Suspense>
    );
  }

  const recording = !!recorder?.recording;
  const openMeeting = selection.type === "meeting" ? meetings.find((m) => m.path === selection.path) : undefined;
  const active = jobs.filter((j) => !DONE.includes(j.state));
  const finishing = selection.type === "meeting" ? active.find((j) => j.output_path === selection.path) ?? null : null;

  return (
    <div className={`shell${recording ? " is-recording" : ""}${settings ? " is-settings" : ""}`}>
      {settings && (
        <Suspense fallback={<aside className="sidebar set-nav" />}>
          <SettingsNav section={settings} onSection={setSettings} onBack={closeSettings} />
        </Suspense>
      )}
      {/* Kept mounted under Settings, so search, scroll and an open meeting are as you left them. */}
      <Sidebar
        meetings={meetings}
        jobs={active}
        selection={selection}
        recording={recording}
        recordingTitle={recorder?.title ?? null}
        elapsed={tick.elapsed_secs}
        onSelect={setSelection}
        person={person}
        onPerson={setPerson}
        askOpen={ask === "all"}
        onAsk={askAll}
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
          <UpdateBanner recording={recording} notify={notify} />
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
          <button className="icon-btn" title={settings ? "Back (Esc)" : "Settings (⌘,)"} aria-label="Settings" aria-pressed={!!settings} onClick={() => (settings ? closeSettings() : openSettings())}>
            <svg width="17" height="17" viewBox="0 0 20 20" aria-hidden><path fill="currentColor" d="M11.4 1.5l.5 2.3c.5.2 1 .5 1.4.8l2.2-.8 1.4 2.4-1.7 1.6c.1.5.1 1.1 0 1.6l1.7 1.6-1.4 2.4-2.2-.8c-.4.3-.9.6-1.4.8l-.5 2.3H8.6l-.5-2.3c-.5-.2-1-.5-1.4-.8l-2.2.8-1.4-2.4 1.7-1.6a4 4 0 010-1.6L3.1 6.2l1.4-2.4 2.2.8c.4-.3.9-.6 1.4-.8l.5-2.3h2.8zM10 7a3 3 0 100 6 3 3 0 000-6z" /></svg>
          </button>
        </header>
        <section className="content">
          {settings && (
            <Suspense fallback={<div className="page page-settings" />}>
              <SettingsPage
                boot={boot}
                section={settings}
                onBack={closeSettings}
                onSaved={(s) => { setBoot((b) => b && { ...b, settings: s }); refreshMeetings(); }}
                onResetBusy={setResetting}
                onReset={() => { setResetting(false); lastSection.current = "general"; setSettings(null); setAsk(null); setSelection({ type: "home" }); lastList.current = ""; setMeetings([]); reboot(); }}
                notify={notify}
              />
            </Suspense>
          )}
          {selection.type === "live" && recorder?.recording && (
            <LiveView recorder={recorder} lines={lines} level={tick.level} />
          )}
          {selection.type === "processing" && (
            <ProcessingView
              key={selection.jobId}
              jobId={selection.jobId}
              job={jobs.find((j) => j.id === selection.jobId) ?? null}
              notes={selection.notes || jobs.find((j) => j.id === selection.jobId)?.draft || ""}
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
              onDeleted={goHome}
              job={finishing}
              onPerson={setPerson}
              onAsk={askMeeting}
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
          {ask && (
            <AskPanel
              meeting={openMeeting ? { path: openMeeting.path, title: openMeeting.title } : null}
              scope={ask}
              onScope={setAsk}
              onOpenMeeting={(path) => setSelection({ type: "meeting", path })}
              onClose={() => setAsk(null)}
            />
          )}
        </section>
      </main>
      {toast && (
        <div className={`toast toast-${toast.tone}`} role="status" onClick={() => setToast(null)}>
          {toast.text}
          {/* Permission errors name System Settings; Margin's own page shows the state and the way there. */}
          {toast.tone === "error" && toast.text.includes("Privacy & Security") && (
            <button className="toast-action" onClick={() => openSettings("permissions")}>Open Permissions</button>
          )}
        </div>
      )}
      <KeyboardShortcuts
        onSettings={openSettings}
        onStart={start}
        canStart={!recording && !busy && !resetting}
        onAsk={() => {
          if (resettingRef.current) return;
          // From Settings, ⌘J goes back and opens Ask rather than toggling it out of sight.
          setAsk((a) => (a && !settings ? null : a ?? (openMeeting ? "meeting" : "all")));
          closeSettings();
        }}
        onSearch={closeSettings}
      />
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
      <button className="btn btn-primary split-main" disabled={busy} onClick={() => onStart("call")} title="Records your mic and the other people on the call (⌘⇧R)">
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

/** In-window shortcuts: ⌘, settings, ⌘F search, ⌘J Ask, ⌘⇧R record a call, ⌘⇧I record in person. */
function KeyboardShortcuts({ onSettings, onStart, canStart, onAsk, onSearch }: { onSettings: () => void; onStart: (k: Kind) => void; canStart: boolean; onAsk: () => void; onSearch: () => void }) {
  useEffect(() => {
    const h = (e: KeyboardEvent) => {
      if (!e.metaKey) return;
      const key = e.key.toLowerCase();
      if (key === ",") {
        e.preventDefault();
        onSettings();
      } else if (key === "f" && !e.shiftKey) {
        e.preventDefault();
        onSearch();
        // After Settings has gone, so the search field is visible and can take focus.
        requestAnimationFrame(() => document.querySelector<HTMLInputElement>(".search input")?.focus());
      } else if (key === "j" && !e.shiftKey) {
        e.preventDefault();
        onAsk();
      } else if (e.shiftKey && (key === "r" || key === "i") && canStart) {
        e.preventDefault();
        onStart(key === "r" ? "call" : "in-person");
      }
    };
    window.addEventListener("keydown", h);
    return () => window.removeEventListener("keydown", h);
  }, [onSettings, onStart, canStart, onAsk, onSearch]);
  return null;
}

/** Leaving Settings unmounts a focused field without a blur, so its save-on-blur would never
 * run (React ignores events during a commit). Blur it first, while its handlers still work. */
function blurSettingsField() {
  const focused = document.activeElement;
  if (focused instanceof HTMLElement && focused.closest(".page-settings")) focused.blur();
}
