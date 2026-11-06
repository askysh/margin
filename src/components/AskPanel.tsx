import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorText, on, type ChatMessage, type ChatReadiness, type ChatThread } from "../api";
import { clock } from "../util";
import { Markdown, type LinkHandler } from "./Markdown";

export type AskScope = "meeting" | "all";

interface Props {
  /** The meeting open in the window, if any. */
  meeting: { path: string; title: string } | null;
  scope: AskScope;
  onScope: (s: AskScope) => void;
  onOpenMeeting: (path: string) => void;
  onClose: () => void;
}

const SUGGESTIONS: Record<AskScope, string[]> = {
  meeting: ["What did I commit to?", "Draft a follow-up email", "What decisions were made?", "What questions are still open?"],
  all: ["What did I commit to this week?", "What decisions did we make recently?", "What's still open across my meetings?"],
};

/** Ask: a chat with Claude about the open meeting or about all meetings, docked on the
 * right of the window. Answers stream in; follow-ups continue the same conversation. */
export function AskPanel({ meeting, scope, onScope, onOpenMeeting, onClose }: Props) {
  const about = scope === "meeting" && meeting ? meeting : null;
  const path = about?.path ?? null;
  const [thread, setThread] = useState<ChatThread | null>(null);
  const [ready, setReady] = useState<ChatReadiness | null>(null);
  const [partial, setPartial] = useState("");
  const [activity, setActivity] = useState<string | null>(null);
  const [startedAt, setStartedAt] = useState(0);
  const [sendError, setSendError] = useState<string | null>(null);
  const [draft, setDraft] = useState("");
  const keyRef = useRef<string | null>(null);
  // Counts finished turns, so a send that resolves after its own "chat-done" is ignored.
  const doneSeq = useRef(0);
  // Counts stream events, so a late send reply doesn't wipe text that already arrived.
  const streamSeq = useRef(0);
  const input = useRef<HTMLTextAreaElement>(null);
  const body = useRef<HTMLDivElement>(null);

  const show = useCallback((t: ChatThread) => {
    keyRef.current = t.key;
    setThread(t);
    setPartial(t.running ? t.partial : "");
    setActivity(t.running ? t.activity : null);
    setStartedAt(Date.now() - t.elapsed_secs * 1000);
  }, []);

  const checkReady = useCallback(() => api.chatStatus().then(setReady).catch(() => setReady(null)), []);
  useEffect(() => { checkReady(); }, [checkReady]);

  useEffect(() => {
    setThread(null);
    setSendError(null);
    api.chatThread(path).then(show).catch((e) => setSendError(errorText(e)));
    input.current?.focus();
  }, [path, show]);

  useEffect(() => {
    const mine = (key: string) => key === keyRef.current;
    const subs = [
      on<{ key: string; text: string }>("chat-delta", (d) => {
        if (!mine(d.key)) return;
        streamSeq.current++;
        setPartial(d.text);
      }),
      on<{ key: string; text: string }>("chat-activity", (a) => {
        if (!mine(a.key)) return;
        streamSeq.current++;
        setActivity(a.text);
      }),
      on<ChatThread>("chat-done", (t) => {
        if (!mine(t.key)) return;
        doneSeq.current++;
        show(t);
      }),
    ];
    return () => subs.forEach((p) => p.then((u) => u()));
  }, [show]);

  const running = !!thread?.running;
  const messages = thread?.messages ?? [];
  useEffect(() => {
    const el = body.current;
    if (el) el.scrollTop = el.scrollHeight;
  }, [messages.length, partial, running]);

  const send = async (text: string) => {
    if (!text.trim() || running) return;
    setSendError(null);
    setDraft("");
    const seq = doneSeq.current;
    const streamed = streamSeq.current;
    try {
      const t = await api.chatSend(path, text);
      if (doneSeq.current !== seq) return;
      if (streamSeq.current === streamed) show(t);
      else setThread(t);
    } catch (e) {
      setDraft(text);
      setSendError(errorText(e));
      checkReady();
    }
  };

  const clear = async () => {
    await api.chatClear(path).catch(() => {});
    if (thread) show({ ...thread, messages: [], session_id: null, running: false, partial: "", activity: null });
    input.current?.focus();
  };

  const notReady = ready && !ready.ready ? ready : null;

  return (
    <aside className="ask" aria-label="Ask" onKeyDown={(e) => e.key === "Escape" && onClose()}>
      <header className="ask-head">
        <span className="ask-title">Ask</span>
        {meeting && (
          <div className="ask-scope" role="radiogroup" aria-label="Ask about">
            <button role="radio" aria-checked={scope === "meeting"} onClick={() => onScope("meeting")}>This meeting</button>
            <button role="radio" aria-checked={scope === "all"} onClick={() => onScope("all")}>All meetings</button>
          </div>
        )}
        <span className="toolbar-spacer" />
        {messages.length > 0 && !running && (
          <button className="link-btn" onClick={clear} title="Clear this conversation">Clear</button>
        )}
        <button className="icon-btn small" aria-label="Close Ask" title="Close (Esc)" onClick={onClose}>×</button>
      </header>

      <div className="ask-body" ref={body}>
        <p className="ask-about">{about ? <>About “{about.title}”</> : "Across all your meetings"}</p>
        {notReady && (
          <div className="callout ask-setup" role="status">
            <Markdown source={notReady.message ?? ""} />
            <button className="btn btn-quiet" onClick={checkReady}>Check again</button>
          </div>
        )}
        {thread && messages.length === 0 && !running && !notReady && (
          <div className="ask-suggest">
            {SUGGESTIONS[about ? "meeting" : "all"].map((s) => (
              <button key={s} className="ask-chip" onClick={() => send(s)}>{s}</button>
            ))}
          </div>
        )}
        {messages.map((m, i) => (
          <Turn key={i} message={m} onOpenMeeting={onOpenMeeting} />
        ))}
        {running && <Pending text={partial} activity={activity} startedAt={startedAt} />}
        {sendError && !notReady && <div className="ask-error" role="alert"><Markdown source={sendError} /></div>}
      </div>

      <form className="ask-compose" onSubmit={(e) => { e.preventDefault(); send(draft); }}>
        <textarea
          ref={input}
          rows={1}
          value={draft}
          aria-label="Ask a question"
          placeholder={about ? "Ask about this meeting" : "Ask about your meetings"}
          onChange={(e) => {
            setDraft(e.target.value);
            e.target.style.height = "auto";
            e.target.style.height = `${Math.min(e.target.scrollHeight, 140)}px`;
          }}
          onKeyDown={(e) => {
            if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
              e.preventDefault();
              send(draft);
            }
          }}
        />
        {running ? (
          <button type="button" className="btn btn-quiet" onClick={() => api.chatStop(path)}>Stop</button>
        ) : (
          <button type="submit" className="btn btn-primary" disabled={!draft.trim() || !!notReady}>Ask</button>
        )}
      </form>
    </aside>
  );
}

