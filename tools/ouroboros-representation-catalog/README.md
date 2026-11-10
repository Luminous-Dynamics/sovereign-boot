# Ouroboros representation catalog

**Status:** standalone, dependency-free Rust catalog oracle. This is an inventory/compatibility contract, not a renderer and not a quality-ranking policy. Rust compilation/tests have not yet been executed.

Tracking: [Issue #42](https://github.com/Luminous-Dynamics/sovereign-boot/issues/42)  
Stack: draft PR based on [PR #30](https://github.com/Luminous-Dynamics/sovereign-boot/pull/30)

## Build and test

```sh
cargo test --manifest-path tools/ouroboros-representation-catalog/Cargo.toml
```

The crate uses only the Rust standard library, has an isolated Cargo workspace/lockfile and forbids unsafe code.

## Paradigm: one identity, many typed representations

An Ouroboros entity is not one mesh. It has:
- a stable `entity_id`;
- an entity semantic `entity_revision_root`;
- one authoritative `OUROLND1` landmark digest;
- one coordinate-frame digest;
- a canonical catalog of independently versioned representation lines.

A `representation_id` identifies a persistent representation line. Its `representation_revision` and `payload_digest` identify a particular payload version. Replacing a mesh with a better mesh or creating a Gaussian-splat capture does not create a new entity. Renderer/session IDs do not participate in entity identity.

Every record binds the same entity-level landmark and coordinate-frame digests. This does not imply that every representation carries exact vertex-to-landmark correspondence: each record states its own anchor mask and fidelity (Unavailable, Approximate or Exact).

## Supported typed representation families

| Format | Allowed role(s) | Semantics |
|---|---|---|
| glTF 2.0 mesh | VisualAppearance, EditableGeometry, CollisionProxy | Indexed geometry; may carry morph targets/skins where authored |
| `KHR_gaussian_splatting` | VisualAppearance only | Point primitives carrying Gaussian position/rotation/scale/opacity/SH data |
| glTF point cloud | VisualAppearance only | Degraded compatibility path; not assumed visually equivalent to Gaussian splats |
| `OUROGRD1` | ImplicitField only | Canonical sampled scalar field for downstream reconstruction/evaluation |
| Ouroboros landmark sidecar | SemanticRig only | Exact authoritative anchor/rig associations |
| CPU raster | ReferenceCapture only | Deterministic reference image; not editable geometry |

The typed role restriction is deliberate. A Gaussian splat is not an editable mesh, collision proxy or skeletal rig merely because it depicts the same subject. It may be used as a high-quality visual appearance representation while the independent `OUROLND1` rig remains the semantic anchor authority.

## Khronos interoperability boundary

The glTF extension registry marks `KHR_gaussian_splatting` as ratified. The extension stores splats as point primitives and specifies position, rotation, scale, opacity and spherical-harmonics attributes. When an implementation does not support the extension, the base glTF fallback is a sparse point cloud. That fallback is a different quality/semantic result, so the catalog requires an explicit fallback link and an equivalence classification.

- [Ratified extension registry](https://github.com/KhronosGroup/glTF/blob/main/extensions/README.md)
- [`KHR_gaussian_splatting` specification](https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Khronos/KHR_gaussian_splatting/README.md)

Base glTF 2.0 supports storing node-transform and morph-target animation data, but does not define runtime semantics such as animation start, stop, looping or timeline mapping. Ouroboros continues to own persistent state, retargeting, playback and checkpoint semantics; glTF is an asset interchange format, not the entity runtime.

- [glTF 2.0 animation specification](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html)

## Compatibility versus ranking

`compatible_entries` returns every record satisfying hard host capabilities, required extensions, semantic feature coverage and requested anchor fidelity. It preserves canonical catalog order. It deliberately does **not** rank candidates.

The separate [Pareto quality negotiation issue #36](https://github.com/Luminous-Dynamics/sovereign-boot/issues/36) decides among compatible candidates using resource limits and measured/declared quality. Keeping the catalog and optimizer separate prevents device-specific preferences from mutating the source-of-truth inventory.

The catalog's required visual feature mask must be represented by at least one visual-appearance record. Its required anchor mask must be represented by an exact semantic-rig record. Visual splats may declare approximate anchors without replacing that rig sidecar.

## Explicit fallback semantics

Every fallback link declares one of:
- `ExactEquivalent`: semantic features and anchor coverage/fidelity must match.
- `SemanticallyReduced`: the fallback feature/anchor masks must be subsets of the source.
- `NonEquivalent`: the fallback is known to be a different result. The caller must not count it as satisfying an exact requirement.

Fallback targets must exist and have the same role. Cycles are rejected during catalog validation. A missing extension never authorizes silent reinterpretation.

The synthetic fixture `fixtures/ouroboros-representation-catalog-v1.json` includes a Gaussian-splat visual record whose explicit fallback is a point cloud marked `NonEquivalent`.

## Measurement and trust model

Measured quality is optional and separately provenance-bound by:
- benchmark suite digest;
- named-host profile digest;
- raw result digest;
- silhouette and landmark errors;
- p95 build time and peak memory;
- topology flags.

Absent evidence remains unknown. It is not filled with zero or replaced with an estimate. The catalog is content-addressable through canonical `OUROREP1` bytes; a content digest is not an authentication claim. Build, asset-generation and capture signatures remain a separate attestation layer.

## Canonical encoding and validation

- Catalog records are sorted by bytewise `representation_id`; duplicates and unsorted records are invalid.
- Each record is bound to the catalog's entity revision context, landmark digest and coordinate frame.
- Unknown schema/capability/semantic bits, zero digests, invalid format/role combinations, missing mandatory format extensions and invalid fallback graphs fail closed.
- Canonical little-endian bytes include every semantic, compatibility, fallback and measurement field.
- Compatibility filtering never mutates catalog order or entity identity.

## Current authored conformance tests

The crate contains 20 unit tests covering:
- one identity across mesh, splat, SDF, rig and raster representations;
- payload/revision change without entity identity change;
- canonical ordering and duplicate rejection;
- landmark/frame binding;
- Gaussian splat role and extension/capability requirements;
- format/role constraints and specialized base-runtime requirements;
- hard capability and semantic filtering;
- exact-anchor policy and separate rig requirements;
- explicit non-equivalent point-cloud fallback;
- missing/cyclic/invalid fallbacks;
- measured evidence provenance;
- deterministic canonical encoding and exact record-size accounting;
- unknown schema, zero roots and unknown capability rejection.

Tests are authored but have not been compiled or run. No green CI is implied.
