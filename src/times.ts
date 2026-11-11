// Start, Stop and ready times as the app shows them (DECISIONS.md D58). A time that the
// job record does not have is left out. The app never shows a guessed time.
import type { JobTimes } from "./api";

/** The locale and time zone to format in. The app uses the Mac's. Tests set both. */
export interface Zone {
  locale?: string;
  timeZone?: string;
}

/** A length of time, in the style core writes durations: "2h 36m", "4m 12s", "48s". */
export function span(ms: number): string {
  const secs = Math.max(0, Math.round(ms / 1000));
  const h = Math.floor(secs / 3600);
  const m = Math.floor((secs % 3600) / 60);
  const s = secs % 60;
  if (h) return m ? `${h}h ${m}m` : `${h}h`;
  if (m) return s ? `${m}m ${s}s` : `${m}m`;
  return `${s}s`;
}

/** The parts of a meeting's "when", in order: the day, then the start time, or the start
 * and stop times. Empty when there is no date. */
export function whenParts(date: string | null, times: JobTimes | null, zone: Zone = {}): string[] {
  const start = parse(times?.recording_started_at) ?? parse(date);
  if (!start) return [];
  const f = formats(zone);
  const stop = after(start, parse(times?.recording_finished_at));
  if (!stop) return [f.day(start), f.time(start)];
  const sameDay = f.key(stop) === f.key(start);
  // A recording inside one minute: "10:37 AM", not "10:37 AM – 10:37 AM".
  if (sameDay && f.time(stop) === f.time(start)) return [f.day(start), f.time(start)];
  const end = sameDay ? f.time(stop) : `${f.day(stop)}, ${f.time(stop)}`;
  return [f.day(start), `${f.time(start)} – ${end}`];
}

/** "Ready at 3:00 PM · 2h 36m after Stop": when processing ended, and how long after Stop.
 * Null when the record has no Stop time or no ready time. */
export function readyText(times: JobTimes | null, zone: Zone = {}): string | null {
  const stop = parse(times?.recording_finished_at);
  const ready = after(stop, parse(times?.ready_at));
  if (!stop || !ready) return null;
  return `Ready at ${at(ready, stop, zone)} · ${span(ready.getTime() - stop.getTime())} after Stop`;
}

/** "Stopped at 12:24 PM · 4m 12s ago", for a meeting that is still processing. */
export function stoppedText(stopped: string | null, now: number, zone: Zone = {}): string | null {
  const stop = parse(stopped);
  if (!stop) return null;
  return `Stopped at ${at(stop, new Date(now), zone)} · ${span(now - stop.getTime())} ago`;
}

/** The time alone, or the day and the time when `t` is not on the same day as `ref`. */
function at(t: Date, ref: Date, zone: Zone) {
  const f = formats(zone);
  return f.key(t) === f.key(ref) ? f.time(t) : `${f.day(t)}, ${f.time(t)}`;
}

function parse(iso: string | null | undefined): Date | null {
  if (!iso) return null;
  const d = new Date(iso);
  return isNaN(d.getTime()) ? null : d;
}

/** `t`, if it is not before `from`. A clock that went back gives no number at all. */
function after(from: Date | null, t: Date | null): Date | null {
  return from && t && t.getTime() >= from.getTime() ? t : null;
}

const cache = new Map<string, { day: (d: Date) => string; time: (d: Date) => string; key: (d: Date) => string }>();
function formats({ locale, timeZone }: Zone) {
  const id = `${locale ?? ""}|${timeZone ?? ""}`;
  let f = cache.get(id);
  if (!f) {
    const day = new Intl.DateTimeFormat(locale, { weekday: "short", day: "numeric", month: "short", timeZone });
    const time = new Intl.DateTimeFormat(locale, { hour: "numeric", minute: "2-digit", timeZone });
    // The calendar day, to compare two times in the same zone.
    const key = new Intl.DateTimeFormat("en-CA", { year: "numeric", month: "2-digit", day: "2-digit", timeZone });
    f = { day: (d) => day.format(d), time: (d) => time.format(d), key: (d) => key.format(d) };
    cache.set(id, f);
  }
  return f;
}
