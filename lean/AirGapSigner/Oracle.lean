import Lean.Data.Json
import AirGapSigner.Common

/-!
Generic JSON oracle driver for conformance replay.

Reads one JSON request from standard input, dispatches on the declared
`surface` name, writes one JSON result or named refusal to standard output,
and exits non-zero on any parse, schema, or unknown-surface failure.

No per-theorem logic belongs in this driver: each formalization slice registers
its executable transition function and boundary observations here as named
surfaces.
-/

namespace AirGapSigner.Oracle

open Lean

/-- Declared executable oracle surfaces currently registered in the model. -/
def registeredSurfaces : List String := []

def dispatch (req : Json) : Except String Json := do
  let surface ← req.getObjValAs? String "surface"
  match surface with
  | "surfaces" =>
      pure <| Json.mkObj [
        ("status", Json.str "ok"),
        ("surfaces", toJson registeredSurfaces)
      ]
  | other =>
      throw s!"unknown oracle surface: {other}"

end AirGapSigner.Oracle

def main : IO UInt32 := do
  let stdin ← IO.getStdin
  let raw ← stdin.readToEnd
  match Lean.Json.parse raw with
  | .error err =>
      IO.eprintln s!"invalid JSON input: {err}"
      return 1
  | .ok req =>
      match AirGapSigner.Oracle.dispatch req with
      | .error err =>
          IO.eprintln err
          return 1
      | .ok resp =>
          IO.println resp.compress
          return 0
