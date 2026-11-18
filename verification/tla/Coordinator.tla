------------------------------ MODULE Coordinator ------------------------------
(* The protocol of the first distributed slice: a Front_End, an untrusted
   Coordinator and Workers. One Job per prover call, Unpatched_Solvers only;
   no heartbeats, Races, certification or Lemma traffic yet.

   The Coordinator and the network are adversarial: any Result that the
   attacker can build may reach the Front_End, in any order, any number of
   times, so the network is not a variable: the Front_End may receive any
   buildable Result at any step. What the attacker cannot build is a Result
   signed by a registered honest Worker that the Worker did not sign (TB-2).

   A Worker's solver is modelled as sound for the proved-standalone route
   (NG-3): an honest Worker signs "proof" only for a goal that is valid. *)
EXTENDS Naturals, FiniteSets

CONSTANTS
  Goals,          \* the Goal_Versions of the Session
  ValidGoals,     \* the goals whose Printed_Goal is unsatisfiable
  Workers,        \* registered honest Workers
  Rogue,          \* an identity that is not in the registry
  MaxJobs         \* bound on the Jobs the Front_End submits

ASSUME ValidGoals \subseteq Goals /\ Rogue \notin Workers /\ MaxJobs \in Nat

Answers == {"proof", "noproof"}
JobIds  == 1..MaxJobs

(* A Result names its Job, the task it ran on, its answer and its signer.
   "signer" is the key that made the tag; "worker" is the name it carries. *)
Results == [job : JobIds, task : Goals, answer : Answers, worker : Workers \cup {Rogue},
            signer : Workers \cup {Rogue}]

VARIABLES
  submitted,   \* job id -> goal, for the Jobs the Front_End submitted
  open,        \* the submitted Jobs that have not ended
  queue,       \* Jobs at the Coordinator, not yet assigned
  running,     \* worker -> job id, or 0
  signedBy,    \* the Results that honest Workers signed
  verdict      \* goal -> "none" or "proved-standalone"

vars == <<submitted, open, queue, running, signedBy, verdict>>

Init ==
  /\ submitted = [j \in {} |-> CHOOSE g \in Goals : TRUE]
  /\ open = {}
  /\ queue = {}
  /\ running = [w \in Workers |-> 0]
  /\ signedBy = {}
  /\ verdict = [g \in Goals |-> "none"]

(* The Front_End submits a Job for a goal that has no Verdict yet. *)
Submit(g) ==
  /\ verdict[g] = "none"
  /\ Cardinality(DOMAIN submitted) < MaxJobs
  /\ LET j == Cardinality(DOMAIN submitted) + 1 IN
       /\ submitted' = [k \in DOMAIN submitted \cup {j} |->
                          IF k = j THEN g ELSE submitted[k]]
       /\ open' = open \cup {j}
       /\ queue' = queue \cup {j}
  /\ UNCHANGED <<running, signedBy, verdict>>

(* The Coordinator assigns a queued Job to an idle Worker. It may hand the
   Worker any task, not the one that was submitted. *)
Assign(w, j, g) ==
  /\ j \in queue /\ running[w] = 0
  /\ queue' = queue \ {j}
  /\ running' = [running EXCEPT ![w] = j]
  \* The Worker runs its solver on the task it was given and signs what the
  \* solver answered: a proof only if that task is valid.
  /\ \E a \in Answers :
       /\ (a = "proof") => (g \in ValidGoals)
       /\ signedBy' = signedBy \cup
            {[job |-> j, task |-> g, answer |-> a, worker |-> w, signer |-> w]}
  /\ UNCHANGED <<submitted, open, verdict>>

(* The Worker finishes its Job and becomes idle. *)
Report(w) ==
  /\ running[w] # 0
  /\ running' = [running EXCEPT ![w] = 0]
  /\ UNCHANGED <<submitted, open, queue, signedBy, verdict>>

(* What the attacker can deliver: anything signed with the rogue key, under
   any name, and any copy of a Result that an honest Worker signed. *)
Buildable(r) == r \in Results /\ (r.signer = Rogue \/ r \in signedBy)

(* accept_remote of dp-core: registered Worker, valid tag for that Worker,
   an open Job that the Front_End submitted, and the Task_Hash of the local
   print of that Job's goal. *)
Accepted(r) ==
  /\ r.worker \in Workers
  /\ r.signer = r.worker
  /\ r.job \in open
  /\ r.task = submitted[r.job]

(* The Front_End receives a Result. An authentic Result ends
   its Job; an accepted proof gives the goal its Verdict, which never changes. *)
Receive(r) ==
  /\ Buildable(r)
  /\ IF r.worker \in Workers /\ r.signer = r.worker /\ r.job \in open
       THEN /\ open' = open \ {r.job}
            /\ verdict' = IF Accepted(r) /\ r.answer = "proof"
                               /\ verdict[submitted[r.job]] = "none"
                            THEN [verdict EXCEPT ![submitted[r.job]] = "proved-standalone"]
                            ELSE verdict
       ELSE UNCHANGED <<open, verdict>>
  /\ UNCHANGED <<submitted, queue, running, signedBy>>

Next ==
  \/ \E g \in Goals : Submit(g)
  \/ \E w \in Workers, j \in JobIds, g \in Goals : Assign(w, j, g)
  \/ \E w \in Workers : Report(w)
  \/ \E r \in Results : Receive(r)

Spec == Init /\ [][Next]_vars

--------------------------------------------------------------------------------
TypeOK ==
  /\ DOMAIN submitted \subseteq JobIds
  /\ open \subseteq DOMAIN submitted
  /\ queue \subseteq DOMAIN submitted
  /\ signedBy \subseteq Results
  /\ verdict \in [Goals -> {"none", "proved-standalone"}]

(* R7.1 for this slice: a goal is proved only if it is valid, whatever the
   Coordinator and the network do. *)
VerdictSound == \A g \in Goals : verdict[g] = "proved-standalone" => g \in ValidGoals

(* R1.5 criteria 1 and 8: a Verdict, once recorded, never changes. *)
VerdictFinal == [][\A g \in Goals : verdict[g] # "none" => verdict'[g] = verdict[g]]_vars

(* A Job ends at most once: a closed Job is never open again. *)
JobsEndOnce == [][\A j \in DOMAIN submitted : j \notin open => j \notin open']_vars
================================================================================
