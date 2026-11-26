#!/usr/bin/env python3
"""Hint ablation of a Creusot project (exploratory): what happens to the
goals of a Rust function when its `proof_assert!` hints are removed.

For every Rust function with at least one `proof_assert!`, a variant of the
project is made in which that function has none; nothing else is changed.
Each variant is translated to Coma again, and the Coma files that the change
touched are proved in the original and in the ablated version, with every
prover alone on every goal (`DP_MATRIX_SECONDS` of dp-frontend, through
`corpus_hardness.py`): first at a short limit, then at a long limit on the
files that have a goal that no prover proved at the short one.

    hint_ablation.py scan      --src DIR --out DIR [--crates A,B,...]
    hint_ablation.py translate --src DIR --out DIR --work DIR --creusot DIR
    hint_ablation.py measure   --out DIR --work DIR --frontend PROGRAM --seconds N [--unproved-at M]
    hint_ablation.py summarize --out DIR --short N --long M --provers P1,P2,...

`--src` is a checkout of the project, which `translate` edits in place and
restores (it must be a git work tree). `--creusot` is the directory with
`cargo-creusot` and `creusot-rustc` of the Creusot version that the project
was written for. `--work` holds the Coma files (they are not part of the
results: `translate` makes them again from the checkout and the patches).
`measure` runs inside the development shell of this repository.

Output, under --out:
  asserts.json            the functions and the hints removed from each
  patches/<id>.diff       the change of each variant
  translate.jsonl         per variant: compiled or not and why, the Coma files it changed
  original-<N>s/, ablated-<N>s/
                          the output of corpus_hardness.py at the limit N:
                          records/ (one line per prover run), goals.jsonl, runs.jsonl
  goals.jsonl             one line per goal of either version, with its class
  hard.jsonl              the hard goals of the ablated versions
  summary.json            counts per function and in total

A goal is a node of a proof tree that got a prover call: a verification
condition, or a goal that a tactic of why3find made of one. Classes:
  easy          some prover proves it alone within the short limit
  hard          no prover does within the short limit, one does within the long one
  out-of-reach  no prover proves it alone within the long limit
  unmeasured    not proved at the short limit, and the run at the long limit
                did not come to this goal (its proof search went another way)
"""

import argparse
import collections
import json
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


# ---- Reading Rust -----------------------------------------------------------

def code_mask(text):
    """For every character, whether it is code: not in a comment, a string or
    a character literal."""
    mask = [True] * len(text)
    i, n = 0, len(text)
    while i < n:
        c = text[i]
        if text.startswith("//", i):
            j = text.find("\n", i)
            j = n if j < 0 else j
            mask[i:j] = [False] * (j - i)
            i = j
        elif text.startswith("/*", i):
            depth, j = 1, i + 2
            while j < n and depth:
                if text.startswith("/*", j):
                    depth, j = depth + 1, j + 2
                elif text.startswith("*/", j):
                    depth, j = depth - 1, j + 2
                else:
                    j += 1
            mask[i:j] = [False] * (j - i)
            i = j
        elif c == '"' or (c == "r" and text[i + 1:i + 2] in ('"', "#") and not (i and (text[i - 1].isalnum() or text[i - 1] == "_"))):
            if c == "r":
                j = i + 1
                while j < n and text[j] == "#":
                    j += 1
                if j >= n or text[j] != '"':
                    i += 1
                    continue
                close = '"' + "#" * (j - i - 1)
                k = text.find(close, j + 1)
                k = n if k < 0 else k + len(close)
            else:
                k = i + 1
                while k < n and text[k] != '"':
                    k += 2 if text[k] == "\\" else 1
                k = min(k + 1, n)
            mask[i:k] = [False] * (k - i)
            i = k
        elif c == "'":
            # A character literal, or a lifetime.
            if text[i + 1:i + 2] == "\\":
                k = text.find("'", i + 2)
                k = i + 1 if k < 0 else k + 1
                mask[i:k] = [False] * (k - i)
                i = k
            elif text[i + 2:i + 3] == "'":
                mask[i:i + 3] = [False] * 3
                i += 3
            else:
                i += 1
        else:
            i += 1
    return mask


