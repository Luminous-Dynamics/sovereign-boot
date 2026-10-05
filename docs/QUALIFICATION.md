# Sovereign Boot Qualification

This document records what is actually qualified versus merely present in the
repository. Green means the exact evidence named in the row exists.

| Gate | Status | Evidence required |
|---|---|---|
| Standalone Cargo topology | PASS_STATIC | Root workspace contains only the renderer. |
| Dependency lock | PASS_STATIC | Committed Cargo.lock contains the renderer closure. |
| CLI/module alignment | PASS_STATIC | Module invokes only arguments implemented by quicken-fb. |
| DRM probe | UNQUALIFIED | Physical or VM run of quicken-fb --probe. |
| Renderer execution | UNQUALIFIED | Captured manual canary run with CRTC restoration evidence. |
| Nix package build | PASS_LOCAL_OBSERVED / PENDING_HOSTED | Local x86_64-linux `nix build .#quicken-fb` completed successfully on 2026-10-05; hosted build still pending. |
| VM boot integration | BLOCKED | Requires successful package + VM gates first. |
| Physical boot integration | BLOCKED | Must remain outside the boot-critical path until VM qualification. |
| Lifecycle/state/LKG integration | NOT_EXPORTED | Requires standalone binaries and independent evidence contracts. |

## Safety invariant

A failure of Sovereign Boot must reduce to a missing or failed decorative service;
it must never prevent NixOS from reaching its normal display-manager path.

## Recent build-gate evidence

The first standalone Nix build reached the Cargo vendor phase and exposed two omitted
transitive lock entries: `wasi 0.11.1+wasi-snapshot-preview1` (via `getrandom 0.2.17`) and
`serde_core 1.0.228`. Both are now committed with registry checksums, and an automated
closure audit reports zero unresolved dependency names across 119 locked packages.
The Nix package source boundary was also corrected to include the workspace root and
use `buildAndTestSubdir = "crates/quicken-fb"` so the root Cargo.lock is available
without widening the Cargo build target.

This evidence upgrades the lock/source review. The local x86_64-linux package build has now completed successfully; hosted CI remains pending.
