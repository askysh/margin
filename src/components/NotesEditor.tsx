import { useLayoutEffect, useRef } from "react";

interface Props {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  readOnly?: boolean;
}

/** A plain-Markdown notepad that grows with its text, so the page scrolls instead of a
 * box inside it. Enter on a list line continues the list; Enter on an empty item ends it. */
export function NotesEditor({ value, onChange, placeholder, autoFocus, readOnly }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);
  // Where the caret belongs after a programmatic edit. It is applied in the same commit
  // as the new value, so keys typed immediately afterwards land in the right place.
  const caret = useRef<number | null>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    if (caret.current !== null) {
      el.setSelectionRange(caret.current, caret.current);
      caret.current = null;
    }
    fit(el);
  }, [value]);

  // A narrower column wraps more lines, so the box must grow then too, or the end of the
  // notes is cut off. Only a change of width fits it again: fit() changes the height.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    let width = el.getBoundingClientRect().width;
    let frame = 0;
    const observer = new ResizeObserver(() => {
      const now = el.getBoundingClientRect().width;
      if (now === width) return;
      width = now;
      // In the next frame: a height change inside the callback is a ResizeObserver loop.
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(() => fit(el));
    });
    observer.observe(el);
    return () => {
      observer.disconnect();
      cancelAnimationFrame(frame);
    };
  }, []);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.metaKey || e.altKey || e.nativeEvent.isComposing) return;
    const el = e.currentTarget;
    if (el.selectionStart !== el.selectionEnd) return;
    const at = el.selectionStart;
    const lineStart = el.value.lastIndexOf("\n", at - 1) + 1;
    const line = el.value.slice(lineStart, at);
    const m = line.match(/^(\s*)([-*•]|\d+\.)(\s+\[[ xX]\])?\s/);
    if (!m) return;
    e.preventDefault();
    if (line.trim() === m[0].trim()) {
      // Enter on an empty item ends the list.
      caret.current = lineStart;
      onChange(el.value.slice(0, lineStart) + el.value.slice(at));
      return;
    }
    const marker = /\d+\./.test(m[2]) ? `${parseInt(m[2]) + 1}.` : m[2];
    const insert = `\n${m[1]}${marker}${m[3] ? " [ ]" : ""} `;
    caret.current = at + insert.length;
    onChange(el.value.slice(0, at) + insert + el.value.slice(at));
  };

  return (
    <textarea
      ref={ref}
      className="notes"
      value={value}
      onChange={(e) => onChange(e.target.value)}
      onKeyDown={onKeyDown}
      placeholder={placeholder}
      autoFocus={autoFocus}
      readOnly={readOnly}
      spellCheck
      aria-label="Your notes"
    />
  );
}

/** Sets the box to the height of its text, at least 240 px, and keeps the page's scroll. */
function fit(el: HTMLTextAreaElement) {
  const scroller = el.closest(".notes-col");
  const keep = scroller?.scrollTop ?? 0;
  el.style.height = "0px";
  el.style.height = `${Math.max(el.scrollHeight, 240)}px`;
  if (scroller) scroller.scrollTop = keep;
}
