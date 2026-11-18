#!/usr/bin/env python3
"""Test harness for the why3find dispatch-mode patch (design C2, D-3, AS-37).

The harness plays the parent process of `why3find prove --dispatch`: a
miniature Front_End that answers prover calls and cache accesses and uses the
bridge requests. It checks the behaviour that task 5.1 lists.

    dispatch_harness.py --patched BIN [--baseline BIN] PROJECT FILE

PROJECT is a directory with a why3find.json and FILE, a Why3 file in it. The
provers of the project configuration must be on PATH. The harness copies
PROJECT for every scenario and never writes to it.

The harness uses only the Python standard library. The message grammar is
documented in src/dispatch.mli of the patch.
"""

import argparse
import hashlib
import os
import select
import shutil
import subprocess
import sys
import tempfile
import time

# --- Canonical S-expressions: atoms are bytes, lists are Python lists. --------


def enc(x):
    if isinstance(x, list):
        return b"(" + b"".join(enc(i) for i in x) + b")"
    if isinstance(x, str):
        x = x.encode()
    return str(len(x)).encode() + b":" + x


class Incomplete(Exception):
    pass


def dec(buf, pos):
    if pos >= len(buf):
        raise Incomplete
    if buf[pos : pos + 1] == b"(":
        pos += 1
        items = []
        while True:
            if pos >= len(buf):
                raise Incomplete
            if buf[pos : pos + 1] == b")":
                return items, pos + 1
            item, pos = dec(buf, pos)
            items.append(item)
    colon = buf.find(b":", pos, pos + 11)
    if colon < 0:
        if len(buf) - pos < 11:
            raise Incomplete
        raise ValueError("atom length")
    n = int(buf[pos:colon])
    if colon + 1 + n > len(buf):
        raise Incomplete
    return buf[colon + 1 : colon + 1 + n], colon + 1 + n


# --- The parent side of the pipe. ---------------------------------------------


