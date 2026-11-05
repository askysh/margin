import { useEffect, useRef } from "react";

export function clock(secs: number): string {
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = Math.floor(secs % 60);
  const mm = String(m).padStart(h ? 2 : 1, "0");
  return h ? `${h}:${mm}:${String(s).padStart(2, "0")}` : `${mm}:${String(s).padStart(2, "0")}`;
}

export function bytes(n: number): string {
  if (n >= 1e9) return `${(n / 1e9).toFixed(1)} GB`;
  if (n >= 1e6) return `${Math.round(n / 1e6)} MB`;
  if (n >= 1e3) return `${Math.round(n / 1e3)} KB`;
  return `${n} B`;
}

export function tildify(path: string, home?: string): string {
  const h = home ?? guessHome(path);
  return h && path.startsWith(h) ? "~" + path.slice(h.length) : path;
}

function guessHome(path: string): string | null {
  const m = path.match(/^\/Users\/[^/]+/);
  return m ? m[0] : null;
}

const dayFmt = new Intl.DateTimeFormat(undefined, { weekday: "short", day: "numeric", month: "short" });
const timeFmt = new Intl.DateTimeFormat(undefined, { hour: "numeric", minute: "2-digit" });

export function meetingDate(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "";
  return `${dayFmt.format(d)} · ${timeFmt.format(d)}`;
}

export function shortTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  return isNaN(d.getTime()) ? "" : timeFmt.format(d);
}

/** Library groups, newest first: Today, Yesterday, This week, then month names. */
export function groupLabel(iso: string | null, now = new Date()): string {
  if (!iso) return "Undated";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return "Undated";
  const startOf = (x: Date) => new Date(x.getFullYear(), x.getMonth(), x.getDate()).getTime();
  const days = Math.round((startOf(now) - startOf(d)) / 86_400_000);
  if (days <= 0) return "Today";
  if (days === 1) return "Yesterday";
  if (days < 7) return "This week";
  const sameYear = d.getFullYear() === now.getFullYear();
  return new Intl.DateTimeFormat(undefined, sameYear ? { month: "long" } : { month: "long", year: "numeric" }).format(d);
}

/** Display names for a meeting's labels. Raw `SPEAKER_n` become "Speaker k", where k is
 * the speaker's order of first appearance among all speakers, named or not. Renaming one
 * speaker never renumbers the others. Names pass through. */
export function speakerNames(order: string[]): Map<string, string> {
  const out = new Map<string, string>();
  let n = 0;
  for (const label of order) {
    if (out.has(label)) continue;
    n++;
    out.set(label, label === "MICROPHONE" ? "You" : label === "SYSTEM" ? "Them" : isRawSpeaker(label) ? `Speaker ${n}` : label);
  }
  return out;
}

/** Labels core writes before anyone is named: diarized `SPEAKER_n`, or the capture source
 * (`MICROPHONE` = your mic, `SYSTEM` = the call) on call recordings. */
export function isRawSpeaker(label: string | null): boolean {
  return !!label && /^(SPEAKER_\d+|MICROPHONE|SYSTEM)$/.test(label);
}

/** Run `fn(value)` once `value` has been stable for `ms`. A change still pending when the
 * component unmounts is flushed, not dropped (switching meetings mid-sentence keeps it). */
export function useDebounced<T>(value: T, ms: number, fn: (v: T) => void) {
  const first = useRef(true);
  const fnRef = useRef(fn);
  fnRef.current = fn;
  const pending = useRef<{ value: T } | null>(null);
  useEffect(() => {
    if (first.current) {
      first.current = false;
      return;
    }
    pending.current = { value };
    const t = setTimeout(() => {
      pending.current = null;
      fnRef.current(value);
    }, ms);
    return () => clearTimeout(t);
  }, [value, ms]);
  useEffect(
    () => () => {
      if (pending.current) {
        const { value: v } = pending.current;
        pending.current = null;
        fnRef.current(v);
      }
    },
    [],
  );
}

/** Copy text. The async clipboard API isn't available in every WebView context, so fall
 * back to a hidden textarea and execCommand. */
export async function copyText(text: string): Promise<boolean> {
  try {
    await navigator.clipboard.writeText(text);
    return true;
  } catch {
    const ta = document.createElement("textarea");
    ta.value = text;
    ta.style.position = "fixed";
    ta.style.opacity = "0";
    document.body.appendChild(ta);
    ta.select();
    const ok = document.execCommand("copy");
    ta.remove();
    return ok;
  }
}
