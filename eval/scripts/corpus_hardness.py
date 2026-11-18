#!/usr/bin/env python3
"""Hardness measurement of corpus goals (R6.1): proves every Coma file of a
project from scratch and records, per goal, whether it was proved and what it
took.

    corpus_hardness.py --prover PROGRAM --project DIR --out DIR [--files-from FILE]
                       [--file-timeout SECONDS] [--export] [--env NAME=VALUE ...] [FILE.coma ...]

PROGRAM is `dp-frontend` or unmodified `why3find`. DIR holds a why3find.json
and the Coma files, which are named relative to it; without FILE arguments
every Coma file under DIR is proved. Run inside the development shell, which
provides the provers and Creusot's prelude.

Output, under --out:
  records/<file>.jsonl   one line per prover call (dp-frontend only)
  goals.jsonl            one line per goal: proved, class and totals
  runs.jsonl             one line per file: exit status and wall-clock time
  goals/                 the Printed_Goals, with --export (dp-frontend only)

Classes: `fast` (proved by the first sequential pass alone), `slow` (proved,
but only after that pass), `unsolved` (not proved), `incomplete` (the file's
run was cut off). The hard goals of the corpus are the slow and unsolved ones.
"""

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
import time


def complete(node):
    if isinstance(node, dict) and "prover" in node:
        return True
    if isinstance(node, dict) and isinstance(node.get("children"), list):
        return all(complete(c) for c in node["children"])
    return False


def proof_shape(node):
    """(provers used, tactic depth) of a proof node."""
    if isinstance(node, dict) and "prover" in node:
        return {node["prover"]}, 0
    if isinstance(node, dict) and isinstance(node.get("children"), list):
        provers, depth = set(), 0
        for c in node["children"]:
            p, d = proof_shape(c)
            provers |= p
            depth = max(depth, d)
        return provers, depth + 1
    return set(), 0


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--prover", required=True)
    ap.add_argument("--project", required=True)
    ap.add_argument("--out", required=True)
    ap.add_argument("--file-timeout", type=float, default=1800)
    ap.add_argument("--export", action="store_true")
    ap.add_argument("--env", action="append", default=[])
    ap.add_argument("--files-from")
    ap.add_argument("files", nargs="*")
    args = ap.parse_args()

    home = os.environ["DP_CREUSOT_HOME"]
    project = os.path.abspath(args.project)
    out = os.path.abspath(args.out)
    os.makedirs(os.path.join(out, "records"), exist_ok=True)
    files = list(args.files)
    if args.files_from:
        files += [l.strip() for l in open(args.files_from) if l.strip()]
    if not files:
        for root, _, names in os.walk(project):
            files += [os.path.relpath(os.path.join(root, n), project) for n in names if n.endswith(".coma")]
    files.sort()

    env = dict(os.environ)
    env["PATH"] = os.path.join(home, "bin") + ":" + env["PATH"]
    env["DUNE_DIR_LOCATIONS"] = "why3find:lib:" + os.path.join(home, "share/why3find")
    env["WHY3CONFIG"] = os.path.join(home, "creusot_why3.conf")
    if os.environ.get("DP_CLUSTER"):
        # The Workers of a local Cluster need time to answer on a busy machine.
        env.setdefault("DP_JOB_MARGIN_SECONDS", "30")
    for pair in args.env:
        name, _, value = pair.partition("=")
        env[name] = value
    if args.export:
        env["DP_EXPORT_DIR"] = os.path.join(out, "goals")
        os.makedirs(env["DP_EXPORT_DIR"], exist_ok=True)

    goals_out = open(os.path.join(out, "goals.jsonl"), "a")
    runs_out = open(os.path.join(out, "runs.jsonl"), "a")
    for file in files:
        work = tempfile.mkdtemp(prefix="corpus-")
        try:
            os.makedirs(os.path.dirname(os.path.join(work, file)) or work, exist_ok=True)
            shutil.copy(os.path.join(project, file), os.path.join(work, file))
            shutil.copy(os.path.join(project, "why3find.json"), work)
            record = os.path.join(out, "records", file.replace("/", "__") + ".jsonl")
            if os.path.exists(record):
                os.unlink(record)
            env["DP_RECORD"] = record
            start = time.time()
            try:
                p = subprocess.run(
                    [args.prover, "prove", "-f", "--summary", "--no-autodetect", "--no-autoload", file],
                    cwd=work, env=env, capture_output=True, timeout=args.file_timeout)
                status = p.returncode
                # What the prover said last, when it did not end normally.
                error = p.stderr.decode(errors="replace")[-600:] if status not in (0, 1) else None
            except subprocess.TimeoutExpired:
                status = None
                error = "file timeout"
                subprocess.run(["pkill", "-f", work], capture_output=True)
            wall = time.time() - start
            calls = []
            if os.path.exists(record):
                calls = [json.loads(l) for l in open(record)]
            proofs = {}
            for root, _, names in os.walk(os.path.join(work, "why3find.proofs")):
                for n in names:
                    for theory, goals in json.load(open(os.path.join(root, n))).get("proofs", {}).items():
                        for goal, proof in goals.items():
                            proofs[(theory, goal)] = proof
            names = set(proofs) | {(c["theory"], c["goal"]) for c in calls}
            for theory, goal in sorted(names):
                mine = [c for c in calls if c["theory"] == theory and c["goal"] == goal]
                proved = complete(proofs.get((theory, goal)))
                provers, depth = proof_shape(proofs.get((theory, goal)))
                beyond_fast = any(c["step"] != "fast" for c in mine)
                if status is None and not proved:
                    cls = "incomplete"
                elif not proved:
                    cls = "unsolved"
                elif mine and not beyond_fast and depth == 0:
                    cls = "fast"
                elif not mine and depth == 0:
                    cls = "proved"  # no call records (unmodified why3find)
                else:
                    cls = "slow"
                goals_out.write(json.dumps({
                    "file": file, "theory": theory, "goal": goal, "proved": proved, "class": cls,
                    "tactic_depth": depth, "provers": sorted(provers), "calls": len(mine),
                    "prover_seconds": round(sum(c["time_us"] for c in mine) / 1e6, 3),
                    "timeouts": sum(c["label"] == "timeout" for c in mine),
                }) + "\n")
            goals_out.flush()
            runs_out.write(json.dumps({"file": file, "status": status, "error": error, "wall_seconds": round(wall, 1),
                                       "goals": len(names), "proved": sum(complete(p) for p in proofs.values())}) + "\n")
            runs_out.flush()
            print("%s: status %s, %.1fs, %d goals, %d proved" % (
                file, status, wall, len(names), sum(complete(p) for p in proofs.values())), flush=True)
        finally:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    main()
