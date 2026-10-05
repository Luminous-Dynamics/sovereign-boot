# Sovereign Boot Ecology

Fail-open, state-aware NixOS boot animation and lifecycle management.

## Overview

Sovereign Boot (formerly Spore Boot) provides three small, safety-critical binaries for NixOS boot:

| Binary | Crate | Role |
|--------|-------|------|
| `quicken-fb` | `crates/quicken-fb` | DRM/KMS bare-metal boot animation renderer |
| `spore-boot-state` | `crates/spore-kernel` | Read/write lifecycle state receipts |
| `spore-recovery-linux` | `crates/spore-kernel` | Linux recovery executor (fail-open) |

**Safety rule**: Sovereign Boot may *observe* boot; it must *never* be required for boot.

## NixOS Integration

```nix
# flake.nix
inputs.sovereign-boot.url = "github:Luminous-Dynamics/sovereign-boot";

# In your NixOS module
imports = [ inputs.sovereign-boot.nixosModules.sovereignBoot ];
luminous.services.sporeBoot = {
  enable = true;
};
```

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
