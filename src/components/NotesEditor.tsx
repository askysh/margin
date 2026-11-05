import { useLayoutEffect, useRef } from "react";

interface Props {
  value: string;
  onChange: (v: string) => void;
  placeholder?: string;
  autoFocus?: boolean;
  readOnly?: boolean;
}

/** A plain-Markdown notepad that grows with its text, so the page scrolls instead of a
 * box inside it. Enter on a list line continues the list. */
export function NotesEditor({ value, onChange, placeholder, autoFocus, readOnly }: Props) {
  const ref = useRef<HTMLTextAreaElement>(null);

  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    el.style.height = "0px";
    el.style.height = `${Math.max(el.scrollHeight, 240)}px`;
  }, [value]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key !== "Enter" || e.shiftKey || e.metaKey || e.nativeEvent.isComposing) return;
    const el = e.currentTarget;
    const before = el.value.slice(0, el.selectionStart);
    const lineStart = before.lastIndexOf("\n") + 1;
    const line = before.slice(lineStart);
    const m = line.match(/^(\s*)([-*•]|\d+\.)(\s+\[[ x]\])?\s/);
    if (!m) return;
    e.preventDefault();
    if (line.trim() === m[0].trim()) {
      // Enter on an empty bullet ends the list.
      const next = el.value.slice(0, lineStart) + el.value.slice(el.selectionStart);
      onChange(next);
      requestAnimationFrame(() => el.setSelectionRange(lineStart, lineStart));
      return;
    }
    const marker = /\d+\./.test(m[2]) ? `${parseInt(m[2]) + 1}.` : m[2];
    const insert = `\n${m[1]}${marker}${m[3] ? " [ ]" : ""} `;
    const pos = el.selectionStart + insert.length;
    onChange(el.value.slice(0, el.selectionStart) + insert + el.value.slice(el.selectionEnd));
    requestAnimationFrame(() => el.setSelectionRange(pos, pos));
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
