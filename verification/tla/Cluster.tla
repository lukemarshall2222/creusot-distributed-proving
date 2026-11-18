-------------------------------- MODULE Cluster --------------------------------
(* The Coordinator's protocol: joining, queueing, assignment, cancellation,
   lost peers and the reassignment of their Jobs. This is the model of the
   dp-core Coordinator machine (crates/dp-core/src/coord.rs); the actions
   have the names that the machine's steps carry in their trace records.

   Trace validation (R1.5 criterion 10): `trace_check.py` turns a trace of
   the Coordinator's shell into a behaviour and has TLC check that it is a
   behaviour of this specification.

   What the model leaves out: time. A peer is lost when the machine says so
   (a closed connection or a silence); heartbeats change nothing here. The
   soundness of labels is not this model's subject: see Coordinator.tla. *)
EXTENDS Naturals, FiniteSets

CONSTANTS
  Peers,     \* connections
  Jobs,      \* Job identifiers
  Provers,
  Retries,   \* how often a Job is given to another Worker after a loss
  None       \* no Job

VARIABLES
  frontEnds,   \* the connected Front_Ends
  workers,     \* the connected Workers
  offers,      \* worker -> the provers that it offers
  busy,        \* worker -> the Job that it runs, or None
  draining,    \* the Workers that the operator drains
  queue,       \* the Jobs that wait for a Worker
  owner,       \* held Job -> the Front_End that submitted it
  prover,      \* held Job -> its prover
  lost         \* held Job -> how many Workers that ran it were lost

vars == <<frontEnds, workers, offers, busy, draining, queue, owner, prover, lost>>

Running == {busy[w] : w \in workers} \ {None}
Held    == queue \cup Running

Init ==
  /\ frontEnds = {} /\ workers = {} /\ queue = {} /\ draining = {}
  /\ offers = [p \in Peers |-> {}]
  /\ busy = [p \in Peers |-> None]
  /\ owner = [j \in Jobs |-> CHOOSE p \in Peers : TRUE]
  /\ prover = [j \in Jobs |-> CHOOSE a \in Provers : TRUE]
  /\ lost = [j \in Jobs |-> 0]

FrontEndJoined(p) ==
  /\ p \notin frontEnds \cup workers
  /\ frontEnds' = frontEnds \cup {p}
  /\ UNCHANGED <<workers, offers, busy, draining, queue, owner, prover, lost>>

(* A Worker joins, or a connected Worker replaces its offer. *)
WorkerJoined(p, os) ==
  /\ p \notin frontEnds
  /\ workers' = workers \cup {p}
  /\ offers' = [offers EXCEPT ![p] = os]
  /\ UNCHANGED <<frontEnds, busy, draining, queue, owner, prover, lost>>

(* The operator drains a connected Worker: it gets no new Job (R1.7). The
   operator's other commands, remove and revoke, are the loss of the Worker. *)
Drain(p) ==
  /\ p \in workers
  /\ draining' = draining \cup {p}
  /\ UNCHANGED <<frontEnds, workers, offers, busy, queue, owner, prover, lost>>

(* Only a Front_End submits, and a Job is held once. *)
Submit(p, j, a) ==
  /\ p \in frontEnds /\ j \notin Held
  /\ queue' = queue \cup {j}
  /\ owner' = [owner EXCEPT ![j] = p]
  /\ prover' = [prover EXCEPT ![j] = a]
  /\ lost' = [lost EXCEPT ![j] = 0]
  /\ UNCHANGED <<frontEnds, workers, offers, busy, draining>>

(* A queued Job goes to an idle Worker that offers its prover. The machine
   also sends an assignment again to a Worker that already has it. *)
Assign(w, j) ==
  /\ w \in workers
  /\ \/ /\ j \in queue /\ busy[w] = None /\ prover[j] \in offers[w] /\ w \notin draining
        /\ queue' = queue \ {j}
        /\ busy' = [busy EXCEPT ![w] = j]
     \/ /\ busy[w] = j
        /\ UNCHANGED <<queue, busy>>
  /\ UNCHANGED <<frontEnds, workers, offers, draining, owner, prover, lost>>

(* No connected Worker that is not drained offers the prover of a queued
   Job: it fails. *)
FailNoWorker(j) ==
  /\ j \in queue
  /\ \A w \in workers \ draining : prover[j] \notin offers[w]
  /\ queue' = queue \ {j}
  /\ UNCHANGED <<frontEnds, workers, offers, busy, draining, owner, prover, lost>>

(* A Front_End cancels one of its Jobs, queued or running. *)
CancelCall(p, j) ==
  /\ p \in frontEnds
  /\ queue' = IF owner[j] = p THEN queue \ {j} ELSE queue
  /\ busy' = [w \in Peers |-> IF busy[w] = j /\ j # None /\ owner[j] = p THEN None ELSE busy[w]]
  /\ UNCHANGED <<frontEnds, workers, offers, draining, owner, prover, lost>>

(* The Worker that runs a Job ends it: with a Result, which goes to the
   Job's Front_End unread (Forward), or without one (JobNotRun). *)
Finish(w, j) ==
  /\ w \in workers /\ j # None /\ busy[w] = j
  /\ busy' = [busy EXCEPT ![w] = None]
  /\ UNCHANGED <<frontEnds, workers, offers, draining, queue, owner, prover, lost>>

Forward(w, j)   == Finish(w, j)
JobNotRun(w, j) == Finish(w, j)

(* A Worker is lost. Its Job waits for another Worker, until the retry
   limit; then it is dropped (the Front_End is told). *)
WorkerLost(p) ==
  /\ p \in workers
  /\ workers' = workers \ {p}
  /\ draining' = draining \ {p}
  /\ offers' = [offers EXCEPT ![p] = {}]
  /\ busy' = [busy EXCEPT ![p] = None]
  /\ LET j == busy[p] IN
       IF j # None /\ lost[j] < Retries
         THEN /\ queue' = queue \cup {j}
              /\ lost' = [lost EXCEPT ![j] = lost[j] + 1]
         ELSE UNCHANGED <<queue, lost>>
  /\ UNCHANGED <<frontEnds, owner, prover>>

(* A Front_End is lost: its queued Jobs go, its running Jobs are cancelled. *)
FrontEndLost(p) ==
  /\ p \in frontEnds
  /\ frontEnds' = frontEnds \ {p}
  /\ queue' = {j \in queue : owner[j] # p}
  /\ busy' = [w \in Peers |-> IF busy[w] # None /\ owner[busy[w]] = p THEN None ELSE busy[w]]
  /\ UNCHANGED <<workers, offers, draining, owner, prover, lost>>

PeerLost(p) == WorkerLost(p) \/ FrontEndLost(p)

(* Steps that change nothing here: heartbeats, ticks, discarded inputs, and
   the loss of a peer that is not connected. *)
Stutter == UNCHANGED vars

Next ==
  \/ \E p \in Peers : FrontEndJoined(p) \/ PeerLost(p) \/ Drain(p)
  \/ \E p \in Peers, os \in SUBSET Provers : WorkerJoined(p, os)
  \/ \E p \in Peers, j \in Jobs, a \in Provers : Submit(p, j, a)
  \/ \E p \in Peers, j \in Jobs : Assign(p, j) \/ CancelCall(p, j) \/ Forward(p, j) \/ JobNotRun(p, j)
  \/ \E j \in Jobs : FailNoWorker(j)

Spec == Init /\ [][Next]_vars

--------------------------------------------------------------------------------
TypeOK ==
  /\ frontEnds \subseteq Peers /\ workers \subseteq Peers
  /\ frontEnds \cap workers = {}
  /\ draining \subseteq workers
  /\ queue \subseteq Jobs
  /\ busy \in [Peers -> Jobs \cup {None}]
  /\ lost \in [Jobs -> 0..Retries]

(* A Job is in one place: queued, or with exactly one Worker. *)
OnePlace ==
  /\ queue \cap Running = {}
  /\ \A v, w \in workers : (v # w /\ busy[v] # None) => busy[v] # busy[w]

(* Only a connected Worker runs a Job. *)
OnlyWorkersRun == \A p \in Peers : busy[p] # None => p \in workers

(* The Coordinator holds Jobs of connected Front_Ends only (R1.5
   criterion 15): what a lost Front_End submitted is gone. *)
NoOrphans == \A j \in Held : owner[j] \in frontEnds

(* A Job is given to a Worker only while that Worker offers its prover
   (R1.7 criterion 2). *)
AssignedToOfferer ==
  [][\A w \in Peers : (busy'[w] # None /\ busy'[w] # busy[w]) => prover[busy'[w]] \in offers[w]]_vars

(* A Worker that the operator drains gets no new Job (R1.7 criterion 3). *)
DrainedGetNothing ==
  [][\A w \in Peers : (w \in draining /\ busy'[w] # busy[w]) => busy'[w] = None]_vars
================================================================================
