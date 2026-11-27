#!/usr/bin/env python3
"""Unsplit verification conditions as a baseline of hard goals (exploratory).

why3find proves a verification condition by splitting it when no prover
proves it whole. The pieces are easy; the whole is a goal that is valid,
real, and sometimes beyond every prover alone. This script measures the
verification conditions as they are before any tactic, and how much the
outcome of a prover on them depends on its random seed.

    unsplit_baseline.py measure   --project DIR [--subdir D] [--settings FILE] --name NAME --out DIR --work DIR
                                  --frontend PROGRAM --seconds N [--unproved-at M]
    unsplit_baseline.py seeds     --out DIR --work DIR --names A,B --seconds N --seeds 1,2,3
                                  [--jobs J] [--at M]
    unsplit_baseline.py summarize --out DIR --names A,B --short N --long M --provers P1,P2,...

`measure` proves every Coma file under DIR/<subdir of the project> with a
copy of the project's why3find.json in which the tactics are taken away
(depth 0), through `corpus_hardness.py` with `DP_MATRIX_SECONDS=N`: every
prover alone on every verification condition, at the reference limit N.
With `--unproved-at M` it runs only the files that have a verification
condition that no prover proved in the run at the limit M. The Printed_Goals
are exported to --work, where `seeds` reads them.

`seeds` runs the provers that take a random seed (Z3, cvc5, CVC4) on the
exported Printed_Goals of every verification condition, once per seed, with
the command lines of Creusot's prover configuration and the seed options of
dp-shell's `with_seed`. A run is a proof when the solver prints `unsat`.
With `--at M` it takes only the verification conditions that no prover
proved at the limit M.

Run `measure` and `seeds` inside the development shell of this repository.

Output, under --out:
  <name>-<N>s/        output of corpus_hardness.py: records/, goals.jsonl, runs.jsonl
  seeds-<N>s.jsonl    one line per run: goal, prover, seed, outcome, seconds
  vcs.jsonl           one line per verification condition, with its class
  hard.jsonl          the hard and the out-of-reach ones
  unstable.jsonl      the verification conditions on which a prover proves
                      with some seeds and not with others
  summary.json        counts per corpus

Classes of a verification condition:
  easy          some prover proves it whole within the short limit
  hard          none does within the short limit, one does within the long one
  out-of-reach  no prover proves it whole within the long limit
"""

import argparse
import collections
import concurrent.futures
import json
import os
import shutil
import subprocess
import sys
import time

HERE = os.path.dirname(os.path.abspath(__file__))


def matrix_runs(directory):
    """Per verification condition of a corpus_hardness output: the run of
    every prover on it."""
    goals = collections.OrderedDict()
    records = os.path.join(directory, "records")
    if not os.path.isdir(records):
        return goals
    for name in sorted(os.listdir(records)):
        for line in open(os.path.join(records, name)):
            r = json.loads(line)
            if r["where"] != "matrix" or r["path"]:
                continue
            key = (r["file"], r["theory"], r["goal"])
            goals.setdefault(key, {})[r["prover"].split("@")[0]] = {
                "outcome": r["label"], "seconds": round(r["time_us"] / 1e6, 3), "task_hash": r["task_hash"]}
    return goals


def proved(runs):
    return any(r["outcome"] == "valid" for r in runs.values())


def done_files(directory):
    path = os.path.join(directory, "runs.jsonl")
    if not os.path.exists(path):
        return set()
    return {r["file"] for r in map(json.loads, open(path)) if r["status"] in (0, 1)}


