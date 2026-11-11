# Ouroboros generic canonical scalar grid (OUROSGD1)

**Status:** isolated Rust research implementation, source-inspected but not compiled or executed. This crate introduces a generic, versioned interchange for scalar samples. It does not change the existing face-specific OUROGRD1 contract.

Tracking: [Issue #51](https://github.com/Luminous-Dynamics/sovereign-boot/issues/51)  
Classical extractor: [PR #47](https://github.com/Luminous-Dynamics/sovereign-boot/pull/47)  
SDF source fixture: [PR #35](https://github.com/Luminous-Dynamics/sovereign-boot/pull/35)

## Build and generate fixtures

    cargo test --manifest-path tools/ouroboros-scalar-grid/Cargo.toml
    cargo run --release --manifest-path tools/ouroboros-scalar-grid/Cargo.toml --example sample-exact-fixtures -- /tmp/ouroboros-exact-sdf

The example writes seven 64³ canonical grids: sphere, axis-aligned box, 45° Z-rotated box, capsule, ring torus, capped cylinder and plane. It prints one JSONL metadata record per artifact. These are generated artifacts, not results committed as binary files.

## OUROSGD1 contract

The canonical byte stream binds:
- schema magic/version;
- field semantics (ExactAnalyticSdf, ApproximateAnalyticField, MeshDerivedSdf, Unknown);
- the negative-inside / positive-outside sign convention;
- forward/lateral/up axis convention and first-axis-fastest storage;
- microunit coordinate and distance quantization;
- the isovalue, stored as field minus isovalue;
- canonical field/source descriptor bytes;
- dimensions, min/max bounds, sample count and ordered signed-int32 values.

The first-axis-fastest index is i + nx * (j + ny * k). Coordinates and parameters are authored as signed integer microunits; distance samples are signed int32 microunits. Quaternion/plane-normal components use signed Q30 values.

A grid is authoritative for extraction. Downstream algorithms must consume the exact same OUROSGD1 bytes; they must not call the source field, re-sample, re-evaluate or silently substitute a float recomputation. This crate has no hashing or signing dependency; digests and provenance attestations remain external.

## Exact analytic fields implemented

| Field | Definition | Constraints |
|---|---|---|
| Sphere | Euclidean distance to sphere surface | Positive radius |
| Box | Standard exact axis-aligned box SDF | Positive half-extents |
| Oriented box | Box SDF after inverse unit-quaternion transform | Canonical Q30 unit quaternion |
| Capsule | Distance to closest point on segment minus radius | Positive radius; coincident endpoints reduce to sphere |
| Ring torus | Circular ring-torus distance formula | Major radius greater than minor radius, both positive |
| Capped cylinder | Exact finite capped-cylinder SDF | Positive radius and half-length |
| Plane | Signed distance to an infinite plane | Canonical Q30 unit normal |

The formulas define exact signed distance in the stated mathematical domain before discretization. The stored grid is still quantized, so sampling error is bounded by the declared microunit quantization plus floating-point evaluation error. This is not a claim of exact rational arithmetic for the transcendental square-root evaluations.

## Semantics are not interchangeable

The generic wrapper can also preserve externally generated sample arrays with an explicit semantic class and descriptor bytes:
- ApproximateAnalyticField for approximate implicit fields and smooth/CSG compositions whose values are not a true Euclidean distance;
- MeshDerivedSdf for a pinned mesh, distance backend, sign policy and preprocessing chain;
- Unknown when the source cannot be qualified.

The existing OUROGRD1 head+nose field remains its own source-grounded application fixture and must be classified as approximate when converted into a generic interchange. A minimum or polynomial smooth-min of primitive distances may preserve a useful zero-level set but is not automatically the exact Euclidean distance to the union boundary. Do not label it exact merely because the sign is useful.

External descriptors are caller-supplied provenance data. The format preserves the declared semantics; it does not authenticate the claim or infer exactness.

## Limits

| Limit | Default maximum |
|---|---:|
| Samples | 2,097,152 |
| Field/source descriptor | 1 MiB |
| Canonical grid bytes | 16 MiB |
| Simultaneous sample + canonical byte payload | 32 MiB |

Limits are checked before allocating the sample buffer or canonical receipt. They are payload estimates, not an operating-system RSS cap. All length arithmetic is checked.

## Tests authored

The source contains 16 authored unit tests covering:
- sphere, box, oriented box, capsule, torus, capped cylinder and plane distance values;
- unit quaternion/normal validation;
- per-field descriptor byte lengths;
- negative-inside/positive-outside and nonzero isovalue binding;
- first-axis-fastest sample ordering;
- stable canonical bytes and field-parameter binding;
- preserving mesh-derived and approximate semantic tags;
- sample/descriptor budget rejection;
- int32 distance-quantization overflow;
- exact canonical header/payload length.

These tests are authored, not compiled or executed. The current environment has no Rust toolchain; no test pass or performance result is claimed.

## Benchmark protocol

Use separate fixture strata:
1. Exact analytic shapes with known implicit ground truth.
2. Mesh-derived samples with a pinned source mesh digest, distance backend/version and sign/repair pipeline.
3. Approximate analytic/application fields, including the existing Ouroboros head+nose estimator.

Test resolutions 32³, 64³, 128³ and 256³ only when the declared byte/working budget allows. Include axis-aligned and fixed-seed rotations, half-cell translations, thin features from 0.5 to 2 voxels, close parallel sheets, multiple components, exact-zero lattice samples and boundary cuts.

Compare Marching Cubes, PR #47 classical Hermite/QEF, maintained Gpytoolbox SIGGRAPH 2026 DCSDFD and any future Rust port on identical canonical bytes. Report symmetric Chamfer, Hausdorff p95/p99/max, edge-feature Chamfer, normal error, silhouette IoU, component count, topology, face flips, self-intersection status, latency percentiles, peak RSS and output bytes separately. No weighted composite leaderboard.

## Non-claims

This is a canonical interchange and exact-primitive sampling lab, not a mesher, adaptive octree, mesh-to-SDF implementation, exact-predicate geometry engine, perceptual validator or rendering backend. It does not establish any SOTA, accuracy, energy or performance improvement.
