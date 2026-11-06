# Generate N realistic one-hour meeting files into a folder.
import os, random, sys, datetime
out = sys.argv[1]; n = int(sys.argv[2]) if len(sys.argv) > 2 else 600
os.makedirs(out, exist_ok=True)
random.seed(7)
words = ("launch plan onboarding pricing legal review budget hiring roadmap customer churn metrics design "
         "contract renewal security audit migration database latency release marketing copy partner demo "
         "quarter goals feedback interview backlog sprint retro incident outage dashboard model download").split()
people = ["Priya", "Sam", "Jordan", "Alex", "Mei", "Omar", "Lena", "Carlos"]
start = datetime.datetime(2026, 11, 5, 17, 0, tzinfo=datetime.timezone(datetime.timedelta(hours=5, minutes=30)))
def sentence(k=14):
    return " ".join(random.choice(words) for _ in range(random.randint(6, k))).capitalize() + "."
for i in range(n):
    d = start - datetime.timedelta(hours=9 * i)
    title = f"{random.choice(words).capitalize()} {random.choice(words)} sync {i}"
    slug = title.lower().replace(" ", "-")
    ppl = random.sample(people, 3)
    fm = [ "---", f"title: {title}", "type: meeting", f"date: {d.isoformat()}", "duration: 58m", "status: complete",
           "attendees:", *[f"- {p}" for p in ppl], "speaker_map:",
           *[x for j, p in enumerate(ppl) for x in (f"- speaker_label: SPEAKER_{j}", f"  name: {p}", "  confidence: high", "  source: enrollment")],
           "action_items:", *[x for p in ppl[:2] for x in (f"- assignee: {p}", f"  task: {sentence(8)}", "  status: open")],
           "---", "" ]
    body = ["## Summary", "", *[f"- {sentence()}" for _ in range(5)], "", "## Action Items", "",
            *[f"- [ ] @{p}: {sentence(8)}" for p in ppl[:2]], "", "## Notes", "", *[f"- {sentence(9)}" for _ in range(6)], "",
            "## Transcript", ""]
    t = 0
    while t < 3480:
        sp = random.randrange(3)
        body.append(f"[SPEAKER_{sp} {t // 60}:{t % 60:02d}] {sentence(22)}")
        t += random.randint(3, 8)
    with open(os.path.join(out, f"{d.date()}-{slug}.md"), "w") as f:
        f.write("\n".join(fm + body) + "\n")
print("wrote", n)
