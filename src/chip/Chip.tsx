import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, chipApi, errorText, on, type RecorderView } from "../api";
import { clock } from "../util";
import { Logo } from "../components/Logo";

/** Room left of the pill for its shadow, in points. The same as PAD_LEFT in chip.rs. */
const PAD = 20;
/** The pill runs this far past the right edge of the window, which is the screen edge. */
const PAST_EDGE = 16;
/** The window width before the controls slide out. It shrinks to fit when they settle. */
const WIDE = 260;
/** Seconds with no sound before the chip says so, and how long the words then show. */
const SILENT_AFTER = 20;
const SILENT_SAYS = 8;
/** Stop needs a second click within this time. */
const DISARM_MS = 3000;
/** The controls close this long after the pointer leaves, so a diagonal exit does not flicker. */
const CLOSE_MS = 250;
/** One move of an arrow key, and with Shift, in points. */
const NUDGE = 8;
const NUDGE_BIG = 40;

type Tick = { elapsed_secs: number; level: number };

/** The recording chip (D53): the elapsed time and a red dot, docked to the right edge of the
 * screen while the Margin window is away. Stop and Notes slide out on hover or focus. It
 * owns no state: everything comes from the recorder's events, as in the main window. */
export function Chip() {
  const [shown, setShown] = useState(false);
  const [entering, setEntering] = useState(false);
  const [rec, setRec] = useState({ recording: false, stopping: false });
  const [tick, setTick] = useState<Tick>({ elapsed_secs: 0, level: 0 });
  const [silentFor, setSilentFor] = useState(0);
  const zeroFrom = useRef<number | null>(null);
  const [stopClicked, setStopClicked] = useState(false);
  const [armed, setArmed] = useState(false);
  const [hoverDom, setHoverDom] = useState(false);
  const [hoverNative, setHoverNative] = useState(false);
  const [keyFocus, setKeyFocus] = useState(false);
  const [open, setOpen] = useState(false);
  const [dragging, setDragging] = useState(false);
  const [said, setSaid] = useState("");
  const pill = useRef<HTMLDivElement>(null);
  const keyboard = useRef(false);
  const justDragged = useRef(false);

  // Keyboard or pointer, whichever was used last: focus from Tab opens the controls, and a
  // click must not keep them open.
  useEffect(() => {
    const key = () => (keyboard.current = true);
    const pointer = () => (keyboard.current = false);
    window.addEventListener("keydown", key, true);
    window.addEventListener("pointerdown", pointer, true);
    return () => {
      window.removeEventListener("keydown", key, true);
      window.removeEventListener("pointerdown", pointer, true);
    };
  }, []);

  const onTick = useCallback((t: Tick) => {
    setTick(t);
    // Silence is counted in recording time, not wall time, so it follows the recorder.
    if (t.level > 0) zeroFrom.current = null;
    else if (zeroFrom.current === null || zeroFrom.current > t.elapsed_secs) zeroFrom.current = t.elapsed_secs;
    setSilentFor(zeroFrom.current === null ? 0 : t.elapsed_secs - zeroFrom.current);
  }, []);

  useEffect(() => {
    const subs = [
      on<boolean>("chip-visible", setShown),
      on<boolean>("chip-pointer", setHoverNative),
      on<RecorderView>("recorder", (v) => setRec({ recording: v.recording, stopping: v.stopping })),
      on<Tick>("recorder-tick", onTick),
    ];
    // Asked after the listeners, so no change in between is lost.
    Promise.all(subs).then(() => {
      chipApi.hello().then(setShown).catch(() => {});
      api.recorderView().then((v) => {
        setRec({ recording: v.recording, stopping: v.stopping });
        setTick((t) => (t.elapsed_secs ? t : { elapsed_secs: v.elapsed_secs, level: v.level }));
      }).catch(() => {});
    });
    return () => subs.forEach((p) => p.then((u) => u()));
  }, [onTick]);

  // A new appearance starts fresh: the slide-in, then the dot pops.
  useEffect(() => {
    if (!shown) return;
    setEntering(true);
    const t = window.setTimeout(() => setEntering(false), 800);
    return () => window.clearTimeout(t);
  }, [shown]);
  useEffect(() => {
    if (rec.recording && !rec.stopping) setStopClicked(false);
  }, [rec.recording, rec.stopping]);

  const saving = stopClicked || rec.stopping || (!rec.recording && shown);
  const silent = !saving && silentFor >= SILENT_AFTER;
  const announce = silent && silentFor < SILENT_AFTER + SILENT_SAYS;
  const minutes = Math.floor(tick.elapsed_secs / 60);

  // Stop disarms by itself after a few seconds.
  useEffect(() => {
    if (!armed) return;
    const t = window.setTimeout(() => setArmed(false), DISARM_MS);
    return () => window.clearTimeout(t);
  }, [armed]);
  useEffect(() => {
    if (saving) setArmed(false);
  }, [saving]);

  // VoiceOver hears changes of state, not the clock.
  useEffect(() => {
    if (armed) setSaid("Click Stop again to stop the recording.");
  }, [armed]);
  useEffect(() => {
    if (silent) setSaid("Margin hears no sound. It is still recording.");
  }, [silent]);
  useEffect(() => {
    if (saving && shown) setSaid("Recording stopped. Saving.");
  }, [saving, shown]);

  // The window must be wide before the controls slide out, so they are never cut off.
  const want = !saving && !dragging && (hoverDom || hoverNative || keyFocus || armed);
  useEffect(() => {
    if (want) {
      let live = true;
      chipApi.width(WIDE).catch(() => {}).finally(() => live && setOpen(true));
      return () => {
        live = false;
      };
    }
    const t = window.setTimeout(() => {
      setOpen(false);
      // A click made Margin the active app. Give the keyboard back to the call.
      if (open && !keyboard.current && document.hasFocus()) {
        (document.activeElement as HTMLElement | null)?.blur();
        chipApi.releaseFocus().catch(() => {});
      }
    }, open && !dragging ? CLOSE_MS : 0);
    return () => window.clearTimeout(t);
  }, [want]); // eslint-disable-line react-hooks/exhaustive-deps

  // The window follows the pill: it grows at once and shrinks when the pill has settled.
  const width = useRef(0);
  const shrinkTimer = useRef(0);
  const fit = useCallback(() => {
    const el = pill.current;
    if (!el) return;
    const need = Math.ceil(el.offsetWidth - PAST_EDGE + PAD);
    window.clearTimeout(shrinkTimer.current);
    if (need > width.current) {
      width.current = need;
      chipApi.width(need).catch(() => {});
    } else if (need < width.current) {
      shrinkTimer.current = window.setTimeout(() => {
        width.current = need;
        chipApi.width(need).catch(() => {});
      }, 260);
    }
  }, []);
  useLayoutEffect(() => {
    const el = pill.current;
    if (!el) return;
    const ro = new ResizeObserver(fit);
    ro.observe(el);
    return () => ro.disconnect();
  }, [fit]);
  useEffect(() => {
    if (open) width.current = Math.max(width.current, WIDE);
    const t = window.setTimeout(fit, 320);
    return () => window.clearTimeout(t);
  }, [open, armed, saving, silent, announce, fit]);

  const openNotes = () => {
    if (saving || justDragged.current) return;
    chipApi.openNotes().catch(() => {});
  };
  const stop = () => {
    if (saving) return;
    if (!armed) {
      setArmed(true);
      return;
    }
    setArmed(false);
    setStopClicked(true);
    chipApi.stop().catch((e) => {
      // The backend also sends a notification, since the window is away.
      setSaid(`The recording was not saved. ${errorText(e)}`);
    });
  };

  // Drag along the edge: y only. Under 4 points of travel it is a click. The pointer is
  // followed in screen points, because the window moves under it.
  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0 || (e.target as HTMLElement).closest(".cbtn")) return;
    const y0 = e.screenY;
    let moved = false;
    let frame = 0;
    let dy = 0;
    const move = (ev: PointerEvent) => {
      dy = ev.screenY - y0;
      if (!moved && Math.abs(dy) < 4) return;
      if (!moved) {
        moved = true;
        setDragging(true);
        chipApi.dragStart().catch(() => {});
      }
      if (!frame) frame = requestAnimationFrame(() => {
        frame = 0;
        chipApi.drag(dy).catch(() => {});
      });
    };
    const up = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", up);
      window.removeEventListener("pointercancel", up);
      if (!moved) return;
      cancelAnimationFrame(frame);
      chipApi.drag(dy).then(() => chipApi.dragEnd()).catch(() => {});
      setDragging(false);
      justDragged.current = true;
      window.setTimeout(() => (justDragged.current = false), 0);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", up);
    window.addEventListener("pointercancel", up);
  };

  const onKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowUp" || e.key === "ArrowDown") {
      e.preventDefault();
      chipApi.nudge((e.key === "ArrowUp" ? -1 : 1) * (e.shiftKey ? NUDGE_BIG : NUDGE)).catch(() => {});
    } else if (e.key === "Escape") {
      keyboard.current = false;
      setKeyFocus(false);
      setArmed(false);
      (document.activeElement as HTMLElement | null)?.blur();
      chipApi.releaseFocus().catch(() => {});
    }
  };

  const msg = saving ? "Saving" : announce ? "No sound comes in" : "";
  const classes = [
    "chip",
    !shown && "off",
    entering && "entering",
    open && "open",
    dragging && "dragging",
    silent && "silent",
    msg && "has-msg",
    saving && "saving",
  ].filter(Boolean).join(" ");
  const lv = saving || silent ? 0 : Math.min(1, Math.sqrt(Math.max(0, tick.level) / 60));
  const faceLabel = saving
    ? "Saving the recording"
    : `Margin is recording, ${spoken(minutes)}${silent ? ". No sound comes in" : ""}. Open notes`;

  return (
    <div className="chip-stage">
      <div
        ref={pill}
        className={classes}
        role="group"
        aria-label="Margin recording"
        style={{ ["--lv" as string]: lv.toFixed(2) }}
        onPointerEnter={() => setHoverDom(true)}
        onPointerLeave={() => setHoverDom(false)}
        onPointerDown={onPointerDown}
        onKeyDown={onKeyDown}
        onFocus={() => keyboard.current && setKeyFocus(true)}
        onBlur={(e) => !e.currentTarget.contains(e.relatedTarget as Node | null) && setKeyFocus(false)}
      >
        <div className="reveal ctrls">
          <div>
            <button
              type="button"
              className={`cbtn stop${armed ? " armed" : ""}`}
              aria-label={armed ? "Click again to stop recording" : "Stop recording"}
              title={armed ? "Click again to stop" : "Stop recording"}
              onClick={stop}
            >
              {armed ? <span>Stop?</span> : <i />}
            </button>
            <button type="button" className="cbtn notes" aria-label="Open notes" title="Open notes" onClick={openNotes}>
              <Logo size={16} />
            </button>
            <span className="sep" />
          </div>
        </div>
        <div className="reveal msg" aria-hidden>
          <div><span>{msg}</span></div>
        </div>
        <button type="button" className="face" aria-label={faceLabel} title={saving ? "Saving" : "Open notes"} onClick={openNotes}>
          <svg className="warn" viewBox="0 0 16 16" aria-hidden>
            <path d="M8 1.8a2.3 2.3 0 0 0-2.3 2.3v3.7a2.3 2.3 0 0 0 4.6 0V4.1A2.3 2.3 0 0 0 8 1.8Z M3.8 7.6a4.2 4.2 0 0 0 8.4 0 M8 11.9v2.3 M2 2l12 12" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
          </svg>
          <span className="dot" aria-hidden />
          <span className="time" aria-hidden>{clock(tick.elapsed_secs)}</span>
        </button>
        <span className="sr" role="status" aria-live="polite">{said}</span>
      </div>
    </div>
  );
}

/** The time for VoiceOver, in whole minutes, so it changes once a minute. */
export function spoken(minutes: number): string {
  if (minutes < 1) return "under a minute";
  const h = Math.floor(minutes / 60);
  const m = minutes % 60;
  const part = (n: number, unit: string) => `${n} ${unit}${n === 1 ? "" : "s"}`;
  if (!h) return part(m, "minute");
  return m ? `${part(h, "hour")} ${part(m, "minute")}` : part(h, "hour");
}