function Turn({ message: m, onOpenMeeting }: { message: ChatMessage; onOpenMeeting: (path: string) => void }) {
  if (m.role === "user") return <p className="ask-q">{m.text}</p>;
  const link: LinkHandler = (href) => {
    const name = decodeURIComponent(href).replace(/^\.\//, "");
    const hit = m.sources.find((s) => s.path === name || s.path.endsWith(`/${name}`));
    return hit ? () => onOpenMeeting(hit.path) : null;
  };
  return (
    <div className="ask-a">
      {m.text.trim() && <Markdown source={m.text} onLink={link} />}
      {m.stopped && <p className="quiet-note">Stopped.</p>}
      {m.error && <div className="ask-error" role="alert"><Markdown source={m.error} /></div>}
      {m.sources.length > 0 && (
        <div className="ask-sources">
          <span>From</span>
          {m.sources.map((s) => (
            <button key={s.path} className="link-btn" onClick={() => onOpenMeeting(s.path)}>{s.title}</button>
          ))}
        </div>
      )}
    </div>
  );
}

/** The answer while it streams, with what Claude is doing and, after a while, how long. */
function Pending({ text, activity, startedAt }: { text: string; activity: string | null; startedAt: number }) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => {
    const t = window.setInterval(() => setNow(Date.now()), 1000);
    return () => window.clearInterval(t);
  }, []);
  const secs = Math.max(0, Math.floor((now - startedAt) / 1000));
  return (
    <div className="ask-a ask-pending" aria-live="polite">
      {text.trim() && <Markdown source={text} />}
      <p className="ask-status">
        <span className="spinner" aria-hidden />
        {text.trim() ? "Writing…" : activity ?? "Thinking…"}
        {secs >= 10 && <span className="mono"> · {clock(secs)}</span>}
      </p>
      {secs >= 60 && <p className="quiet-note">This is taking a while. You can stop and ask a narrower question.</p>}
    </div>
  );
}
