# SRQ boot-observation probe (D0 diagnostic only)

This standalone, dependency-free Rust tool reads boot identity without modifying system state. It is a first diagnostic adapter slice for SRQ issue #98 and Sovereign Boot issue #11.

Default mode reads:
- `/proc/sys/kernel/random/boot_id`
- `/run/booted-system` (actual booted system)
- `/run/current-system` (current userspace activation)

It emits a single JSON record with schema `srq-boot-observation-diagnostic-v1`. The record reports `MATCH`, `MISMATCH`, or `BLOCKED`, and always sets `authenticated=false` and `release_qualifying=false`. It does not invent a target, decision ID, challenge ID, artifact digest, policy measurement or signature; those are null because this diagnostic probe cannot obtain them.

Use `--root PATH` only for a disposable fixture tree in tests. Absolute symlink targets are resolved inside that fixture root, and path escapes fail closed. The probe writes no files and does not change symlinks, boot configuration, EFI variables, the filesystem, or the NixOS host.

## Qualification boundary

This is deliberately a D0 probe:
- A local root-readable JSON record is not authenticated attestation.
- A matching pair of system paths is not proof that an approved decision caused that deployment to boot.
- A SHA-shaped string is not proof of a measurement.
- It cannot qualify SRQ release gates.

The next tier requires a separate post-commit verifier challenge bound to target/decision/deployment/policy, a fresh response, and a defined authentication/measurement trust root. The standalone recovery/state binaries remain tracked by [issue #3](https://github.com/Luminous-Dynamics/sovereign-boot/issues/3); challenge-bound observation integration is tracked by [issue #11](https://github.com/Luminous-Dynamics/sovereign-boot/issues/11).
