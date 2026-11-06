# idle.sh <bin> <port> <seconds>: CPU time and RSS of an idle backend (`--dev serve` runs the
# same jobs watcher and call detector as the app). HOME and MARGIN_HOME come from the environment.
BIN=$1; PORT=$2; SECS=${3:-60}
$BIN --dev serve $PORT >/dev/null 2>&1 &
PID=$!
until curl -s localhost:$PORT/latest >/dev/null; do sleep 0.1; done
sleep 5
cpu() { ps -o time= -p $PID | awk -F'[:.]' '{print ($1*60+$2)*1000+$3*10}'; }
C0=$(cpu); T0=$(date +%s)
sleep $SECS
C1=$(cpu); RSS=$(ps -o rss= -p $PID)
echo "idle ${SECS}s: cpu_ms=$((C1-C0)) cpu_pct=$(echo "scale=3; ($C1-$C0)/($SECS*10)" | bc) rss_kb=$RSS"
kill $PID