def is_ident(c):
    return c.isalnum() or c == "_"


def word_at(text, mask, i, word):
    return (text.startswith(word, i) and mask[i]
            and not (i and is_ident(text[i - 1]))
            and not is_ident(text[i + len(word):i + len(word) + 1]))


def matching(text, mask, i):
    """The index after the delimiter that closes the one at i."""
    pairs = {"(": ")", "{": "}", "[": "]"}
    stack = [pairs[text[i]]]
    j = i + 1
    while j < len(text) and stack:
        if mask[j]:
            if text[j] in pairs:
                stack.append(pairs[text[j]])
            elif text[j] == stack[-1]:
                stack.pop()
        j += 1
    if stack:
        raise ValueError("unbalanced delimiter at offset %d" % i)
    return j


def functions(text, mask):
    """The functions with a body: name, the context they are in (modules and
    impl headers), the offsets of the `fn` keyword and of the body."""
    out = []
    context = []  # (close offset, label)
    i, n = 0, len(text)
    while i < n:
        while context and i >= context[-1][0]:
            context.pop()
        for keyword in ("impl", "mod", "trait"):
            if word_at(text, mask, i, keyword):
                j = i
                while j < n and not (mask[j] and text[j] in "{;"):
                    j += 1
                if j < n and text[j] == "{":
                    header = " ".join(text[i:j].split())
                    context.append((matching(text, mask, j), header))
                i = j
                break
        else:
            if word_at(text, mask, i, "fn"):
                j = i + 2
                while j < n and text[j].isspace():
                    j += 1
                k = j
                while k < n and is_ident(text[k]):
                    k += 1
                name = text[j:k]
                if name:
                    # The body: the first brace outside parentheses and
                    # brackets, before any semicolon there.
                    depth, m = 0, k
                    while m < n:
                        if mask[m]:
                            if text[m] in "([":
                                depth += 1
                            elif text[m] in ")]":
                                depth -= 1
                            elif depth == 0 and text[m] in "{;":
                                break
                        m += 1
                    if m < n and text[m] == "{":
                        out.append({"name": name, "context": [label for _, label in context],
                                    "start": i, "body": m, "end": matching(text, mask, m)})
                        i = m + 1
                        continue
                i = k
                continue
            i += 1
            continue
        i += 1
    return out


def hints(text, mask):
    """The `proof_assert!` invocations: offsets of the macro and of what is
    removed with it (the semicolon after it, and its line if nothing else is
    on it)."""
    out = []
    i = 0
    while True:
        i = text.find("proof_assert", i)
        if i < 0:
            return out
        if not word_at(text, mask, i, "proof_assert"):
            i += 1
            continue
        j = i + len("proof_assert")
        while text[j].isspace():
            j += 1
        if text[j] != "!":
            i = j
            continue
        j += 1
        while text[j].isspace():
            j += 1
        end = matching(text, mask, j)
        cut = end
        k = cut
        while k < len(text) and text[k] in " \t":
            k += 1
        if text[k:k + 1] == ";":
            cut = k + 1
        # The whole line, if the hint is all that is on it.
        line_start = text.rfind("\n", 0, i) + 1
        line_end = text.find("\n", cut)
        line_end = len(text) if line_end < 0 else line_end + 1
        if not text[line_start:i].strip() and not text[cut:line_end].strip():
            remove = (line_start, line_end)
        else:
            remove = (i, cut)
        out.append({"start": i, "end": end, "remove": remove,
                    "line": text.count("\n", 0, i) + 1, "text": " ".join(text[i:cut].split())})
        i = end


def without(text, removals):
    for start, end in sorted(removals, reverse=True):
        text = text[:start] + text[end:]
    return text


