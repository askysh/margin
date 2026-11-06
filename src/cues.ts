import { useEffect, useRef } from "react";
import { api, on, type Job, type RecorderView } from "./api";

type Cue = "start" | "stop" | "complete" | "error";

/** Short sounds when a recording starts, stops, is ready, or fails to process. They
 * confirm a start or stop made from the menu bar or the global shortcut, when the window
 * isn't in view. The same sounds as upstream Minutes. */
export function useCues(enabled: boolean) {
  const enabledRef = useRef(enabled);
  enabledRef.current = enabled;

  useEffect(() => {
    const sounds = new Map<Cue, HTMLAudioElement>();
    const play = (cue: Cue) => {
      if (!enabledRef.current) return;
      let a = sounds.get(cue);
      if (!a) {
        a = new Audio(`audio/cue-${cue}.wav`);
        sounds.set(cue, a);
      }
      a.currentTime = 0;
      a.play().catch(() => {});
    };
    // Seed what is already true, so loading the window mid-recording plays nothing.
    let recording: boolean | null = null;
    const failed = new Set<string>();
    let seeded = false;
    api.recorderView().then((v) => (recording ??= v.recording), () => {});
    api.listJobs().then((jobs) => {
      jobs.filter((j) => j.state === "failed").forEach((j) => failed.add(j.id));
      seeded = true;
    }, () => (seeded = true));

    const subs = [
      on<RecorderView>("recorder", (v) => {
        if (v.recording && recording === false) play("start");
        recording = v.recording;
      }),
      on("recording-stopped", () => play("stop")),
      on("meeting-ready", () => play("complete")),
      on<Job[]>("jobs", (jobs) => {
        for (const j of jobs) {
          if (j.state !== "failed" || failed.has(j.id)) continue;
          failed.add(j.id);
          if (seeded) play("error");
        }
      }),
    ];
    return () => subs.forEach((p) => p.then((u) => u()));
  }, []);
}
