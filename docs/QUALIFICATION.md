# Sovereign Boot Qualification

This document records what is actually qualified versus merely present in the
repository. Green means the exact evidence named in the row exists.

| Gate | Status | Evidence required |
|---|---|---|
| Standalone Cargo topology | PASS_STATIC | Root workspace contains only the renderer. |
| Dependency lock | PASS_STATIC | Committed Cargo.lock contains the renderer closure. |
| CLI/module alignment | PASS_STATIC | Module invokes only arguments implemented by quicken-fb. |
| DRM probe | PASS_OBSERVED | 2026-10-05: `/dev/dri/card1`, Intel `i915`, `boot_vga=1`, connector `eDP-1`, CRTC `59`, 1920x1080@144Hz; card0/NVIDIA reported disconnected connectors. |
| Renderer execution | FAIL_OBSERVED / REWORKED | First live physical canary (binary SHA256 `2264808f1583f12c70b7584838d3bdf57ad1ab8424f25798520b55d857808bc0`) produced a black screen; the session was recovered by reboot, so no CRTC restoration evidence was captured. The manual canary boundary has since been hardened with active-VT ownership, KD_GRAPHICS handoff, SIGHUP handling, bounded duration, and fail-closed blitting. |
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

## DRM lifecycle hardening

The renderer now captures all connectors currently attached to the selected CRTC before
calling legacy SETCRTC, and restores that connector set together with the original
framebuffer, position, and mode on drop. The non-mutating probe emits the selected
connector interface/id and CRTC in its receipt.

This is still source-level qualification until a controlled renderer execution
observes and records successful restoration on the target hardware or a VM.

## Reproducibility hardening

Hosted CI uses Cargo --locked for check/test/clippy and Nix --no-write-lock-file for
the package build, preventing silent lockfile or dependency-graph drift during
qualification.
