# HHLF-inspired Living Lattice Lab

This is an isolated, standard-library-only Rust research scaffold. It is deliberately **not** part of the Sovereign Boot workspace or trusted boot path.

It is not a complete HHLF implementation: it has no hyperdimensional binding algebra, holographic reconstruction, validated fractal-dimension estimator, cryptography, artificial-life proof or renderer. It provides only a bounded multiscale graph with deterministic local state evolution, resource accounting, one controlled perturbation and replay comparison.

## Run

```bash
cargo run --release --manifest-path research/hhlf-lattice/Cargo.toml
cargo test --manifest-path research/hhlf-lattice/Cargo.toml
```

## Current model

- A small hierarchy of ring lattices, with cross-scale parent links.
- Scalar node activity evolved by a bounded Euler update rule.
- A finite per-node abstract resource budget.
- Validated configuration bounds and a maximum step budget.
- Atomic stepping: if any node would exhaust its resource, the step is rejected before any node is mutated.
- Deterministic initialization from a fixed integer seed.
- A same-build replay digest using FNV-1a, explicitly **not cryptographic**.

The resource unit is an abstract simulation unit, not joules. The lattice is not a measured geometric fractal. A node recovering activity after a perturbation does not demonstrate autopoiesis or life.

## First proof and limitations

The built-in tests check deterministic replay, recovery of a perturbed scalar state, budget rejection without partial mutation, invalid-configuration rejection and resource-exhaustion handling.

These are software-contract tests only. They do not establish physical validity, self-maintenance, emergent intelligence, consciousness, entropy quality or cryptographic security.

## Next experiments

1. Add a genuine HDC/VSA representation and test binding/bundling/retrieval against the flat-HDC baseline.
2. Add a reconstruction-under-erasure experiment before using the word holographic.
3. Measure scale-dependent correlations over a declared scale range before using the word fractal.
4. Add explicit uncertainty and cross-scale intervention tests.
5. Compare the full system against simpler graph, reservoir and continuous-time baselines.
6. Keep the CPU model as reference before adding any renderer.

The visual theme may consume only bounded, non-sensitive, allowlisted lifecycle observations. No boot, recovery, integrity, safety or authority decision may depend on this research lab.
