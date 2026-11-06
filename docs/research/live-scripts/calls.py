import re,datetime,sys,statistics
lines=open(sys.argv[1]).read().splitlines()
def t(s): return datetime.datetime.fromisoformat(s[:26])
calls=[];prev=None
for l in lines:
    m=re.search(r'starting whisper transcription samples=(\d+) duration_secs=([\d.]+)',l)
    if m: prev=(t(l),float(m.group(2)))
    elif 'transcription complete' in l and prev: calls.append(((t(l)-prev[0]).total_seconds(),prev[1])); prev=None
n=len(calls); sx=sum(a for _,a in calls); sy=sum(c for c,_ in calls)
sxx=sum(a*a for _,a in calls); sxy=sum(a*c for c,a in calls)
b=(n*sxy-sx*sy)/(n*sxx-sx*sx); a=(sy-b*sx)/n
print(f"calls={n} audio={sx:.0f}s compute={sy:.0f}s mean_chunk={sx/n:.1f}s median_wall={statistics.median(c for c,_ in calls):.2f}s fit: {a:.2f}s/call + {b:.3f}s per audio s")
for lo,hi in [(0,3),(3,10),(10,30),(30,46)]:
    sel=[c for c,d in calls if lo<=d<hi]
    if sel: print(f"  chunks {lo}-{hi}s: n={len(sel)} median wall {statistics.median(sel):.2f}s")
