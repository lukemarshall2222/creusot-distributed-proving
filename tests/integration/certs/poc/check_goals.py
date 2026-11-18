"""For each exported goal: cvc5 with CPC proofs, then Ethos against the goal as reference.

    check_goals.py CVC5 ETHOS CPC_SIGNATURE_DIR GOALS_DIR [extra cvc5 options] > results.json

Writes one JSON record per goal: the cvc5 answer with and without proofs, the
proof size and rules, and the Ethos verdict (from its exit status and standard
output). Proofs are written next to the goals as .cpc files.
"""
import glob, json, os, re, subprocess, sys, time
from concurrent.futures import ThreadPoolExecutor
cvc5, ethos, sig, goals, extra = sys.argv[1], sys.argv[2], sys.argv[3], sys.argv[4], sys.argv[5:]

def run(cmd, timeout):
    t = time.time()
    try:
        p = subprocess.run(cmd, capture_output=True, timeout=timeout)
        return p.returncode, p.stdout.decode(errors="replace"), p.stderr.decode(errors="replace"), time.time() - t
    except subprocess.TimeoutExpired:
        return None, "", "timeout", time.time() - t

def one(path):
    r = {"goal": os.path.basename(path)[:-5], "bytes": os.path.getsize(path)}
    rc, out, err, dt = run([cvc5, "--tlimit=10000", path], 30)
    r["plain"] = (out.split("\n", 1)[0] or err.strip().split("\n")[-1][:60]) if rc is not None else "timeout"
    r["plain_s"] = round(dt, 2)
    rc, out, err, dt = run([cvc5, "--tlimit=20000", "--dump-proofs", "--proof-format=cpc",
                            "--no-proof-dag-global", "--no-proof-allow-trust", *extra, path], 60)
    r["proof_s"] = round(dt, 2)
    first = out.split("\n", 1)[0] if rc is not None else "timeout"
    r["with_proof"] = first or err.strip().split("\n")[-1][:80]
    if first != "unsat":
        return r
    lines = out.split("\n")[1:]
    # drop cvc5's outer parentheses, the trailing reason-unknown output and declarations
    body = [l for l in lines if l not in ("(", ")") and not l.startswith("(declare-")]
    # the goal ends with (get-info :reason-unknown), which cvc5 answers with an (error ...) line
    body = [l for l in body if l.startswith("(") and not l.startswith("(error ")]
    r["steps"] = sum(l.startswith("(step") for l in body)
    r["commands"] = sorted({m.group(1) for l in body for m in [re.match(r"\((\S+)", l)] if m})
    rules = {}
    for l in body:
        m = re.search(r":rule (\S+)", l)
        if m: rules[m.group(1)] = rules.get(m.group(1), 0) + 1
    r["rules"] = rules
    pf = path[:-5] + ".cpc"
    open(pf, "w").write("\n".join(body) + "\n")
    r["proof_bytes"] = os.path.getsize(pf)
    rc, out, err, dt = run([ethos, "--include=" + sig + "/Cpc.eo", "--reference=" + path,
                            "--require-proof-of-false", pf], 300)
    outl = [l for l in out.split("\n") if l.strip()]
    errl = [l for l in err.split("\n") if l.strip() and not l.startswith(("Unsupported attribute", "Fatal failure")) and "Unprocessed attribute" not in l]
    r["ethos_rc"] = rc
    r["ethos"] = "timeout" if rc is None else (outl[-1] if outl and rc == 0 else re.sub(r"/\S*/", "", (errl or outl or ["?"])[-1])[:240])
    r["ethos_warnings"] = sorted({re.sub(r".*: ", "", l)[:40] for l in err.split("\n") if "attribute" in l})
    r["ethos_s"] = round(dt, 2)
    return r

paths = sorted(glob.glob(os.path.join(goals, "*.smt2")))
with ThreadPoolExecutor(6) as ex:
    res = list(ex.map(one, paths))
json.dump(res, sys.stdout, indent=1)
