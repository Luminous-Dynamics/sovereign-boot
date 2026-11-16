# HHLF / Living Lattice — Sovereign Boot Research Programme

**Status:** experimental research proposal, 2026-11-16. HHLF (Hyperdimensional Holographic Liquid Fractals) is a working name for a proposed synthesis, not an established scientific category. This document does not claim that the system is alive, conscious, cryptographically secure, post-quantum, or superior to existing renderers.

## Decision

Adopt HHLF as an **optional, CPU-first Ouroboros visual-research theme** and a separate computational-substrate experiment. Keep it outside the trusted boot chain.

Sovereign Boot's current safety contract says it may observe boot but must never be required for boot. HHLF must preserve that contract. A crash, timeout, invalid state, resource exhaustion, unavailable GPU or unsupported display mode must degrade to the existing safe/static/fallback path without affecting boot, recovery or trust decisions.

This programme has three independent research tracks:

1. **Living Lattice renderer:** a bounded, deterministic visual composition for boot/lifecycle presentation.
2. **Artificial-life substrate:** experiments in self-maintenance, repair, viability and open-ended evolution. Not part of boot-critical code.
3. **Cryptographic research:** an entirely separate theoretical track. No HHLF-derived primitive is allowed to protect data or authenticate boot artifacts.

## What HHLF means operationally

Do not make the name substitute for an implementation definition.

- **Hyperdimensional:** structured high-dimensional vectors with explicit encoders, binding, bundling, similarity and retrieval semantics.
- **Holographic:** distributed, redundant representation with measured reconstruction under controlled erasures/noise. It does not mean a metaphysical hologram.
- **Liquid:** a time-evolving state model with an explicit update equation, bounded step size, resource budget and stability/viability conditions.
- **Fractal:** a multiscale structure for which scaling behaviour is measured over a declared range with uncertainty. Recursive nesting alone is not sufficient evidence of a fractal.
- **Living:** a research hypothesis requiring operational evidence of endogenous maintenance, boundary regeneration, repair, resource throughput, reproduction and evolution. A fluid animation is not alive.

The first prototype should be called an **HHLF-inspired Living Lattice**, not “a living system.”

## Architecture

### A. Pure state model

Start with a deterministic, CPU-only reference model. A candidate state may contain:

- a fixed-dimensional or explicitly bounded hypervector state;
- a small, versioned multiscale graph/lattice with stable node IDs;
- per-node activity and local state;
- cross-scale couplings with bounded coefficients;
- a fixed-step update rule;
- explicit resource counters and terminal/inconclusive states.

Do not choose a dimension, branching factor, criticality value or fractal dimension because it looks impressive. Sweep them under a frozen budget and measure actual benefit.

The transition function must be pure for fixed state, input observation, configuration and seed. Use a documented deterministic non-cryptographic PRNG only for repeatable visual initialization. Never treat its seed or output as entropy, secret material or a cryptographic primitive.

### B. Observation adapter

The renderer may consume only an allowlisted, bounded, non-sensitive projection of lifecycle observations, such as stage identifiers or coarse progress categories. It must not receive passwords, secret keys, raw command lines, environment dumps, user data, TPM secrets or unredacted security events.

Every observation is versioned, bounded and validated. Missing or unknown inputs map to a neutral state, not a guessed success state.

### C. Renderer adapter

- CPU reference renderer is mandatory and authoritative for deterministic fixture output.
- GPU/WGPU/Vulkan backends are optional optimizations, never prerequisites.
- Rendering and state evolution have independent budgets.
- The renderer cannot mutate boot state, select recovery policy, grant authority or certify integrity.
- No network access, plugin loading or arbitrary runtime code from the visual theme.
- Reduced Motion and static-image modes preserve all boot semantics.
- Renderer failures are recorded as presentation failures only.

### D. Evidence

A rendering receipt may say that a given versioned state/configuration produced a frame under a named renderer/build and budget. It cannot claim human perception, system integrity or cryptographic verification unless those claims are independently evidenced elsewhere.

Keep presentation observations separate from secure-boot measurements. Use existing receipt/canonicalization patterns without silently changing OUROCKP1 or OUROEVB4 bytes.

## Paradigm shifts to test

