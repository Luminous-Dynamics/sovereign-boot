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

## Cross-platform direction

The long-term architecture and honest per-platform boundaries are documented in [Platform Portability](docs/PLATFORM_PORTABILITY.md). This is a roadmap, not a claim that Windows, macOS, mobile, or TV adapters exist today.

A first portable implementation slice is now present on the hardening branch: `crates/visual-core` contains the Rust scene simulation, `crates/visual-wasm` exposes a browser RGBA API, `crates/visual-component` exports a typed WIT Component Model scene API for WASI hosts, and `crates/visual-wasi` is a headless PPM-output smoke target. Exact-head compile/test qualification is pending; none of these packages, by itself, is a desktop wallpaper, phone live wallpaper, TV screensaver, lock screen, or firmware integration. See [WASM/WASI Architecture](docs/WASM_WASI_ARCHITECTURE.md) for the distinction and next gates.

## Safe first test

Before allowing the renderer to take display ownership, probe the DRM path without creating a framebuffer or changing CRTC state:

```bash
sudo ./result/bin/quicken-fb --probe --device /dev/dri/cardN
```

Expected shape:

```
drm-ok device=/dev/dri/cardN connector=eDP-1 crtc=... mode=1920x1080 refresh=144Hz
```

On multi-GPU systems, select the card that owns the connected connector; do not
assume `card0`.

A probe failure is a hardware/DRM compatibility result, not a reason to weaken the boot boundary.

After the probe succeeds, the repository provides two intentionally different
physical qualification paths.

For **automated qualification**, the safe path is boot-scoped. It records a one-shot
request, reboots normally, and lets systemd run the canary from a real text VT before
the display manager starts:

```bash
sudo nix run --no-update-lock-file --no-write-lock-file .#arm-physical-canary -- --device /dev/dri/card1 --seconds 5
```

The arming command performs the non-mutating probe first, writes a root-owned
one-shot request under `/var/lib/sovereign-boot/`, and then reboots. The request also records the SHA-256 digest of the probed renderer and the
selected DRM card, so the next-boot service refuses to execute a stale or mismatched
artifact. The boot service consumes the request before modesetting, performs a fresh
non-mutating probe, and requires an explicit restoration receipt in addition to a zero
renderer exit code. It **does not**
call `systemctl isolate`, stop SDDM, kill Plasma, or take DRM away from the current
desktop. On the next boot, `sovereign-boot-physical-canary.service` is ordered before
the display manager and uses `/dev/tty1` as its controlling VT. The request is
consumed
once, and a persistent result is written to
`/var/lib/sovereign-boot/physical-canary.result` with the boot ID, selected device,
duration, renderer digest, boot-time probe receipt, restoration receipt, and renderer
exit status.

For direct operator testing from an already-active Linux console, the non-destructive
launcher remains available:

```bash
sudo nix run --no-update-lock-file --no-write-lock-file .#physical-canary -- --device /dev/dri/card1 --seconds 5
```

It allocates a real VT with `openvt --switch --wait`, but refuses to run while
`display-manager.service` is active. It no longer has an isolation mode. This is
deliberate: the previous `--isolate` implementation was observed terminating the
active KDE/SDDM session during qualification.

For the manual VT path:

```bash
# From a real, active Linux VT (for example Ctrl-Alt-F3), after logging in.
# Keep a second recovery shell available on another VT or over SSH.
# The launcher runs quicken-fb on a real VT, but refuses an active
# display-manager.service rather than taking ownership from the desktop.
sudo ./scripts/launch-physical-canary.sh --device /dev/dri/cardN --seconds 5
sudo systemctl start display-manager.service
```

The bounded canary exits after at most 30 seconds and drops `DrmFramebuffer`,
which attempts to restore the original CRTC framebuffer, mode, position, and
connector attachment set. Treat successful desktop recovery after the canary as
separate evidence from successful rendering.

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
    drmDevice = "/dev/dri/cardN";
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
