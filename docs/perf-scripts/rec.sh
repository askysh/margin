# rec.sh <debug-bin> <port> <secs> <call.wav>: backend CPU and RSS while a simulated call recording runs.
# Debug builds only (MARGIN_SIMULATE_CAPTURE). HOME and MARGIN_HOME come from the environment.
BIN=$1; PORT=$2; SECS=${3:-30}; WAV=$4
MARGIN_SIMULATE_CAPTURE=$WAV $BIN --dev serve $PORT >/dev/null 2>&1 &
PID=$!
until curl -s localhost:$PORT/latest >/dev/null; do sleep 0.1; done
curl -s localhost:$PORT/invoke -d '{"cmd":"start_recording","args":{"kind":"call","title":"Perf"}}' | head -c 80; echo
sleep 3
cpu() { ps -o time= -p $PID | awk -F'[:.]' '{print ($1*60+$2)*1000+$3*10}'; }
C0=$(cpu); sleep $SECS; C1=$(cpu); RSS=$(ps -o rss= -p $PID)
echo "recording ${SECS}s: cpu_ms=$((C1-C0)) cpu_pct=$(echo "scale=2; ($C1-$C0)/($SECS*10)" | bc) rss_kb=$RSS"
curl -s localhost:$PORT/invoke -d '{"cmd":"stop_recording","args":{"notes":null,"title":null}}' | head -c 80; echo
sleep 2; kill $PID
