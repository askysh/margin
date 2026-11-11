// Run with `bun test`. The zone is fixed, so the results do not depend on the Mac.
import { describe, expect, test } from "bun:test";
import { readyText, span, stoppedText, whenParts } from "./times";

const zone = { locale: "en-GB", timeZone: "Asia/Kolkata" };
const times = (started: string | null, stopped: string | null, ready: string | null) => ({
  recording_started_at: started,
  recording_finished_at: stopped,
  ready_at: ready,
});

describe("span", () => {
  test("writes durations as core does", () => {
    expect(span(48_000)).toBe("48s");
    expect(span(252_000)).toBe("4m 12s");
    expect(span(240_000)).toBe("4m");
    expect(span((2 * 3600 + 36 * 60 + 20) * 1000)).toBe("2h 36m");
    expect(span(3600_000)).toBe("1h");
    expect(span(400)).toBe("0s");
    expect(span(-5000)).toBe("0s");
  });
});

describe("whenParts", () => {
  test("shows the start and the Stop time", () => {
    const t = times("2026-11-10T11:31:00+05:30", "2026-11-10T12:24:03+05:30", null);
    expect(whenParts("2026-11-10T11:31:00+05:30", t, zone)).toEqual(["Tue 10 Nov", "11:31 – 12:24"]);
    expect(whenParts(null, t, { locale: "en-US", timeZone: "Asia/Kolkata" })).toEqual(["Tue, Nov 10", "11:31 AM – 12:24 PM"]);
  });

  test("shows one time for a recording inside one minute", () => {
    const t = times("2026-11-10T10:37:05+05:30", "2026-11-10T10:37:31+05:30", null);
    expect(whenParts(null, t, zone)).toEqual(["Tue 10 Nov", "10:37"]);
  });

  test("names the day of a Stop after midnight", () => {
    const t = times("2026-11-10T23:40:00+05:30", "2026-11-11T00:20:00+05:30", null);
    expect(whenParts(null, t, zone)).toEqual(["Tue 10 Nov", "23:40 – Wed 11 Nov, 0:20"]);
  });

  test("shows less, not a guess, when the record has less", () => {
    expect(whenParts("2026-11-10T11:31:00+05:30", null, zone)).toEqual(["Tue 10 Nov", "11:31"]);
    expect(whenParts("2026-11-10T11:31:00+05:30", times(null, null, null), zone)).toEqual(["Tue 10 Nov", "11:31"]);
    // A Stop before the start is a broken clock: no Stop time.
    expect(whenParts(null, times("2026-11-10T11:31:00+05:30", "2026-11-10T11:00:00+05:30", null), zone)).toEqual(["Tue 10 Nov", "11:31"]);
    expect(whenParts(null, null, zone)).toEqual([]);
    expect(whenParts("not a date", null, zone)).toEqual([]);
  });
});

describe("readyText", () => {
  test("says when the meeting was ready and how long after Stop", () => {
    const t = times("2026-11-10T11:31:00+05:30", "2026-11-10T12:24:03+05:30", "2026-11-10T15:00:41+05:30");
    expect(readyText(t, zone)).toBe("Ready at 15:00 · 2h 36m after Stop");
    expect(readyText(t, { locale: "en-US", timeZone: "Asia/Kolkata" })).toBe("Ready at 3:00 PM · 2h 36m after Stop");
  });

  test("names the day when it was ready on another day", () => {
    const t = times(null, "2026-11-10T23:50:00+05:30", "2026-11-11T00:05:30+05:30");
    expect(readyText(t, zone)).toBe("Ready at Wed 11 Nov, 0:05 · 15m 30s after Stop");
  });

  test("is null without a Stop time or a ready time", () => {
    expect(readyText(null, zone)).toBeNull();
    expect(readyText(times(null, null, "2026-11-10T15:00:41+05:30"), zone)).toBeNull();
    expect(readyText(times(null, "2026-11-10T12:24:03+05:30", null), zone)).toBeNull();
    expect(readyText(times(null, "2026-11-10T12:24:03+05:30", "2026-11-10T12:00:00+05:30"), zone)).toBeNull();
  });
});

describe("stoppedText", () => {
  test("counts the time since Stop", () => {
    const stop = "2026-11-10T12:24:03+05:30";
    const now = new Date(stop).getTime() + 252_000;
    expect(stoppedText(stop, now, zone)).toBe("Stopped at 12:24 · 4m 12s ago");
    expect(stoppedText(stop, now + 86_400_000, zone)).toBe("Stopped at Tue 10 Nov, 12:24 · 24h 4m ago");
    expect(stoppedText(null, now, zone)).toBeNull();
  });
});
