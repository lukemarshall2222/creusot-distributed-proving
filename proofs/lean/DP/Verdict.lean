/-
`verdict_sound` (P7.1.2): in the trace model, a Goal with a proof label
has unsatisfiable assertions, under four named hypotheses and nothing
else.

- proved-standalone: from `SolverSound`, `WorkerHonest` and `NoCollision`.
- certified: from `CheckerSound` and `NoCollision` alone.
-/
import DP.Trace

namespace DP

variable {W : World} {S : Session W}

/-- NG-3, TB-12: an Unpatched_Solver of the Lock_File that reports a proof
of a problem is right. -/
def SolverSound (S : Session W) : Prop :=
  ∀ s p, S.unpatched s = true → W.Proves s p → W.Unsat p

/-- NG-7, TB-2: what a Result reports is what happened. For every Result
of the trace that is authentic (or is of a run that the Front_End started
itself) and reports a proof, the solver that it names ran on a problem
with the Task_Hash that it names, and reported a proof of it. -/
def WorkerHonest (trace : List (Event W S)) : Prop :=
  ∀ g r, Event.result g r ∈ trace →
    (match r.remote with | none => True | some f => authentic f = true) →
    r.answer = Answer.valid →
    ∃ p, W.hash p = r.taskHash ∧ W.Proves r.solver p

/-- TB-1: the hash function is injective on the problems whose hashes the
trace compares. -/
def NoCollision (W : World) : Prop :=
  ∀ p q : W.Problem, W.hash p = W.hash q → p = q

/-- TB-3, TB-4, TB-11: for every checker run of the trace by the pinned
checker with the pinned signature files that reports accept, the
certificate is a derivation of false from assertions of the reference
file that the checker was given. -/
def CheckerSound (trace : List (Event W S)) : Prop :=
  ∀ g c, Event.check g c ∈ trace →
    c.checkerPinned = true → c.signaturePinned = true → c.accepted = true →
    ∃ p, W.hash p = c.referenceHash ∧ W.Refutes c.cert p

/-- A Verdict is justified when its Goal's assertions are unsatisfiable. -/
def Justified (S : Session W) (v : Verdicts S) : Prop :=
  ∀ g l, v g = some l → W.Unsat (S.printLocal g)

theorem record_justified (v : Verdicts S) (g : S.Goal) (l : Label)
    (hv : Justified S v) (hg : W.Unsat (S.printLocal g)) : Justified S (record S v g l) := by
  intro g' l' h
  unfold record at h
  cases hold : v g' with
  | some old => exact hv g' old hold
  | none =>
    rw [hold] at h
    by_cases hgg : g' = g
    · rw [hgg]; exact hg
    · simp [hgg] at h