def measure(args):
    project = os.path.join(args.work, args.name)
    # The project without tactics: its Coma files and its prover settings.
    if not os.path.exists(os.path.join(project, "why3find.json")):
        settings = json.load(open(args.settings or os.path.join(args.project, "why3find.json")))
        settings["depth"] = 0
        settings["tactics"] = []
        files = []
        for root, _, names in os.walk(os.path.join(args.project, args.subdir)):
            if os.sep + "target" in root or os.sep + "." in root[len(args.project):]:
                continue
            for name in names:
                if name.endswith(".coma"):
                    rel = os.path.relpath(os.path.join(root, name), args.project)
                    os.makedirs(os.path.dirname(os.path.join(project, rel)), exist_ok=True)
                    shutil.copy(os.path.join(root, name), os.path.join(project, rel))
                    files.append(rel)
        json.dump(settings, open(os.path.join(project, "why3find.json"), "w"), indent=1)
        json.dump(sorted(files), open(os.path.join(project, "files.json"), "w"))
    files = json.load(open(os.path.join(project, "files.json")))
    if args.unproved_at:
        earlier = matrix_runs(os.path.join(args.out, "%s-%ds" % (args.name, args.unproved_at)))
        keep = {file for (file, _, _), runs in earlier.items() if not proved(runs)}
        files = [f for f in files if f in keep]
    directory = os.path.join(args.out, "%s-%ds" % (args.name, args.seconds))
    exported = os.path.join(args.work, "goals-%s-%ds" % (args.name, args.seconds))
    os.makedirs(exported, exist_ok=True)
    for file in files:
        for attempt in range(3):
            if file in done_files(directory):
                break
            p = subprocess.run(
                [sys.executable, os.path.join(HERE, "corpus_hardness.py"), "--prover", args.frontend,
                 "--project", project, "--out", directory, "--file-timeout", str(args.file_timeout),
                 "--env", "DP_MATRIX_SECONDS=%d" % args.seconds, "--env", "DP_EXPORT_DIR=" + exported, file],
                capture_output=True)
            said = p.stdout.decode(errors="replace").strip().splitlines()
            print("%s (%d s%s): %s" % (args.name, args.seconds, ", again" if attempt else "",
                                       said[-1] if said else p.stderr.decode(errors="replace")[-300:]), flush=True)


# ---- Seeds -------------------------------------------------------------------

# The command lines of Creusot's prover configuration (creusot_why3.conf of
# the pinned Creusot) with the seed as dp-shell's `with_seed` passes it.
# Alt-Ergo has no seed option and is not run.
def command(prover, seed, seconds, path):
    if prover == "z3":
        return ["z3", "-smt2", "-T:%d" % seconds, "sat.random_seed=%d" % seed, "nlsat.randomize=false",
                "smt.random_seed=%d" % seed, "-st", path]
    if prover == "cvc5":
        return ["cvc5", "--stats-internal", "--tlimit=%d" % (seconds * 1000), "--seed=%d" % seed, path]
    if prover == "cvc4":
        return ["cvc4", "--stats", "--tlimit=%d" % (seconds * 1000), "--lang=smt2", "--seed=%d" % seed, path]
    raise ValueError(prover)


def run_seed(job):
    prover, seed, seconds, path = job
    start = time.time()
    try:
        p = subprocess.run(command(prover, seed, seconds, path), capture_output=True, timeout=seconds + 5)
        first = p.stdout.decode(errors="replace").strip().splitlines()[:1]
        outcome = {"unsat": "valid", "sat": "invalid", "unknown": "unknown"}.get(first[0] if first else "", "other")
        if outcome in ("unknown", "other") and time.time() - start >= seconds * 0.98:
            outcome = "timeout"
    except subprocess.TimeoutExpired:
        outcome = "timeout"
    return outcome, round(time.time() - start, 3)


