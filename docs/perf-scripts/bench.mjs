// node bench.mjs <port> [runs]: latency of the main bridge commands.
const port = process.argv[2], runs = Number(process.argv[3] || 20);
const inv = async (cmd, args = {}) => {
  const t = performance.now();
  const r = await fetch(`http://127.0.0.1:${port}/invoke`, { method: "POST", body: JSON.stringify({ cmd, args }) });
  const body = await r.json();
  if (!body.ok) throw new Error(`${cmd}: ${body.error}`);
  return { ms: performance.now() - t, value: body.value, bytes: JSON.stringify(body.value).length };
};
const stats = (xs) => { xs.sort((a, b) => a - b); return { median: +xs[Math.floor(xs.length / 2)].toFixed(1), p95: +xs[Math.floor(xs.length * 0.95)].toFixed(1), min: +xs[0].toFixed(1) }; };
const list = (await inv("list_meetings")).value;
const path = list[Math.floor(list.length / 2)].path;
const cases = [
  ["boot", {}], ["list_meetings", {}], ["list_jobs", {}], ["read_meeting", { path }],
  ["search_meetings", { query: "legal review" }], ["search_meetings", { query: "zebra unicorn" }],
  ["models_status", {}], ["permissions_state", {}],
];
console.log(`meetings: ${list.length}`);
for (const [cmd, args] of cases) {
  const xs = []; let bytes = 0;
  for (let i = 0; i < runs; i++) { const r = await inv(cmd, args); xs.push(r.ms); bytes = r.bytes; }
  console.log(cmd.padEnd(18), JSON.stringify(args).slice(0, 30).padEnd(30), JSON.stringify(stats(xs)), `${(bytes / 1024).toFixed(1)} KB`);
}
