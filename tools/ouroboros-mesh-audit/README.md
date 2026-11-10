# Ouroboros layered semantic mesh audit

**Status:** standalone Rust topology/semantic audit prototype. Self-intersection detection is intentionally `NotEvaluated`. Rust compilation/tests have not yet run in the available environment.

Tracking: [Issue #44](https://github.com/Luminous-Dynamics/sovereign-boot/issues/44)

## Build and test

This crate is isolated from the production workspace, standard-library-only and forbids unsafe code:

```sh
cargo test --manifest-path tools/ouroboros-mesh-audit/Cargo.toml
```

## Why a vector of claims, not one valid flag

A closed edge-manifold mesh can still have inconsistent orientation, pinched vertices, zero-area or nearly degenerate triangles, coincident-but-distinct vertices, or geometric self-intersections. CGAL's Polygon Mesh Processing documentation explicitly notes that its combinatorial 2-manifold mesh model does not itself preclude self-intersections or degenerate faces. Its separate self-intersection predicates and triangle-soup autorefinement APIs demonstrate why these checks must remain independent.

References:
- [CGAL 6.2 Polygon Mesh Processing](https://doc.cgal.org/latest/Polygon_mesh_processing/)
- [CGAL self-intersection predicates](https://doc.cgal.org/latest/Polygon_mesh_processing/group__PMP__intersection__grp.html)
- [CGAL autorefinement and snap rounding](https://www.cgal.org/2025/06/13/autorefine-and-snap/)

## Input contract

`SemanticTriangleMesh` contains:
- f64 XYZ vertices;
- one u32 feature bitmask per vertex (bits 0..4 map to stable source feature IDs 1..5; multiple bits preserve blended provenance);
- u32 triangle indices;
- one semantic face-region ID (1..5) per face.

Zero masks/region IDs and unsupported bits/regions are rejected. Indices and vector lengths are checked before allocating audit workspace. Current fixed limits are 100,000 vertices, 200,000 faces, 32 MiB input payload and 96 MiB estimated audit-workspace payload. Those byte values are conservative **payload estimates**, not process RSS or allocator overhead.

## Independently reported checks

| Check | Meaning |
|---|---|
| Buffer validity | Finite positions, valid indices, non-repeated face indices, shape and size constraints |
| Topology manifold | Every edge has incidence 1 or 2 and every used vertex link is one cycle (interior) or one path (boundary) |
| Closed surface | Topology-manifold plus zero boundary edges |
| Orientation | Two incident faces use every shared edge in opposite directions |
| Exact degeneracy | Zero squared triangle cross-product magnitude |
| Near degeneracy | Nonzero squared cross-product magnitude at or below the declared threshold |
| Semantic provenance | Nonzero, supported vertex feature masks and face-region IDs; no orphan vertices |
| Self intersections | **NotEvaluated** in this Rust-only first phase |

The report also includes boundary-component count, components that are not simple closed loops, semantic region-transition edge count, edge-incidence classes, coincident-position groups, orphan vertices, connected face components, and per-component Euler characteristic.

Euler characteristic is descriptive, not a universal correctness target. A sphere-like component has `χ=2`; handles and other topologies have different values.

## Method

- Undirected edges are keyed by sorted vertex-index pairs in a `BTreeMap`, giving deterministic iteration.
- Edge incidence is classified once: 1 = boundary, 2 = manifold candidate, more than 2 = non-manifold.
- Direction is retained for every edge use; two-face edges traversed in the same direction fail orientation consistency.
- Per-vertex link graphs are checked for connectedness and the correct cycle/path degree pattern. This detects a pinched vertex even when every edge individually has incidence two.
- Face components are connected by shared edges and summarized with V/E/F counts and `V-E+F`.
- Coincident positions normalize signed zero before comparing exact f64 bit patterns. Near-coincident positions are not claimed.
- Exact zero-area and thresholded near-degenerate faces are reported separately.
- No input is silently repaired.

The estimated working payload is checked before edge maps, vertex-link adjacency, DSU and component sets are allocated. It deliberately does not claim an exact process-memory ceiling.

## Canonical audit receipt

`OUROMAU1` little-endian bytes bind:
- exact vertex coordinates, feature masks, face indices and face regions;
- independent check statuses;
- edge, boundary, manifold, orientation, degeneracy, provenance and component metrics;
- resource estimates and all audit limits, including the exact near-degenerate threshold bits.

The receipt builder recomputes the audit against the supplied mesh and limits and rejects a mutated report. It returns bytes, not a hash or signature; digest/signature and build provenance remain external.

## Adversarial unit fixtures

The 16 authored tests cover:
- closed tetrahedron;
- open disk and boundary loop;
- two disconnected components;
- pinched vertex with disconnected link cycles;
- three faces on one edge;
- inconsistent shared-edge winding;
- exact-zero and near-degenerate triangles;
- coincident positions and orphan vertices;
- invalid feature masks/region IDs;
- bad indices, repeated indices and NaN;
- input and workspace budgets;
- semantic region-transition edges;
- receipt determinism, tolerance binding and mutation rejection.

These tests are authored, not executed. No passing Rust test/CI claim is made.

## Self-intersection: separate future qualification

The Rust crate does not implement triangle-triangle intersection tests. A future phase must use a pinned CGAL exact-predicate implementation as the reference oracle, compare `does_self_intersect` / `self_intersections`, and evaluate `autorefine_triangle_soup` on offline copies only. A bounded Rust BVH candidate can follow after the reference corpus is fixed. No float-based broadphase/narrowphase result should be called equivalent to CGAL exact predicates without measured false-positive/false-negative evidence.

## Non-claims

This crate is not an exact geometric embedding checker, self-intersection solver, mesh repair tool, SDF extractor, finished face mesh, human-perception test or GPU-performance benchmark. Where a check is not implemented, the result remains explicitly NotEvaluated.
