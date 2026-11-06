#!/bin/zsh
# usage: dev.sh <name> <python config edit or ""> <dev args...>  (fresh sandbox per name)
R=/private/tmp/claude-501/margin-live
BIN=${BIN:-/Users/akash/.t3/worktrees/minutes-shell/t3-fc2f25cc/src-tauri/target/release/margin}
name=$1; edit=$2; shift 2
SB=$R/runs/$name; rm -rf $SB; mkdir -p $SB/home $SB/mh/state/models
cp -Rc $R/models/ $SB/mh/state/models/
export HOME=$SB/home MARGIN_HOME=$SB/mh
$BIN --dev list >/dev/null 2>&1
python3 - "$SB/mh/config.toml" "$edit" <<'PY'
import sys,re
p,edit=sys.argv[1],sys.argv[2]
s=open(p).read()
def setkey(sec,key,val):
    global s
    m=re.search(r'(?m)^\['+re.escape(sec)+r'\]\s*$',s)
    if not m: s+=f'\n[{sec}]\n{key} = {val}\n'; return
    start=m.end(); nxt=re.search(r'(?m)^\[',s[start:]); end=start+(nxt.start() if nxt else len(s)-start)
    body=s[start:end]
    if re.search(r'(?m)^'+re.escape(key)+r'\s*=',body): body=re.sub(r'(?m)^'+re.escape(key)+r'\s*=.*$',f'{key} = {val}',body)
    else: body='\n'+f'{key} = {val}'+body
    s=s[:start]+body+s[end:]
setkey('identity','name','"Sam Tester"')
exec(edit or '')
open(p,'w').write(s)
PY
echo "load: $(uptime | sed 's/.*averages: //')" > $SB/stderr.txt
/usr/bin/time -l $BIN --dev "$@" 2>> $SB/stderr.txt > $SB/result.json
grep -vE "^ +[0-9]+  [a-z]" $SB/stderr.txt | grep -v "^ *[0-9]* *\(page\|block\|messages\|signals\|voluntary\|involuntary\|swaps\|average\|instructions\|cycles\|peak\)" | tail -${TAILN:-12}