def scan(args):
    crates = args.crates.split(",")
    found = []
    orphans = []
    for crate in crates:
        for root, _, names in os.walk(os.path.join(args.src, crate, "src")):
            for name in sorted(names):
                if not name.endswith(".rs"):
                    continue
                path = os.path.join(root, name)
                rel = os.path.relpath(path, args.src)
                text = open(path).read()
                mask = code_mask(text)
                fns = functions(text, mask)
                by_function = collections.OrderedDict()
                for hint in hints(text, mask):
                    inside = [f for f in fns if f["body"] <= hint["start"] < f["end"]]
                    if not inside:
                        orphans.append({"file": rel, "line": hint["line"], "text": hint["text"]})
                        continue
                    f = max(inside, key=lambda f: f["body"])
                    by_function.setdefault(f["start"], (f, []))[1].append(hint)
                for f, hs in by_function.values():
                    line = text.count("\n", 0, f["start"]) + 1
                    ident = "%s.%s.L%d" % (rel[:-3].replace("/src/", "/").replace("/", "."), f["name"], line)
                    found.append({
                        "id": ident, "crate": crate, "file": rel, "function": f["name"], "line": line,
                        "context": f["context"],
                        "asserts": [{"line": h["line"], "text": h["text"]} for h in hs],
                    })
    os.makedirs(args.out, exist_ok=True)
    json.dump({"crates": crates, "functions": found, "outside_a_function": orphans},
              open(os.path.join(args.out, "asserts.json"), "w"), indent=1)
    print("%d functions with %d hints in %s; %d hints outside a function" % (
        len(found), sum(len(f["asserts"]) for f in found), ", ".join(crates), len(orphans)))


# ---- Translating the variants ----------------------------------------------

def coma_files(src):
    out = {}
    for root, _, names in os.walk(os.path.join(src, "verif")):
        for name in names:
            if name.endswith(".coma"):
                path = os.path.join(root, name)
                out[os.path.relpath(path, src)] = open(path, "rb").read()
    return out


def run_creusot(args, log, crates):
    # cargo does not see a change of Creusot's options, nor a file that was
    # written back with its old contents within the same second: every
    # crate is translated again.
    for crate in crates:
        for root, _, names in os.walk(os.path.join(args.src, crate, "src")):
            for name in names:
                os.utime(os.path.join(root, name))
    cmd = [os.path.join(args.creusot, "cargo-creusot"), "creusot", "--only", "coma",
           "--creusot-rustc", os.path.join(args.creusot, "creusot-rustc"), "--span-mode", "off"]
    p = subprocess.run(cmd, cwd=args.src, capture_output=True)
    open(log, "wb").write(p.stdout + p.stderr)
    return p.returncode, (p.stdout + p.stderr).decode(errors="replace")


def git(args, *command):
    return subprocess.run(["git", *command], cwd=args.src, capture_output=True, check=True).stdout.decode()


