/-
The world that the labels speak about (AS-12, as far as `verdict_sound`
needs it).

A Goal is printed to a problem: the text that a prover or a checker reads.
What matters of a problem here is whether its assertions are satisfiable;
the logic itself stays abstract. Solvers and checkers are real programs
whose behaviour the System does not control: the model only names what
they did.
-/

namespace DP

/-- The parts of the world that a Session cannot compute for itself. -/
structure World where
  /-- A printed problem. -/
  Problem : Type
  /-- Hashes of problems (Task_Hashes). -/
  Hash : Type
  /-- A solver: prover, version and binary. -/
  Solver : Type
  /-- A certificate. -/
  Cert : Type
  hash : Problem → Hash
  /-- The assertions of the problem have no model. -/
  Unsat : Problem → Prop
  /-- The solver ran on the problem and reported a proof. -/
  Proves : Solver → Problem → Prop
  /-- The certificate is a derivation of false whose open assumptions are
  assertions of the problem. -/
  Refutes : Cert → Problem → Prop
  /-- The pinned checker, run with the options of the TCB_Document on the
  problem as its reference, accepts the certificate. -/
  CheckerAccepts : Problem → Cert → Prop
  /-- A derivation of false from assertions of a problem shows that they
  have no model. This is the soundness of the proof calculus, a fact of
  the logic, not a hypothesis about a tool. -/
  refutes_unsat : ∀ c p, Refutes c p → Unsat p

end DP