/-- A Result that gives proved-standalone is of an Unpatched_Solver, bound
to the local print, accepted, and reports a proof. -/
theorem standalone_facts (g : S.Goal) (r : ResultEvent W)
    (h : (accepted S g r && label (resultFacts S g r) == Label.provedStandalone) = true) :
    r.answer = Answer.valid ∧ S.unpatched r.solver = true ∧
    r.taskHash = W.hash (S.printLocal g) ∧
    (match r.remote with | none => True | some f => authentic f = true) := by
  have hacc : accepted S g r = true := by
    cases ha : accepted S g r <;> simp [ha] at h ⊢
  have hlab : label (resultFacts S g r) = Label.provedStandalone := by
    cases ha : accepted S g r <;> simp [ha] at h
    exact h
  -- The label is proved-standalone only through `standalone`.
  have hst : standalone (resultFacts S g r) = true := by
    unfold label at hlab
    cases hc : classify (resultFacts S g r).answer (lemmaDependent (resultFacts S g r)) with
    | some o => rw [hc] at hlab; cases hlab
    | none =>
      rw [hc] at hlab
      cases hs : standalone (resultFacts S g r) with
      | true => rfl
      | false => simp [hs] at hlab
  simp only [standalone, bound, lemmaDependent, resultFacts, Bool.and_eq_true, Bool.not_eq_true',
    Bool.or_eq_false_iff, beq_iff_eq, Bool.not_eq_false'] at hst
  obtain ⟨⟨⟨⟨_, hans⟩, hhash, _⟩, _⟩, ⟨⟨⟨_, _⟩, _⟩, hunp⟩, _⟩ := hst
  refine ⟨hans, hunp, (S.hashEq_iff _ _).mp hhash, ?_⟩
  unfold accepted at hacc
  cases hr : r.remote with
  | none => trivial
  | some f =>
    rw [hr] at hacc
    simp only [acceptRemote, Bool.and_eq_true] at hacc
    exact hacc.1

/-- A checker run that gives certified is of the pinned checker with the
pinned signature files, accepted, on a reference with the hash of the
local print. -/
theorem certified_facts (g : S.Goal) (c : CheckEvent W)
    (h : (labelFromChecker (checkFacts S g c) == Label.certified) = true) :
    c.checkerPinned = true ∧ c.signaturePinned = true ∧ c.accepted = true ∧
    c.referenceHash = W.hash (S.printLocal g) := by
  have hl : labelFromChecker (checkFacts S g c) = Label.certified := by simpa using h
  unfold labelFromChecker at hl
  simp only [checkFacts] at hl
  split at hl
  · rename_i hall
    simp only [Bool.and_eq_true] at hall
    obtain ⟨⟨⟨⟨⟨⟨_, hp⟩, hs⟩, hb, _⟩, ha⟩, _⟩, _⟩ := hall
    exact ⟨hp, hs, ha, (S.hashEq_iff _ _).mp hb⟩
  · split at hl <;> cases hl

/-- Each step keeps the Verdicts justified, for the events of a trace
under the four hypotheses. -/
theorem step_justified (trace : List (Event W S)) (hS : SolverSound S)
    (hW : WorkerHonest trace) (hN : NoCollision W) (hC : CheckerSound trace)
    (v : Verdicts S) (e : Event W S) (he : e ∈ trace) (hv : Justified S v) :
    Justified S (step S v e) := by
  unfold step
  cases e with
  | result g r =>
    simp only [proofLabel]
    split
    · rename_i h
      split at h
      · rename_i hdec
        cases h
        obtain ⟨hans, hunp, hhash, hauth⟩ := standalone_facts _ r hdec
        obtain ⟨p, hp, hproves⟩ := hW _ r he hauth hans
        have : p = S.printLocal _ := hN _ _ (hp.trans hhash)
        exact record_justified v _ _ hv (this ▸ hS _ _ hunp hproves)
      · cases h
    · exact hv
  | check g c =>
    simp only [proofLabel]
    split
    · rename_i h
      split at h
      · rename_i hdec
        cases h
        obtain ⟨hp, hs, ha, hhash⟩ := certified_facts _ c hdec
        obtain ⟨p, hph, href⟩ := hC _ c he hp hs ha
        have : p = S.printLocal _ := hN _ _ (hph.trans hhash)
        exact record_justified v _ _ hv (this ▸ W.refutes_unsat _ _ href)
      · cases h
    · exact hv

theorem foldl_justified (trace : List (Event W S)) (hS : SolverSound S)
    (hW : WorkerHonest trace) (hN : NoCollision W) (hC : CheckerSound trace) :
    ∀ (es : List (Event W S)) (v : Verdicts S), (∀ e, e ∈ es → e ∈ trace) → Justified S v →
      Justified S (es.foldl (step S) v) := by
  intro es
  induction es with
  | nil => intro v _ hv; exact hv
  | cons e es ih =>
    intro v hsub hv
    exact ih _ (fun e' h => hsub e' (List.mem_cons_of_mem _ h))
      (step_justified trace hS hW hN hC v e (hsub e (List.mem_cons_self ..)) hv)

/-- **Verdict soundness** (P7.1.2). For every trace of a Session and every
Verdict that it records: the assertions of the Goal's local print are
unsatisfiable. -/
theorem verdict_sound (trace : List (Event W S)) (hS : SolverSound S)
    (hW : WorkerHonest trace) (hN : NoCollision W) (hC : CheckerSound trace)
    (g : S.Goal) (l : Label) (h : run S trace g = some l) : W.Unsat (S.printLocal g) :=
  foldl_justified trace hS hW hN hC trace (fun _ => none) (fun _ h => h)
    (fun _ _ h => by cases h) g l h

/-! ### The certified route alone

Part (b) of P7.1.2: a certified Verdict is justified from `CheckerSound`
and `NoCollision`, whatever the solvers and the Workers of the trace did.
-/

/-- Every certified Verdict is justified. -/
def CertifiedJustified (S : Session W) (v : Verdicts S) : Prop :=
  ∀ g, v g = some Label.certified → W.Unsat (S.printLocal g)

theorem record_certified_justified (v : Verdicts S) (g : S.Goal) (l : Label)
    (hv : CertifiedJustified S v) (hg : l = Label.certified → W.Unsat (S.printLocal g)) :
    CertifiedJustified S (record S v g l) := by
  intro g' h
  unfold record at h
  cases hold : v g' with
  | some old =>
    rw [hold] at h
    simp at h
    exact hv g' (by rw [hold, h])
  | none =>
    rw [hold] at h
    by_cases hgg : g' = g
    · simp [hgg] at h
      rw [hgg]; exact hg h
    · simp [hgg] at h

theorem step_certified_justified (trace : List (Event W S)) (hN : NoCollision W)
    (hC : CheckerSound trace) (v : Verdicts S) (e : Event W S) (he : e ∈ trace)
    (hv : CertifiedJustified S v) : CertifiedJustified S (step S v e) := by
  unfold step
  cases e with
  | result g r =>
    simp only [proofLabel]
    split
    · rename_i h
      split at h
      · cases h
        exact record_certified_justified v _ _ hv (fun hl => by cases hl)
      · cases h
    · exact hv
  | check g c =>
    simp only [proofLabel]
    split
    · rename_i h
      split at h
      · rename_i hdec
        cases h
        obtain ⟨hp, hs, ha, hhash⟩ := certified_facts _ c hdec
        obtain ⟨p, hph, href⟩ := hC _ c he hp hs ha
        have : p = S.printLocal _ := hN _ _ (hph.trans hhash)
        exact record_certified_justified v _ _ hv (fun _ => this ▸ W.refutes_unsat _ _ href)
      · cases h
    · exact hv

theorem certified_sound (trace : List (Event W S)) (hN : NoCollision W) (hC : CheckerSound trace)
    (g : S.Goal) (h : run S trace g = some Label.certified) : W.Unsat (S.printLocal g) := by
  have key : ∀ (es : List (Event W S)) (v : Verdicts S), (∀ e, e ∈ es → e ∈ trace) →
      CertifiedJustified S v → CertifiedJustified S (es.foldl (step S) v) := by
    intro es
    induction es with
    | nil => intro v _ hv; exact hv
    | cons e es ih =>
      intro v hsub hv
      exact ih _ (fun e' h => hsub e' (List.mem_cons_of_mem _ h))
        (step_certified_justified trace hN hC v e (hsub e (List.mem_cons_self ..)) hv)
  exact key trace (fun _ => none) (fun _ h => h) (fun _ h => by cases h) g h

end DP
