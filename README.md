# Sovereign Boot Ecology

Fail-open boot visuals and boot-state/recovery integration for NixOS.

**Deployment status (2026-10-20): prototype; not qualified for production hosts.** The CPU renderer and visual-audit harness are developed in [PR #5](https://github.com/Luminous-Dynamics/sovereign-boot/pull/5). This branch adds a standalone lifecycle-tool package and a clean Cargo boundary. No QEMU boot-to-display-manager qualification, real signature verification, or physical boot qualification is claimed.

## Public, independently buildable components

- `quicken-fb`: optional DRM/KMS CPU renderer, deterministic procedural themes, reduced-motion behavior, and capture/benchmark tooling.
- `crates/boot-lifecycle`: a dependency-free, standalone Rust package exporting `spore-boot-state` and `spore-recovery-linux`.
- `nix/modules/sovereign-boot.nix`: an optional NixOS module reference that must remain non-blocking and disabled by default until the lifecycle gates pass.

The public Nix flake exports three packages:

- `packages.<system>.quicken-fb` — renderer only.
- `packages.<system>.boot-lifecycle` — the two lifecycle executables.
- `packages.<system>.spore-boot-tools` — a bundle with all three executables, used by the module unless a consumer supplies explicit package overrides.

The imported `crates/spore-kernel` directory is retained as legacy source material, but it has monorepo-relative Symthaea dependencies. It is **excluded from the public Cargo workspace and is not built or shipped as the lifecycle implementation**. The standalone binaries are now implemented in `crates/boot-lifecycle`; their existence is not by itself proof of safe deployment.

## Lifecycle contract

`spore-recovery-linux prepare` resolves only a canonical, existing `/nix/store/<hash>-<name>` deployment path; rotates `current` and `previous` GC-root symlinks with rename + directory sync; consumes a prior clean-shutdown marker; and atomically writes the local runtime receipt and lineage record.

`spore-recovery-linux qualify` returns exit code 75 while the graphical target / display manager is not healthy and stable. It promotes the Last Known Good GC-root symlink only when the booted deployment equals the current userspace deployment, the display manager is active with a nonzero main PID and monotonic start time, and the configured stability interval has elapsed. A deadline-expired, unready boot is recorded as skipped and leaves the prior LKG root untouched.

`spore-boot-state shutdown` atomically records a clean reboot, poweroff, suspend, or hibernate marker with the boot ID and available deployment/hardware facts.

These commands write **local unsigned diagnostic JSON**. They do not authenticate the machine, perform a rollback, prove the content digest of a Nix store closure, implement a TPM quote, or qualify cross-host locking or power-loss durability. The LKG link is a GC root, not a bootloader rollback command.

## Build and test

```bash
cargo test --manifest-path crates/boot-lifecycle/Cargo.toml --locked
cargo build --manifest-path crates/boot-lifecycle/Cargo.toml --locked --bins
cargo run --manifest-path crates/boot-lifecycle/Cargo.toml --locked --bin spore-boot-state -- --help
cargo run --manifest-path crates/boot-lifecycle/Cargo.toml --locked --bin spore-recovery-linux -- --help
```

The lifecycle crate has its own nested Cargo workspace and lockfile so it can be built from its directory without resolving the legacy `symthaea-spore` path dependencies. The top-level workspace intentionally includes the renderer but excludes the legacy monorepo tree.

## NixOS integration

The module stays disabled by default. Once the standalone package and the VM gates are qualified, a consumer can import the flake module:

```nix
imports = [ inputs.sovereign-boot.nixosModules.sovereignBoot ];

luminous.services.sporeBoot.enable = true;
```

Explicit renderer/state package overrides remain available for canaries and VM tests. Do not enable on a production host yet: [issue #3](https://github.com/Luminous-Dynamics/sovereign-boot/issues/3) tracks the standalone extraction boundary, [issue #4](https://github.com/Luminous-Dynamics/sovereign-boot/issues/4) tracks clean renderer build reproducibility, and [issue #9](https://github.com/Luminous-Dynamics/sovereign-boot/issues/9) tracks the non-destructive NixOS/QEMU display-manager handoff and DRM-release benchmark.

## Governing safety rule

Sovereign Boot may observe and decorate boot, but it must never be required for boot, login, locking, recovery, or rollback.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
