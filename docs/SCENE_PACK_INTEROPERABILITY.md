# Scene Pack v1 Interoperability Contract

**Status:** strict production parser and typed core mapping implemented on the hardening branch; exact-head qualification pending  
**Source contract:** [Luminous Platform Scene Pack v1 schema](https://github.com/Luminous-Dynamics/luminous-platform/blob/a9ba54242d19579f5d84e03e8fae483678aa3f24/contracts/ambient-scene-pack-v1.schema.json)  
**Source example:** [First Germination](https://github.com/Luminous-Dynamics/luminous-platform/blob/a9ba54242d19579f5d84e03e8fae483678aa3f24/contracts/examples/first-germination.scene.json)

## Decision

Do not create a second mycelial simulation. The hardening branch now contains `crates/visual-pack`, a strict parser for the pinned Scene Pack v1 contract that maps valid manifests into the existing `SceneSettings` API. It remains separate from the render loop and has no filesystem, network, display, GPU or OS lifecycle authority.

The parser rejects duplicate JSON object keys, unknown fields, invalid versions/IDs/colors/ranges, unsafe relative asset paths, duplicate identifiers, invalid safe regions, inconsistent branch budgets, unconsented inputs and unsupported required capabilities. It returns a private-field `ValidatedScenePack` with read-only getters plus a method to instantiate the shared core. Asset declarations are not read automatically: a host must supply an explicit `AssetHashProvider` whose implementation safely resolves files under the pack root and computes their SHA-256 digest; mismatch or unsafe resolution fails closed. This is a capability boundary, not a claim that the parser itself accesses files or computes hashes.

## Implemented core/API slice on the hardening branch

The reusable core exposes `SceneSettings` and `ScenePalette` through the additive `MycelialNetwork::with_settings(width, height, seed, settings)` constructor. Browser WASM exposes the numeric-seed `VisualScene.createConfigured(...)` API; WIT accepts the same settings record through atomic `configure`. The typed profile carries two distinct branch ceilings: the scene request (`simulation.parameters.branchLimit`) and the independent host/resource ceiling (`resourceBudget.maxBranches`). Construction fails closed if either is outside the hard contract or if the requested branch count exceeds the host ceiling; the growth loop enforces the scene limit before appending children. Configured settings also control depth, growth rate, fixed-step frequency, tick-based pulse timing, deterministic drift, and all six palette roles. A call advances at most `min(fixed_step_hz, 120)` ticks (one simulated second). The renderer enforces a modeled allocation estimate; this is not a whole-process memory limit.

The browser runtime smoke fixture exercises the configured API with the First Germination numeric seed, settings and palette, checks replay identity and invalid configuration rejection, and retains the original phrase-based fixture for backward compatibility. The WIT Component Model now accepts the same typed settings record through an atomic `configure` operation and exposes the bounded `advance-ticks` method. Its Rust unit fixtures compare WIT-configured output against the same native core settings; the Wasmtime host harness exercises that path through the actual component ABI.

The canonical upstream First Germination JSON is pinned byte-for-byte at `tests/fixtures/first-germination.scene.json` (source blob `98165b8958f0121825fa28e373521bb61ce9078c`). The browser smoke test and Wasmtime host test mapper read its declared seed, parameters, RGB palette and static-fallback brightness rather than maintaining separate hard-coded copies. This mapper is deliberately narrower than the production validation pipeline; it does not perform JSON Schema Draft 2020-12 validation, duplicate-key rejection, asset path/hash checking, attribution validation, or signed-pack authorization.

The renderer’s branch storage, dimensions and simulation timing state are now private to `visual-core`; adapters receive read-only accessors. Hosts cannot mutate a validated scene object to bypass dimension and branch settings after construction.

**Still not implemented or qualified:** execution of the parser/core/WASI/Wasmtime tests on the exact PR head; production browser demo integration with `ValidatedScenePack`; host-side safe asset resolution plus actual file SHA-256 verification; signed-pack/attribution policy; general composition mapping for all presentation variants and normalized safe regions; complete OS lock/suspend/wake lifecycle policy; and whole-process/runtime/compositor memory accounting. The WASI CLI and capability-denied Wasmtime qualification harness now use the parser; the WASI CLI supports centered-network `boot` and gradient-only `staticFallback` only, and rejects packs requiring asset access, external capabilities, inputs or safe-region composition it cannot implement. The overall product must not yet be called fully Scene Pack v1 compatible. Exact-head CI and real runtime qualification remain pending.

## Current gaps to resolve

The core is a deterministic CPU reference and the parser validates the Scene Pack v1 manifest contract, but validated metadata is not yet consumed by every browser/WASI/native production host.

| Scene Pack field | Meaning | Required implementation rule | Current implementation status |
|---|---|---|---|
| `simulation.seed` (uint32) | Stable scene identity input | Use a versioned numeric-seed encoding; never stringify the integer as a phrase | **Core implemented.** Domain-separated BLAKE3 seed material; production manifest loader still missing |
| `palette.canvas` | Initial canvas color | Parse `#RRGGBB` once and apply through the configured palette | **Typed renderer implemented.** Test adapters parse the pinned fixture |
| `palette.substrate` | Settled background color | Use as steady background color | **Implemented** |
| `palette.filament` | Growing thread color | Use for primary branches | **Implemented** |
| `palette.node` | Node/pulse color | Use for formed nodes and node-state effects | **Implemented** |
| `palette.lichen` | Secondary/deeper branches | Use for depth-based branch shading | **Implemented** |
| `palette.glow` | Glow/highlight | Map to a documented pulse/highlight role; never ignore it silently | **Implemented** for convergence glow |
| `simulation.parameters.branchLimit` | Scene-requested branch ceiling | Enforce before allocating/appending children | **Implemented** with a hard ceiling and growth-loop enforcement |
| `simulation.parameters.maxDepth` | Branch-depth ceiling | Enforce at every child-spawn check | **Implemented** |
| `simulation.parameters.growthRate` | Growth-speed multiplier | Apply with finite bounds | **Implemented** |
| `simulation.fixedStepHz` | Simulation clock | Use defined fixed-step ticks for replayable state evolution | **Core/API implemented.** Cross-target exact output still requires CI/runtime evidence |
| `simulation.parameters.pulsePeriodSeconds` | Periodic pulse period | Schedule deterministically from integer simulation ticks, not wall-clock time | **Implemented** in fixed-tick mode |
| `simulation.parameters.driftAmplitude` | Normalized drift amount | Implement deterministic drift or reject unsupported settings | **Implemented.** Exact cross-target pixel identity with drift enabled is not yet qualified |
| `resourceBudget.maxBranches` | Independent host/resource branch ceiling | Require `branchLimit` to fit this ceiling; enforce both | **Implemented** in core validation and carried by browser/WIT settings |
| `resourceBudget.maxMemoryMiB` | Resource ceiling | Bound modeled framebuffer and renderer-state allocations | **Partial.** Estimate enforced before configured construction; process/runtime memory is not capped |
| `presentations.*` | Motion/FPS/brightness/composition/safe regions | Resolve the selected named variant in the presentation adapter | **Partial.** Browser reduced-motion and visibility behavior exist; general variant/safe-region mapping is not implemented |
| `presentations.staticFallback` | Safe no-motion fallback | Produce a visible static fallback when animation/GPU is unavailable | **Primitive implemented.** Fixed-point gradient in core, browser WASM and WIT; selection/activation remains host policy |
| `lifecycle.*` | Visibility/suspend/wake policy | Enforce in each host adapter | **Partial.** Browser pauses when hidden; lock/suspend/wake semantics for each OS are not qualified |

The current core has hard ceilings of 8,192 branches and depth 24; default depth is 12. The scene-requested branch limit and `resourceBudget.maxBranches` are separate fields, each constrained by the hard ceiling, with the request required to fit the host ceiling. The configured memory estimate covers only modeled renderer allocations; it does not cap the process, WASM runtime, compositor or browser allocations. For host-facing fields not implemented by the renderer, the parser preserves validated metadata so the selected adapter can apply it or explicitly decline an unsupported presentation/capability.

## Proposed typed settings boundary

The adapter should produce an explicit versioned settings object with these semantic groups:

- **Identity:** scene-contract version, engine version, and a numeric uint32 seed.
- **Palette:** six explicit RGB colors with semantic roles.
- **Simulation:** branch limit, maximum depth, growth-rate multiplier, fixed-step frequency, pulse period, and drift amplitude.
- **Resources:** requested branch ceiling, memory ceiling, max FPS, and pixel budget.
- **Presentation selection:** an exact named presentation variant with motion, brightness, composition, normalized safe regions, and the required static fallback.

The first API extension is now additive: the core retains the phrase-based constructor and adds `MycelialNetwork::with_settings(width, height, seed: u32, settings)`; browser WASM exposes `VisualScene.createConfigured`; WIT accepts the same record through an atomic `configure` operation. The existing phrase constructor and dt-based method keep their historical meanings. All settings are validated before replacing/constructing a scene; unsupported/out-of-range values return errors. WIT exposes the scene-contract version and effective dimensions, but does not yet provide a full readback record for every effective setting.

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

The capability-denied Wasmtime host is the qualification path for the actual resource-based WIT API. The expanded harness now creates a configured scene, advances bounded fixed ticks, compares two independent WIT replays byte-for-byte, checks branch/memory limits, verifies failed reconfiguration is atomic, and explicitly disposes every resource. This test implementation is not evidence of a passing run until CI executes it against the exact head.

Presentation and host lifecycle policy stay outside the guest. In particular, this component never authenticates users, changes the lock state, controls the boot transaction, or obtains direct display access.

## WASI smoke invocation

The WASI CLI can consume the pinned example through bounded stdin without granting filesystem access:

```sh
cat tests/fixtures/first-germination.scene.json | \
  cargo run -p sovereign-visual-wasi -- --scene-pack-stdin \
    --presentation staticFallback --width 640 --height 360 > fallback.ppm
```

The centered-network boot presentation is also supported. Other current compositions are rejected rather than silently ignored. Asset-bearing packs are rejected until a safe host resolver/digest provider is wired; the fixture has no assets.

## Production parser API

- Crate: `sovereign-visual-pack` in `crates/visual-pack`.
- Entry point: `parse_scene_pack_v1(&[u8]) -> Result<ValidatedScenePack, ScenePackError>`.
- Boundaries: 1 MiB manifest input; duplicate-key-aware JSON parsing; unknown-field rejection; typed versioned metadata; strict field/range and cross-field checks.
- Host handoff: `ValidatedScenePack::settings()` exposes immutable effective settings; `instantiate(width, height)` constructs the same `MycelialNetwork` used by other adapters; `verify_asset_hashes(provider)` requires a trusted host-supplied resolver/digest provider.
- Fixture: tests parse `tests/fixtures/first-germination.scene.json`, the byte-identical snapshot of the pinned upstream example. Tests are authored to reject duplicate keys, unknown fields, invalid IDs, budget contradictions, null optional values, asset traversal, invalid safe-region bounds and mismatched asset digests.

The hand-written validator implements the v1 constraints used by the pinned schema rather than depending on a general JSON Schema engine. CI still has to compile/run this crate against the exact head, and an independent conformance corpus should compare schema-valid/schema-invalid examples against the upstream Draft 2020-12 schema before calling the parser fully qualified.

## Traceable source

The contract and First Germination example are at [Luminous Platform PR #16](https://github.com/Luminous-Dynamics/luminous-platform/pull/16); the implementation/integration investigation is tracked by [luminous-platform issue #17](https://github.com/Luminous-Dynamics/luminous-platform/issues/17).
