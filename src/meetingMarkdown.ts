import type { ActionItem, MeetingDetail } from "./api";
import { meetingDate } from "./util";

/** Markdown for each part of a meeting, as Copy puts it on the clipboard. Speaker labels
 * use the names the meeting view shows, so "Speaker 2" never becomes SPEAKER_1. */

export const ACTION_HEADING = "Action Items";

export function summaryMarkdown(m: MeetingDetail): string {
  return m.sections
    .map((s) => ({ ...s, body: withoutCoverage(s.body).body }))
    .filter((s) => s.body.trim())
    .map((s) => `## ${s.heading}\n\n${s.body.trim()}`)
    .join("\n\n");
}

/** Core ends a summary by the claude agent with a delivery report, "> Agent input coverage:
 * complete. 1/1 source chunks; …" (summarize.rs, SummaryCoverage). It is a debug record, so
 * the meeting view and Copy leave it out. The meeting file keeps it. `incomplete` is true
 * when part of the meeting did not reach the summarizer. */
const COVERAGE = /^>\s*Agent input coverage: (complete|incomplete)\./;

export function withoutCoverage(body: string): { body: string; incomplete: boolean } {
  let incomplete = false;
  let inReport = false;
  const kept: string[] = [];
  for (const line of body.split("\n")) {
    const report = COVERAGE.exec(line);
    if (report) {
      incomplete ||= report[1] === "incomplete";
      inReport = true;
      continue;
    }
    // A long report can wrap onto more quoted lines.
    if (inReport && line.startsWith(">")) continue;
    inReport = false;
    kept.push(line);
  }
  return { body: kept.join("\n").trimEnd(), incomplete };
}

export function actionItemsMarkdown(items: ActionItem[]): string {
  return items
    .map((i) => {
      const who = i.assignee ? `@${i.assignee}: ` : "";
      const due = i.due ? ` (due ${i.due})` : "";
      return `- [${i.done ? "x" : " "}] ${who}${i.task}${due}`;
    })
    .join("\n");
}

export function transcriptMarkdown(m: MeetingDetail, names: Map<string, string>): string {
  return m.transcript
    .map((t) => {
      const who = t.speaker ? `${names.get(t.speaker) ?? t.speaker} ` : "";
      return `[${who}${t.ts}] ${t.text}`;
    })
    .join("\n");
}

/** The whole meeting: title, date line, summary, notes and transcript. No frontmatter. */
export function meetingMarkdown(m: MeetingDetail, names: Map<string, string>, notes: string): string {
  const meta = [meetingDate(m.date), m.duration, m.attendees.join(", ")].filter(Boolean).join(" · ");
  const parts = [`# ${m.title}`, meta, summaryMarkdown(m)];
  if (notes.trim()) parts.push(`## Notes\n\n${notes.trim()}`);
  if (m.transcript.length) parts.push(`## Transcript\n\n${transcriptMarkdown(m, names)}`);
  return parts.filter((p) => p.trim()).join("\n\n") + "\n";
}

/** "1:02" or "1:02:03" → seconds. */
export function tsSeconds(ts: string): number {
  return ts.split(":").reduce((acc, part) => acc * 60 + (Number(part) || 0), 0);
}
