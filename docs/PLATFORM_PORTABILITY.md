# Platform Portability: Sovereign Visual System

**Status:** architecture plus an initial portable-core/WASM/WASI implementation slice; compile/test qualification and all native surface integrations remain pending  
**Reviewed:** 2026-10-10  
**Scope:** the long-term visual system built around Sovereign Boot's Rust animation work

## Decision

Build one portable Rust visual core with multiple native platform adapters. Do not try to make the existing Linux DRM/KMS executable run unchanged on every device.

The same visual identity, scene logic, deterministic inputs, animation rules, and asset pipeline can be reused widely. The host integration cannot be universal: each OS owns different graphics lifecycles, permission models, power rules, boot paths, and privileged surfaces. A feature is supported only for a tested OS/version/device/surface combination.

Sovereign Boot must remain optional and fail safely. It must never become necessary for normal boot, device unlock, or recovery.

## Separate the portable core from the host

Target architecture; the first visual-core, browser-WASM and headless-WASI packages have been added on the hardening branch, but exact-head qualification is pending and native desktop/mobile/TV/privileged adapters remain unimplemented:

- **Visual core (Rust):** `crates/visual-core` now owns the extracted mycelial scene state, deterministic seed, palette and CPU pixel renderer with no DRM dependency. Accessibility settings, schema-versioned lifecycle events, serializable scene state and power/thermal policy are still work to add.
- **Scene contract:** versioned, typed inputs and events; explicit dimensions, scale, color space, frame timing, lifecycle, reduced-motion, pause/resume, and power/thermal hints. Scene state must be serializable for deterministic replay and cross-platform regression tests.
- **Renderer backends:** direct DRM/KMS remains the Linux boot-only backend. `crates/visual-wasm` returns RGBA frame bytes for a host-owned browser canvas, and `crates/visual-wasi` emits a headless PPM frame; neither owns a display. Compositor/window, native mobile/TV, component-model WIT, and GPU-backed implementations remain future work.
- **Native adapters:** thin Swift/SwiftUI, Kotlin/Android, Windows, browser, and TV platform glue where each platform requires it. Keep substantive animation/state logic in Rust and expose a small stable C ABI or the platform's supported Rust bridge. Do not force a single UI toolkit into privileged shell, mobile, and TV surfaces.
- **Surface-specific lifecycle adapters:** boot splash, desktop wallpaper, screensaver/ambient mode, app visualizer, widgets/at-a-glance UI, and system lock/login surfaces are different integrations—not one generic full-screen process.

Do not make a network service, cloud account, or always-running background daemon a prerequisite for the visuals. Prefer local deterministic rendering. Optional synchronization may distribute signed/versioned scene configuration but cannot be in the critical rendering or boot path.

## Portability matrix

This matrix describes what is technically plausible, not what this repository currently ships.

