"""Make a harder, more meeting-like variant of a generated meeting.
Adds: short backchannels ("Yeah.", "Right."), far-away babble and a noise floor on the mic stem,
speaker-to-mic bleed of the system stem, and a call-codec noise floor on the system stem.
usage: gen_hard.py <audio_dir> <name e.g. meet60>"""
import sys, os, json, subprocess, wave, numpy as np
d, name = sys.argv[1], sys.argv[2]
SR = 16000
rng = np.random.default_rng(11)
def rd(p):
    with wave.open(p) as w: return np.frombuffer(w.readframes(w.getnframes()), dtype=np.int16).astype(np.float32) / 32768
def wr(p, x):
    with wave.open(p, "wb") as w:
        w.setnchannels(1); w.setsampwidth(2); w.setframerate(SR)
        w.writeframes((np.clip(x, -1, 1) * 32767).astype(np.int16).tobytes())
voice, system = rd(f"{d}/{name}.voice.wav"), rd(f"{d}/{name}.system.wav")
refs = [json.loads(l) for l in open(f"{d}/{name}.ref.jsonl")]
voices = ["Samantha", "Daniel", "Karen", "Rishi"]
bc_words = ["Yeah.", "Right.", "Okay.", "Mm-hmm.", "Sure.", "Got it.", "Yes.", "Exactly.", "Hmm, okay."]
os.makedirs(f"{d}/bc", exist_ok=True)
bc = {}
for v in range(4):
    for i, t in enumerate(bc_words):
        f = f"{d}/bc/{v}-{i}.wav"
        if not os.path.exists(f):
            subprocess.run(["say", "-v", voices[v], "-o", f, "--file-format=WAVE", "--data-format=LEI16@16000", t], check=True)
        bc[(v, i)] = rd(f)
# Backchannels: in a gap after an utterance, by a different speaker.
new_refs = []
for a, b in zip(refs, refs[1:] + [None]):
    new_refs.append(a)
    end = a["offset_ms"] + a["duration_ms"]
    gap = (b["offset_ms"] if b else len(voice) * 1000 // SR) - end
    if gap > 900 and rng.random() < 0.45:
        spk = int(rng.choice([s for s in range(4) if s != a["speaker"]]))
        i = int(rng.integers(len(bc_words)))
        clip = bc[(spk, i)]
        if len(clip) * 1000 // SR + 200 < gap:
            start = (end + 150) * SR // 1000
            (voice if spk == 0 else system)[start:start + len(clip)] += clip
            new_refs.append({"offset_ms": end + 150, "duration_ms": len(clip) * 1000 // SR, "speaker": spk, "text": bc_words[i]})
# Babble: far-away chatter (other utterances, low-passed, -30 dB) under 35% of the mic stem.
utts = sorted(os.listdir(f"{d}/utt"))
pos = 0
while pos < len(voice):
    seg = int(rng.uniform(20, 90) * SR)
    if rng.random() < 0.35:
        src = rd(f"{d}/utt/{utts[int(rng.integers(len(utts)))]}")
        src = np.convolve(src, np.ones(6) / 6, mode="same") * 0.035
        n = min(len(src), seg, len(voice) - pos)
        voice[pos:pos + n] += src[:n]
    pos += seg
# Bleed: system audio from the laptop speakers into the mic, 40 ms late, -24 dB, smeared.
delay = int(0.04 * SR)
bleed = np.convolve(system, np.exp(-np.arange(400) / 80) / 40, mode="full")[:len(system)] * 0.063 * 2.5
voice[delay:] += bleed[:len(voice) - delay]
# Noise floors: pinkish room noise on the mic (-48 dBFS), codec hiss on system (-62 dBFS).
def pink(n, rms):
    w = rng.standard_normal(n).astype(np.float32)
    p = np.convolve(w, np.exp(-np.arange(64) / 16), mode="same")
    return p / (np.sqrt(np.mean(p ** 2)) + 1e-9) * rms
voice += pink(len(voice), 10 ** (-48 / 20))
system += pink(len(system), 10 ** (-62 / 20))
mixed = voice + system
out = f"{d}/{name}h"
wr(out + ".voice.wav", voice); wr(out + ".system.wav", system); wr(out + ".wav", mixed)
with open(out + ".ref.jsonl", "w") as f:
    for r in new_refs: f.write(json.dumps(r) + "\n")
print(out, len(new_refs), "utterances", sum(len(r["text"].split()) for r in new_refs), "words")
