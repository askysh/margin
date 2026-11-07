import { useEffect, useState, type ReactNode } from "react";

/** The title block of a settings section: a name and one line about what it holds. */
export function SectionHead({ title, about, danger }: { title: string; about?: string; danger?: boolean }) {
  return (
    <header className={`set-head${danger ? " danger" : ""}`}>
      <h1 id="set-title" tabIndex={-1}>{title}</h1>
      {about && <p>{about}</p>}
    </header>
  );
}

/** A group of rows on one card, with an optional heading above and a note below. `plain`
 * drops the card for content that is already a set of boxes, such as a list of choices. */
export function Card({ title, note, danger, plain, children }: { title?: string; note?: ReactNode; danger?: boolean; plain?: boolean; children: ReactNode }) {
  return (
    <section className="set-group">
      {title && <h2 className="set-group-title">{title}</h2>}
      <div className={`set-card${danger ? " danger" : ""}${plain ? " plain" : ""}`}>{children}</div>
      {note && <p className="set-note">{note}</p>}
    </section>
  );
}

/** Label and help on the left, the control on the right. `htmlFor` names the control. */
export function Row({ title, help, htmlFor, children }: { title: ReactNode; help?: ReactNode; htmlFor?: string; children?: ReactNode }) {
  const Title = htmlFor ? "label" : "span";
  return (
    <div className="set-row">
      <span className="set-row-text">
        <Title className="set-row-title" htmlFor={htmlFor}>{title}</Title>
        {help && <span className="set-help" id={htmlFor && `${htmlFor}-help`}>{help}</span>}
      </span>
      {children && <span className="set-row-control">{children}</span>}
    </div>
  );
}

/** A labelled on/off switch with one line of help. The whole row is the label. */
export function ToggleRow({ title, help, checked, onChange, disabled }: { title: string; help: ReactNode; checked: boolean; onChange: (on: boolean) => void; disabled?: boolean }) {
  return (
    <label className={`set-row set-toggle${disabled ? " set-toggle-off" : ""}`}>
      <span className="set-row-text">
        <span className="set-row-title">{title}</span>
        <span className="set-help">{help}</span>
      </span>
      <span className="set-row-control">
        <input type="checkbox" role="switch" className="switch" checked={checked} disabled={disabled} onChange={(e) => onChange(e.target.checked)} />
      </span>
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
    <Row title={label} help={help} htmlFor={id}>
      <select
        id={id}
        className="field set-select"
        aria-describedby={help ? `${id}-help` : undefined}
        value={String(value)}
        onChange={(e) => {
          const picked = shown.find((c) => String(c.value) === e.target.value);
          if (picked) onChange(picked.value);
        }}
      >
        {shown.map((c) => <option key={String(c.value)} value={String(c.value)}>{c.label}</option>)}
      </select>
    </Row>
  );
}

/** A labelled text area under its label, saved when it loses focus. */
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
    <div className="set-row set-row-stacked">
      <span className="set-row-text">
        <label className="set-row-title" htmlFor={id}>{label}</label>
        <span className="set-help" id={`${id}-help`}>{help}</span>
      </span>
      <textarea
        id={id}
        aria-describedby={`${id}-help`}
        className="field"
        rows={3}
        value={draft}
        placeholder={placeholder}
        spellCheck={false}
        onChange={(e) => setDraft(e.target.value)}
        onBlur={() => draft !== value && onSave(draft)}
      />
    </div>
  );
}
