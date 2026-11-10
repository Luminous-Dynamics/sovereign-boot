# Ouroboros bounded semantic SDF lab

**Status:** standalone CPU sampling oracle, source-grounded prototype. Mesh extraction is deliberately not implemented here. Exact-head Rust build/tests have not yet run.

Tracking: [Issue #34](https://github.com/Luminous-Dynamics/sovereign-boot/issues/34)  
Source geometry: PR #15 head `d5a7f877d71757db49e63ad1b1582a661f6e4ef2`  
Machine-readable fixture: [`benchmarks/ouroboros-face-sdf/fixtures-v1.json`](../../benchmarks/ouroboros-face-sdf/fixtures-v1.json)

## Purpose

Before comparing mesh extractors, establish one deterministic, bounded sampled field. All algorithms must consume the same sampled data so we do not accidentally compare a change in geometry composition against a change in reconstruction method.

The first field is intentionally narrow: a candidate unified skin made from the source-derived head shell and nose. Eyes remain separate semantic anatomy, and the mouth remains a patch/cavity candidate. The lab still exposes all five source-derived primitive records to keep the fixture traceable.

## Build and test

The crate is isolated from the production Cargo workspace and uses only the Rust standard library:

```sh
cargo test --manifest-path tools/ouroboros-sdf-lab/Cargo.toml
```

Create a canonical 128³ smooth-union sample:

```sh
cargo run --release --manifest-path tools/ouroboros-sdf-lab/Cargo.toml \
  --example sample-face-skin -- /tmp/ouroboros-face-skin-128.ourogrid
sha256sum /tmp/ouroboros-face-skin-128.ourogrid
```

The example emits a small JSON metadata line and writes the canonical binary grid. SHA-256 is an external transport/checking digest; it is not a signature or an attestation. The crate intentionally has no hashing dependency.

## Source-derived feature contract

Coordinates are normalized by the Ouroboros major radius and expressed in the `OUROLND1` face basis: **forward, lateral, up**.

| Region ID | Feature | Center (F,L,U) | Radii (F,L,U) | Initial surface role |
|---:|---|---|---|---|
| 1 | Head shell | (0.030, 0, 0) | (0.120, 0.060, 0.110) | Skin-union candidate |
| 2 | Left eye | (0.060, -0.050, 0.020) | (0.016, 0.018, 0.014) | Independent attached anatomy |
| 3 | Right eye | (0.060, +0.050, 0.020) | (0.016, 0.018, 0.014) | Independent attached anatomy |
| 4 | Nose | (0.160, 0, 0.005) | (0.020, 0.014, 0.018) | Skin-union candidate |
| 5 | Mouth | (0.140, 0, -0.035) | (0.008, 0.035, 0.006) | Surface patch/cavity candidate |

The parameters match the pinned `ouroboros.rs` landmark positions and `ouroboros_face.rs` ellipsoid generator. The nose center is the nose-tip landmark offset backward by 0.020 major-radius units.

## Field definition

The per-ellipsoid field uses the IQ-style ellipsoid distance approximation:

```text
q  = (p - center) / radii
k0 = length(q)
k1 = length(q / radii)
d  = k0 * (k0 - 1) / k1
```

At the exact ellipsoid center, the implementation uses the negative minimum radius to avoid the undefined 0/0 expression. This is an **approximate signed-distance estimator**, not exact Euclidean distance to an ellipsoid. Keep this distinction in all error reporting.

The two skin candidates are combined with either:
- sharp union: `min(head, nose)`;
- polynomial smooth union: `min(a,b) - 0.25*h*h*k`, where `h=max(k-abs(a-b),0)/k`.

Smooth width is fixed-point canonicalized, must be greater than zero, and is bounded to 0.05 normalized units. Initial comparison widths are 0, 0.002, 0.004, 0.006 and 0.010.

The sign convention is negative inside, zero on the approximate surface, and positive outside.

## Grid and canonical encoding

- Dimensions are grid **vertices**, minimum two per axis.
- Current axes are forward, lateral, up.
- Flattening order is first-axis-fastest:
  `index = i + nx * (j + ny * k)`.
- Bounds are rounded to signed microunits before sampling; the sampled positions and canonical metadata use the same rounded bounds.
- Distances are rounded to signed 32-bit microunits (`1,000,000 units per normalized major-radius unit`).
- Feature centers/radii and smooth-union width must be exactly representable at the same microunit precision. This prevents different floating-point parameters from collapsing to the same canonical descriptor.
- `OUROGRD1` canonical bytes bind the grid schema, fixture ID, field descriptor, dimensions, bounds, quantization and every ordered signed-int32 sample. It is the authoritative content-addressable stream; the caller computes the digest.
- `OUROF32D` is a diagnostic interchange encoding for external extractors. It carries the same metadata and exact same header length, but writes each canonical microunit sample as a dequantized little-endian `f32`. It is not a second/higher-precision SDF evaluation; pair it with the `OUROGRD1` receipt.
- `GridSpec` checks dimension products, sample ceilings, integer conversion, canonical byte ceilings and bounds before allocating the sample vector.

Current hard ceilings: 2,097,152 samples (128³), 16 MiB per canonical grid, and 32 MiB for the simultaneous sample-vector plus encoded-output payload. The working-payload budget excludes allocator metadata and process/runtime overhead; it is not an RSS guarantee. Larger grids require a deliberate versioned budget change rather than an implicit allocation.

## SIGGRAPH 2026 discrete-SDF dual contouring reference

The maintained Gpytoolbox API documents:

```text
dual_contouring_of_signed_distance_data(
    S, GV, nx, ny, nz,
    isovalue=0.0,
    outer_iters=100,
    inner_iters=100,
    batch_size=200000,
    hermite_update=True,
    mu=0.1,
    dc_weight=0.02,
    svd_threshold=0.01,
    new_hermite_pos_weight=0.2,
    new_face_pos_weight=0.2,
    new_hermite_normal_weight=0.2,
    verbose=False
)
```

It accepts SDF values `S` and grid vertex positions `GV`; it returns vertex positions `V` and quad faces `F`. The documented linear row order is exactly `i + nx*(j + ny*k)`, with the first grid axis varying fastest. The API's iterative and QEF-related parameters need to be recorded in the benchmark manifest; do not assume defaults are optimal.

- [Maintained API documentation](https://gpytoolbox.org/latest/dual_contouring_of_signed_distance_data/)
- [SIGGRAPH 2026 paper/project](https://gatc.cs.columbia.edu/projects/dual-contouring-of-signed-distance-data.html)
- [Author repository and implementation warning](https://github.com/xianacarrera/dual-contouring-of-signed-distance-data)

The authors explicitly say the original research repository is for reproducing their paper and direct users to the maintained Gpytoolbox function. The original research code was tested on macOS only. Treat Gpytoolbox as an offline reference/oracle after version, license and platform review; do not add Python to Ouroboros production.

## Initial conformance corpus

The current unit tests cover:
- exact source feature IDs/centers/radii;
- negative-inside/positive-outside sign and axis-boundary zero;
- sharp-union and bounded smooth-union behavior;
- rejection of noncanonical microunit parameters;
- first-axis-fastest index and sample ordering;
- zero dimensions, dimension-product overflow, sample-count, encoded-byte and simultaneous working-payload budget failures;
- non-finite/overflowing distance quantization;
- canonical descriptor size and repeatable grid bytes;
- `OUROF32D` metadata/length parity and f32 reconstruction from the canonical samples;
- fixed-point parity against ten source-grounded Python float64 probe values in `fixtures/reference-probes-v1.json`;
- metadata changes when the field mode or bounds change.

The tests are authored but have not been compiled or executed. The numerical probe fixture is a cross-language reference corpus, not evidence of a successful Rust run. No green CI is implied.

## Next algorithms

Only after the sampler's exact-head qualification:
1. marching cubes baseline;
2. classical dual contouring with an explicit finite-difference or analytic gradient estimator;
3. SIGGRAPH 2026 discrete-SDF dual contouring using the maintained reference API;
4. a bounded Rust implementation if the reference results show meaningful value.

All methods must consume the same canonical `OUROGRD1` samples. Report field approximation error separately from reconstruction error, topology, self-intersections, semantic feature recovery, vertex/index/byte count, peak RSS and named-host p50/p95 timings.

## Non-claims

This crate is not a mesh extractor, production face mesh, exact SDF implementation, self-intersection checker, perceptual-quality evaluation, GPU renderer or implementation of the SIGGRAPH method. It does not add Python or a third-party dependency to production.
