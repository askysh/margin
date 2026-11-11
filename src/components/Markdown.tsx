import { Fragment, type ReactNode } from "react";

/** Turns a link target into a click handler, or null to show the link as plain text. */
export type LinkHandler = (href: string) => (() => void) | null;

/** A deliberately small Markdown renderer for summaries and Ask answers. It handles
 * headings, lists, task items, quotes, paragraphs and inline bold, italic, code and links.
 * It builds React nodes, so it never injects HTML. Links are plain text unless `onLink`
 * makes them clickable. */
export function Markdown({ source, onLink }: { source: string; onLink?: LinkHandler }) {
  const inline = (text: string) => inlineNodes(text, onLink);
  const blocks: ReactNode[] = [];
  const lines = source.replace(/\r/g, "").split("\n");
  let i = 0;
  while (i < lines.length) {
    const line = lines[i];
    if (!line.trim()) {
      i++;
      continue;
    }
    // [\s\S], not ".": a dot does not match U+2028 or U+2029, which can be in a title.
    const h = line.match(/^(#{1,6})\s+([\s\S]*)$/);
    if (h) {
      blocks.push(<h4 key={i} className="md-h">{inline(h[2])}</h4>);
      i++;
      continue;
    }
    if (/^>\s?/.test(line)) {
      const quote: string[] = [];
      while (i < lines.length && /^>\s?/.test(lines[i])) quote.push(lines[i++].replace(/^>\s?/, ""));
      blocks.push(<blockquote key={`q${i}`}>{inline(quote.join(" "))}</blockquote>);
      continue;
    }
    if (/^\s*([-*+]|\d+\.)\s+/.test(line)) {
      const items: ReactNode[] = [];
      const ordered = /^\s*\d+\./.test(line);
      while (i < lines.length && /^\s*([-*+]|\d+\.)\s+/.test(lines[i])) {
        const raw = lines[i].replace(/^\s*([-*+]|\d+\.)\s+/, "");
        const task = raw.match(/^\[( |x|X)\]\s+(.*)$/);
        items.push(
          <li key={i} className={task ? "md-task" : undefined}>
            {task && <span className={`md-box ${task[1] !== " " ? "done" : ""}`} aria-hidden />}
            {inline(task ? task[2] : raw)}
          </li>,
        );
        i++;
      }
      blocks.push(ordered ? <ol key={`l${i}`}>{items}</ol> : <ul key={`l${i}`}>{items}</ul>);
      continue;
    }
    // The first line always goes in, so a line that no rule above took can't stop the loop.
    const para: string[] = [line.trim()];
    i++;
    while (i < lines.length && lines[i].trim() && !/^(#{1,6}\s|>|\s*([-*+]|\d+\.)\s)/.test(lines[i])) {
      para.push(lines[i].trim());
      i++;
    }
    blocks.push(<p key={`p${i}`}>{inline(para.join(" "))}</p>);
  }
  return <div className="md">{blocks}</div>;
}

function inlineNodes(text: string, onLink?: LinkHandler): ReactNode {
  const parts = text.split(/(\*\*[^*]+\*\*|`[^`]+`|\*[^*\s][^*]*\*|_[^_\s][^_]*_|\[[^\]]+\]\([^)]+\))/g);
  return parts.map((p, i) => {
    if (/^\*\*[^*]+\*\*$/.test(p)) return <strong key={i}>{p.slice(2, -2)}</strong>;
    if (/^`[^`]+`$/.test(p)) return <code key={i}>{p.slice(1, -1)}</code>;
    if (/^(\*[^*]+\*|_[^_]+_)$/.test(p)) return <em key={i}>{p.slice(1, -1)}</em>;
    const link = p.match(/^\[([^\]]+)\]\(([^)]+)\)$/);
    const open = link && onLink?.(link[2]);
    if (link && open) return <button key={i} className="md-link md-link-btn" onClick={open}>{link[1]}</button>;
    if (link) return <span key={i} className="md-link" title={link[2]}>{link[1]}</span>;
    return <Fragment key={i}>{p}</Fragment>;
  });
}