def seeds(args):
    home = os.environ["DP_CREUSOT_HOME"]
    os.environ["PATH"] = os.path.join(home, "bin") + ":" + os.environ["PATH"]
    seed_list = [int(s) for s in args.seeds.split(",")]
    jobs = []
    for name in args.names.split(","):
        short = matrix_runs(os.path.join(args.out, "%s-%ds" % (name, args.matrix)))
        # The Printed_Goals of this corpus, of whichever run exported them.
        exported = {}
        for directory in sorted(os.listdir(args.work)):
            if directory.startswith("goals-%s-" % name):
                for file in os.listdir(os.path.join(args.work, directory)):
                    exported[file.split(".")[0]] = os.path.join(args.work, directory, file)
        for (file, theory, goal), runs in short.items():
            if args.at and proved(runs):
                continue
            for prover in ("z3", "cvc5", "cvc4"):
                if prover not in runs:
                    continue
                path = exported.get(runs[prover]["task_hash"])
                if path is None:
                    print("no Printed_Goal for %s %s %s" % (file, goal, prover), flush=True)
                    continue
                for seed in seed_list:
                    jobs.append(({"corpus": name, "file": file, "theory": theory, "goal": goal, "prover": prover,
                                  "seed": seed, "task_hash": runs[prover]["task_hash"]},
                                 (prover, seed, args.seconds, path)))
    out_path = os.path.join(args.out, "seeds-%ds%s.jsonl" % (args.seconds, "-hard" if args.at else ""))
    have = set()
    if os.path.exists(out_path):
        for r in map(json.loads, open(out_path)):
            have.add((r["task_hash"], r["prover"], r["seed"]))
    jobs = [j for j in jobs if (j[0]["task_hash"], j[0]["prover"], j[0]["seed"]) not in have]
    print("%d runs" % len(jobs), flush=True)
    out = open(out_path, "a")
    with concurrent.futures.ThreadPoolExecutor(args.jobs) as pool:
        for n, (record, (outcome, seconds)) in enumerate(zip((j[0] for j in jobs), pool.map(run_seed, (j[1] for j in jobs)))):
            record.update(limit_seconds=args.seconds, outcome=outcome, seconds=seconds)
            out.write(json.dumps(record) + "\n")
            if n % 500 == 499:
                out.flush()
                print("%d of %d" % (n + 1, len(jobs)), flush=True)


# ---- Summary -----------------------------------------------------------------

