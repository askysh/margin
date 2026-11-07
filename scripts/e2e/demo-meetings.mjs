// A demo library for screenshots: meeting files as Margin writes them (frontmatter, then
// Summary, Notes and Transcript sections). The first meeting is the one shown open.
// Dates are days before today at set clock times, so the sidebar groups read "Today",
// "Yesterday" and so on. The open meeting is today, just before now.

const ME = "Maya Chen";

export const MEETINGS = [
  {
    title: "Weekly product sync",
    day: 0,
    duration: "32m 10s",
    people: [ME, "Daniel Okafor"],
    summary: [
      "The beta moves to Thursday so the import fix can ship with it.",
      "Onboarding drop-off is mostly at the permissions step; the copy gets one more pass.",
      "Pricing page stays as it is until the survey results are in.",
    ],
    actions: [
      { who: "Daniel Okafor", task: "Share the import fix build with the beta group", due: "Wednesday" },
      { who: ME, task: "Rewrite the permissions step copy", due: "Friday" },
    ],
    notes: [
      "- beta → Thursday, not Tuesday",
      "- import bug: Daniel has a fix, needs one more review",
      "- drop-off is at permissions (about 30%)",
      "  - say why before the macOS prompt shows",
      "- pricing: wait for the survey",
    ],
    transcript: [
      ["Maya Chen", "0:04", "Okay, let's start with the beta. Are we still on for Tuesday?"],
      ["Daniel Okafor", "0:09", "I'd push it to Thursday. The import fix is done, but I want one more review before it goes out."],
      ["Maya Chen", "0:18", "Thursday works. Better one good build than two rushed ones."],
      ["Daniel Okafor", "0:24", "Agreed. I'll share it with the beta group on Wednesday so people can try it first."],
      ["Maya Chen", "0:33", "Next, onboarding. The funnel shows most people leave at the permissions step."],
      ["Daniel Okafor", "0:41", "About thirty percent. I think the macOS prompt comes as a surprise."],
      ["Maya Chen", "0:48", "So we explain why before the prompt shows. I'll rewrite that copy by Friday."],
      ["Daniel Okafor", "0:57", "And pricing? Do we change the page this week?"],
      ["Maya Chen", "1:02", "Let's wait for the survey. It closes on Monday, and then we decide with real numbers."],
      ["Daniel Okafor", "1:10", "Sounds good. That's everything on my list."],
    ],
  },
  { title: "Design review: onboarding", day: 1, at: "15:00", duration: "45m", people: [ME, "Lena Fischer"], summary: ["Three layouts for the setup steps; the single-column one wins."] },
  { title: "Customer call with Northwind", day: 1, at: "11:00", duration: "28m", people: [ME, "Sam Patel"], summary: ["They want export to their wiki and a shared folder for the team."] },
  { title: "1:1 with Daniel", day: 2, at: "16:30", duration: "24m", people: [ME, "Daniel Okafor"], summary: ["The beta, and the move to the platform team next month."] },
  { title: "Hiring sync", day: 3, at: "14:00", duration: "18m", people: [ME, "Priya Raman"], summary: ["Two designer candidates go to the final round."] },
  { title: "Quarterly planning", day: 4, at: "10:00", duration: "1h 12m", people: [ME, "Lena Fischer", "Daniel Okafor"], summary: ["Three goals for the quarter: retention, the Windows build, pricing."] },
  { title: "Pricing workshop", day: 9, at: "15:30", duration: "55m", people: [ME, "Sam Patel"], summary: ["Two plans, not three. A free tier with a meeting limit."] },
].map((m) => ({ ...m, iso: dated(m.day, m.at), file: "" })).map((m) => ({ ...m, file: `${m.iso.slice(0, 10)}-${slug(m.title)}.md` }));

export function demoFile(m) {
  const fm = ["---", `title: ${JSON.stringify(m.title)}`, "type: meeting", `date: ${m.iso}`, `duration: ${m.duration}`, "attendees:", ...m.people.map((p) => `- ${p}`)];
  if (m.actions) {
    fm.push("action_items:");
    for (const a of m.actions) fm.push(`- assignee: ${a.who}`, `  task: ${a.task}`, `  due: ${a.due}`, "  status: open");
  }
  fm.push("---", "");
  const body = ["## Summary", "", ...m.summary.map((s) => `- ${s}`), ""];
  if (m.actions) body.push("## Action Items", "", ...m.actions.map((a) => `- [ ] @${a.who}: ${a.task} (due ${a.due})`), "");
  body.push("## Notes", "", ...(m.notes ?? [`- ${m.summary[0]}`]), "");
  const turns = m.transcript ?? [[ME, "0:03", m.summary[0]]];
  body.push("## Transcript", "", ...turns.map(([who, ts, text]) => `[${who} ${ts}] ${text}`), "");
  return [...fm, ...body].join("\n");
}

/** A local ISO time with its offset, as core writes it. Without `at`: the last half hour
 * that started at least an hour ago, so the open meeting is the newest one. */
function dated(day, at) {
  const d = new Date(Date.now() - (at ? 0 : 3600e3));
  d.setDate(d.getDate() - day);
  if (at) d.setHours(...at.split(":").map(Number), 0, 0);
  else d.setMinutes(d.getMinutes() < 30 ? 0 : 30, 0, 0);
  const off = -d.getTimezoneOffset();
  const pad = (n) => String(Math.abs(n)).padStart(2, "0");
  const local = new Date(d.getTime() + off * 60e3).toISOString().slice(0, 19);
  return `${local}${off >= 0 ? "+" : "-"}${pad(Math.trunc(off / 60))}:${pad(off % 60)}`;
}

function slug(s) {
  return s.toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-|-$/g, "");
}