1. **From animation frames to a persistent stateful field:** the field is a deterministic function of entity identity, versioned state and bounded observations; it is not merely a sequence of unrelated shader frames.
2. **From hand-authored complexity to measurable emergence:** use small local rules and test whether higher-order patterns arise, then compare with static procedural graphics and simpler dynamical baselines.
3. **From one global “complexity” value to scale-resolved diagnostics:** report scale-dependent activity, coupling, entropy, reconstruction and geometry separately.
4. **From visual realism to structural continuity:** the same entity should remain recognizable across resolution, aspect ratio, renderer and restart while preserving its state identity.
5. **From unbounded adaptivity to viable adaptivity:** local updates operate inside explicit energy, activity, memory and deadline budgets. If a state exits the viable set, freeze or revert without blocking boot.
6. **From GPU-first spectacle to portable determinism:** a CPU oracle defines semantics; GPU implementations must conform to it within explicitly declared visual tolerances.
7. **From aesthetics as a success signal to evidence-qualified presentation:** never let animation quality or motion stand in for boot correctness.
8. **From a single scale to cross-scale interaction:** micro/meso/macro layers exchange bounded signals, and their causal contribution is tested via ablations.
9. **From “edge of chaos” folklore to task-conditioned operating maps:** map stability, exploration, robustness, latency and energy across parameters. Do not assume the critical boundary is optimal.
10. **From a proposed living form to an artificial-life experiment:** test regenerative causal closure and resource-bounded viability independently from the boot animation.

## Benchmark programme

### 1. Correctness and safety

- Same state/config/seed/build => byte-identical CPU reference output.
- Every state variable and update remains finite and within declared bounds.
- Fixed iteration, node, memory and time budgets; exhaustion is explicit, never a pass.
- Identical inputs replay deterministically; input order/serialization changes are detected.
- Unknown schema, oversized observation, invalid coefficient, NaN/Inf, integer overflow and malformed receipt fail closed within the visual subsystem.
- Fallback occurs without blocking boot or altering recovery/security state.
- Static/reduced-motion modes preserve stage ordering and semantic content.
- No secret-bearing input is accepted by the observation adapter.

### 2. Visual and structural continuity

Compare against a procedural/static baseline and existing boot/desktop visual runtimes on matched scenes:

- multi-scale self-similarity diagnostics across a declared scale interval;
- structural similarity and perceptual difference (with color-space and display assumptions documented);
- frame-to-frame motion continuity and flicker/flash constraints;
- topology consistency and landmark/feature drift across renderers;
- state continuity across restart, resolution, aspect ratio and device;
- blinded human preference/readability/comfort studies, separate from technical metrics.

A visually complex image is not a better result unless it improves a pre-registered objective.

### 3. Performance and resource use

Record exact host, CPU, display mode, kernel, build SHA, renderer, config digest and power source.

- setup/initialization time;
- p50/p95/p99 update and render duration;
- missed frame/deadline and fallback counts;
- CPU time, peak RSS, allocation count and bytes;
- GPU time/memory only when a backend provides genuine telemetry;
- energy per animation-second and per generated frame where a wall meter is available;
- idle/hidden/paused CPU/GPU/energy;
- restart and reduced-motion transition time.

Initial targets are hypotheses only: a 30 FPS CPU baseline on a named low-end host is a sensible first quality gate; 60 FPS may be a target for qualified hosts. Establish an actual baseline before adopting memory or energy thresholds. Do not publish cross-device comparisons without matched scenes and trials.

### 4. Artificial-life experiments

A separate simulator should test:

- **Boundary maintenance:** can the process regenerate the structures defining its own operational boundary?
- **Causal closure:** do interacting processes regenerate the components and constraints required for their continued operation?
- **Resource budget:** is there an explicit energy/compute/material analogue, and does the system obey it?
- **Homeostasis:** can it keep essential variables within viability bounds after perturbation?
- **Repair:** can it recover from component loss without a complete external reset?
- **Reproduction:** can it produce viable descendants with a traceable lineage?
- **Evolution:** can inherited variation produce adaptation under changing environments without collapsing diversity?
- **Open-endedness:** do new functional capabilities continue to emerge under controlled environments, rather than merely growing complexity or size?

Include null models: static procedural graphics, ordinary adaptive controllers, self-repair scripts and non-autopoietic evolving automata. A system that simply follows a programmed reset path must not pass an endogenous-repair test.

### 5. Cryptographic research boundary

HHLF is not a cryptographic family until a specific primitive, threat model, hardness assumption and security definition exist.

Rules:

