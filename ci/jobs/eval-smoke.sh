#!/bin/sh
# ci/eval-smoke (R6.2, R6.3): the tests of the metric computations.
set -eu
ls eval/tests/* > /dev/null 2>&1 || { echo "ci/eval-smoke: no inputs yet (eval/tests/); nothing was checked" >&2; exit 0; }
python3 -m pytest eval/tests
