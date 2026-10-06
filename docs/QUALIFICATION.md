# Sovereign Boot Qualification

This document records what is actually qualified versus merely present in the
repository. Green means the exact evidence named in the row exists.

| Gate | Status | Evidence required |
|---|---|---|
| Standalone Cargo topology | PASS_STATIC | Root workspace contains only the renderer. |
| Dependency lock | PASS_STATIC | Committed Cargo.lock contains the renderer closure. |
| CLI/module alignment | PASS_STATIC | Module invokes only arguments implemented by quicken-fb. |
| DRM probe | PASS_OBSERVED | 2026-10-05: rebuilt artifact SHA256 `53902aa03eabbf59210ea57020db803d98530c238ad910b09ef123a3316ab846`; `/dev/dri/card1`, Intel `i915`, `boot_vga=1`, connector `eDP-1`, CRTC `59`, `1920x1080@144Hz`; exact receipt: `drm-ok device=/dev/dri/card1 connector=eDP-1 crtc=crtc::Handle(59) selection=current mode=1920x1080 refresh=144Hz`. Card0/NVIDIA reported disconnected connectors. Probe receipts also distinguish a `current` CRTC from a `free-compatible` CRTC selected after compositor release. |
| Renderer execution | FAIL_OBSERVED / REWORKED | First live physical canary (binary SHA256 `2264808f1583f12c70b7584838d3bdf57ad1ab8424f25798520b55d857808bc0`) produced a black screen; the session was recovered by reboot, so no CRTC restoration evidence was captured. The manual canary boundary has since been hardened with active-VT ownership, KD_GRAPHICS handoff, extended signal handling, bounded duration, strict render-buffer sizing, and explicit restoration verification. A later automated run demonstrated a separate launcher defect: --isolate terminated the active KDE/SDDM session by isolating multi-user.target; no renderer success or restoration receipt was observed. The destructive isolation path is therefore removed from the launcher. |
| Nix package build | PASS_LOCAL_OBSERVED / PENDING_HOSTED | Local x86_64-linux `nix build .#quicken-fb` completed successfully on 2026-10-05; hosted build still pending. |
| VM boot integration | BLOCKED / REWORKED | The no-DRM boundary remains valid; the new boot-scoped physical-canary service needs a VM gate proving request consumption, TTY ownership, ordering before display-manager, and non-required failure semantics. |
| Physical boot integration | BLOCKED | Automated qualification is now designed as a reboot-scoped one-shot service rather than live desktop isolation. Physical render/restore remains unqualified until that path is exercised and its persistent receipt is independently reviewed. |
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

The renderer captures all connectors currently attached to the selected CRTC before
calling legacy SETCRTC. Restoration is now an explicit operation that re-queries the
CRTC and connector topology after SETCRTC and refuses to claim success when framebuffer,
mode, position, or connector attachment differs from the original snapshot. Drop uses
the same restoration path as a final best-effort fallback, and the normal exit path emits
a drm-restore-ok receipt only after verification succeeds.

The renderer also rejects undersized source buffers before mapping the DRM dumb buffer;
there is no partial-frame fallback during qualification.

This is still source-level qualification until a controlled renderer execution
observes and records successful restoration on the target hardware or a VM.

## Reproducibility hardening

Hosted CI uses Cargo --locked for check/test/clippy and Nix --no-write-lock-file for
the package build, preventing silent lockfile or dependency-graph drift during
qualification.

## KMS selection boundary

A connected connector is no longer treated as unusable merely because `current_encoder` is absent. The selector first preserves an existing connector-to-CRTC mapping when one exists; otherwise it searches the connector's advertised encoders for a compatible CRTC that is currently unoccupied. If no such CRTC exists, selection fails closed rather than stealing an unrelated active output.

## Local artifact evidence

The rebuilt `result/bin/quicken-fb` on the canary checkout is 821 KiB and has SHA256 `53902aa03eabbf59210ea57020db803d98530c238ad910b09ef123a3316ab846`. The non-mutating physical probe completed successfully against `/dev/dri/card1` and selected `eDP-1` / CRTC 59 at 1920x1080@144Hz with `selection=current`.


## Boot-scoped physical qualification

The first automated desktop isolation experiment is explicitly rejected as a qualification mechanism. The 2026-10-05 journal showed SDDM receiving SIGTERM at the canary start time, followed by display-manager restart and a new KDE login session. No drm-ok, drm-restore-ok, or CANARY RESULT: PASS receipt was observed for that run.

The replacement design arms a root-owned one-shot request while the desktop remains running, then reboots. On the next boot, systemd starts the request-gated canary before display-manager.service, with StandardInput=tty and TTYPath=/dev/tty1. The service is wanted by multi-user.target but is not a requirement of it, so a failed physical canary cannot make normal boot depend on renderer success. The request is consumed once and the result is retained under /var/lib/sovereign-boot/physical-canary.result. Armed requests expire after 15 minutes, so an abandoned reboot request cannot unexpectedly trigger a later unrelated boot.

This is aligned with systemd's distinction between ordering and requirement dependencies: Before= controls sequencing, while Wants=/Requires= control whether another unit is a dependency.


### Request identity and interruption semantics

The boot-scoped physical canary request is now bound to both the configured DRM
device and the SHA-256 digest of the renderer artifact that was probed before
reboot. A digest mismatch or device mismatch is a fail-closed qualification
failure and is recorded without executing the renderer.

The request is atomically renamed to a transient `physical-canary.request.inflight`
state before renderer execution. This makes the request single-consumption:
an interrupted renderer cannot silently cause the same modeset attempt to repeat
on every future boot. The persistent result remains the authoritative
cross-boot receipt for the consumed request.


### Evidence artifact layout

A completed boot-scoped canary produces a structured manifest plus two raw evidence
captures:

- `physical-canary.result`: stable key/value qualification manifest.
- `physical-canary.probe`: exact boot-time non-mutating probe receipt.
- `physical-canary.output`: exact bounded renderer output.

The manifest records SHA-256 digests of the renderer artifact, preboot probe,
boot-time probe, renderer output, and restoration receipt. The raw evidence files
and manifest are written atomically with mode 0600.


### Service-level termination evidence

The boot-scoped canary has an `ExecStopPost` evidence boundary. If the renderer
is killed before it can emit its normal manifest—for example by the service
timeout—the post-stop helper records `FAIL_SERVICE_TIMEOUT` or
`FAIL_SERVICE_ABORTED` with the systemd service result and exit status. The
helper correlates against the current request ID, so an older persistent result
cannot mask a new failure.

This preserves the distinction between renderer-level receipts and
service-orchestration failures.


### Live-session non-interference

The physical-canary service intentionally uses ordering dependencies
(`Before=display-manager.service`, `Before=getty@tty1.service`) without a
`Conflicts=` relationship against those units. A conflicting unit could create a
stop transaction against an already-running display manager or getty if an
operator accidentally started the canary manually from a live system. The
canary instead checks `display-manager.service` at execution time and refuses
with `FAIL_DISPLAY_MANAGER`, while `StandardInput=tty-fail` prevents silent
fallback when the required controlling VT is unavailable.

This preserves a non-interference invariant: an accidental canary invocation
must fail closed without intentionally tearing down the active desktop.
