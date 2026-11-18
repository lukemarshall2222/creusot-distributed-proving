"""Hand-composed certificate: a Lemma proved in its own scope, then used by the target proof.

    compose.py CVC5 ETHOS CPC_SIGNATURE_DIR      (writes its files to the current directory)

Goal B (the reference):   A1: forall x. f(x) >= x      A2: f(a+1) < a
Lemma L:                  f(a+1) >= a+1                 Dependency_Set {A1}
Lemma problem:            {A1, not L}                   -> cvc5 proof P_L
Extended problem:         {A1, A2, L}                   -> cvc5 proof P_T
Certificate:              assume-push (not L); P_L; step-pop scope; process_scope;
                          not_not_elim gives L; P_T with its assumption of L replaced.
"""
import re, subprocess, sys
cvc5, ethos, sig = sys.argv[1:4]
DECL = "(set-logic UFLIA)\n(declare-fun f (Int) Int)\n(declare-const a Int)\n"
A1 = "(forall ((x Int)) (>= (f x) x))"
A2 = "(< (f (+ a 1)) a)"
L = "(>= (f (+ a 1)) (+ a 1))"

def problem(name, asserts):
    open(name, "w").write(DECL + "".join("(assert %s)\n" % a for a in asserts) + "(check-sat)\n")

def proof(name):
    out = subprocess.run([cvc5, "--dump-proofs", "--proof-format=cpc", "--no-proof-dag-global",
                          "--no-proof-allow-trust", "--dag-thresh=0", name], capture_output=True, text=True).stdout.split("\n")
    assert out[0] == "unsat", out[0]
    return [l for l in out[1:] if l.startswith("(") and l not in ("(",) and not l.startswith("(declare-")]

def ethos_check(cert, reference):
    p = subprocess.run([ethos, "--include=" + sig + "/Cpc.eo", "--reference=" + reference,
                        "--require-proof-of-false", cert], capture_output=True, text=True)
    err = [l for l in p.stderr.split("\n") if l.strip() and "Fatal" not in l]
    return (p.stdout.strip().split("\n")[-1] if p.returncode == 0 else re.sub(r"/\S*/", "", err[-1])[:150])

problem("goal.smt2", [A1, A2])
problem("lemma.smt2", [A1, "(not %s)" % L])
problem("extended.smt2", [A1, A2, L])
PL, PT = proof("lemma.smt2"), proof("extended.smt2")

def assumes(p):
    return {m.group(1): m.group(2) for l in p for m in [re.match(r"\(assume (\S+) (.*)\)$", l)] if m}
print("lemma proof:  %d steps, assumes %s" % (sum(l.startswith("(step") for l in PL), list(assumes(PL).values())))
print("target proof: %d steps, assumes %s" % (sum(l.startswith("(step") for l in PT), list(assumes(PT).values())))

def rename(line, prefix):
    return re.sub(r"@p(\d+)", lambda m: "@%s%s" % (prefix, m.group(1)), line)

def compose(lemma_formula=L, drop_scope=False):
    cert, seen = [], {}
    pl = [rename(l, "l") for l in PL]
    notL = "(not %s)" % L
    nl = [n for n, f in assumes(pl).items() if f == notL]
    assert len(nl) == 1, "lemma proof must assume (not L) once"
    # assumptions of the Lemma proof other than (not L): Assertions of the goal, named once
    for n, f in assumes(pl).items():
        if f != notL:
            cert.append("(assume %s %s)" % (n, f)); seen[f] = n
    if not drop_scope:
        cert.append("(assume-push %s (not %s))" % (nl[0], lemma_formula))
    else:
        cert.append("(assume %s (not %s))" % (nl[0], lemma_formula))
    body = [l for l in pl if not l.startswith("(assume ")]
    cert += body
    last = re.match(r"\(step(?:-pop)? (\S+)", body[-1]).group(1)
    if not drop_scope:
        cert.append("(step-pop @c1 :rule scope :premises (%s))" % last)
        cert.append("(step @c2 :rule process_scope :premises (@c1) :args (false))")
        cert.append("(step @c3 :rule not_not_elim :premises (@c2))")
    # target proof: its assumption of L becomes a reference to the derived Lemma
    pt = [rename(l, "t") for l in PT]
    for n, f in assumes(pt).items():
        if f == L:
            pt = [l for l in pt if not l.startswith("(assume %s " % n)]
            pt = [re.sub(re.escape(n) + r"(?![0-9])", "@c3", l) for l in pt]
        elif f in seen:
            pt = [l for l in pt if not l.startswith("(assume %s " % n)]
            pt = [re.sub(re.escape(n) + r"(?![0-9])", seen[f], l) for l in pt]
    return cert + pt

open("cert.cpc", "w").write("\n".join(compose()) + "\n")
print("composed certificate against the goal:        ", ethos_check("cert.cpc", "goal.smt2"))
# controls
open("pt_only.cpc", "w").write("\n".join(rename(l, "t") for l in PT) + "\n")
print("target proof alone (assumes L) against goal:   ", ethos_check("pt_only.cpc", "goal.smt2"))
open("noscope.cpc", "w").write("\n".join(compose(drop_scope=True)) + "\n")
print("Lemma proof inlined without a scope:           ", ethos_check("noscope.cpc", "goal.smt2"))
open("wrong.cpc", "w").write("\n".join(compose(lemma_formula="(>= (f (+ a 1)) (+ a 2))")) + "\n")
print("scope opened on a different (false) Lemma:     ", ethos_check("wrong.cpc", "goal.smt2"))
problem("goal_no_a1.smt2", [A2])
print("certificate against a goal without A1:         ", ethos_check("cert.cpc", "goal_no_a1.smt2"))
