/-
`dp-lean-ref`: the executable reference definitions (R7.1 criterion 5).

Reads one request per line from standard input and prints one answer per
line. A request is the name of a definition and its facts as `0` and `1`:

  label    A f1 … f9     A: the answer, 0 to 7 in the order of `Answer`;
                         facts in the order of `ResultFacts`
  checker  f1 … f7       facts in the order of `CheckFacts`
  accept   f1 … f8       the six `RemoteFacts`, then `taskHashMatches`
                         and `oneTask`
  classify A d

The answers are the label names of `proof.json`, `accept` or `discard`,
and the outcome names of the TCB_Document.
-/
import DP.Trace

open DP

def answerOf : Nat → Option Answer
  | 0 => some .valid | 1 => some .invalid | 2 => some .timeout | 3 => some .outOfMemory
  | 4 => some .stepLimitExceeded | 5 => some .unknown | 6 => some .failure | 7 => some .highFailure
  | _ => none

def outcomeName : Outcome → String
  | .standaloneInvalid => "standalone-invalid" | .checkedIncomplete => "checked-incomplete"
  | .unchecked => "unchecked" | .dependentInvalid => "dependent-invalid" | .timeout => "timeout"
  | .unknown => "unknown" | .failed => "failed" | .unresolved => "unresolved"

def labelName : Label → String
  | .provedStandalone => "proved-standalone" | .certified => "certified" | .nonProof o => outcomeName o

def bit : String → Option Bool
  | "0" => some false | "1" => some true | _ => none

def answer (line : String) : String :=
  match line.splitOn " " |>.filter (· ≠ "") with
  | ["label", a, f1, f2, f3, f4, f5, f6, f7, f8, f9] =>
    match a.toNat? >>= answerOf, [f1, f2, f3, f4, f5, f6, f7, f8, f9].mapM bit with
    | some a, some [b1, b2, b3, b4, b5, b6, b7, b8, b9] =>
      labelName (label { answer := a, taskHashMatches := b1, oneTask := b2, partition := b3,
                         importMark := b4, importsEmpty := b5, importsHashEmpty := b6,
                         unpatched := b7, binaryMismatch := b8, certifyAll := b9 })
    | _, _ => "error"
  | ["checker", f1, f2, f3, f4, f5, f6, f7] =>
    match [f1, f2, f3, f4, f5, f6, f7].mapM bit with
    | some [b1, b2, b3, b4, b5, b6, b7] =>
      labelName (labelFromChecker { produced := b1, checkerPinned := b2, signaturePinned := b3,
                                    boundToLocalPrint := b4, accepted := b5, complete := b6, stored := b7 })
    | _ => "error"
  | ["accept", f1, f2, f3, f4, f5, f6, f7, f8] =>
    match [f1, f2, f3, f4, f5, f6, f7, f8].mapM bit with
    | some [b1, b2, b3, b4, b5, b6, b7, b8] =>
      let facts : ResultFacts := { answer := .valid, taskHashMatches := b7, oneTask := b8, partition := false,
                                   importMark := false, importsEmpty := true, importsHashEmpty := true,
                                   unpatched := true, binaryMismatch := false, certifyAll := false }
      if acceptRemote facts { signatureValid := b1, registered := b2, revoked := b3, submitted := b4,
                              limitsConsistent := b5, importsConsistent := b6 } then "accept" else "discard"
    | _ => "error"
  | ["classify", a, d] =>
    match a.toNat? >>= answerOf, bit d with
    | some a, some d => match classify a d with | some o => outcomeName o | none => "proof"
    | _, _ => "error"
  | _ => "error"

partial def loop (stdin : IO.FS.Stream) (stdout : IO.FS.Stream) : IO Unit := do
  let line ← stdin.getLine
  if line.isEmpty then return
  stdout.putStrLn (answer ((line.splitOn "\n").headD ""))
  loop stdin stdout

def main : IO Unit := do
  loop (← IO.getStdin) (← IO.getStdout)
