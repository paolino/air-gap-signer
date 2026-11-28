import Lake
open Lake DSL

package airGapSigner where
  leanOptions := #[
    ⟨`autoImplicit, false⟩
  ]

@[default_target]
lean_lib AirGapSigner where
  srcDir := "."

@[default_target]
lean_exe «air-gap-signer-oracle» where
  root := `AirGapSigner.Oracle
