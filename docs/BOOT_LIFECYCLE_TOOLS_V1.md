# Standalone boot lifecycle tool contract v1

Status: implementation proposal; binary build/tests and the existing NixOS/QEMU lifecycle gate must pass before any deployment claim. This document distinguishes locally useful state management from authenticated attestation or rollback.

## Package boundary

The standalone crate lives at `crates/boot-lifecycle`, has no external Rust dependencies, and contains two binary entry points:
- `spore-boot-state`
- `spore-recovery-linux`

The public `crates/spore-kernel` tree is an imported Symthaea monorepo source tree with missing path dependencies in this extraction. The top-level Cargo workspace excludes it; no public lifecycle package is built from that crate. The Nix flake provides `boot-lifecycle` and a `spore-boot-tools` bundle.

## CLI and state transitions

### Prepare boot facts

The NixOS module invokes:

```text
spore-recovery-linux prepare \
  --roots-dir ROOTS --state-dir STATE --runtime-dir RUNTIME \
  --generation /nix/store/HASH-NAME --boot-id UUID \
  --hardware-fingerprint SHA256 --storage-state unknown
```

The tool rejects deployment identities outside the single canonical `/nix/store/<hash>-<name>` component and requires the selected generation to be an existing real directory. It atomically rotates the GC-root `current` / `previous` links (temporary symlink, rename, parent-directory sync), removes only stale scratch symlinks that match its narrow names, consumes a previous clean-shutdown marker once, then writes `boot-state.json` and `lineage.json` through temporary files, file sync, rename, and directory sync. Missing/invalid state makes preparation fail; the module removes partial receipts and merely skips the optional renderer.

### Qualification and LKG root

```text
spore-recovery-linux qualify \
  --roots-dir ROOTS --state-dir STATE --runtime-dir RUNTIME \
  --boot-id UUID --booted-generation /nix/store/HASH-NAME \
  --current-generation /nix/store/HASH-NAME \
  --graphical-state active --display-state active --display-pid PID \
  --active-enter-usec N --observed-usec N --required-stability-usec N \
  [--qualification-deadline-expired] --report-path RUNTIME/lkg-decision-v1.json
```

The tool will not mark a deployment known-good unless:
1. The resolved deployment that booted equals the current userspace deployment (to catch a live userspace switch without reboot).
2. `graphical.target` and `display-manager.service` are both active.
3. The display manager has a nonzero MainPID and a nonzero monotonic active-enter timestamp.
4. The observed continuous-active interval is at least the configured stability interval.

Before that point it returns exit code 75, which tells the module's timer to retry. If its maximum qualification window expires while the gates are not met, it records `skipped` and leaves the prior LKG symlink untouched. On qualification, it atomically replaces the `last-known-good` GC-root symlink and writes the local decision record. Repeated qualification is idempotent: the same target is reported as already known-good.

### Clean shutdown

`spore-boot-state shutdown` accepts the lifecycle kind (`reboot`, `poweroff`, `suspend`, or `hibernate`), uptime, persistent state directory, runtime directory, and optional generation/hardware-fingerprint fields. It atomically writes a clean-shutdown marker and a separate runtime diagnostic receipt. The sleep-resume hook removes the persistent marker after resume so a later unclean loss is not mislabeled clean.

## Durability semantics

The write helper uses a same-directory temporary file, `sync_all`, rename, then parent-directory sync. GC-root changes use a temporary symlink, rename, and parent-directory sync. These are intended atomic local-filesystem update patterns; the package does not claim that a particular device/controller honors cache flushes or write ordering under actual power loss.

This is not a kernel crash test, a filesystem fault-injection VFS, a TPM-backed remote attestation, or a production rollback engine. The JSON says so explicitly: `observation_authentication=local-unsigned-diagnostic`. A path name identifies a deployment object but does not provide an independent content measurement of the policy/artifact actually applied.

## Required next qualification

- Run the standalone package tests and build both binaries with `--locked` from a clean checkout.
- Build the Nix flake's three package outputs from the public source boundary.
- Evaluate the NixOS module using the package bundle, without importing Symthaea or a mutable system path.
- Keep all VM tests non-destructive: no EFI-variable writes, bootloader install, or host boot configuration changes.
- Pass issue #9's QEMU cases for no DRM devices, unavailable renderer, display-manager handoff, sandbox, and p95 <= 750 ms stop-to-DRM-release.
- Test actual process death at persistence boundaries; use QEMU resets and physical power-loss tests as separate tiers.
- Keep the module disabled by default and release gate NOT_QUALIFIED until the applicable evidence exists.

Local JSON receipts are not independently authenticated evidence. Do not use them as SRQ PASS receipts without an adapter that binds a fresh verifier challenge, target and decision identity, observed booted deployment, policy/artifact digests, and an explicit cryptographic trust root.
