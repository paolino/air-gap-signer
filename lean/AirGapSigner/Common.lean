/-!
Shared abstract domain types and step outcomes for the Air-Gapped Transaction
Signer formalization.

Cryptographic primitives, hardware registers, and serialization bytes are
intentionally modeled as opaque identities and evidence predicates rather than
low-level byte arrays. Concrete Rust types bind to these model identities at
the conformance boundary.
-/

namespace AirGapSigner

/-- Identifier for a key slot in the secure element. -/
abbrev KeySlot := Nat

/-- Opaque digest of an immutable transaction or signing request. -/
abbrev RequestDigest := Nat

/-- Opaque identifier for an enrolled signing controller or verifier device. -/
abbrev DeviceId := Nat

/-- Opaque identifier for a FIDO2 recovery or approval token credential. -/
abbrev TokenId := Nat

/-- Single-use cryptographic challenge. -/
abbrev Challenge := Nat

/-- Result of applying an executable state transition: either the next value
or an explicit, named refusal reason reconciled with the model's guards. -/
inductive StepResult (α : Type) where
  | ok (value : α)
  | refused (reason : String)
  deriving Repr, DecidableEq

/-- A refusal never equals an accepted outcome. -/
theorem ok_ne_refused {α : Type} (v : α) (reason : String) :
    StepResult.ok v ≠ StepResult.refused reason := by
  intro h
  cases h

end AirGapSigner