class Session:
    """One run of `why3find prove --dispatch` with a policy for prover calls.

    policy(session, rid, gid, step, prover, time_us) is called for every prove
    message and must eventually call session.answer(rid, result).
    """

    def __init__(self, binary, project, file, policy, extra_args=()):
        self.dir = tempfile.mkdtemp(prefix="dispatch-")
        self.project = os.path.join(self.dir, "p")
        shutil.copytree(project, self.project)
        self.file = file
        self.policy = policy
        self.proc = subprocess.Popen(
            [binary, "prove", "--dispatch", *extra_args, file],
            cwd=self.project,
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        os.set_blocking(self.proc.stdout.fileno(), False)
        self.buf = b""
        self.log = []  # every message from why3find
        self.goals = {}  # gid -> (file, theory, name, path, task)
        self.proves = []  # (rid, gid, step, prover, time_us)
        self.cancelled = set()
        self.cache_sets = []  # (gid, prover, result)
        self.replies = {}  # qid -> reply to a bridge request
        self.done = False
        self.eof = False
        self.jobs = {}  # rid -> (Popen, deadline, start, tmpfile)
        self.qid = 0
        self.pending = []  # prove messages not yet given to the policy
        self.in_policy = False
        self.on_goal = None
        self.store_policy = lambda result: result[0] == b"valid"

    # sending

    def send(self, msg):
        # After a scenario closes the pipe, or why3find exits, messages that
        # were already read can still ask for answers; drop those answers.
        if self.proc.stdin.closed:
            return
        try:
            self.proc.stdin.write(enc(msg))
            self.proc.stdin.flush()
        except BrokenPipeError:
            pass

    def answer(self, rid, result):
        self.send(["answer", rid, result])

    def request(self, *fields):
        """Send a bridge request and wait for its reply."""
        self.qid += 1
        qid = b"q%d" % self.qid
        self.send([fields[0], qid, *fields[1:]])
        while qid not in self.replies:
            if self.eof:
                raise RuntimeError("why3find exited during a request")
            self.step(0.05)
        return self.replies.pop(qid)

    # receiving

    def handle(self, msg):
        self.log.append(msg)
        tag = msg[0]
        if tag == b"goal":
            _, gid, file, theory, name, path, task = msg
            self.goals[gid] = (file, theory, name, path, task)
            if self.on_goal:
                self.on_goal(self, gid)
        elif tag == b"cache-get":
            self.send(["cache", msg[1], ["noresult"]])
        elif tag == b"prove":
            _, rid, gid, step, prover, t = msg
            self.proves.append((rid, gid, step, prover, int(t)))
            self.pending.append((rid, gid, step, prover, int(t)))
        elif tag == b"cancel":
            self.cancelled.add(msg[1])
            job = self.jobs.pop(msg[1], None)
            if job:
                job[0].kill()
                job[0].wait()
                os.unlink(job[3])
                self.answer(msg[1], ["noresult"])
        elif tag == b"cache-set":
            _, rid, gid, prover, result = msg
            self.cache_sets.append((gid, prover, result))
            self.send(["stored", rid, "store" if self.store_policy(result) else "skip"])
        elif tag == b"done":
            self.done = True
        elif tag in (b"printed", b"task", b"provers", b"calibration", b"error"):
            self.replies[msg[1]] = msg
        elif tag != b"hello":
            raise ValueError("unexpected message %r" % tag)

    def step(self, timeout):
        """Read available messages, then poll running solver jobs."""
        ready, _, _ = select.select([self.proc.stdout], [], [], timeout)
        if ready:
            data = self.proc.stdout.read()
            if data == b"":
                self.eof = True
            elif data:
                self.buf += data
                pos = 0
                while True:
                    try:
                        msg, pos = dec(self.buf, pos)
                    except Incomplete:
                        break
                    self.handle(msg)
                self.buf = self.buf[pos:]
        # The policy may send bridge requests, which read further messages;
        # prover calls that arrive meanwhile wait in the queue.
        if not self.in_policy:
            self.in_policy = True
            try:
                while self.pending:
                    self.policy(self, *self.pending.pop(0))
            finally:
                self.in_policy = False
        for rid, (proc, deadline, start, tmp) in list(self.jobs.items()):
            if proc.poll() is not None:
                out = proc.stdout.read().decode(errors="replace")
                us = str(int((time.time() - start) * 1e6))
                first = out.strip().splitlines()[0] if out.strip() else ""
                if first == "unsat" or "Valid" in out:
                    result = ["valid", us]
                else:
                    result = ["unknown", us]
            elif time.time() > deadline:
                proc.kill()
                proc.wait()
                result = ["timeout", str(int((deadline - start) * 1e6))]
            else:
                continue
            del self.jobs[rid]
            os.unlink(tmp)
            self.answer(rid, result)

    def run(self, limit=300):
        end = time.time() + limit
        while not self.eof:
            if time.time() > end:
                self.proc.kill()
                raise RuntimeError("timeout waiting for why3find")
            self.step(0.02)
        self.stderr = self.proc.stderr.read().decode(errors="replace")
        self.status = self.proc.wait()
        return self.status

    def proofs(self):
        """Contents of the proof files, by relative path."""
        out = {}
        for root, _, files in os.walk(self.project):
            if ".why3find" in root.split(os.sep):
                continue
            for f in files:
                if f.endswith(".json") and f != "why3find.json":
                    p = os.path.join(root, f)
                    out[os.path.relpath(p, self.project)] = open(p, "rb").read()
        return out

    def close(self):
        shutil.rmtree(self.dir, ignore_errors=True)


# --- Policies. ----------------------------------------------------------------


def scripted(result):
    return lambda s, rid, gid, step, prover, t: s.answer(rid, result)


class RealProvers:
    """Answer prover calls by running the provers on the bridge's prints."""

    def __init__(self):
        self.commands = None

    def __call__(self, s, rid, gid, step, prover, t):
        if self.commands is None:
            reply = s.request("provers")
            self.commands = {p[0]: p[3].decode() for p in reply[2:]}
        printed = s.request("print", gid, prover, [])
        assert printed[0] == b"printed" and printed[2] == b"1", printed[:3]
        if rid in s.cancelled:
            s.answer(rid, ["noresult"])
            return
        fd, tmp = tempfile.mkstemp(prefix="goal-", dir=s.dir)
        os.write(fd, printed[3])
        os.close(fd)
        seconds = max(1, -(-t // 1000000))
        command = self.commands[prover]
        for key, value in (
            ("%f", tmp),
            ("%t", str(seconds)),
            ("%T", str(seconds + 1)),
            ("%U", str(seconds * 1000)),
            ("%m", "1000"),
            ("%M", "1000000"),
            ("%b", str(1000 * 1024 * 1024)),
        ):
            command = command.replace(key, value)
        proc = subprocess.Popen(
            command, shell=True, stdout=subprocess.PIPE, stderr=subprocess.STDOUT
        )
        now = time.time()
        s.jobs[rid] = (proc, now + t / 1e6 * 2 + 1, now, tmp)


# --- Scenarios. ---------------------------------------------------------------

CHECKS = []


def check(name, cond, detail=""):
    CHECKS.append((name, bool(cond)))
    print("%s  %s%s" % ("ok  " if cond else "FAIL", name, (": " + detail) if detail and not cond else ""))


def proved_goals(stderr):
    """Lines of the final report, which name failed goals and theory totals."""
    return sorted(l.strip() for l in stderr.splitlines() if l.startswith(("Goal ", "Theory ")))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--patched", required=True)
    ap.add_argument("--baseline")
    ap.add_argument("project")
    ap.add_argument("file")
    args = ap.parse_args()
    patched = os.path.abspath(args.patched)

    def session(policy, **kw):
        return Session(patched, args.project, args.file, policy, extra_args=["-f"], **kw)

    # 1. Every answer Valid: every goal is proved, from the answers alone.
    s = session(scripted(["valid", "5000"]))
    status = s.run()
    check("all-valid: exit status 0", status == 0, str(status))
    check("all-valid: done message", s.done)
    check("all-valid: hello first", s.log[0] == [b"hello", b"1"])
    check("all-valid: no failed goal reported", "proof failed" not in s.stderr)
    check("all-valid: first prover call of every goal is the fast step",
          all(next(p for p in s.proves if p[1] == g)[2] == b"fast" for g in {p[1] for p in s.proves}))
    check("all-valid: proof file written", len(s.proofs()) == 1)
    check("all-valid: Valid results offered to the cache",
          len(s.cache_sets) > 0 and all(r[0] == b"valid" for _, _, r in s.cache_sets))
    cached = [f for _, _, files in os.walk(os.path.join(s.project, ".why3find")) for f in files]
    valid_proofs = s.proofs()
    s.close()

    # 2. Every answer Unknown: nothing is proved, tactics create sub-goals, and
    #    nothing reaches the legacy cache layout.
    s = session(scripted(["unknown", "5000"]))
    status = s.run()
    steps = {p[2] for p in s.proves}
    check("all-unknown: exit status 1", status == 1, str(status))
    check("all-unknown: no goal proved", b"prover" not in b"".join(s.proofs().values()))
    check("all-unknown: strategy steps fast, parallel, parallel-long",
          steps == {b"fast", b"parallel", b"parallel-long"}, repr(steps))
    check("all-unknown: tactic sub-goals announced with a path",
          any(g[3] for g in s.goals.values()))
    check("all-unknown: every prover call names an announced goal",
          all(p[1] in s.goals for p in s.proves))
    check("all-unknown: cache answers skip keep the legacy cache empty of results",
          not any(len(f) == 32 for _, _, files in os.walk(os.path.join(s.project, ".why3find")) for f in files))
    tasks_a = {(g[0], g[1], g[2], enc(g[3])): hashlib.sha256(g[4]).hexdigest() for g in s.goals.values()}
    s.close()

    # 3. Bridge requests, asked while the first goal's prover call is open.
    seen = {}

    def bridge(s, rid, gid, step, prover, t):
        if not seen:
            seen["provers"] = s.request("provers")
            seen["task"] = s.request("task-print", gid)
            seen["print1"] = s.request("print", gid, prover, [])
            seen["print2"] = s.request("print", gid, prover, [])
            seen["bad-goal"] = s.request("print", "no-such-goal", prover, [])
            seen["bad-prover"] = s.request("print", gid, "no-such-prover", [])
            seen["calibration"] = s.request("calibration", prover)
            seen["gid"] = gid
            seen["split"] = {}
        name = s.goals[gid][2]
        if name not in seen["split"] and not s.goals[gid][3]:
            seen["split"][name] = s.request("print", gid, prover, ["split_vc"])
        s.answer(rid, ["unknown", "5000"])

    s = session(bridge)
    status = s.run()
    provers = seen["provers"][2:]
    check("bridge: provers lists name, digest, driver and command",
          len(provers) > 0 and all(len(p) == 4 and b"%f" in p[3] for p in provers), repr(provers)[:200])
    check("bridge: task-print equals the announced task", seen["task"][2] == s.goals[seen["gid"]][4])
    check("bridge: print gives one task and the prover input",
          seen["print1"][2] == b"1" and len(seen["print1"][3]) > 0)
    check("bridge: print is repeatable", seen["print1"][3] == seen["print2"][3])
    check("bridge: unknown goal and prover give an error reply",
          seen["bad-goal"][0] == b"error" and seen["bad-prover"][0] == b"error")
    check("bridge: calibration gives a positive velocity",
          seen["calibration"][0] == b"calibration" and int(seen["calibration"][2]) > 0,
          repr(seen["calibration"]))
    counts = {n: int(r[2]) for n, r in seen["split"].items() if r[0] == b"printed"}
    check("bridge: a splitting pipeline reports several tasks and no bytes",
          any(c > 1 for c in counts.values())
          and all((r[3] == b"") == (int(r[2]) != 1) for r in seen["split"].values() if r[0] == b"printed"),
          repr(counts))
    tasks_b = {(g[0], g[1], g[2], enc(g[3])): hashlib.sha256(g[4]).hexdigest() for g in s.goals.values()}
    check("task print is identical in two runs (AS-33, same machine)", tasks_a == tasks_b)
    s.close()

    # 4. Closed pipe: fatal, status 3, no proof file.
    def hang_up(s, rid, gid, step, prover, t):
        s.proc.stdin.close()

    s = session(hang_up)
    status = s.run()
    check("closed pipe: exit status 3", status == 3, str(status))
    check("closed pipe: no proof file written", s.proofs() == {})
    s.close()

    # 5. Malformed message: fatal, status 3, no proof file.
    def garbage(s, rid, gid, step, prover, t):
        s.proc.stdin.write(b"hello world")
        s.proc.stdin.flush()

    s = session(garbage)
    status = s.run()
    check("malformed message: exit status 3", status == 3, str(status))
    check("malformed message: no proof file written", s.proofs() == {})
    s.close()

    # 6. Cancellation: the parallel pass cancels the other provers after a proof.
    def first_prover_wins(s, rid, gid, step, prover, t):
        if step == b"fast":
            s.answer(rid, ["unknown", "5000"])
        elif s.winner is None:
            s.winner = rid
            s.answer(rid, ["valid", "5000"])
        elif len([p for p in s.proves if p[1] == gid and p[2] == step]) > 1 and gid == [p for p in s.proves if p[0] == s.winner][0][1]:
            s.held.append(rid)  # answered only after its cancel arrives
        else:
            s.answer(rid, ["unknown", "5000"])

    s = session(first_prover_wins)
    s.held, s.winner = [], None
    for _ in range(200):
        s.step(0.02)
        for rid in list(s.held):
            if rid in s.cancelled:
                s.held.remove(rid)
                s.answer(rid, ["noresult"])
        if s.eof:
            break
    s.run()
    check("cancel: the losing prover calls of a won parallel pass are cancelled",
          len(s.cancelled) > 0 and s.done, "cancelled=%d done=%s" % (len(s.cancelled), s.done))
    s.close()

    # 7. Real provers through the bridge, against unpatched why3find.
    if args.baseline:
        base_dir = tempfile.mkdtemp(prefix="baseline-")
        base_project = os.path.join(base_dir, "p")
        shutil.copytree(args.project, base_project)
        base = subprocess.run([os.path.abspath(args.baseline), "prove", "-f", args.file],
                              cwd=base_project, capture_output=True)
        legacy = subprocess.run([patched, "prove", "-f", args.file],
                                cwd=shutil.copytree(args.project, os.path.join(base_dir, "q")),
                                capture_output=True)
        check("legacy mode: patched build without --dispatch agrees with unpatched why3find",
              base.returncode == legacy.returncode
              and proved_goals(base.stdout.decode()) == proved_goals(legacy.stdout.decode()))
        s = session(RealProvers())
        status = s.run()
        check("real provers: same exit status as unpatched why3find",
              status == base.returncode, "%d vs %d" % (status, base.returncode))
        check("real provers: same failed goals and theory totals as unpatched why3find",
              proved_goals(s.stderr) == proved_goals(base.stdout.decode()),
              "%r vs %r" % (proved_goals(s.stderr), proved_goals(base.stdout.decode())))
        check("real provers: no output on the protocol channel besides messages", s.buf == b"")
        s.close()
        shutil.rmtree(base_dir, ignore_errors=True)

    failed = [n for n, ok in CHECKS if not ok]
    print("%d checks, %d failed" % (len(CHECKS), len(failed)))
    sys.exit(1 if failed else 0)


if __name__ == "__main__":
    main()
