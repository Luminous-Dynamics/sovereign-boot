# Ouroboros CGAL self-intersection oracle

**Status:** standalone C++ research adapter, source-inspected but not compiled or executed. It is not wired into production or into the mesh-audit status yet.

Tracking: [Issue #68](https://github.com/Luminous-Dynamics/sovereign-boot/issues/68)  
Input exporter: \`DualContourMesh::triangle_soup_canonical_bytes()\` in the classical dual-contouring crate.

## Build

CGAL 6.2.1 is pinned exactly in CMake and in `dependencies-v1.json`, including the upstream source-archive SHA-256. It was the latest stable CGAL release found for the 12 November 2026 research pass. The oracle requires a CGAL installation with GMP/MPFR support and uses the exact-predicate/exact-construction kernel (EPECK).

\`\`\`sh
cmake -S tools/ouroboros-cgal-oracle -B build/ouroboros-cgal-oracle \\
  -DCMAKE_PREFIX_PATH=/path/to/CGAL-6.2.1
cmake --build build/ouroboros-cgal-oracle --parallel
ctest --test-dir build/ouroboros-cgal-oracle --output-on-failure
\`\`\`

## Input contract: OUROTSQ1

The input is a compact little-endian binary stream:

| Field | Encoding |
|---|---|
| Magic | 8 bytes: \`OUROTSQ1\` |
| Schema | u16, currently 1 |
| Vertex count | u32, maximum 50,000 |
| Triangle count | u32, maximum 125,000 |
| Vertex coordinates | 3 × i64 per vertex, signed integer nanounits |
| Triangle indices | 3 × u32 per triangle |

The exact length must be \`18 + 24*vertex_count + 12*triangle_count\`. Maximum input is 32 MiB. All indices, counts, version and length are checked before geometry allocation.

Coordinates are converted as the exact rational \`integer / 1,000,000,000\` using GMP integer/rational types under EPECK. The implementation does not convert through \`double\`, OBJ or a decimal rendering of a float.

## Detection semantics

- Degenerate triangles are detected with the exact kernel and reported in \`degenerate_face_ids\`; they are filtered out of the pairwise intersection query so they are not confused with true face-pair intersections.
- CGAL's triangle-soup predicate ignores a common edge or vertex only when it is represented by the same point IDs in both triangles. Coincident positions carried by distinct point IDs can be reported as intersections.
- The sequential \`triangle_soup_self_intersections\` API returns sorted original face-index pairs after deterministic remapping.
- Pair output is capped at \`max_pairs + 1\` (default cap 10,000, hard maximum 100,000). When the extra item is present, the list is truncated and is never described as complete.
- Any reported face pair means self-intersection **Fail**, even when the pair list is truncated. A complete empty pair list means self-intersection **Pass**. The degenerate-face result is a separate status.
- The program does not run autorefine, snap rounding, repair, stitching, face removal or vertex merging.

Example invocation:

\`\`\`sh
build/ouroboros-cgal-oracle/ouroboros-cgal-oracle mesh.ourotsq
build/ouroboros-cgal-oracle/ouroboros-cgal-oracle mesh.ourotsq 100
\`\`\`

Exit codes: 0 = no self-intersections and no degenerate faces; 1 = geometric defect found; 2 = malformed input/runtime error; 64 = usage error. The JSON output reports CGAL version, kernel, input format, degeneracies, intersecting face pairs, truncation and separate statuses.

## Self-test corpus

The source-level \`--self-test\` exercises:
1. separated triangles;
2. proper interior crossing;
3. triangles sharing an indexed edge;
4. coincident geometry with distinct point IDs;
5. a degenerate triangle classified separately;
6. capped pair output returning Indeterminate-by-truncation semantics (never Pass);
7. OUROTSQ1 exact integer parser round-trip;
8. malformed length, unsupported schema and out-of-range index rejection.

The tests are authored, not compiled or executed.

## Evidence boundary

This oracle is a standalone detector. It does not prove closure, manifold topology, orientation, semantic provenance, normal quality, watertightness under every convention or perceptual correctness. It does not repair the mesh. The Rust/evidence layer must bind the exact OUROTSQ1 digest, oracle binary/build digest, CGAL version, kernel, output JSON and status into the evidence bundle. Until that exact-input evidence path is wired and qualified, the Rust mesh audit must continue to say self-intersection \`NotEvaluated\`.


## Reproducibility pins

- Upstream release source archive: `https://github.com/CGAL/cgal/releases/download/v6.2.1/CGAL-6.2.1.tar.xz`
- SHA-256: `b6be77c60765a8456335de991eeaf6ffec55256984e4a9ecc6a97c37bbfe85bf`
- Full machine-readable pin: `dependencies-v1.json`

The CMake package constraint checks the installed version; the dependency manager/build environment must independently verify the archive hash and pin GMP/MPFR, Boost and the compiler toolchain. Do not infer that CMake's version check alone proves source provenance.
