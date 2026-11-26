/-
The reference definitions of the decision functions, and the trace model
of a Session.

`classify`, `label`, `labelFromChecker` and `acceptRemote` are the
functions of the same names in `crates/dp-core/src/{labels,verdict}.rs`,
over the facts that those functions read. They are executable:
`DP/Ref/Main.lean` runs them, and `tests/diff/label_lean_vs_rust/`
compares the Rust functions with them on every input.
-/
import DP.Model

namespace DP

inductive Answer where
  | valid | invalid | timeout | outOfMemory | stepLimitExceeded | unknown | failure | highFailure
  deriving DecidableEq, Repr

/-- A non-proof outcome, most informative first (R7.2). -/
inductive Outcome where
  | standaloneInvalid | checkedIncomplete | unchecked | dependentInvalid
  | timeout | unknown | failed | unresolved
  deriving DecidableEq, Repr

inductive Label where
  | provedStandalone | certified | nonProof (o : Outcome)
  deriving DecidableEq, Repr

/-- What `label` reads of a Result and of the Front_End's knowledge of
its Job. -/
structure ResultFacts where
  answer : Answer
  /-- The Result's Task_Hash is that of the local print. -/
  taskHashMatches : Bool
  /-- The Transformation_Pipeline yields exactly one task. -/
  oneTask : Bool
  partition : Bool
  importMark : Bool
  /-- The Result lists no imports. -/
  importsEmpty : Bool
  /-- Its import-set hash is the hash of the empty set. -/
  importsHashEmpty : Bool
  /-- The Lock_File lists its solver binary as an Unpatched_Solver for its
  prover and version. -/
  unpatched : Bool
  /-- The local solver binary differed from the Lock_File. -/
  binaryMismatch : Bool
  certifyAll : Bool

def bound (f : ResultFacts) : Bool := f.taskHashMatches && f.oneTask

def lemmaDependent (f : ResultFacts) : Bool :=
  f.importMark || !f.importsEmpty || !f.importsHashEmpty || !f.unpatched || f.binaryMismatch

/-- Every condition of R7.1 criterion 2. -/
def standalone (f : ResultFacts) : Bool :=
  !f.certifyAll && f.answer == Answer.valid && bound f && !f.partition && !lemmaDependent f

def classify (a : Answer) (dependent : Bool) : Option Outcome :=
  match a with
  | .valid => none
  | .invalid => some (if dependent then .dependentInvalid else .standaloneInvalid)
  | .timeout => some .timeout
  | .outOfMemory | .stepLimitExceeded | .unknown => some .unknown
  | .failure | .highFailure => some .failed

def label (f : ResultFacts) : Label :=
  match classify f.answer (lemmaDependent f) with
  | some o => .nonProof o
  | none => if standalone f then .provedStandalone else .nonProof .unchecked

/-- What the shell established about a Result from the Cluster. -/
structure RemoteFacts where
  signatureValid : Bool
  registered : Bool
  revoked : Bool
  submitted : Bool
  limitsConsistent : Bool
  importsConsistent : Bool

def authentic (r : RemoteFacts) : Bool :=
  r.signatureValid && r.registered && !r.revoked && r.submitted && r.limitsConsistent && r.importsConsistent

/-- `accept_remote`: accepted exactly when authentic and bound. -/
def acceptRemote (f : ResultFacts) (r : RemoteFacts) : Bool := authentic r && bound f

/-- What the shell established about a certificate. -/
structure CheckFacts where
  produced : Bool
  checkerPinned : Bool
  signaturePinned : Bool
  boundToLocalPrint : Bool
  accepted : Bool
  complete : Bool
  stored : Bool

def certifies (c : CheckFacts) : Bool :=
  c.produced && c.checkerPinned && c.signaturePinned && c.boundToLocalPrint && c.accepted && c.complete && c.stored

def labelFromChecker (c : CheckFacts) : Label :=
  let checked := c.produced && c.checkerPinned && c.signaturePinned && c.boundToLocalPrint && c.accepted
  if checked && c.complete && c.stored then .certified
  else if checked then .nonProof .checkedIncomplete
  else .nonProof .unchecked

/-! ### The trace model -/