def translate(args):
    spec = json.load(open(os.path.join(args.out, "asserts.json")))
    os.makedirs(os.path.join(args.out, "patches"), exist_ok=True)
    logs = os.path.join(args.work, "logs")
    os.makedirs(logs, exist_ok=True)
    if git(args, "status", "--porcelain", "--", *spec["crates"]).strip():
        raise SystemExit("the sources of the checkout are changed; restore them first")

    # The original, as this Creusot translates it.
    status, said = run_creusot(args, os.path.join(logs, "original.log"), spec["crates"])
    if status != 0:
        raise SystemExit("the original does not translate:\n" + said[-2000:])
    original = coma_files(args.src)
    for rel, data in original.items():
        path = os.path.join(args.work, "original", rel)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        open(path, "wb").write(data)
    shutil.copy(os.path.join(args.src, "why3find.json"), os.path.join(args.work, "original"))
    os.makedirs(os.path.join(args.work, "ablated"), exist_ok=True)
    shutil.copy(os.path.join(args.src, "why3find.json"), os.path.join(args.work, "ablated"))

    out = open(os.path.join(args.out, "translate.jsonl"), "w")
    owner = {}
    for f in spec["functions"]:
        path = os.path.join(args.src, f["file"])
        text = open(path).read()
        mask = code_mask(text)
        mine = [fn for fn in functions(text, mask) if fn["name"] == f["function"] and text.count("\n", 0, fn["start"]) + 1 == f["line"]]
        assert len(mine) == 1, f["id"]
        inner = [fn for fn in functions(text, mask) if mine[0]["body"] < fn["start"] < mine[0]["end"]]
        removals = [h["remove"] for h in hints(text, mask)
                    if mine[0]["body"] <= h["start"] < mine[0]["end"]
                    and not any(fn["body"] <= h["start"] < fn["end"] for fn in inner)]
        assert len(removals) == len(f["asserts"]), f["id"]
        record = {"id": f["id"], "file": f["file"], "function": f["function"], "removed": len(removals)}
        try:
            open(path, "w").write(without(text, removals))
            open(os.path.join(args.out, "patches", f["id"] + ".diff"), "w").write(git(args, "diff", "--", f["file"]))
            status, said = run_creusot(args, os.path.join(logs, f["id"] + ".log"), spec["crates"])
            if status != 0:
                errors = [l for l in said.splitlines() if l.startswith("error")]
                record.update(compiled=False, reason="; ".join(errors[:4]) or said[-400:])
            else:
                now = coma_files(args.src)
                changed = sorted(rel for rel in now if original.get(rel) != now[rel])
                gone = sorted(rel for rel in original if rel not in now)
                record.update(compiled=True, coma=changed, coma_removed=gone)
                for rel in changed:
                    if rel in owner:
                        record.setdefault("shared_with", []).append(owner[rel])
                    owner[rel] = f["id"]
                    target = os.path.join(args.work, "ablated", rel)
                    os.makedirs(os.path.dirname(target), exist_ok=True)
                    open(target, "wb").write(now[rel])
        finally:
            open(path, "w").write(text)
        out.write(json.dumps(record) + "\n")
        out.flush()
        print("%s: %s" % (f["id"], "%d Coma files changed" % len(record["coma"]) if record.get("compiled") else "does not compile: " + record["reason"][:200]), flush=True)
    # The checkout as it was, with the Coma files of the original.
    status, _ = run_creusot(args, os.path.join(logs, "restored.log"), spec["crates"])
    assert status == 0 and coma_files(args.src) == original, "the checkout was not restored"


# ---- Measuring ---------------------------------------------------------------

def variants(out):
    return [json.loads(l) for l in open(os.path.join(out, "translate.jsonl"))]


def matrix_runs(directory):
    """Per goal of a corpus_hardness output: the run of every prover."""
    goals = collections.OrderedDict()
    records = os.path.join(directory, "records")
    if not os.path.isdir(records):
        return goals
    for name in sorted(os.listdir(records)):
        for line in open(os.path.join(records, name)):
            r = json.loads(line)
            if r["where"] != "matrix":
                continue
            key = (r["file"], r["theory"], r["goal"], r["path"])
            goals.setdefault(key, {})[r["prover"].split("@")[0]] = {
                "outcome": r["label"], "seconds": round(r["time_us"] / 1e6, 3), "task_hash": r["task_hash"]}
    return goals


def done_files(directory):
    """The files that a run came to an end on (status 0: all proved, 1: not)."""
    path = os.path.join(directory, "runs.jsonl")
    if not os.path.exists(path):
        return set()
    return {r["file"] for r in map(json.loads, open(path)) if r["status"] in (0, 1)}