| Platform / device | Realistic first integration | System-level limit / qualification |
|---|---|---|
| **Linux / NixOS desktops** | Native desktop renderer; NixOS boot-splash integration; Plasma, COSMIC, and GNOME adapters | Direct DRM/KMS is only for an explicitly qualified boot/VT path. Desktop compositor and lock-screen ownership must be respected. Test each DE, compositor, display server, GPU, and version separately. |
| **Other Linux distributions** | User-space desktop app and screensaver; package-specific boot-splash module | Boot/initramfs packaging and service lifecycle differ by distribution. NixOS support does not imply general Linux boot support. |
| **Windows 10/11** | Desktop app/visualizer and a traditional screensaver implementation; optional animated wallpaper host integration | A normal app cannot assume ownership of the shell, secure desktop, sign-in screen, or lock screen. Windows wallpaper APIs for ordinary images are not themselves a portable animated-wallpaper API. |
| **macOS** | Desktop companion, native Screen Saver integration, static exports and widgets where appropriate | Use the platform's supported screen-saver and app lifecycle. Login window and lock-screen surfaces are separate, restricted system contexts. Build/test Apple Silicon and Intel as required by the selected support window. |
| **Android phones/tablets** | Native app and Android `WallpaperService` live-wallpaper adapter | Launcher and OEM behavior varies. Live wallpaper does not imply arbitrary lock-screen animation or unrestricted background execution. Suspend rendering when hidden and obey battery/thermal budgets. |
| **Android TV / Google TV devices** | Full-screen visualizer app; launcher wallpaper only where the device/launcher actually exposes it | TV devices are not interchangeable with phones. Qualify each launcher, remote-control interaction, screen size, and burn-in/power behavior. |
| **iPhone / iPad** | Native app/visualizer; user-selectable static or motion assets where supported; Lock Screen/Home Screen widgets and Live Activities for glanceable state | Do not promise an unrestricted always-running live wallpaper or arbitrary lock-screen renderer. iOS/iPadOS control background execution and lock-screen presentation. Live Activities/widgets are constrained native surfaces, not full-screen animation hosts. |
| **Apple Watch / Vision Pro** | Small native complications/widgets or an in-app visualization where the OS supports it | Screen size, battery, input model, and available widget/Live Activity features differ. No assumption that a phone surface automatically exists on these devices. |
| **Apple TV / tvOS** | tvOS app with an in-app ambient visualizer | Do not assume third-party apps can replace the system screensaver or use a generic system-wide wallpaper hook. Verify any proposed system integration against the specific public tvOS APIs and distribution rules. |
| **Samsung Tizen TVs** | TV application/visualizer; screensaver-related behavior only through APIs and distribution paths supported for the target TV | Model/year/API profiles vary. The documented AppCommon screensaver API controls screensaver behavior for an app; it is not proof that every app can replace the system screensaver. |
| **LG webOS TVs** | TV app or visualizer using supported webOS TV APIs | Follow model-specific application and screensaver policy. Do not assume an app can run globally over Home, settings, HDMI input, or other applications. |
| **Roku** | Dedicated Roku screensaver app or standalone visual experience | Roku explicitly separates screensaver apps from ordinary streaming/utility apps and restricts interaction while a screensaver runs. Build a native Roku implementation, not a desktop binary port. |
| **ChromeOS** | Web/PWA app, Android app, or Linux app depending on device/policy | These execution modes do not grant a generic right to replace the login screen, lock screen, or system wallpaper. Managed devices add administrator policy constraints. |
| **Browsers / web** | Shared web visualizer, configuration/editor, preview, and embeddable canvas experience | Browser content cannot take over OS boot, lock screens, the desktop shell, or system-wide screensaver behavior. WebGPU/WebGL availability and performance must be checked with fallbacks. |
| **Embedded, automotive, signage, appliances** | Rust core embedded in an OEM-controlled app or user interface | System startup, safety display, and persistent foreground surfaces require product-owner/OEM authorization, hardware qualification, and sometimes a safety/security review. Closed firmware may make integration unavailable without the vendor. |
| **UEFI/firmware splash, secure boot UI, pre-OS screens** | Vendor/OEM or bootloader-specific integration where allowed | Not a normal user-space application surface. Secure Boot, signed binaries, firmware policy, bootloader capabilities, and platform ownership constrain what can run. Do not promise a universal replacement. |

## Device surfaces and support tiers

Use surface capability, rather than a single “supported OS” checkbox, as the unit of support.

- **Tier A — portable core:** builds for a target; deterministic scene tests pass. This proves only that core logic works there.
- **Tier B — app surface:** installed and usable as an ordinary foreground/in-app experience on the exact target.
- **Tier C — user-customizable surface:** a supported wallpaper, screensaver, ambient mode, widget, or launcher integration is packaged and tested through that OS's documented extension point.
- **Tier D — privileged system surface:** boot splash, login/greeter, lock screen, secure desktop, or firmware UI. Requires an explicit supported host integration, owner/OEM authority where necessary, and failure/security qualification.

Never infer Tier C or D from a Tier A build. If an OS intentionally reserves a surface, the truthful path is an adjacent experience or an OEM/system integration—not a bypass.

## Engineering rules

1. **Keep authority with the OS.** The visual process never authenticates the user, handles secrets, or decides whether a session is unlocked. A crashed lock-screen visual must not reveal the underlying desktop.
2. **Fail open for boot, fail closed for security boundaries.** A failed boot animation must not stop boot. If a lock-screen host cannot guarantee session isolation, do not install our renderer there.
3. **Respect display ownership.** Only use direct DRM/KMS under a documented VT/boot canary contract. Desktop/mobile/TV applications render through host-managed surfaces.
4. **Bound work.** Pause when hidden; use frame caps, adaptive quality, reduced-motion/static modes, and explicit GPU/CPU/memory budgets. Respect suspend, battery saver, thermal pressure, content playback, and display power states.
5. **Protect OLED and signage panels.** Avoid static high-contrast regions; use bounded movement, dimming, idle timeout and platform policy. Never interfere with the device's native burn-in protection.
6. **Make scenes deterministic.** Given a scene schema version, seed, and parameter set, the core produces reproducible state transitions. Capture outputs for regression testing across architectures.
7. **Version the boundary.** Core/adapter protocol changes are versioned and capability-negotiated. Unsupported effects degrade predictably to simpler rendering instead of crashing.
8. **Keep privileged footprint minimal.** Desktop/mobile/TV adapters run unprivileged. Only the small, explicit boot integration runs with the privileges required for its chosen host path.
9. **No production Python dependency.** Keep implementation and packaging in Rust and the relevant native build systems. Test harnesses should not turn Python into a runtime requirement.

