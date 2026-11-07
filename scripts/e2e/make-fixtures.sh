#!/bin/bash
# Makes the audio the end-to-end pass uses, with the macOS voices (say + afconvert), and a
# small models folder for --seed-models. Same input, same files, on any Mac.
#
#   scripts/e2e/make-fixtures.sh <out-dir>
#
# <out-dir>/meeting.wav          29 s, two voices, mentions "legal review" (for --audio)
# <out-dir>/call.wav             65 s call: 25 s of speech, then silence (for --simulate)
# <out-dir>/call.voice.wav       your side of the call (the mic stem)
# <out-dir>/call.system.wav      the other side (the system-audio stem)
# <out-dir>/call.live.jsonl      the live transcript lines --simulate plays back
# <out-dir>/models/              symlinks to the small, VAD and diarization models from
#                                your Margin install, when it has them (for --seed-models)
#
# The call ends in 40 s of silence so the test, not the end of the file, stops it.
set -euo pipefail

out="${1:?usage: make-fixtures.sh <out-dir>}"
mkdir -p "$out"
out="$(cd "$out" && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

# Each line: voice|text. Samantha is "you" on the call, Daniel the other side.
lines=(
  "Samantha|Thanks for joining. Let's review the launch plan. We decided to ship Fast by default, and Priya will build the settings screen by Friday."
  "Daniel|Sounds good. I will send the deck on Friday, and we can review the pricing page next week."
  "Samantha|Great. One more thing: the onboarding copy needs a final pass before we release."
  "Daniel|Agreed. I'll take the onboarding copy and share it tomorrow morning."
  "Samantha|Before launch we also need a legal review of the pricing terms."
)
for i in "${!lines[@]}"; do
  voice="${lines[$i]%%|*}"
  text="${lines[$i]#*|}"
  say -v "$voice" -o "$work/s$i.aiff" "$text"
  afconvert -f WAVE -d LEI16@16000 -c 1 "$work/s$i.aiff" "$work/s$i.wav"
  printf '%s\n' "$text" > "$work/s$i.txt"
done

/usr/bin/python3 -I - "$work" "$out" <<'EOF'
import json, sys, wave
work, out = sys.argv[1], sys.argv[2]
RATE = 16000
def read(i):
    with wave.open(f"{work}/s{i}.wav") as r:
        return r.readframes(r.getnframes())
def writer(name):
    w = wave.open(f"{out}/{name}", "wb")
    w.setnchannels(1); w.setsampwidth(2); w.setframerate(RATE)
    return w
gap = b"\0\0" * (RATE // 2)  # 0.5 s between turns
speech = [read(i) for i in range(5)]
texts = [open(f"{work}/s{i}.txt").read().strip() for i in range(5)]

# meeting.wav: all five turns.
with writer("meeting.wav") as m:
    for d in speech:
        m.writeframes(d + gap)

# The call: the first four turns, with one stem per side and the live transcript.
main, voice, system = writer("call.wav"), writer("call.voice.wav"), writer("call.system.wav")
live, t = [], 0
for i, d in enumerate(speech[:4]):
    quiet = b"\0" * len(d)
    main.writeframes(d + gap)
    (voice if i % 2 == 0 else system).writeframes(d + gap)
    (system if i % 2 == 0 else voice).writeframes(quiet + gap)
    ms = len(d) // 32  # 16-bit mono at 16 kHz: 32 bytes per ms
    live.append({"offset_ms": t, "duration_ms": ms, "text": texts[i]})
    t += ms + 500
tail = b"\0\0" * RATE * 40
for w in (main, voice, system):
    w.writeframes(tail)
    w.close()
with open(f"{out}/call.live.jsonl", "w") as f:
    f.write("".join(json.dumps(l) + "\n" for l in live))
EOF

# Links, not copies: about 520 MB stays where it is. Never ggml-large-v3-turbo.bin.
models="$HOME/Library/Application Support/Margin/state/models"
if [ -d "$models" ]; then
  mkdir -p "$out/models"
  for f in ggml-small.bin ggml-silero-v6.2.0.bin diarization; do
    [ -e "$models/$f" ] && ln -sfn "$models/$f" "$out/models/$f"
  done
fi

for f in meeting.wav call.wav; do
  printf '%s %s\n' "$f" "$(afinfo "$out/$f" | awk '/estimated duration/ {printf "%.0f s", $3}')"
done
[ -d "$out/models" ] && ls "$out/models"
