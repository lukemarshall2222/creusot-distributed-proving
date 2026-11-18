#!/usr/bin/env python3
"""Trace validation (R1.5 criterion 10): checks that a trace of the
Coordinator's shell is a behaviour of verification/tla/Cluster.tla.

    trace_check.py [--retries N] [--corrupt] [--keep] TRACE_FILE

The trace file has one canonical S-expression per step of the dp-core
Coordinator machine: component, sequence number, action, time, the input and
the effects. Each step becomes its action of the specification, followed by
one `Assign` or `FailNoWorker` action per such effect; steps that change
nothing in the model (heartbeats, ticks, discards) are left out. TLC then
searches for a behaviour of Cluster.tla that takes exactly these actions; the
invariants of Cluster.cfg are checked on the way.

Exit status 0: the trace is a behaviour of the specification. 1: it is not
(the step where it leaves the specification is printed). With --corrupt one
`Assign` is changed to another Worker first; the check must then fail, which
shows that the validation can fail.

Run inside the development shell, which provides `tlc`.
"""

import argparse
import os
import re
import shutil
import subprocess
import sys
import tempfile


def parse(data):
    """Parses canonical S-expressions: `N:bytes` atoms and lists."""
    pos = 0

    def value():
        nonlocal pos
        if data[pos:pos + 1] == b"(":
            pos += 1
            items = []
            while data[pos:pos + 1] != b")":
                items.append(value())
            pos += 1
            return items
        colon = data.index(b":", pos)
        length = int(data[pos:colon])
        atom = data[colon + 1:colon + 1 + length]
        pos = colon + 1 + length
        return atom.decode()

    out = []
    while pos < len(data):
        if data[pos:pos + 1] in b"\n":
            pos += 1
            continue
        out.append(value())
    return out


def field(text, name):
    m = re.search(r"\b%s: (\d+)" % name, text)
    return int(m.group(1)) if m else None


def events(records):
    """The actions of the specification that the trace takes, with the
    sequence number of the step that each comes from."""
    out = []
    for _, _, seq, action, _, inp, effects in records:
        peer, job = field(inp, "peer"), field(inp, "job")
        main = None
        if action == "FrontEndJoined":
            main = ("FrontEndJoined", peer, None, None, None)
        elif action == "WorkerJoined":
            offered = [int(p) for p in re.findall(r"Offer \{ prover: (\d+)", inp)]
            main = ("WorkerJoined", peer, None, None, offered)
        elif action == "Submit":
            main = ("Submit", peer, job, field(inp, "prover"), None)
        elif action in ("CancelCall", "Forward", "JobNotRun"):
            main = (action, peer, job, None, None)
        elif action == "PeerLost":
            main = ("PeerLost", peer, None, None, None)
        elif action not in ("Heartbeat", "Tick", "Discard"):
            raise SystemExit("step %s: unknown action %s" % (seq, action))
        if main:
            out.append((seq, main))
        # A silent peer is dropped and lost in the same step.
        for dropped in re.findall(r"Drop \{ peer: (\d+) \}", effects):
            out.append((seq, ("PeerLost", int(dropped), None, None, None)))
        for kind, body in re.findall(r"(Assign|Fail) \{([^}]*)\}", effects):
            if kind == "Assign":
                out.append((seq, ("Assign", field(body, "worker"), field(body, "job"), None, None)))
            elif "NoEligibleWorker" in body:
                out.append((seq, ("FailNoWorker", None, field(body, "job"), None, None)))
    return out