/-- What a Session knows by itself: its Goals, how it prints them, its
Lock_File and its certify-all setting. -/
structure Session (W : World) where
  Goal : Type
  decGoal : DecidableEq Goal
  /-- The Front_End's own print of the Goal. -/
  printLocal : Goal → W.Problem
  /-- The Transformation_Pipeline yields one task for the Goal. -/
  oneTask : Goal → Bool
  /-- Task_Hashes can be compared. -/
  hashEq : W.Hash → W.Hash → Bool
  hashEq_iff : ∀ a b, hashEq a b = true ↔ a = b
  /-- The Lock_File marks the solver as an Unpatched_Solver. -/
  unpatched : W.Solver → Bool
  certifyAll : Bool

/-- A Result as it reaches the Verdict_Recorder: what it claims, and for a
Result from the Cluster what the shell established about it. -/
structure ResultEvent (W : World) where
  answer : Answer
  taskHash : W.Hash
  /-- The solver that the Result names: prover, version, binary hash. -/
  solver : W.Solver
  partition : Bool
  importMark : Bool
  importsEmpty : Bool
  importsHashEmpty : Bool
  binaryMismatch : Bool
  /-- `none` for a run that the Front_End started on this machine. -/
  remote : Option RemoteFacts

/-- A run of the checker on a certificate. -/
structure CheckEvent (W : World) where
  cert : W.Cert
  /-- The Task_Hash of the reference file that the checker was given. -/
  referenceHash : W.Hash
  produced : Bool
  checkerPinned : Bool
  signaturePinned : Bool
  /-- What the checker that ran printed: accept or not. -/
  accepted : Bool
  complete : Bool
  stored : Bool

inductive Event (W : World) (S : Session W) where
  | result (g : S.Goal) (r : ResultEvent W)
  | check (g : S.Goal) (c : CheckEvent W)

variable {W : World} {S : Session W}

/-- The facts that the Front_End computes for a Result of Goal `g`. -/
def resultFacts (S : Session W) (g : S.Goal) (r : ResultEvent W) : ResultFacts where
  answer := r.answer
  taskHashMatches := S.hashEq r.taskHash (W.hash (S.printLocal g))
  oneTask := S.oneTask g
  partition := r.partition
  importMark := r.importMark
  importsEmpty := r.importsEmpty
  importsHashEmpty := r.importsHashEmpty
  unpatched := S.unpatched r.solver
  binaryMismatch := r.binaryMismatch
  certifyAll := S.certifyAll

/-- `accept_result` for a local run, `accept_remote` for a Result from the
Cluster. -/
def accepted (S : Session W) (g : S.Goal) (r : ResultEvent W) : Bool :=
  match r.remote with
  | none => bound (resultFacts S g r)
  | some f => acceptRemote (resultFacts S g r) f

/-- The facts of a checker run for Goal `g`: the reference is bound to
the local print when its hash is that of the print and the print is one
task. -/
def checkFacts (S : Session W) (g : S.Goal) (c : CheckEvent W) : CheckFacts where
  produced := c.produced
  checkerPinned := c.checkerPinned
  signaturePinned := c.signaturePinned
  boundToLocalPrint := S.hashEq c.referenceHash (W.hash (S.printLocal g)) && S.oneTask g
  accepted := c.accepted
  complete := c.complete
  stored := c.stored

/-- The proof label that an event gives its Goal, if any (`decide` and
`label_from_checker_output`). -/
def proofLabel (S : Session W) : Event W S → Option (S.Goal × Label)
  | .result g r =>
    if accepted S g r && label (resultFacts S g r) == Label.provedStandalone then
      some (g, .provedStandalone) else none
  | .check g c =>
    if labelFromChecker (checkFacts S g c) == Label.certified then some (g, .certified) else none

/-- The Verdicts of a Session: at most one per Goal. -/
def Verdicts (S : Session W) := S.Goal → Option Label

/-- `record_verdict`: the first Verdict of a Goal stays. -/
def record (S : Session W) (v : Verdicts S) (g : S.Goal) (l : Label) : Verdicts S :=
  fun g' => match v g' with
    | some old => some old
    | none => if S.decGoal g' g |>.decide then some l else none

def step (S : Session W) (v : Verdicts S) (e : Event W S) : Verdicts S :=
  match proofLabel S e with
  | some (g, l) => record S v g l
  | none => v

/-- The Verdicts after a trace, from none. -/
def run (S : Session W) (trace : List (Event W S)) : Verdicts S :=
  trace.foldl (step S) (fun _ => none)

end DP
