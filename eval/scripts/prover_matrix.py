#!/usr/bin/env python3
"""Per-prover outcomes of corpus goals (R6.1): which goals need more than the
first prover of the default order, and what a race of the provers would gain.

    prover_matrix.py --records DIR --provers P1,P2,... --out DIR

DIR/records holds the record files of `corpus_hardness.py` runs made with
`--env DP_MATRIX_SECONDS=T`: for every goal that got a prover call, one line
per prover with the outcome of that prover alone at the reference limit T.
P1,P2,... is the order of the provers in the project's why3find.json.

Output, under --out:
  matrix.jsonl   one line per goal: outcome and time of every prover, class
  hard.jsonl     the goals that are not proved by the first prover alone
  summary.json   counts by class and the times below

Classes of a goal (a leaf or an inner node of a proof tree):
  first        the first prover of the order proves it
  later        the first prover does not, a later one does
  single       exactly one prover proves it (also counted in first or later)
  none         no prover proves it alone within T (it needs a tactic or more time)

Times, over the goals that some prover proves:
  first_prover_seconds   time of the first prover, T where it fails
  in_order_seconds       time until the first proof when the provers run one
                         after the other in the order, each up to T
  best_seconds           time of the fastest prover that proves the goal: what
                         a race of all provers on separate machines takes
"""

import argparse
import collections
import json
import os


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--records", required=True)
    ap.add_argument("--provers", required=True)
    ap.add_argument("--out", required=True)
    args = ap.parse_args()
    order = args.provers.split(",")
    os.makedirs(args.out, exist_ok=True)

    goals = collections.OrderedDict()
    limit = None
    for name in sorted(os.listdir(args.records)):
        for line in open(os.path.join(args.records, name)):
            r = json.loads(line)
            if r["where"] != "matrix":
                continue
            limit = r["limit_us"]
            key = (r["file"], r["theory"], r["goal"], r["path"])
            prover = r["prover"].split("@")[0]
            goals.setdefault(key, {})[prover] = (r["label"], r["time_us"], r["task_hash"])
    if limit is None:
        raise SystemExit("no matrix records under %s" % args.records)

    counts = collections.Counter()
    totals = collections.Counter()
    solved_by = collections.Counter()
    matrix = open(os.path.join(args.out, "matrix.jsonl"), "w")
    hard = open(os.path.join(args.out, "hard.jsonl"), "w")
    for (file, theory, goal, path), runs in goals.items():
        proving = [p for p in order if runs.get(p, ("",))[0] == "valid"]
        for p in proving:
            solved_by[p] += 1
        if not proving:
            cls = "none"
        elif proving[0] == order[0]:
            cls = "first"
        else:
            cls = "later"
        counts[cls] += 1
        if len(proving) == 1:
            counts["single"] += 1
        row = {
            "file": file, "theory": theory, "goal": goal, "path": path, "class": cls,
            "proved_by": proving,
            "runs": {p: {"outcome": runs[p][0], "seconds": round(runs[p][1] / 1e6, 3), "task_hash": runs[p][2]}
                     for p in order if p in runs},
        }
        if proving:
            first = runs.get(order[0])
            first_time = first[1] if first and first[0] == "valid" else limit
            in_order = 0
            for p in order:
                if p in proving:
                    in_order += runs[p][1]
                    break
                in_order += min(runs[p][1], limit) if p in runs else 0
            best = min(runs[p][1] for p in proving)
            totals["first_prover"] += first_time
            totals["in_order"] += in_order
            totals["best"] += best
            row["in_order_seconds"] = round(in_order / 1e6, 3)
            row["best_seconds"] = round(best / 1e6, 3)
        matrix.write(json.dumps(row) + "\n")
        if cls != "first":
            hard.write(json.dumps(row) + "\n")
    summary = {
        "limit_seconds": limit / 1e6,
        "prover_order": order,
        "goals": len(goals),
        "classes": dict(counts),
        "proved_alone_by": dict(solved_by),
        "first_prover_seconds": round(totals["first_prover"] / 1e6, 1),
        "in_order_seconds": round(totals["in_order"] / 1e6, 1),
        "best_seconds": round(totals["best"] / 1e6, 1),
    }
    json.dump(summary, open(os.path.join(args.out, "summary.json"), "w"), indent=1)
    print(json.dumps(summary, indent=1))


if __name__ == "__main__":
    main()