def measure(args):
    seconds = args.seconds
    plan = []
    for v in variants(args.out):
        for rel in v.get("coma", []):
            plan.append((v["id"], rel))
    if args.unproved_at:
        # Only the files with a goal that no prover proved at the short limit.
        keep = set()
        for version in ("original", "ablated"):
            for (file, _, _, _), runs in matrix_runs(os.path.join(args.out, "%s-%ds" % (version, args.unproved_at))).items():
                if not any(r["outcome"] == "valid" for r in runs.values()):
                    keep.add((version, file))
    for ident, rel in plan:
        # The original and the ablated version of a function one after the
        # other, so that both see the same machine.
        for version in ("original", "ablated"):
            if args.unproved_at and (version, rel) not in keep:
                continue
            directory = os.path.join(args.out, "%s-%ds" % (version, seconds))
            for attempt in range(3):
                if rel in done_files(directory):
                    break
                p = subprocess.run(
                    [sys.executable, os.path.join(HERE, "corpus_hardness.py"), "--prover", args.frontend,
                     "--project", os.path.join(args.work, version), "--out", directory,
                     "--file-timeout", str(args.file_timeout), "--env", "DP_MATRIX_SECONDS=%d" % seconds, rel],
                    capture_output=True)
                said = p.stdout.decode(errors="replace").strip().splitlines()
                print("%s %s (%s, %d s%s): %s" % (version, rel, ident, seconds, ", again" if attempt else "",
                                                 said[-1] if said else p.stderr.decode(errors="replace")[-300:]), flush=True)


# ---- Summary -----------------------------------------------------------------