def summarize(args):
    order = args.provers.split(",")
    seed_runs = collections.defaultdict(lambda: collections.defaultdict(dict))
    seed_limits = {}
    for name in sorted(os.listdir(args.out)):
        if name.startswith("seeds-") and name.endswith(".jsonl"):
            for r in map(json.loads, open(os.path.join(args.out, name))):
                key = (r["corpus"], r["file"], r["theory"], r["goal"])
                seed_runs[(key, r["limit_seconds"])][r["prover"]][r["seed"]] = (r["outcome"], r["seconds"])
                seed_limits[r["limit_seconds"]] = True
    vcs_out = open(os.path.join(args.out, "vcs.jsonl"), "w")
    hard_out = open(os.path.join(args.out, "hard.jsonl"), "w")
    unstable_out = open(os.path.join(args.out, "unstable.jsonl"), "w")
    summary = {"status": "exploratory", "limits_seconds": {"short": args.short, "long": args.long},
               "prover_order": order, "corpora": {}}
    for corpus in args.names.split(","):
        short = matrix_runs(os.path.join(args.out, "%s-%ds" % (corpus, args.short)))
        long = matrix_runs(os.path.join(args.out, "%s-%ds" % (corpus, args.long)))
        counts = collections.Counter()
        by_file = collections.Counter()
        proved_by = collections.Counter()
        for key, runs in short.items():
            file, theory, goal = key
            at_long = long.get(key, {})
            if proved(runs):
                cls = "easy"
            elif not at_long:
                cls = "unmeasured"
            elif proved(at_long):
                cls = "hard"
            else:
                cls = "out-of-reach"
            counts[cls] += 1
            counts["vcs"] += 1
            provers_short = [p for p in order if runs.get(p, {}).get("outcome") == "valid"]
            if len(provers_short) == 1:
                counts["easy_for_one_prover_only"] += 1
            row = {"corpus": corpus, "file": file, "theory": theory, "goal": goal, "class": cls,
                   "proved_by_%ds" % args.short: provers_short,
                   "runs_%ds" % args.short: {p: runs[p] for p in order if p in runs}}
            if at_long:
                row["proved_by_%ds" % args.long] = [p for p in order if at_long.get(p, {}).get("outcome") == "valid"]
                row["runs_%ds" % args.long] = {p: at_long[p] for p in order if p in at_long}
            # The seeds: per prover, how many of them prove the goal.
            seeds_row = {}
            unstable = []
            for limit in sorted(seed_limits):
                per = seed_runs.get(((corpus, file, theory, goal), limit))
                if not per:
                    continue
                seeds_row["%ds" % limit] = {}
                for prover, by_seed in sorted(per.items()):
                    valid = sorted(seed for seed, (outcome, _) in by_seed.items() if outcome == "valid")
                    seeds_row["%ds" % limit][prover] = {
                        "proved_with": valid, "seeds": len(by_seed),
                        "seconds": {str(seed): seconds for seed, (_, seconds) in sorted(by_seed.items())}}
                    if 0 < len(valid) < len(by_seed):
                        unstable.append((limit, prover, len(valid), len(by_seed)))
            if seeds_row:
                row["seeds"] = seeds_row
            vcs_out.write(json.dumps(row) + "\n")
            if cls in ("hard", "out-of-reach"):
                by_file[file] += 1
                for p in row.get("proved_by_%ds" % args.long, []):
                    proved_by[p] += 1
                hard_out.write(json.dumps({
                    "corpus": corpus, "file": file, "theory": theory, "goal": goal, "class": cls,
                    "task_hash": {p: (at_long or runs)[p]["task_hash"] for p in order if p in (at_long or runs)},
                    "proved_by": row.get("proved_by_%ds" % args.long, []),
                    "seconds": {p: at_long[p]["seconds"] for p in row.get("proved_by_%ds" % args.long, [])},
                }) + "\n")
            if unstable:
                counts["unstable"] += 1
                counts["unstable_" + cls] += 1
                unstable_out.write(json.dumps({
                    "corpus": corpus, "file": file, "theory": theory, "goal": goal, "class": cls,
                    "unstable": [{"limit_seconds": l, "prover": p, "proved_with": v, "of_seeds": n} for l, p, v, n in unstable],
                    "seeds": seeds_row,
                }) + "\n")
            # Proved by some seed of some prover, where the default seeds prove nothing.
            for limit in sorted(seed_limits):
                per = seed_runs.get(((corpus, file, theory, goal), limit), {})
                some = any(outcome == "valid" for by_seed in per.values() for outcome, _ in by_seed.values())
                if cls != "easy" and some and limit <= args.short:
                    counts["not_easy_and_proved_by_some_seed_at_%ds" % limit] += 1
        summary["corpora"][corpus] = {
            "counts": dict(counts),
            "hard_or_out_of_reach_by_file": dict(by_file.most_common()),
            "hard_proved_at_long_limit_by": dict(proved_by),
        }
    json.dump(summary, open(os.path.join(args.out, "summary.json"), "w"), indent=1)
    print(json.dumps({c: v["counts"] for c, v in summary["corpora"].items()}, indent=1))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    s = sub.add_parser("measure")
    s.add_argument("--project", required=True)
    s.add_argument("--name", required=True)
    s.add_argument("--out", required=True)
    s.add_argument("--work", required=True)
    s.add_argument("--frontend", required=True)
    s.add_argument("--settings", help="the why3find.json of the project, if it is not in DIR")
    s.add_argument("--subdir", default="", help="the directory of DIR that holds the Coma files of the corpus")
    s.add_argument("--seconds", type=int, required=True)
    s.add_argument("--unproved-at", type=int)
    s.add_argument("--file-timeout", type=float, default=7200)
    s = sub.add_parser("seeds")
    s.add_argument("--out", required=True)
    s.add_argument("--work", required=True)
    s.add_argument("--names", required=True)
    s.add_argument("--seconds", type=int, required=True)
    s.add_argument("--seeds", required=True)
    s.add_argument("--matrix", type=int, default=5, help="the limit of the measure run whose goals are taken")
    s.add_argument("--at", type=int, help="only the verification conditions that no prover proved at this limit")
    s.add_argument("--jobs", type=int, default=6)
    s = sub.add_parser("summarize")
    s.add_argument("--out", required=True)
    s.add_argument("--names", required=True)
    s.add_argument("--short", type=int, default=5)
    s.add_argument("--long", type=int, default=60)
    s.add_argument("--provers", required=True)
    args = ap.parse_args()
    for name in ("project", "out", "work"):
        if getattr(args, name, None):
            setattr(args, name, os.path.abspath(getattr(args, name)))
    {"measure": measure, "seeds": seeds, "summarize": summarize}[args.command](args)


if __name__ == "__main__":
    main()
