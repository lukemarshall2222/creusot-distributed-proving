#!/bin/sh
# ci/corpus-check (R6.1): the tests of the corpus configuration and of the
# Corpus_Exporter.
set -eu
ls eval/corpus/tests/* > /dev/null 2>&1 || { echo "ci/corpus-check: no inputs yet (eval/corpus/tests/); nothing was checked" >&2; exit 0; }
python3 -m pytest eval/corpus/tests