def summarize(args):
    order = args.provers.split(",")
    spec = {f["id"]: f for f in json.load(open(os.path.join(args.out, "asserts.json")))["functions"]}
    short = {v: matrix_runs(os.path.join(args.out, "%s-%ds" % (v, args.short))) for v in ("original", "ablated")}
    long = {v: matrix_runs(os.path.join(args.out, "%s-%ds" % (v, args.long))) for v in ("original", "ablated")}
    proved_files = {}
    for v in ("original", "ablated"):
        path = os.path.join(args.out, "%s-%ds" % (v, args.short), "runs.jsonl")
        if os.path.exists(path):
            for r in map(json.loads, open(path)):
                proved_files[(v, r["file"])] = r
    goals_out = open(os.path.join(args.out, "goals.jsonl"), "w")
    hard_out = open(os.path.join(args.out, "hard.jsonl"), "w")
    per_function = []
    total = {v: collections.Counter() for v in ("original", "ablated")}
    with_hard, with_out = set(), set()
    for v in variants(args.out):
        entry = {"id": v["id"], "file": v["file"], "function": v["function"], "removed_asserts": v["removed"]}
        if not v.get("compiled"):
            entry["skipped"] = v["reason"]
            per_function.append(entry)
            continue
        entry["coma"] = v["coma"]
        if not v["coma"]:
            entry["skipped"] = "the removal changes no Coma file: the function has none (it is trusted, or not translated)"
        for version in ("original", "ablated"):
            counts = collections.Counter()
            for rel in v["coma"]:
                mine = [(k, r) for k, r in short[version].items() if k[0] == rel]
                for key, runs in mine:
                    file, theory, goal, path = key
                    at_long = long[version].get(key, {})
                    if any(r["outcome"] == "valid" for r in runs.values()):
                        cls = "easy"
                    elif not at_long:
                        cls = "unmeasured"
                    elif any(r["outcome"] == "valid" for r in at_long.values()):
                        cls = "hard"
                    else:
                        cls = "out-of-reach"
                    # A leaf of the proof search: no goal of this run was
                    # made of it by a tactic. A path is the tactics from the
                    # verification condition down, separated by `/`.
                    others = [k[3] for k, _ in mine if k[1] == theory and k[2] == goal and k[3] != path]
                    leaf = not any(path == "" or other.startswith(path + "/") for other in others)
                    row = {
                        "version": version, "function": v["id"], "file": file, "theory": theory, "goal": goal,
                        "path": path, "class": cls, "leaf": leaf,
                        "runs_%ds" % args.short: {p: runs[p] for p in order if p in runs},
                    }
                    if at_long:
                        row["runs_%ds" % args.long] = {p: at_long[p] for p in order if p in at_long}
                        row["proved_by_%ds" % args.long] = [p for p in order if at_long.get(p, {}).get("outcome") == "valid"]
                    goals_out.write(json.dumps(row) + "\n")
                    counts[cls] += 1
                    counts["goals"] += 1
                    if cls in ("hard", "out-of-reach") and leaf:
                        counts[cls + "_leaves"] += 1
                    if version == "ablated" and cls == "hard":
                        source = at_long
                        hard_out.write(json.dumps({
                            "function": v["id"], "rust_file": v["file"], "rust_function": v["function"],
                            "file": file, "theory": theory, "goal": goal, "path": path, "leaf": leaf,
                            "task_hash": {p: source[p]["task_hash"] for p in order if p in source},
                            "proved_by": [p for p in order if source.get(p, {}).get("outcome") == "valid"],
                            "seconds": {p: source[p]["seconds"] for p in order if source.get(p, {}).get("outcome") == "valid"},
                            "removed_asserts": spec[v["id"]]["asserts"],
                        }) + "\n")
                run = proved_files.get((version, rel))
                if run:
                    counts["vcs"] += run["goals"]
                    counts["vcs_proved_by_default_strategy"] += run["proved"]
                else:
                    counts["files_not_run"] += 1
            entry[version] = dict(counts)
            total[version].update(counts)
            if version == "ablated":
                if counts["hard"]:
                    with_hard.add(v["id"])
                if counts["out-of-reach"]:
                    with_out.add(v["id"])
        per_function.append(entry)
    measured = [e for e in per_function if "skipped" not in e]
    summary = {
        "status": "exploratory",
        "limits_seconds": {"short": args.short, "long": args.long},
        "prover_order": order,
        "functions_with_hints": len(per_function),
        "functions_measured": len(measured),
        "functions_skipped": [{"id": e["id"], "why": e["skipped"]} for e in per_function if "skipped" in e],
        "total": {v: dict(c) for v, c in total.items()},
        "ablated_functions_with_a_hard_goal": sorted(with_hard),
        "ablated_functions_with_an_out_of_reach_goal": sorted(with_out),
        "ablated_functions_with_either": len(with_hard | with_out),
        "functions": per_function,
    }
    json.dump(summary, open(os.path.join(args.out, "summary.json"), "w"), indent=1)
    brief = {k: summary[k] for k in ("functions_with_hints", "functions_measured", "total", "ablated_functions_with_either")}
    brief["functions_with_hard"] = len(with_hard)
    brief["functions_with_out_of_reach"] = len(with_out)
    print(json.dumps(brief, indent=1))


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="command", required=True)
    s = sub.add_parser("scan")
    s.add_argument("--src", required=True)
    s.add_argument("--out", required=True)
    s.add_argument("--crates", default="CreuSAT,Robinson,Friday")
    s = sub.add_parser("translate")
    s.add_argument("--src", required=True)
    s.add_argument("--out", required=True)
    s.add_argument("--work", required=True)
    s.add_argument("--creusot", required=True)
    s = sub.add_parser("measure")
    s.add_argument("--out", required=True)
    s.add_argument("--work", required=True)
    s.add_argument("--frontend", required=True)
    s.add_argument("--seconds", type=int, required=True)
    s.add_argument("--unproved-at", type=int)
    s.add_argument("--file-timeout", type=float, default=7200)
    s = sub.add_parser("summarize")
    s.add_argument("--out", required=True)
    s.add_argument("--short", type=int, default=5)
    s.add_argument("--long", type=int, default=60)
    s.add_argument("--provers", required=True)
    args = ap.parse_args()
    for name in ("src", "out", "work", "creusot"):
        if getattr(args, name, None):
            setattr(args, name, os.path.abspath(getattr(args, name)))
    {"scan": scan, "translate": translate, "measure": measure, "summarize": summarize}[args.command](args)


if __name__ == "__main__":
    main()