## Qualification gates

A platform/surface is not supported until its exact release artifact has:

- a locked and reproducible build for the target architecture;
- core deterministic tests and golden-scene comparisons;
- lifecycle tests for start, stop, suspend, resume, display changes, and repeated activation;
- resource measurements on representative low-, mid-, and high-end devices;
- reduced-motion, accessibility, thermal, battery, and burn-in checks where applicable;
- explicit no-DRM/no-GPU/renderer-crash fallback tests;
- security-boundary review for lock, login, kiosk, vehicle, and firmware contexts;
- installation/uninstall/upgrade rollback tests and clear versioned support notes.

CI should cross-compile where practical, but cross-compilation is not device qualification. Use simulators/emulators for broad deterministic coverage, then physical device captures and owner-playtests for each launch surface. Record exact artifact hash, OS version, device/SoC/GPU, surface, adapter version, tests, and outcome. A queued run is not a pass.

## Recommended delivery order

1. Finish independent build/test qualification for the current Linux DRM/KMS renderer and NixOS module. Keep lifecycle/recovery binaries out until actually extracted and qualified.
2. Extract a platform-neutral Rust scene/state contract from the renderer; add deterministic fixtures and a CPU/static fallback before creating new adapters.
3. Ship a reliable NixOS boot integration, then Linux desktop support through compositor-managed rendering. Prove Plasma first, then separately qualify COSMIC and GNOME.
4. Add a shared-core desktop app for Windows and macOS, with native packaging and supported screensaver/wallpaper integrations.
5. Add Android live wallpaper and TV visualizer/screen-saver targets through their own SDKs and lifecycle APIs.
6. Add iOS/iPadOS through the surfaces Apple permits (app, static/motion wallpaper workflow where available, widgets/Live Activities), not by promising a permanently running live wallpaper.
7. Extend to Roku, Tizen, webOS, ChromeOS, watchOS, visionOS, automotive, signage, and embedded targets one verified capability at a time. OEM-only surfaces remain partnership work.

## External platform references

These official references illustrate why separate adapters are required; they are not claims of integration by this repository.

- Android `WallpaperService`: https://developer.android.com/reference/android/service/wallpaper/WallpaperService
- Apple WidgetKit (widgets and other glanceable surfaces): https://developer.apple.com/documentation/widgetkit/
- Apple ActivityKit / Live Activities: https://developer.apple.com/documentation/activitykit/displaying-live-data-with-live-activities
- Apple Screen Saver framework for macOS: https://developer.apple.com/documentation/ScreenSaver
- Apple background execution constraints: https://developer.apple.com/documentation/Xcode/configuring-background-execution-modes
- Wayland session-lock protocol (staging; compositor policy applies): https://wayland.app/protocols/ext-session-lock-v1
- Microsoft Windows desktop wallpaper API reference: https://learn.microsoft.com/en-us/windows/win32/api/shobjidl_core/nf-shobjidl_core-idesktopwallpaper-getwallpaper
- Samsung Tizen TV screensaver guidance: https://developer.samsung.com/smarttv/develop/guides/fundamentals/setting-screensaver.html
- LG webOS TV screensaver guidance: https://webostv.developer.lge.com/develop/guides/screensaver
- Roku screensaver app contract: https://developer.roku.com/dev/docs/screensavers

## Current truth

As of this document's review date, Sovereign Boot is not a cross-platform product. The hardening branch now contains a portable Rust scene crate, a browser-oriented WASM binding crate, and a headless WASI PPM-output executable; their exact-head CI builds and tests are pending. Symthaea separately already has its own browser-targeted Spore WASM kernel, but that is not the same ABI as the visual renderer. No desktop wallpaper, phone wallpaper, TV screensaver, lock/login screen, firmware splash, or OEM integration is implied by a successful portable-core build. This document defines the evidence required to earn those claims.
