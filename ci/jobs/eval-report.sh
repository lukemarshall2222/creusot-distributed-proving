#!/bin/sh
# ci/eval-report (R6.4): the checks of the evaluation report against the
# raw records.
set -eu
ls eval/report/check/* > /dev/null 2>&1 || { echo "ci/eval-report: no inputs yet (eval/report/check/); nothing was checked" >&2; exit 0; }
python3 -m pytest eval/report/check
