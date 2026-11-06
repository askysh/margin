import { useEffect, useState } from "react";

/** A labelled on/off switch with one line of help. */
export function ToggleRow({ title, help, checked, onChange }: { title: string; help: string; checked: boolean; onChange: (on: boolean) => void }) {
  return (
    <label className="set-toggle">
      <span className="set-toggle-text">
        <span className="set-toggle-title">{title}</span>
        <span className="set-help">{help}</span>
      </span>
      <input type="checkbox" role="switch" className="switch" checked={checked} onChange={(e) => onChange(e.target.checked)} />
    </label>
  );
}

export interface Choice<T extends string | number> {
  value: T;
  label: string;
}

/** A labelled drop-down. */
export function SelectRow<T extends string | number>({ id, label, help, value, choices, onChange }: {
  id: string;
  label: string;
  help?: string;
  value: T;
  choices: Choice<T>[];
  onChange: (v: T) => void;
}) {
  // A stored value the list doesn't offer (yet) still shows, rather than the first choice.
  const shown = choices.some((c) => c.value === value) ? choices : [...choices, { value, label: String(value) }];
  return (
    <div className="set-field">
      <label className="field-label" htmlFor={id}>{label}</label>
      <select
        id={id}
        className="field"
        value={String(value)}
        onChange={(e) => {
          const picked = shown.find((c) => String(c.value) === e.target.value);
          if (picked) onChange(picked.value);
        }}
      >
        {shown.map((c) => <option key={String(c.value)} value={String(c.value)}>{c.label}</option>)}
      </select>
      {help && <p className="set-help">{help}</p>}
    </div>
  );
}

/** A labelled text area that saves when it loses focus. */
export function TextRow({ id, label, help, value, placeholder, onSave }: {
  id: string;
  label: string;
  help: string;
  value: string;
  placeholder?: string;
  onSave: (text: string) => void;
}) {
  const [draft, setDraft] = useState(value);
  useEffect(() => setDraft(value), [value]);
  return (
    <div className="set-field">
      <label className="field-label" htmlFor={id}>{label}</label>
      <textarea
        id={id}
        className="field"
        rows={3}
        value={draft}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => draft !== value && onSave(draft)}
      />
      <p className="set-help">{help}</p>
    </div>
  );
}
