# Scene Pack v1 Interoperability Contract

**Status:** integration design and gap inventory; the Scene Pack is not yet consumed by this renderer  
**Source contract:** [Luminous Platform Scene Pack v1 schema](https://github.com/Luminous-Dynamics/luminous-platform/blob/a9ba54242d19579f5d84e03e8fae483678aa3f24/contracts/ambient-scene-pack-v1.schema.json)  
**Source example:** [First Germination](https://github.com/Luminous-Dynamics/luminous-platform/blob/a9ba54242d19579f5d84e03e8fae483678aa3f24/contracts/examples/first-germination.scene.json)

## Decision

Do not create a second mycelial simulation and do not pretend the current seed-phrase renderer is already a Scene Pack consumer. Qualify the existing visual core, then add a typed adapter from the validated Scene Pack to a versioned core-settings API.

The Scene Pack remains data, not executable code. JSON Schema validation, duplicate-key rejection, path and asset-hash checks belong in the loader/adapter boundary, outside the frame loop. The renderer receives an already-validated, bounded typed configuration.

## Implemented core/API slice on the hardening branch

The reusable core now includes `SceneSettings` and `ScenePalette` plus the additive `MycelialNetwork::with_settings(width, height, seed, settings)` constructor. The browser adapter also exposes a numeric-seed `VisualScene.createConfigured(...)` entry point and `advance_ticks`. The implementation enforces branch/depth limits during growth, growth-rate scaling, tick-based pulse scheduling, deterministic positional drift, six renderer palette roles, and a conservative renderer-owned memory estimate. Catch-up batches are capped at 120 ticks.

The browser runtime smoke fixture now exercises this configured API with the First Germination seed/settings/palette values, checks replay identity and invalid configuration rejection, and retains the original phrase-based fixture for backward compatibility.

**Still not implemented:** JSON/Scene Pack parsing, presentation-variant/safe-region semantics, static gradient fallback generation, full-process memory accounting, lifecycle enforcement in each host, and the configured settings constructor in the WIT Component Model interface. Therefore the project must not yet claim complete Scene Pack v1 compatibility or equivalent configured behavior across WASM and WASI.

## Current gaps to resolve

The current core is a deterministic CPU reference, but the exposed APIs do not implement every Scene Pack v1 semantic.

| Scene Pack field | Meaning | Required implementation rule |
|---|---|---|
| simulation.seed (uint32) | Stable scene identity input | Define a versioned numeric-seed encoding; do not stringify the integer as a seed phrase |
| palette.canvas | Initial canvas color | Parse #RRGGBB once and apply through the configured palette |
| palette.substrate | Settled background color | Use as the steady background color |
| palette.filament | Growing thread color | Use for primary branches |
| palette.node | Node/pulse color | Use for formed nodes and node-state effects |
| palette.lichen | Secondary/deeper branches | Use for depth-based branch shading |
| palette.glow | Glow/highlight | Map to a documented pulse/highlight role; never ignore it silently |
| simulation.parameters.branchLimit | Per-scene branch ceiling | Enforce before allocating/appending new branches |
| simulation.parameters.maxDepth | Branch-depth ceiling | Enforce at every child-spawn check |
| simulation.parameters.growthRate | Growth speed multiplier | Apply to the growth equation with finite bounds |
| simulation.fixedStepHz | Simulation clock | Use a defined fixed-step tick model for replayable state evolution |
| simulation.parameters.pulsePeriodSeconds | Periodic pulse period | Schedule pulses deterministically from fixed simulation ticks, not wall-clock time |
| simulation.parameters.driftAmplitude | Normalized drift amount | Implement deterministic drift or reject the setting until the engine version supports it |
| resourceBudget.maxBranches | Host-level aggregate branch ceiling | Require branchLimit to fit this ceiling; enforce both in the renderer |
| resourceBudget.maxMemoryMiB | Total resource ceiling | Estimate/enforce framebuffer plus renderer-state allocations; dimensions alone are not enough |
| presentations.* | Motion/FPS/brightness/composition/safe regions | Resolve through the selected presentation adapter and record the effective variant |
| presentations.staticFallback | Safe no-motion fallback | Produce a host-visible static fallback even when animation/GPU support is unavailable |
| lifecycle.* | Visibility/suspend/wake policy | Enforce in each host adapter; do not assume the guest can observe the OS lifecycle itself |

The current core has hard ceilings of 8,192 branches and depth 12. Those are implementation defaults, not evidence that all schema values are applied. A declared value must either affect runtime behavior or cause a typed unsupported-setting error.

## Proposed typed settings boundary

The adapter should produce an explicit versioned settings object with these semantic groups:

- **Identity:** scene-contract version, engine version, and a numeric uint32 seed.
- **Palette:** six explicit RGB colors with semantic roles.
- **Simulation:** branch limit, maximum depth, growth-rate multiplier, fixed-step frequency, pulse period, and drift amplitude.
- **Resources:** requested branch ceiling, memory ceiling, max FPS, and pixel budget.
- **Presentation selection:** an exact named presentation variant with motion, brightness, composition, normalized safe regions, and the required static fallback.

The first API extension should be additive: retain the existing phrase-based constructor for callers that depend on it, and expose a versioned settings-based construction path. Do not change the meaning of the current WIT/browser v1 constructor in place. A later settings-based API should expose the effective settings (including normalized limits) to the host and return typed errors for unsupported values.

### Numeric seed rules

The v1 settings adapter must define how the uint32 becomes the RNG input. A safe initial rule is a domain-separated byte encoding: fixed ASCII domain tag luminous.scene-pack.seed.v1, a NUL separator, and the seed encoded as four little-endian bytes; hash this byte string with BLAKE3 and use the 32-byte output as the ChaCha seed. The domain tag and byte order become part of the versioned engine contract. Do not use Rust's default hasher, Display formatting, an OS RNG, or an implicit platform-endian representation.

Legacy phrase-based seeding remains a distinct constructor path until its users are deliberately migrated. The two seed modes must not be described as equivalent.

### Fixed-step rules

For deterministic engine state, the canonical advance operation should be an integer count of simulation ticks rather than an arbitrary floating-point wall-clock delta. Each tick represents exactly the reciprocal of the validated fixedStepHz within the engine's declared numeric model. Presentation hosts can use their own clock and frame rate, but should advance zero or more canonical ticks; hidden/suspended hosts must pause rather than perform an unbounded catch-up after resume.

If the current dt-based API remains for compatibility, keep it explicitly separate and do not claim it implements fixed-step Scene Pack v1 semantics until the mapping and replay behavior are specified and tested.

## Fail-closed acceptance criteria

Before claiming Scene Pack v1 support:

1. **Validated sample mapping:** load the exact First Germination fixture and record a normalized typed configuration, including every schema field that affects simulation.
2. **No ignored required semantics:** every simulation and palette field is applied, or the adapter reports a typed unsupported-field failure. The sample's seed, six colors, 2,048-branch ceiling, depth 10, 0.28 growth rate, 30 Hz tick rate, 7.5-second pulse period and 0.12 drift amplitude all need explicit handling.
3. **Budget enforcement:** prove the 2,048 effective branch budget is enforced inside child spawning, not after rendering; prove memory-budget rejection/normalization with boundary tests.
4. **Golden fixture:** store expected effective settings and reference RGBA bytes for a named engine version and replay sequence. Record manifest digest, settings digest, source commit, dimensions and output digest.
5. **Adapter parity:** exercise browser WASM and WIT Component Model settings-based APIs with the same fixture. Keep cross-target pixel identity claims scoped to targets and runtime versions actually tested.
6. **Presentation mapping:** test all five presentation variants, safe-region bounds, reduced motion and static fallback as host-level behavior. A valid manifest or rendered frame alone does not prove compositor lifecycle behavior.
7. **Negative fixtures:** invalid seeds/values, out-of-range colors, contradictory branch budgets, unsupported engine versions, unknown required settings, oversized output and unsupported presentation capabilities fail closed.
8. **Exact-head evidence:** the host runtime, core tests, browser-WASM parity and Nix/Cargo standalone gates must finish green for the exact commit before adoption is claimed.

## Relationship to the current Wasmtime work

The capability-denied Wasmtime host is a prerequisite qualification test for the existing resource-based WIT API. Its existence does not itself make the core Scene Pack-compatible. The new typed core and browser settings path can be tested independently; extend the WIT Component Model surface after the base host harness compiles and instantiates the current interface, so the two contract changes remain separately diagnosable.

Presentation and host lifecycle policy stay outside the guest. In particular, this component never authenticates users, changes the lock state, controls the boot transaction, or obtains direct display access.

## Traceable source

The contract and First Germination example are at [Luminous Platform PR #16](https://github.com/Luminous-Dynamics/luminous-platform/pull/16); the implementation/integration investigation is tracked by [luminous-platform issue #17](https://github.com/Luminous-Dynamics/luminous-platform/issues/17).
