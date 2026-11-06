import sys,re,json,glob
sys.argv=[sys.argv[0]]+sys.argv[1:]
ref=[json.loads(l) for l in open(sys.argv[1])]; md=open(glob.glob(sys.argv[2])[0]).read(); md=md[md.find("## Transcript"):]
def norm(t): return re.sub(r"[^a-z0-9' ]+"," ",t.lower().replace("’","'")).split()
def wer(R,H):
    prev=list(range(len(H)+1))
    for i,r in enumerate(R,1):
        cur=[i]+[0]*len(H)
        for j,h in enumerate(H,1): cur[j]=min(prev[j]+1,cur[j-1]+1,prev[j-1]+(r!=h))
        prev=cur
    return prev[-1]/len(R)
lines=[(m.group(1),m.group(2)) for m in re.finditer(r"(?m)^\[([A-Z_0-9]+) \d+:\d+\]\s*(?:\[([A-Z]+)\]\s*)?(.*)$",md) for _ in [0]]
mic=[];sysw=[]
for m in re.finditer(r"(?m)^\[([A-Z_0-9]+) \d+:\d+\]\s*(?:\[([A-Z]+)\]\s*)?(.*)$",md):
    lab,src,t=m.groups(); src=src or lab
    (mic if src=="MICROPHONE" or lab=="SPEAKER_0" else sysw).extend(norm(t))
Rm=[w for r in ref if r["speaker"]==0 for w in norm(r["text"])]; Rs=[w for r in ref if r["speaker"]!=0 for w in norm(r["text"])]
print(json.dumps({"mic_wer":round(wer(Rm,mic),4),"system_wer":round(wer(Rs,sysw),4)}))