def module(evs, retries):
    peers = sorted({e[1] for _, e in evs if e[1] is not None})
    jobs = sorted({e[2] for _, e in evs if e[2] is not None})
    provers = sorted({e[3] for _, e in evs if e[3] is not None} | {p for _, e in evs for p in (e[4] or [])})
    q = lambda prefix, n: '"%s%d"' % (prefix, n)
    rows = []
    for _, (action, peer, job, prover, offered) in evs:
        rows.append('[a |-> "%s", p |-> %s, j |-> %s, x |-> %s, os |-> {%s}]' % (
            action,
            q("p", peer) if peer is not None else '"-"',
            q("j", job) if job is not None else '"-"',
            q("a", prover) if prover is not None else '"-"',
            ", ".join(q("a", p) for p in (offered or []))))
    sets = lambda prefix, xs: "{%s}" % ", ".join(q(prefix, x) for x in xs)
    return """---- MODULE TraceCluster ----
EXTENDS Cluster, Sequences
VARIABLE i
TracePeers == %s
TraceJobs == %s
TraceProvers == %s
TraceNone == "none"
Trace == <<
  %s
>>
Step(e) ==
  CASE e.a = "FrontEndJoined" -> FrontEndJoined(e.p)
    [] e.a = "WorkerJoined" -> WorkerJoined(e.p, e.os)
    [] e.a = "Submit" -> Submit(e.p, e.j, e.x)
    [] e.a = "Assign" -> Assign(e.p, e.j)
    [] e.a = "FailNoWorker" -> FailNoWorker(e.j)
    [] e.a = "CancelCall" -> CancelCall(e.p, e.j)
    [] e.a = "Forward" -> Forward(e.p, e.j)
    [] e.a = "JobNotRun" -> JobNotRun(e.p, e.j)
    [] e.a = "PeerLost" -> PeerLost(e.p) \\/ (e.p \\notin workers \\cup frontEnds /\\ UNCHANGED vars)
TraceNext == i <= Len(Trace) /\\ Step(Trace[i]) /\\ i' = i + 1
TraceSpec == Init /\\ i = 1 /\\ [][TraceNext]_<<vars, i>>
NotDone == i <= Len(Trace)
====
""" % (sets("p", peers), sets("j", jobs), sets("a", provers), ",\n  ".join(rows)), """SPECIFICATION TraceSpec
CONSTANTS
  Peers <- TracePeers
  Jobs <- TraceJobs
  Provers <- TraceProvers
  Retries = %d
  None <- TraceNone
INVARIANTS
  NotDone
  TypeOK
  OnePlace
  OnlyWorkersRun
  NoOrphans
""" % retries


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--retries", type=int, default=2)
    ap.add_argument("--corrupt", action="store_true")
    ap.add_argument("--keep", action="store_true", help="keep the generated module and TLC's output")
    ap.add_argument("trace")
    args = ap.parse_args()

    records = [r for r in parse(open(args.trace, "rb").read()) if r[0] == "trace" and r[1] == "coordinator"]
    evs = events(records)
    if not evs:
        raise SystemExit("%s: no step of the Coordinator changes the model" % args.trace)
    if args.corrupt:
        assigns = [k for k, (_, e) in enumerate(evs) if e[0] == "Assign"]
        workers = sorted({e[1] for _, e in evs if e[0] == "WorkerJoined"})
        if not assigns or len(workers) < 2:
            raise SystemExit("%s: nothing to corrupt" % args.trace)
        k = assigns[len(assigns) // 2]
        seq, (_, worker, job, _, _) = evs[k]
        other = next(w for w in workers if w != worker)
        # The same Job is assigned once more, to a Worker that the machine
        # did not choose.
        evs.insert(k + 1, (seq, ("Assign", other, job, None, None)))

    here = os.path.dirname(os.path.abspath(__file__))
    work = tempfile.mkdtemp(prefix="trace-check-")
    try:
        shutil.copy(os.path.join(here, "Cluster.tla"), work)
        tla, cfg = module(evs, args.retries)
        open(os.path.join(work, "TraceCluster.tla"), "w").write(tla)
        open(os.path.join(work, "TraceCluster.cfg"), "w").write(cfg)
        p = subprocess.run(
            ["tlc", "-deadlock", "-workers", "1", "TraceCluster.tla", "-config", "TraceCluster.cfg"],
            cwd=work, capture_output=True, text=True)
        out = p.stdout + p.stderr
        if args.keep:
            open(os.path.join(work, "tlc.out"), "w").write(out)
            print("kept in", work)
        violated = re.findall(r"Invariant (\w+) is violated", out)
        if violated == ["NotDone"]:
            print("%s: %d steps, %d actions of the specification: a behaviour of Cluster.tla"
                  % (args.trace, len(records), len(evs)))
            return 0
        if violated:
            print("%s: invariant %s is violated on the trace" % (args.trace, ", ".join(violated)))
            return 1
        if "Model checking completed. No error has been found." in out:
            depth = re.search(r"depth of the complete state graph search is (\d+)", out)
            k = int(depth.group(1)) if depth else 1
            seq, e = evs[min(k, len(evs)) - 1]
            print("%s: not a behaviour of Cluster.tla: after %d actions, step %s (%s) is not enabled"
                  % (args.trace, k - 1, seq, " ".join(str(x) for x in e if x is not None)))
            return 1
        print(out[-3000:])
        print("%s: TLC failed" % args.trace)
        return 2
    finally:
        if not args.keep:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
