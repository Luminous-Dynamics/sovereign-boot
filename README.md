# Sovereign Boot Ecology

Fail-open NixOS boot rendering with a deliberately narrow host boundary.

## Current status

**Renderer canary: ready for automated qualification; not yet approved as a physical boot dependency.**

The standalone repository currently contains the DRM/KMS renderer and the source material extracted from the former Spore Boot tree. The lifecycle/state/recovery helpers referenced by the old monorepo integration are **not** wired into the exported flake yet because their standalone binaries and evidence contracts are not present at this repository head.

The safety rule is therefore explicit:

> Sovereign Boot may observe and render boot; it must never be required for boot.

## Components

| Component | Status | Role |
|---|---|---|
| `quicken-fb` | exported | DRM/KMS bare-metal procedural renderer |
| `spore-kernel` extraction | retained, excluded from workspace | source archive pending standalone dependency extraction |
| lifecycle/LKG binaries | not exported | require an independent standalone extraction + qualification gate |

The repository intentionally does not pretend that an extracted source tree is a qualified executable surface.

## Safe first test

Before allowing the renderer to take display ownership, probe the DRM path without creating a framebuffer or changing CRTC state:

```bash
sudo ./result/bin/quicken-fb --probe --device /dev/dri/card0
```

Expected shape:

```
drm-ok device=/dev/dri/card0 mode=1920x1080 refresh=60Hz
```

A probe failure is a hardware/DRM compatibility result, not a reason to weaken the boot boundary.

After the probe succeeds, the actual renderer can be tested from a text console, with the display manager stopped, using a disposable build:

```bash
sudo systemctl stop display-manager.service
sudo ./result/bin/quicken-fb --genesis-phrase "Sovereign Boot" --device /dev/dri/card0
```

Terminate with Ctrl-C. The renderer attempts to restore the original CRTC state on exit.

Do **not** make this unit part of a machine's required boot target until the CI/VM gates below are green and the physical canary has been reviewed.

## NixOS integration

The exported module is intentionally renderer-only:

```nix
{
  inputs.sovereign-boot.url = "github:Luminous-Dynamics/sovereign-boot";

  # In your host module:
  imports = [ inputs.sovereign-boot.nixosModules.sovereignBoot ];

  luminous.services.sovereignBoot = {
    enable = true;
    package = inputs.sovereign-boot.packages.${pkgs.system}.quicken-fb;
    genesisPhrase = "Sovereign Boot";
    drmDevice = "/dev/dri/card0";
  };
}
```

The module:
- is wanted by `multi-user.target`, not required by it;
- refuses to acquire DRM if `display-manager.service` is already active;
- hands DRM back explicitly from `display-manager.service`'s `ExecStartPre`;
- bounds startup/stop time;
- runs without additional Linux capabilities.

A failed decorative renderer therefore cannot be promoted into a boot authority path by accident.

## Qualification boundary

Before full lifecycle integration is restored, the next gate is:

```
cargo check --workspace
cargo test --workspace
nix build .#quicken-fb
```

Then qualify on representative hardware/VMs:

1. Intel/AMD or virtual DRM device.
2. NVIDIA systems as a separate compatibility class.
3. live `nixos-rebuild test/switch` while the desktop is already active.
4. interrupted renderer exit and CRTC restoration.
5. no-DRM environment: service must cleanly skip.

Only after those are evidenced should the state/recovery/LKG layer be reintroduced as a separately reviewable surface.

## License

AGPL-3.0-or-later.
