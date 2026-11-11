import { useEffect, useState } from "react";
import { api, errorText, type ActionItem } from "../api";
import { actionItemsMarkdown } from "../meetingMarkdown";
import { copyText } from "../util";

interface Props {
  path: string;
  items: ActionItem[];
  /** Display names for raw speaker labels used as assignees. */
  names: Map<string, string>;
  locked: boolean;
  notify: (t: string, tone?: "info" | "error") => void;
  onChanged: () => void;
}

/** The meeting's action items as a checklist. Ticking one writes it back to the file
 * (frontmatter status and the `- [ ]` box), so Obsidian shows the same state. */
export function ActionItems({ path, items, names, locked, notify, onChanged }: Props) {
  const [shown, setShown] = useState(items);
  useEffect(() => setShown(items), [items]);

  const toggle = async (index: number) => {
    const done = !shown[index].done;
    setShown((list) => list.map((it, i) => (i === index ? { ...it, done } : it)));
    try {
      await api.setActionItem(path, index, done, shown[index].task);
      onChanged();
    } catch (e) {
      setShown(items);
      notify(errorText(e), "error");
      // The list may have changed under the tick: show the one in the file.
      onChanged();
    }
  };

  const named = shown.map((i) => ({ ...i, assignee: i.assignee ? names.get(i.assignee) ?? i.assignee : null }));
  const copy = async () => notify((await copyText(actionItemsMarkdown(named))) ? "Action items copied." : "Couldn't copy to the clipboard.");

  return (
    <section className="summary-block action-items" aria-label="Action items">
      <h2 className="eyebrow">
        Action items
        <button className="link-btn eyebrow-btn" onClick={copy}>Copy</button>
      </h2>
      <ul>
        {named.map((item, i) => (
          <li key={i} className={item.done ? "done" : undefined}>
            <button
              className={`md-box ai-box ${item.done ? "done" : ""}`}
              role="checkbox"
              aria-checked={item.done}
              aria-label={item.task}
              disabled={locked}
              onClick={() => toggle(i)}
            />
            <span className="ai-text">
              <span className="ai-task">{item.task}</span>
              {(item.assignee || item.due) && (
                <span className="ai-meta">{[item.assignee, item.due && `due ${item.due}`].filter(Boolean).join(" · ")}</span>
              )}
            </span>
          </li>
        ))}
      </ul>
    </section>
  );
}
