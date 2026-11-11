# Ouroboros exact-predicate CGAL intersection oracle

**Status:** source implementation opened; parser tests authored; CGAL geometry tests authored. No parser build, CGAL build or test pass is claimed yet.

Tracking: [Issue #53](https://github.com/Luminous-Dynamics/sovereign-boot/issues/53)  
Input artifact: [Issue #46](https://github.com/Luminous-Dynamics/sovereign-boot/issues/46) / [draft PR #47](https://github.com/Luminous-Dynamics/sovereign-boot/pull/47)  
Mesh-audit input status: self-intersections remain `NotEvaluated` until this oracle's receipt is independently verified.

## Why this is a separate oracle

The Rust mesh audit evaluates combinatorial topology, orientation, closure, degeneracy and semantic provenance. Those checks do not establish that two geometrically non-adjacent triangles cannot intersect in 3D.

CGAL 6.2.1 is the latest stable release as of 11 November 2026. Its Polygon Mesh Processing package provides:
- `does_self_intersect(mesh)`: an early-out boolean;
- `self_intersections(faces(mesh), mesh, output_iterator)`: a full list of non-adjacent intersecting face pairs.

This implementation runs both. The full pair list is mapped back to the exact input triangle indices. Enumeration is bounded at 1,000,000 pairs with a one-pair sentinel using CGAL's `maximum_number` named parameter; exceeding the bound returns `NotEvaluated: IntersectionPairBudgetExceeded`. An internal disagreement between the boolean and pair-list paths is `NotEvaluated`, never coerced to Pass.

Primary documentation:
- https://www.cgal.org/releases.html
- https://doc.cgal.org/6.2.1/Polygon_mesh_processing/
- https://doc.cgal.org/6.2.1/Surface_mesh/

## Coordinate and input contract

The CLI consumes the canonical `OURODCQ1` binary artifact, not OBJ/PLY text.

The parser validates:
- exact magic/schema and bounded artifact length;
- embedded `OUROGRD1` source-grid schema, `OUROFLD1` descriptor length, axis/layout identifiers, bounds, microunit quantization, sample count and source dimensions;
- source dimensions equal the mesh's recorded grid dimensions;
- serialized per-run vertex, quad, geometry and working-payload ceilings;
- every fixed-point vertex record, normal range and nonzero feature mask;
- quad/triangle index bounds, non-repeated indices, fixed diagonal (0,2) correspondence and geometry byte accounting;
- all metrics, exact embedded audit-section size and absence of trailing bytes.

The vertex positions are stored as signed int64 nanounits. The CGAL executable constructs each coordinate as an exact rational `integer / 1,000,000,000` in `Exact_predicates_exact_constructions_kernel`. It does not pass through decimal text or an intermediate float32 representation. SHA-256 is calculated over the complete original artifact bytes and emitted in the result.

A digest identifies bytes; it does not authenticate who produced them. Signatures and trusted build provenance remain separate.

## Surface construction and failure states

The oracle builds a `CGAL::Surface_mesh` without welding, repairing or reordering vertices/faces. If any triangle is exactly collinear, the result is `NotEvaluated: DegenerateTriangle`. If `add_face()` rejects the triangle due to the topology/orientation constraints of the surface-mesh structure, the result is `NotEvaluated: SurfaceMeshConstructionRejected` and reports the first input triangle index.

Only after successful construction does it run the geometric tests. It emits:
- `Pass` if no self-intersections are found;
- `Fail` if the boolean predicate and pair enumerator agree that one or more pairs intersect;
- `NotEvaluated` if input construction or oracle consistency fails.

CGAL's documented self-intersection pair semantics report non-adjacent face pairs; an intersection that is only the shared edge or vertex incident to both faces is not treated as a self-intersection. The exact CGAL version and API are bound in the result. Tests cover coplanar/touching/duplicate cases as they are added.

## Build

The artifact parser uses C++20 and OpenSSL SHA-256. It can be built without CGAL:

```sh
cmake -S tools/ouroboros-cgal-intersection-oracle \
  -B /tmp/ouroboros-cgal-parser-build \
  -DBUILD_TESTING=ON \
  -DOUROBOROS_BUILD_CGAL_ORACLE=OFF
cmake --build /tmp/ouroboros-cgal-parser-build --parallel
ctest --test-dir /tmp/ouroboros-cgal-parser-build --output-on-failure
```

The exact geometry executable is opt-in and requires CGAL 6.2.1 exactly:

```sh
cmake -S tools/ouroboros-cgal-intersection-oracle \
  -B /tmp/ouroboros-cgal-build \
  -DBUILD_TESTING=ON \
  -DOUROBOROS_BUILD_CGAL_ORACLE=ON \
  -DCMAKE_BUILD_TYPE=Release
cmake --build /tmp/ouroboros-cgal-build --parallel
ctest --test-dir /tmp/ouroboros-cgal-build --output-on-failure
/tmp/ouroboros-cgal-build/ouroboros-cgal-intersection-oracle \
  /path/to/mesh.ourodc
```

CGAL 6.2.1's Linux compiler baseline includes GCC 13.3 or later; GCC 14.2 in the present development container is new enough, but CGAL headers and libraries are not installed in that container.

The parser-only test target is intentionally independent of CGAL. Its source is authored but has not yet been compiled/executed here.

## Result format

The CLI emits one JSON object with schema `OUROCGI1`:
- exact input SHA-256 / byte length;
- CGAL version, exact-kernel identifier, compiler and build mode;
- parser, surface-construction and self-intersection status;
- source geometry counts;
- reason / offending triangle when not evaluated;
- sorted, original zero-based triangle index pairs;
- local elapsed nanoseconds, labelled as an observation rather than portable proof.

The process exit code is 0 for Pass, 3 for Fail, 4 for NotEvaluated, 2 for parser/I/O exception and 64 for CLI usage.

## Authored fixtures

Parser fixture tests cover a valid artifact, SHA stability, invalid magic, truncation, invalid triangle index, incorrect quad/triangle correspondence, trailing bytes and inconsistent metrics.

CGAL fixture tests cover two disjoint closed tetrahedra, overlapping closed components, deterministic pair ordering, a degenerate triangle, inconsistent winding, coplanar overlap, coincident faces with distinct vertex IDs and one-nanounit parallel separation. These CGAL tests are authored but not run without CGAL 6.2.1 installed.

## Next qualification steps

1. Build and run parser-only CTest with the available C++20 compiler/OpenSSL.
2. Install/pin CGAL 6.2.1 in a Nix dev shell or CI image; build the geometry target.
3. Add coplanar overlap, duplicate face, coincident-coordinate/different-index, exact point/segment touch, near-nanounit perturbation and 50k/100k face stress fixtures.
4. Run the oracle over exact `OURODCQ1` outputs from classical and SIGGRAPH-method extractors.
5. Bind the resulting receipt to the exact geometry digest in a new evidence schema revision; do not change `OUROEVB4` in place.

## Non-claims

This is an offline geometry qualification tool, not a mesh repair tool, not a renderer dependency and not a proof of anatomy or human identity. A CGAL Pass applies only to the exact fixed-point triangle artifact whose SHA-256 is reported. Any geometry edit, vertex reorder, retriangulation or coordinate conversion invalidates that result.
