"""WER and speaker purity of a transcript against a reference JSONL.
usage: eval.py <ref.jsonl> <meeting.md | live.jsonl> [--max-ms N]"""
import sys, re, json, collections
ref_path, hyp_path = sys.argv[1], sys.argv[2]
max_ms = int(sys.argv[sys.argv.index("--max-ms")+1]) if "--max-ms" in sys.argv else None
refs = [json.loads(l) for l in open(ref_path)]
if max_ms: refs = [r for r in refs if r["offset_ms"] + r["duration_ms"] <= max_ms]
def norm(t):
    t = t.lower().replace("’", "'")
    t = re.sub(r"[^a-z0-9' ]+", " ", t)
    return t.split()
lines = []  # (ms, label, text)
if hyp_path.endswith(".jsonl"):
    for l in open(hyp_path):
        if l.strip():
            j = json.loads(l); lines.append((j.get("offset_ms", 0), j.get("speaker") or "?", j["text"]))
else:
    s = open(hyp_path).read()
    s = s[s.find("## Transcript"):]
    for m in re.finditer(r"(?m)^\[([^\]]+?) (\d+):(\d+)(?::(\d+))?\]\s*(?:\[[A-Z]+\]\s*)?(.*)$", s):
        lab, a, b, c, text = m.groups()
        ms = ((int(a)*3600 + int(b)*60 + int(c)) if c else (int(a)*60 + int(b))) * 1000
        lines.append((ms, lab, text))
R = [w for r in refs for w in norm(r["text"])]
H = [w for _, _, t in lines for w in norm(t)]
# word-level edit distance (banded would be faster; plain DP on ints is fine for 10k words)
def wer(R, H):
    import array
    prev = array.array("i", range(len(H)+1))
    for i, rw in enumerate(R, 1):
        cur = array.array("i", [i]) * (len(H)+1)
        for j, hw in enumerate(H, 1):
            cur[j] = min(prev[j]+1, cur[j-1]+1, prev[j-1] + (rw != hw))
        prev = cur
    return prev[len(H)] / max(1, len(R))
# speaker purity: each hyp line -> ref speaker at its start time; majority mapping per label
def ref_at(ms):
    # [m:ss] stamps are truncated: the line starts in [ms, ms+1000).
    best = min(refs, key=lambda r: abs(r["offset_ms"] - (ms + 500)), default=None)
    if best is None or abs(best["offset_ms"] - (ms + 500)) > 3000:
        inside = [r for r in refs if r["offset_ms"] <= ms <= r["offset_ms"] + r["duration_ms"]]
        return inside[0]["speaker"] if inside else None
    return best["speaker"]
counts = collections.defaultdict(collections.Counter)
for ms, lab, t in lines:
    spk = ref_at(ms)
    if spk is not None: counts[lab][spk] += len(norm(t))
tot = sum(sum(c.values()) for c in counts.values())
pure = sum(max(c.values()) for c in counts.values())
out = {"ref_words": len(R), "hyp_words": len(H), "lines": len(lines),
       "speaker_purity": round(pure/max(1,tot), 3), "labels": {k: dict(v) for k, v in counts.items()}}
if "--no-wer" not in sys.argv:
    out["wer"] = round(wer(R, H), 4)
print(json.dumps(out))
