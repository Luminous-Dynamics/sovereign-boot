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
| Nix package build | PENDING_HOSTED | Successful hosted nix build .#quicken-fb. |
| VM boot integration | BLOCKED | Requires successful package + VM gates first. |
| Physical boot integration | BLOCKED | Must remain outside the boot-critical path until VM qualification. |
| Lifecycle/state/LKG integration | NOT_EXPORTED | Requires standalone binaries and independent evidence contracts. |

## Safety invariant

A failure of Sovereign Boot must reduce to a missing or failed decorative service;
it must never prevent NixOS from reaching its normal display-manager path.
