"""Build long synthetic meetings with `say`: mixed .wav, .voice.wav/.system.wav stems, and a reference JSONL.
usage: gen_corpus.py <out_dir> <minutes> <docs...>"""
import os, re, sys, random, subprocess, wave, struct, json, array, concurrent.futures as cf
out, minutes, docs = sys.argv[1], float(sys.argv[2]), sys.argv[3:]
random.seed(7)
text = []
for d in docs:
    s = open(d).read()
    s = re.sub(r"```.*?```", " ", s, flags=re.S)
    for line in s.splitlines():
        line = line.strip()
        if not line or line.startswith(("|", "#", ">", "<", "─", "┌", "│", "└", "├")): continue
        line = re.sub(r"`[^`]*`", "", line)
        line = re.sub(r"\[([^\]]*)\]\([^)]*\)", r"\1", line)
        line = re.sub(r"[*_~>]|^\s*[-\d.]+\s", " ", line)
        text.append(line)
sents = []
for chunk in re.split(r"(?<=[.!?])\s+", " ".join(text)):
    w = chunk.split()
    if 6 <= len(w) <= 35 and re.fullmatch(r"[A-Za-z0-9 ,.'’;:()?!-]+", chunk) and sum(c.isalpha() for c in chunk) > 0.75*len(chunk):
        sents.append(re.sub(r"\s+", " ", chunk).strip().lstrip("-: "))
seen=set(); sents=[s for s in sents if not (s in seen or seen.add(s))]
print(len(sents), "sentences", file=sys.stderr)
voices = ["Samantha", "Daniel", "Karen", "Rishi"]  # speaker 0 is "You" (mic stem)
SR = 16000
target = int(minutes*60*SR)
os.makedirs(out+"/utt", exist_ok=True)
# plan utterances
plan=[]; i=0; spk=0
est=0
while est < target and i < len(sents):
    n = random.choice([1,1,2,2,3])
    txt = " ".join(sents[i:i+n]); i += n
    plan.append((spk, txt))
    est += int(len(txt.split())/2.6*SR) + SR
    spk = random.choice([s for s in range(4) if s != spk]) if random.random() < 0.85 else spk
def render(k):
    spk, txt = plan[k]
    f = f"{out}/utt/{k:05d}.wav"
    if not os.path.exists(f):
        subprocess.run(["say", "-v", voices[spk], "-r", str(random.Random(k).choice([175,185,195])), "-o", f, "--file-format=WAVE", "--data-format=LEI16@16000", txt], check=True)
    with wave.open(f) as w: return array.array("h", w.readframes(w.getnframes()))
with cf.ThreadPoolExecutor(6) as ex: pcm = list(ex.map(render, range(len(plan))))
voice = array.array("h"); system = array.array("h"); ref=[]
def pad(n):
    voice.extend([0]*n); system.extend([0]*n)
pos = 0
for (spk, txt), a in zip(plan, pcm):
    r = random.random()
    gap = random.uniform(0.25, 1.2) if r < 0.8 else (random.uniform(2, 6) if r < 0.97 else random.uniform(10, 25))
    pad(int(gap*SR))
    start = len(voice)
    if len(voice) + len(a) > target: break
    (voice if spk == 0 else system).extend(a)
    (system if spk == 0 else voice).extend([0]*len(a))
    ref.append({"offset_ms": start*1000//SR, "duration_ms": len(a)*1000//SR, "speaker": spk, "text": txt})
pad(target - len(voice))
rng = random.Random(1)
def noisy(x): return array.array("h", (max(-32768, min(32767, v + rng.randint(-6, 6))) for v in x))
voice, system = noisy(voice), noisy(system)
mixed = array.array("h", (max(-32768, min(32767, a+b)) for a, b in zip(voice, system)))
name = f"{out}/meet{int(minutes)}"
for suffix, data in [("", mixed), (".voice", voice), (".system", system)]:
    with wave.open(name+suffix+".wav", "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR); w.writeframes(data.tobytes())
with open(name+".ref.jsonl", "w") as f:
    for r in ref: f.write(json.dumps(r)+"\n")
print(name, len(ref), "utterances", sum(len(r["text"].split()) for r in ref), "words", file=sys.stderr)
