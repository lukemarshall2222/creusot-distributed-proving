#!/bin/sh
# sched/eval (R6): the experiment runs of the evaluation. Without the
# runner, no run is recorded: the rows that name this job stay pending.
set -eu
[ -f eval/run.sh ] || { echo "sched/eval: no inputs yet (eval/run.sh); no run is recorded" >&2; exit 78; }
eval/run.sh
