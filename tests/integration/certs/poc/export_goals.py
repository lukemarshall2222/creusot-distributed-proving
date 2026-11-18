"""Export the cvc5 Printed_Goals of Creusot example files through the dispatch bridge.

    export_goals.py WHY3FIND EXAMPLES_DIR OUT_DIR CAP TRANSFORMATIONS FILE.coma...

WHY3FIND is the patched why3find. CAP is the number of goals exported per
file. TRANSFORMATIONS is a comma-separated list of Why3 transformations applied
before the driver prints ("" for none; "eliminate_algebraic" removes `match`).
cvc5, z3 and why3 must be on PATH, and why3find must find the `creusot`
package (DUNE_DIR_LOCATIONS="why3find:lib:<creusot wrapper>/share/why3find").
"""
import os, sys, shutil, json
sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), "../../../../patches/why3find/test"))
import dispatch_harness as h

patched, examples, out, cap = sys.argv[1], sys.argv[2], sys.argv[3], int(sys.argv[4])
transformations = [t for t in sys.argv[5].split(",") if t]
files = sys.argv[6:]
os.makedirs(out, exist_ok=True)
index = []
for f in files:
    proj = os.path.join(out, "_proj")
    shutil.rmtree(proj, ignore_errors=True)
    os.makedirs(proj)
    shutil.copy(os.path.join(examples, f), proj)
    json.dump({"fast": 0.2, "time": 1.0, "depth": 6, "packages": ["creusot"],
               "provers": ["cvc5", "z3"], "tactics": ["compute_specified", "split_vc"],
               "drivers": [], "warnoff": ["unused_variable", "axiom_abstract"]},
              open(os.path.join(proj, "why3find.json"), "w"))
    done = {}
    def policy(s, rid, gid, step, prover, t):
        if prover.startswith(b"cvc5") and gid not in done and len(done) < cap:
            r = s.request("print", gid, prover, transformations)
            if r[0] == b"printed" and r[2] == b"1":
                g = s.goals[gid]
                name = "%s__%s%s" % (f.replace(".coma", ""), g[2].decode().replace("'", "_"),
                                     "".join("_%s" % p[1].decode() for p in g[3]))
                path = os.path.join(out, name + ".smt2")
                open(path, "wb").write(r[3])
                done[gid] = path
                index.append({"file": f, "goal": g[2].decode(), "path": [[p[0].decode(), p[1].decode()] for p in g[3]], "smt2": path})
        # Unknown everywhere makes why3find explore tactics, so sub-goals are exported too.
        s.answer(rid, ["unknown", "5000"] if len(done) < cap else ["valid", "5000"])
    s = h.Session(patched, proj, f, policy, extra_args=["-f"])
    st = s.run(limit=600)
    print(f, "status", st, "goals announced", len(s.goals), "exported", len(done), file=sys.stderr)
    if st == 3: print(s.stderr[-400:], file=sys.stderr)
    s.close()
json.dump(index, open(os.path.join(out, "index.json"), "w"), indent=1)
