# Ouroboros persistent representation catalog

**Status:** version-1 contract and reference oracle. No Gaussian-splat, SDF or cross-device renderer is qualified by this document.

Tracking: [Issue #42](https://github.com/Luminous-Dynamics/sovereign-boot/issues/42)  
Implementation: standalone crate `tools/ouroboros-representation-catalog`  
Example: [`fixtures/ouroboros-representation-catalog-v1.json`](../fixtures/ouroboros-representation-catalog-v1.json)

## 1. The architectural shift

Ouroboros should have one persistent semantic identity and many replaceable representations.

```text
                 Entity identity
              entity_id / revision
                       |
             OUROLND1 / coordinate frame
                       |
              Representation catalog
                       |
      +----------------+-----------------+
      |                |                 |
   glTF mesh     Gaussian splat       OUROGRD1 SDF
  editable/LOD    visual capture      implicit field
      |                |                 |
      +----------------+-----------------+
                       |
              Host capability filter
                       |
             Quality/Pareto policy
                       |
              Renderer / evidence
```

The catalog is not the optimizer and not the renderer. It is the source-of-truth inventory of what representations exist, what they mean, which entity revision and coordinate frame they belong to, and what a fallback actually preserves.

### Identity hierarchy

| Identifier | Meaning | Changes when |
|---|---|---|
| `entity_id` | Persistent entity identity | Never during ordinary representation replacement |
| `entity_revision_root` | Semantic entity revision | Semantic state/definition changes |
| `representation_id` | Stable representation line | Never when updating the payload of that line |
| `representation_revision` | Version of one representation line | Payload or its interpretation changes |
| `payload_digest` | Exact representation payload bytes | Payload bytes change |
| `landmark_digest` | Authoritative `OUROLND1` semantic attachments | Anchor contract changes |
| `coordinate_frame_digest` | Local basis, units and coordinate convention | Coordinate contract changes |

A backend, renderer process, display output or GPU driver must not become part of entity identity. It belongs in a separate execution/session or measurement record.

## 2. Representation families and roles

Each entry has one **format** and one **role**. The role is a hard semantic restriction, not a marketing label.

| Format | Allowed role(s) | Intended use | What it must not silently become |
|---|---|---|---|
| glTF 2.0 mesh | VisualAppearance, EditableGeometry, CollisionProxy | Indexed triangles, morph targets, skins | A guaranteed runtime/state machine |
| `KHR_gaussian_splatting` | VisualAppearance | High-fidelity captured appearance | Editable topology, collision proxy or skeletal rig |
| glTF point cloud | VisualAppearance | Explicit degraded compatibility path | A quality-equivalent splat |
| `OUROGRD1` SDF | ImplicitField | Sampled field for reconstruction, geometric queries or controlled LOD research | A mesh without extraction/topology evidence |
| Landmark sidecar | SemanticRig | Exact entity anchors and attachment semantics | Renderer-private offsets |
| CPU raster | ReferenceCapture | Deterministic image/evidence reference | A 3D editable representation |

A representation also states:
- visual feature coverage (head silhouette, eyes, nose, mouth, expressions, body);
- anchor coverage and fidelity (Unavailable, Approximate, Exact);
- required host capabilities and format extensions;
- payload bytes and content digest;
- optional fallback and equivalence classification;
- optional measured-quality evidence, bound to a suite, host profile and raw result.

The catalog-level `landmark_digest` and `coordinate_frame_digest` bind every representation to one semantic anchor system. A visual representation may approximate the correspondence to those anchors. An exact landmark sidecar remains the authority when exact semantic attachments are required.

## 3. Khronos formats: use the standard, preserve the runtime gap

The Khronos glTF extension registry lists `KHR_gaussian_splatting` as ratified. It defines splats as point primitives and specifies position, rotation, scale, opacity and spherical-harmonics attributes. Its base unsupported-extension behavior is a sparse point cloud. This is a valid compatibility fallback, but not automatically equivalent visual fidelity.

Therefore:
- declare `KHR_gaussian_splatting` as a required extension for that representation;
- declare a splat backend capability separately from general glTF support;
- store the fallback explicitly;
- mark the point-cloud fallback `NonEquivalent` unless a matched evaluation demonstrates a stronger equivalence claim;
- never infer that a splat has mesh topology, collision semantics or a face rig.

References:
- [Khronos ratified extension registry](https://github.com/KhronosGroup/glTF/blob/main/extensions/README.md)
- [`KHR_gaussian_splatting` specification](https://github.com/KhronosGroup/glTF/blob/main/extensions/2.0/Khronos/KHR_gaussian_splatting/README.md)

Base glTF 2.0 can store node-transform and morph-target animation. It does not define runtime playback semantics such as starting, stopping, looping or mapping a timeline. Ouroboros must retain the persistent state machine and temporal semantics in its own runtime; glTF remains one asset interchange path, not the persistent entity runtime.

Reference: [glTF 2.0 animation section](https://registry.khronos.org/glTF/specs/2.0/glTF-2.0.html).

## 4. Fallbacks are typed claims

A fallback link records one of three classifications:

- **ExactEquivalent:** semantic feature and anchor masks/fidelity must match.
- **SemanticallyReduced:** fallback feature/anchor masks must be subsets of the source representation.
- **NonEquivalent:** known different result; cannot satisfy an exact semantic or quality requirement.

The validator rejects missing fallback targets, role mismatches, equivalence contradictions and cycles. Fallback chains remain inspectable; each hop keeps its own equivalence classification.

No host may silently choose an unsupported representation and pretend the fallback has the same visual quality.

## 5. Compatibility is separate from ranking

`compatible_entries` filters by:
- required host capability bits;
- required extension bits;
- role;
- required visual features;
- requested anchor coverage and exactness.

It returns all compatible entries in canonical catalog order. It does not select a winner.

The separate Pareto/quality work in [Issue #36](https://github.com/Luminous-Dynamics/sovereign-boot/issues/36) may rank the compatible set using hard resource constraints and explicit quality/energy/latency objectives. It must not rewrite the catalog or pretend unmeasured fields are measured.

The catalog itself guarantees:
- at least one visual appearance entry covers all catalog-required visual features;
- at least one exact semantic-rig record covers all catalog-required anchors;
- every entry binds the same entity-level landmark and coordinate-frame digests.

## 6. Measurement and evidence

A `MeasuredQuality` record is optional and carries:
- benchmark-suite digest;
- named-host profile digest;
- raw result digest;
- silhouette error;
- landmark error;
- p95 build time;
- peak memory;
- topology flags.

Missing measurement means **unknown**. It must not be filled with zero or an estimate. Planning estimates should live in a separately typed policy/candidate object. Renderer-reported data, compositor feedback and external measurements retain separate evidence levels.

The catalog's canonical `OUROREP1` bytes are content-addressable; the digest alone does not authenticate who produced a payload. Signed build provenance and capture attestations remain separate.

## 7. Canonical schema

The standard-library-only Rust oracle is at `tools/ouroboros-representation-catalog`. It:
- forbids unsafe code;
- uses a standalone Cargo workspace and lockfile;
- serializes versioned little-endian `OUROREP1` bytes;
- sorts and validates representation IDs;
- rejects unknown bits, zero digests, unsupported formats/roles and invalid fallbacks;
- filters compatible entries without ranking;
- includes a synthetic mesh/splat/point-cloud/SDF/rig/raster fixture.

All example digests in the JSON fixture are placeholders, not real payload identifiers. The fixture is a schema/conformance example, not a signed asset package.

## 8. Conformance and benchmark plan

The initial 19 authored tests cover cross-format identity, representation payload changes, canonical order, semantic bindings, Gaussian-splat role/capability enforcement, exact anchor requirements, degraded fallback, quality provenance and deterministic encoding.

Next qualification steps:
1. Compile, rustfmt and run the isolated crate on exact head.
2. Parse and validate the JSON fixture against the Rust canonical model.
3. Generate one real glTF mesh, one real `KHR_gaussian_splatting` asset, one `OUROGRD1` field, one `OUROLND1` sidecar and one CPU reference raster from the same entity revision.
4. Record which hosts support each representation and extension. Do not infer Vulkan/WGPU/glTF viewer support from a product name.
5. Evaluate cross-representation landmark drift, silhouette overlap, identity recognition, feature visibility and p50/p95 load/convert cost.
6. Compare resource/energy/perceptual Pareto frontiers only after matched host measurements are available.

## 9. Non-claims

This catalog does not implement a Gaussian-splat renderer, an SDF mesh extractor or a new Khronos extension. It does not prove that all representations preserve the same perception, geometry or behavior. It establishes the contracts required to compare and negotiate them honestly.
