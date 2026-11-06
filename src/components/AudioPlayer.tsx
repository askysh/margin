import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import { convertFileSrc } from "@tauri-apps/api/core";
import { api } from "../api";
import { clock } from "../util";

export interface AudioHandle {
  /** Jump to `secs` and play from there. */
  playFrom: (secs: number) => void;
}

interface Props {
  path: string;
  /** Called as playback moves (a few times a second). */
  onTime: (secs: number) => void;
}

/** A slim player for the meeting's recording: play/pause, time and a scrubber. */
export const AudioPlayer = forwardRef<AudioHandle, Props>(function AudioPlayer({ path, onTime }, ref) {
  const audio = useRef<HTMLAudioElement>(null);
  const [src, setSrc] = useState<string | null>(null);
  const [failed, setFailed] = useState(false);
  const [playing, setPlaying] = useState(false);
  const [time, setTime] = useState(0);
  const [duration, setDuration] = useState(0);

  useEffect(() => {
    let live = true;
    api.meetingAudio(path).then((p) => live && setSrc(convertFileSrc(p)), () => live && setFailed(true));
    return () => {
      live = false;
    };
  }, [path]);

  useImperativeHandle(ref, () => ({
    playFrom(secs: number) {
      const el = audio.current;
      if (!el) return;
      el.currentTime = Math.max(0, secs);
      el.play().catch(() => setFailed(true));
    },
  }));

  if (failed) return <p className="player-note">This recording can't be played here. Use Show in Finder to open it.</p>;
  if (!src) return null;

  const toggle = () => {
    const el = audio.current;
    if (!el) return;
    if (el.paused) el.play().catch(() => setFailed(true));
    else el.pause();
  };
  const moved = () => {
    const t = audio.current?.currentTime ?? 0;
    setTime(t);
    onTime(t);
  };

  return (
    <div className="player">
      <button className="play-btn" onClick={toggle} aria-label={playing ? "Pause recording" : "Play recording"}>
        {playing ? (
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden><path d="M3 2h2v8H3zM7 2h2v8H7z" fill="currentColor" /></svg>
        ) : (
          <svg width="12" height="12" viewBox="0 0 12 12" aria-hidden><path d="M3 1.8v8.4L10 6z" fill="currentColor" /></svg>
        )}
      </button>
      <span className="player-time mono">{clock(time)} / {clock(duration)}</span>
      <input
        className="scrub"
        type="range"
        min={0}
        max={duration || 0}
        step={0.1}
        value={Math.min(time, duration || 0)}
        onChange={(e) => {
          if (audio.current) audio.current.currentTime = Number(e.target.value);
          moved();
        }}
        aria-label="Position in recording"
        style={{ ["--pos" as string]: duration ? `${(time / duration) * 100}%` : "0%" }}
      />
      <audio
        ref={audio}
        src={src}
        preload="metadata"
        onLoadedMetadata={(e) => setDuration(Number.isFinite(e.currentTarget.duration) ? e.currentTarget.duration : 0)}
        onTimeUpdate={moved}
        onPlay={() => setPlaying(true)}
        onPause={() => setPlaying(false)}
        onEnded={() => setPlaying(false)}
        onError={() => setFailed(true)}
      />
    </div>
  );
});
