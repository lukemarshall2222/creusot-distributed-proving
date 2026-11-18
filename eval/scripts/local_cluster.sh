#!/bin/sh
# Runs a command with a Cluster on this machine: one Coordinator and N
# Workers, with fresh keys and an operator-signed registry.
#
#   local_cluster.sh N BIN_DIR COMMAND [ARGUMENT...]
#
# BIN_DIR holds dp-coordinator and dp-worker. The command runs with
# DP_CLUSTER, DP_REGISTRY and DP_OPERATOR_KEY set. Run inside the development
# shell, which provides DP_CREUSOT_HOME and DP_LOCK.
set -eu

n=$1; bin=$2; shift 2
dir=$(mktemp -d)
pids=""
cleanup() {
  for pid in $pids; do kill "$pid" 2>/dev/null || true; done
  rm -rf "$dir"
}
trap cleanup EXIT INT TERM

port=$(python3 -c 'import socket; s = socket.socket(); s.bind(("127.0.0.1", 0)); print(s.getsockname()[1])')
"$bin/dp-coordinator" --listen "127.0.0.1:$port" > /dev/null 2>&1 &
pids="$pids $!"

operator=$("$bin/dp-coordinator" operator-keygen "$dir/operator.key")
entries=""
i=1
while [ "$i" -le "$n" ]; do
  key=$("$bin/dp-worker" keygen "$dir/w$i.key")
  entries="$entries w$i=$key"
  i=$((i + 1))
done
# shellcheck disable=SC2086
"$bin/dp-coordinator" sign-registry "$dir/operator.key" 1 $entries > "$dir/registry"

# Workers find the solvers of Creusot's wrapper, as dp-frontend does.
i=1
while [ "$i" -le "$n" ]; do
  PATH="$DP_CREUSOT_HOME/bin:$PATH" "$bin/dp-worker" run --coordinator "127.0.0.1:$port" --id "w$i" \
    --key "$dir/w$i.key" --why3-conf "$DP_CREUSOT_HOME/creusot_why3.conf" --lock "$DP_LOCK" \
    > "$dir/w$i.log" 2>&1 &
  pids="$pids $!"
  i=$((i + 1))
done
i=1
while [ "$i" -le "$n" ]; do
  tries=0
  until grep -q connected "$dir/w$i.log" 2>/dev/null; do
    tries=$((tries + 1))
    [ "$tries" -lt 100 ] || { echo "local_cluster: worker w$i did not connect" >&2; cat "$dir/w$i.log" >&2; exit 1; }
    sleep 0.1
  done
  i=$((i + 1))
done
sleep 0.5

DP_CLUSTER="tcp://127.0.0.1:$port" DP_REGISTRY="$dir/registry" DP_OPERATOR_KEY="$operator" "$@"