- No HHLF state, chaotic sequence, fractal seed or pseudorandom visual state may generate keys, nonces, salts, entropy claims or boot trust decisions.
- Do not derive security from high dimension, fractal appearance, sensitivity to initial conditions, statistical randomness tests or difficulty of visual reconstruction.
- Preserve standard, independently reviewed PQC for production: ML-KEM for key establishment; ML-DSA and SLH-DSA for signatures; follow current NIST migration guidance and evaluate standardized/selected alternatives such as HQC on their actual standardization status.
- HHLF cryptographic experiments, if any, live in an isolated research crate/repository and are never negotiated by the boot path.
- Any candidate must undergo public cryptanalysis, classical and quantum security analysis, constant-time/side-channel review, formal analysis and independent implementation review before even a deployment proposal.

The research question is not “can fractals encrypt?” It is “does a precisely defined problem derived from a multiscale/dynamical algebra have a defensible hardness assumption against classical and quantum adversaries, and can it support a secure, efficient primitive?” A negative result is valuable and should be published.

## Industry comparison

Do not compare HHLF with an entire industry category. Compare a fixed render workload against:
- simple static image and shader baselines;
- deterministic procedural fractal renderers;
- ordinary particle/fluid systems;
- CPU software renderer;
- a qualified GPU shader implementation;
- existing boot/desktop presentation runtimes only where integration and power behavior are comparable.

HHLF must demonstrate a specific advantage: greater state continuity, more useful adaptation, lower energy at equal perceptual quality, better degradation under constrained compute, or an experimentally validated artificial-life property. “Looks more complicated” is not a competitive advantage.

## Execution plan — no CI runner dependency

### Phase 0 — Contract and safety fence
- Freeze this definition and threat model.
- Define versioned state/config/input types and observation allowlist.
- Specify hard resource caps and safe fallback.
- Keep all work opt-in and off the secure boot trust path.

### Phase 1 — CPU reference oracle
- Add a minimal no-network, no-GPU Rust model in an isolated research module/crate.
- Use deterministic seed/config and fixed-point or carefully specified floating-point arithmetic.
- Add canonical state serialization, bounded stepping and deterministic test fixtures.
- Implement a simple static/procedural baseline first.

### Phase 2 — Ablation and emergence
- Compare HDC-only, fractal graph-only, liquid-dynamics-only, pairwise combinations and full HHLF.
- Measure whether the combined system exhibits any benefit not explained by a simpler component.
- Record negative results and fragile parameter regions.

### Phase 3 — Ouroboros theme
- Add a theme adapter that consumes allowlisted lifecycle observations.
- Preserve entity identity and state continuity.
- Add CPU render and static fallback before GPU.
- Add reduced-motion and accessibility fixtures.

### Phase 4 — Artificial-life lab
- Implement boundary/resource/repair/reproduction/evolution protocols in a separate simulation harness.
- Require intervention-based evidence of causal closure; resilience alone does not pass.
- Compare with null models.

### Phase 5 — Optional acceleration and independent qualification
- Port the same semantics to GPU only after CPU correctness is frozen.
- Cross-backend conformance, power measurement and named-host comparisons.
- External scientific and security review before broad claims.

## Non-claims

This programme does not establish that HHLF is alive or conscious; that its visual appearance reflects system health; that it is a new PQC family; that fractal/chaotic dynamics provide entropy or cryptographic hardness; that it improves boot security; or that it outperforms existing renderers. Those are separate hypotheses requiring separate experiments.

## References and watch list

- IEEE CIS Task Force on HDC/VSA: https://cis.taskforce.ieee.org/hdcvsa/
- Vergés et al. (2025), HDC comparative review: https://doi.org/10.1007/s10462-025-11181-2
- “Information theory for complex systems scientists: What, why, and how” (2025): https://doi.org/10.1016/j.physrep.2025.09.007
- Quantum HDC / neuromorphic architecture proposal (2026): https://www.nature.com/articles/s44335-026-00064-6
- GHRR matrix-based symbolic reasoning, DATE 2026: https://doi.org/10.23919/DATE69613.2026.11539261
- Reservoir computing research agenda: https://doi.org/10.1038/s41467-024-45187-1
- Autopoietic self-maintenance experiments (2026): https://doi.org/10.1016/j.biosystems.2026.105928
- NIST IR 8547, PQC transition guidance (2026-10-27): https://csrc.nist.gov/pubs/ir/8547/final
- NIST IR 8610, additional PQC signatures (2026-05-14): https://csrc.nist.gov/pubs/ir/8610/final

**First proof:** one bounded CPU lattice, one fixed seed/config, deterministic replay, a resource-exhaustion case, one perturbation/recovery case, and a static fallback. Nothing in the first proof is allowed to influence whether the machine boots or whether its boot chain is trusted.
