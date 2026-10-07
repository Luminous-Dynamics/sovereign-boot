# Sovereign Boot Ecology

Fail-open, state-aware NixOS boot animation and lifecycle management.

## Current extraction status

The public repository is **not yet fully qualified as a standalone boot/recovery implementation**.

The currently shipped standalone package is:

| Capability | Current state |
|---|---|
| `quicken-fb` | **Available:** standalone DRM/KMS framebuffer renderer |
| `spore-boot-state` | **Blocked:** lifecycle state binary is not present in the extracted source tree |
| `spore-recovery-linux` | **Blocked:** recovery binary is not present in the extracted source tree |

The `crates/spore-kernel` directory is retained for extraction work, but its manifest still references monorepo-local crates that are not present in this repository. See issue #3 for the exact standalone-boundary finding.

Until that extraction is completed and independently qualified, **do not treat this repository or its NixOS module as a qualified end-to-end boot/recovery implementation**. The module remains fail-closed: enabling the full service requires explicit package inputs for the renderer and lifecycle state/recovery tool.

## Overview

Sovereign Boot provides the renderer and the host-integration boundary for a future state-aware boot ecology.

**Safety rule**: Sovereign Boot may *observe* boot; it must *never* be required for boot.

## NixOS Integration

```nix
# flake.nix
inputs.sovereign-boot.url = "github:Luminous-Dynamics/sovereign-boot";

# In your NixOS module
imports = [ inputs.sovereign-boot.nixosModules.sovereignBoot ];
```

The standalone flake currently exports the `quicken-fb` renderer. Full lifecycle operation is intentionally blocked until the missing state/recovery executables are independently extracted and qualified.

## License

AGPL-3.0-or-later. See [LICENSE](LICENSE).
