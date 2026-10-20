# Sovereign Boot Ecology

Fail-open boot visuals and boot-state/recovery integration for NixOS.

**Deployment status (2026-10-18): renderer prototype; not yet qualified for production hosts.** The visual renderer and its qualification harness are under active development in [PR #5](https://github.com/Luminous-Dynamics/sovereign-boot/pull/5). No exact-head CI pass or physical boot qualification is claimed.

## What currently exists

- `quicken-fb`: Rust DRM/KMS CPU reference renderer, deterministic procedural themes, bounded rendering, reduced-motion behavior, and capture/benchmark tooling.
- `themes/visual-genome-v1.json`: theme, motion and surface-policy manifest, with generated Rust contracts and CI parity checks.
- `nix/modules/sovereign-boot.nix`: a state-aware NixOS module reference that is disabled by default and must remain optional to boot.

## Important extraction limitation

The lifecycle binaries referenced by the module—`spore-boot-state` and `spore-recovery-linux`—are **not yet a qualified, independently buildable deliverable from this repository**. The extraction/build boundary is tracked in [issue #3](https://github.com/Luminous-Dynamics/sovereign-boot/issues/3) and [PR #1](https://github.com/Luminous-Dynamics/sovereign-boot/pull/1). A declared module or package expression is not proof that these binaries can be built and safely used.

## NixOS integration

The intended module interface is:

```nix
imports = [ inputs.sovereign-boot.nixosModules.sovereignBoot ];

# Reference only. Do not enable on a production host until the standalone
# binaries, clean-checkout build, VM handoff, and recovery gates are qualified.
# luminous.services.sporeBoot.enable = true;
```

The current module intentionally defaults to disabled. Before enabling it, close [issue #3](https://github.com/Luminous-Dynamics/sovereign-boot/issues/3) (self-contained lifecycle binaries), [issue #4](https://github.com/Luminous-Dynamics/sovereign-boot/issues/4) (reproducible renderer build), and [issue #9](https://github.com/Luminous-Dynamics/sovereign-boot/issues/9) (NixOS/VM device discovery and display-manager handoff), plus the physical-hardware checks tracked in the qualification docs.

## Governing safety rule

Sovereign Boot may observe and decorate boot, but it must never be required for boot, login, locking, recovery, or rollback.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
