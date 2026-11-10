#![forbid(unsafe_code)]
//! Canonical multi-representation catalog for one persistent Ouroboros entity.
//!
//! The catalog binds heterogeneous visual/geometry payloads to one entity
//! revision, semantic landmark contract and coordinate frame. It inventories
//! representations and filters hard compatibility requirements; it does not
//! rank or choose among candidates (that belongs to the separate quality
//! negotiation/Pareto policy).
//!
//! Content digests are identifiers, not authentication. Build provenance,
//! measured-quality attestations and trusted capture remain separate layers.

use std::{error::Error, fmt};

pub const CATALOG_SCHEMA_VERSION: u16 = 1;
pub const MAX_REPRESENTATIONS: usize = 128;
pub const MAX_CATALOG_BYTES: usize = 4 * 1024 * 1024;
const CATALOG_MAGIC: [u8; 8] = *b"OUROREP1";

pub type Digest = [u8; 32];
pub type RepresentationId = [u8; 16];

pub const CAP_CPU: u64 = 1 << 0;
pub const CAP_VULKAN: u64 = 1 << 1;
pub const CAP_WGPU: u64 = 1 << 2;
pub const CAP_GLTF_2: u64 = 1 << 3;
pub const CAP_GAUSSIAN_SPLATTING: u64 = 1 << 4;
pub const CAP_SDF_GRID: u64 = 1 << 5;
pub const CAP_RASTER_IMAGE: u64 = 1 << 6;
pub const CAP_MORPH_TARGETS: u64 = 1 << 7;
pub const CAP_KNOWN_MASK: u64 = (1 << 8) - 1;

pub const EXT_KHR_GAUSSIAN_SPLATTING: u64 = 1 << 0;
pub const EXT_KHR_DRACO_MESH_COMPRESSION: u64 = 1 << 1;
pub const EXT_KHR_MESH_QUANTIZATION: u64 = 1 << 2;
pub const EXT_OURO_SDF_GRID: u64 = 1 << 32;
pub const EXT_OURO_LANDMARK_SIDECAR: u64 = 1 << 33;
pub const EXT_KNOWN_MASK: u64 =
    EXT_KHR_GAUSSIAN_SPLATTING
        | EXT_KHR_DRACO_MESH_COMPRESSION
        | EXT_KHR_MESH_QUANTIZATION
        | EXT_OURO_SDF_GRID
        | EXT_OURO_LANDMARK_SIDECAR;

pub const FEATURE_HEAD_SILHOUETTE: u64 = 1 << 0;
pub const FEATURE_EYE_PAIR: u64 = 1 << 1;
pub const FEATURE_NOSE: u64 = 1 << 2;
pub const FEATURE_MOUTH: u64 = 1 << 3;
pub const FEATURE_EXPRESSIONS: u64 = 1 << 4;
pub const FEATURE_BODY_SILHOUETTE: u64 = 1 << 5;
pub const FEATURE_KNOWN_MASK: u64 = (1 << 6) - 1;

pub const ANCHOR_HEAD_ROOT: u64 = 1 << 0;
pub const ANCHOR_LEFT_EYE: u64 = 1 << 1;
pub const ANCHOR_RIGHT_EYE: u64 = 1 << 2;
pub const ANCHOR_NOSE_TIP: u64 = 1 << 3;
pub const ANCHOR_MOUTH_CENTER: u64 = 1 << 4;
pub const ANCHOR_CROWN: u64 = 1 << 5;
pub const ANCHOR_CHIN: u64 = 1 << 6;
pub const ANCHOR_KNOWN_MASK: u64 = (1 << 7) - 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RepresentationFormat {
    GltfMesh = 1,
    GltfGaussianSplat = 2,
    GltfPointCloud = 3,
    OuroSdfGrid = 4,
    LandmarkSidecar = 5,
    CpuRaster = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum RepresentationRole {
    VisualAppearance = 1,
    EditableGeometry = 2,
    CollisionProxy = 3,
    SemanticRig = 4,
    ImplicitField = 5,
    ReferenceCapture = 6,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum AnchorFidelity {
    Unavailable = 0,
    Approximate = 1,
    Exact = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FallbackEquivalence {
    ExactEquivalent = 1,
    SemanticallyReduced = 2,
    NonEquivalent = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FallbackLink {
    pub representation_id: RepresentationId,
    pub equivalence: FallbackEquivalence,
}

/// Measurements are included only when backed by a separate benchmark result.
/// Absent measurement data means unknown, never zero and never an estimate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredQuality {
    pub suite_digest: Digest,
    pub host_profile_digest: Digest,
    pub raw_result_digest: Digest,
    pub silhouette_error_microunits: u32,
    pub landmark_error_microunits: u32,
    pub p95_build_time_ns: u64,
    pub peak_memory_bytes: u64,
    pub topology_flags: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepresentationRecord {
    /// Stable identity of this representation line, independent of payload.
    pub representation_id: RepresentationId,
    /// Increments when the payload or its semantic interpretation changes.
    pub representation_revision: u32,
    pub format: RepresentationFormat,
    pub role: RepresentationRole,
    pub lod_rank: u16,
    pub payload_digest: Digest,
    pub payload_bytes: u64,
    pub coordinate_frame_digest: Digest,
    pub landmark_digest: Digest,
    /// Features actually represented; do not infer support from the format.
    pub semantic_feature_mask: u64,
    /// Anchors directly/indirectly associated with this representation.
    pub anchor_mask: u64,
    pub anchor_fidelity: AnchorFidelity,
    pub required_capabilities: u64,
    pub required_extensions: u64,
    pub fallback: Option<FallbackLink>,
    pub measured_quality: Option<MeasuredQuality>,
}

impl RepresentationRecord {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if is_zero(&self.representation_id)
            || self.representation_revision == 0
            || is_zero(&self.payload_digest)
            || self.payload_bytes == 0
            || is_zero(&self.coordinate_frame_digest)
            || is_zero(&self.landmark_digest)
        {
            return Err(CatalogError::InvalidRepresentation);
        }
        if self.semantic_feature_mask & !FEATURE_KNOWN_MASK != 0
            || self.anchor_mask & !ANCHOR_KNOWN_MASK != 0
            || self.required_capabilities & !CAP_KNOWN_MASK != 0
            || self.required_extensions & !EXT_KNOWN_MASK != 0
        {
            return Err(CatalogError::UnknownCapabilityOrSemantic);
        }
        if !role_matches_format(self.role, self.format) {
            return Err(CatalogError::InvalidRoleFormat);
        }
        match self.format {
            RepresentationFormat::GltfGaussianSplat => {
                if self.required_capabilities & CAP_GAUSSIAN_SPLATTING == 0
                    || self.required_extensions & EXT_KHR_GAUSSIAN_SPLATTING == 0
                {
                    return Err(CatalogError::MissingRequiredFormatCapability);
                }
            }
            RepresentationFormat::OuroSdfGrid => {
                if self.required_capabilities & CAP_SDF_GRID == 0
                    || self.required_extensions & EXT_OURO_SDF_GRID == 0
                {
                    return Err(CatalogError::MissingRequiredFormatCapability);
                }
            }
            RepresentationFormat::LandmarkSidecar => {
                if self.required_extensions & EXT_OURO_LANDMARK_SIDECAR == 0
                    || self.anchor_fidelity != AnchorFidelity::Exact
                    || self.anchor_mask == 0
                {
                    return Err(CatalogError::MissingRequiredFormatCapability);
                }
            }
            RepresentationFormat::CpuRaster => {
                if self.required_capabilities & CAP_RASTER_IMAGE == 0 {
                    return Err(CatalogError::MissingRequiredFormatCapability);
                }
            }
            RepresentationFormat::GltfMesh => {
                if self.required_capabilities & CAP_GLTF_2 == 0 {
                    return Err(CatalogError::MissingRequiredFormatCapability);
                }
            }
            RepresentationFormat::GltfPointCloud => {
                if self.required_capabilities & CAP_GLTF_2 == 0 {
                    return Err(CatalogError::MissingRequiredFormatCapability);
                }
            }
        }
        if self.role == RepresentationRole::SemanticRig
            && (self.anchor_fidelity != AnchorFidelity::Exact || self.anchor_mask == 0)
        {
            return Err(CatalogError::InvalidSemanticRig);
        }
        if let Some(measured) = self.measured_quality {
            measured.validate()?;
        }
        if let Some(fallback) = self.fallback {
            if fallback.representation_id == self.representation_id {
                return Err(CatalogError::InvalidFallback);
            }
        }
        Ok(())
    }

    fn canonical_fields(&self, out: &mut Vec<u8>) -> Result<(), CatalogError> {
        self.validate()?;
        out.extend_from_slice(&self.representation_id);
        put_u32(out, self.representation_revision);
        out.push(self.format as u8);
        out.push(self.role as u8);
        put_u16(out, self.lod_rank);
        out.extend_from_slice(&self.payload_digest);
        put_u64(out, self.payload_bytes);
        out.extend_from_slice(&self.coordinate_frame_digest);
        out.extend_from_slice(&self.landmark_digest);
        put_u64(out, self.semantic_feature_mask);
        put_u64(out, self.anchor_mask);
        out.push(self.anchor_fidelity as u8);
        put_u64(out, self.required_capabilities);
        put_u64(out, self.required_extensions);
        match self.fallback {
            Some(fallback) => {
                out.push(1);
                out.extend_from_slice(&fallback.representation_id);
                out.push(fallback.equivalence as u8);
            }
            None => out.push(0),
        }
        match self.measured_quality {
            Some(measured) => {
                out.push(1);
                measured.canonical_fields(out);
            }
            None => out.push(0),
        }
        Ok(())
    }
}

impl MeasuredQuality {
    pub fn validate(self) -> Result<(), CatalogError> {
        if is_zero(&self.suite_digest)
            || is_zero(&self.host_profile_digest)
            || is_zero(&self.raw_result_digest)
            || self.p95_build_time_ns == 0
            || self.peak_memory_bytes == 0
        {
            return Err(CatalogError::InvalidMeasurementEvidence);
        }
        Ok(())
    }

    fn canonical_fields(self, out: &mut Vec<u8>) {
        out.extend_from_slice(&self.suite_digest);
        out.extend_from_slice(&self.host_profile_digest);
        out.extend_from_slice(&self.raw_result_digest);
        put_u32(out, self.silhouette_error_microunits);
        put_u32(out, self.landmark_error_microunits);
        put_u64(out, self.p95_build_time_ns);
        put_u64(out, self.peak_memory_bytes);
        put_u32(out, self.topology_flags);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RepresentationCatalog {
    pub schema_version: u16,
    pub entity_id: [u8; 16],
    pub entity_revision_root: Digest,
    pub landmark_digest: Digest,
    pub coordinate_frame_digest: Digest,
    pub required_visual_features: u64,
    pub required_anchors: u64,
    /// Canonically ordered by representation_id, bytewise ascending.
    pub representations: Vec<RepresentationRecord>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HostRepresentationCapabilities {
    pub capability_mask: u64,
    pub extension_mask: u64,
}

impl RepresentationCatalog {
    pub fn validate(&self) -> Result<(), CatalogError> {
        if self.schema_version != CATALOG_SCHEMA_VERSION {
            return Err(CatalogError::UnsupportedSchema);
        }
        if is_zero(&self.entity_id)
            || is_zero(&self.entity_revision_root)
            || is_zero(&self.landmark_digest)
            || is_zero(&self.coordinate_frame_digest)
        {
            return Err(CatalogError::InvalidCatalogIdentity);
        }
        if self.required_visual_features & !FEATURE_KNOWN_MASK != 0
            || self.required_anchors & !ANCHOR_KNOWN_MASK != 0
        {
            return Err(CatalogError::UnknownCapabilityOrSemantic);
        }
        if self.representations.is_empty()
            || self.representations.len() > MAX_REPRESENTATIONS
        {
            return Err(CatalogError::InvalidRepresentationCount);
        }

        for (index, representation) in self.representations.iter().enumerate() {
            representation.validate()?;
            if representation.landmark_digest != self.landmark_digest
                || representation.coordinate_frame_digest != self.coordinate_frame_digest
            {
                return Err(CatalogError::SemanticBindingMismatch);
            }
            if index > 0
                && self.representations[index - 1].representation_id
                    >= representation.representation_id
            {
                return Err(CatalogError::NonCanonicalOrdering);
            }
        }

        if self.required_visual_features != 0
            && !self.representations.iter().any(|entry| {
                entry.role == RepresentationRole::VisualAppearance
                    && entry.semantic_feature_mask & self.required_visual_features
                        == self.required_visual_features
            })
        {
            return Err(CatalogError::RequiredVisualFeaturesUnavailable);
        }
        if self.required_anchors != 0
            && !self.representations.iter().any(|entry| {
                entry.role == RepresentationRole::SemanticRig
                    && entry.anchor_fidelity == AnchorFidelity::Exact
                    && entry.anchor_mask & self.required_anchors == self.required_anchors
            })
        {
            return Err(CatalogError::RequiredAnchorsUnavailable);
        }

        for representation in &self.representations {
            if let Some(fallback) = representation.fallback {
                let target = self
                    .representations
                    .iter()
                    .find(|entry| entry.representation_id == fallback.representation_id)
                    .ok_or(CatalogError::UnknownFallback)?;
                if target.role != representation.role {
                    return Err(CatalogError::FallbackRoleMismatch);
                }
                match fallback.equivalence {
                    FallbackEquivalence::ExactEquivalent => {
                        if target.semantic_feature_mask != representation.semantic_feature_mask
                            || target.anchor_mask != representation.anchor_mask
                            || target.anchor_fidelity != representation.anchor_fidelity
                        {
                            return Err(CatalogError::FallbackEquivalenceMismatch);
                        }
                    }
                    FallbackEquivalence::SemanticallyReduced => {
                        if target.semantic_feature_mask & !representation.semantic_feature_mask != 0
                            || target.anchor_mask & !representation.anchor_mask != 0
                        {
                            return Err(CatalogError::FallbackEquivalenceMismatch);
                        }
                    }
                    FallbackEquivalence::NonEquivalent => {}
                }
            }
        }

        // Fallbacks form directed edges. Reject cycles at catalog validation,
        // not only when a particular chain happens to be resolved.
        for start in &self.representations {
            let mut seen = Vec::new();
            let mut current = start;
            loop {
                if seen.contains(&current.representation_id) {
                    return Err(CatalogError::FallbackCycle);
                }
                seen.push(current.representation_id);
                let Some(link) = current.fallback else {
                    break;
                };
                current = self
                    .representations
                    .iter()
                    .find(|entry| entry.representation_id == link.representation_id)
                    .ok_or(CatalogError::UnknownFallback)?;
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, CatalogError> {
        self.validate()?;
        let mut out = Vec::new();
        out.try_reserve_exact(self.estimated_canonical_len()?)
            .map_err(|_| CatalogError::AllocationFailed)?;
        out.extend_from_slice(&CATALOG_MAGIC);
        put_u16(&mut out, self.schema_version);
        out.extend_from_slice(&self.entity_id);
        out.extend_from_slice(&self.entity_revision_root);
        out.extend_from_slice(&self.landmark_digest);
        out.extend_from_slice(&self.coordinate_frame_digest);
        put_u64(&mut out, self.required_visual_features);
        put_u64(&mut out, self.required_anchors);
        put_u16(
            &mut out,
            u16::try_from(self.representations.len()).map_err(|_| CatalogError::SizeOverflow)?,
        );
        for representation in &self.representations {
            representation.canonical_fields(&mut out)?;
        }
        if out.len() > MAX_CATALOG_BYTES {
            return Err(CatalogError::CatalogTooLarge);
        }
        Ok(out)
    }

    fn estimated_canonical_len(&self) -> Result<usize, CatalogError> {
        let base = 8_usize
            .checked_add(2)
            .and_then(|n| n.checked_add(16))
            .and_then(|n| n.checked_add(32 * 3))
            .and_then(|n| n.checked_add(8 * 2))
            .and_then(|n| n.checked_add(2))
            .ok_or(CatalogError::SizeOverflow)?;
        self.representations.iter().try_fold(base, |total, entry| {
            total
                .checked_add(entry.canonical_len()?)
                .ok_or(CatalogError::SizeOverflow)
        })
    }

    /// Return all records meeting hard format, extension and semantic needs.
    /// Their catalog order is preserved; this function does not rank quality.
    pub fn compatible_entries(
        &self,
        host: HostRepresentationCapabilities,
        required_role: RepresentationRole,
        required_visual_features: u64,
        required_anchors: u64,
        require_exact_anchors: bool,
    ) -> Result<Vec<&RepresentationRecord>, CatalogError> {
        self.validate()?;
        if host.capability_mask & !CAP_KNOWN_MASK != 0
            || host.extension_mask & !EXT_KNOWN_MASK != 0
            || required_visual_features & !FEATURE_KNOWN_MASK != 0
            || required_anchors & !ANCHOR_KNOWN_MASK != 0
        {
            return Err(CatalogError::UnknownCapabilityOrSemantic);
        }
        Ok(self
            .representations
            .iter()
            .filter(|entry| {
                entry.role == required_role
                    && entry.required_capabilities & !host.capability_mask == 0
                    && entry.required_extensions & !host.extension_mask == 0
                    && entry.semantic_feature_mask
                        & (self.required_visual_features | required_visual_features)
                        == (self.required_visual_features | required_visual_features)
                    && entry.anchor_mask
                        & (if required_role == RepresentationRole::SemanticRig {
                            self.required_anchors | required_anchors
                        } else {
                            required_anchors
                        })
                        == (if required_role == RepresentationRole::SemanticRig {
                            self.required_anchors | required_anchors
                        } else {
                            required_anchors
                        })
                    && (!(require_exact_anchors
                        || required_role == RepresentationRole::SemanticRig)
                        || entry.anchor_fidelity == AnchorFidelity::Exact)
            })
            .collect())
    }

    /// Resolve a declared fallback chain. Each hop retains its equivalence
    /// classification; callers must not treat a reduced/non-equivalent result
    /// as an exact match for the original record.
    pub fn fallback_chain(
        &self,
        representation_id: RepresentationId,
    ) -> Result<Vec<(&RepresentationRecord, FallbackEquivalence)>, CatalogError> {
        self.validate()?;
        let mut current = self
            .representations
            .iter()
            .find(|entry| entry.representation_id == representation_id)
            .ok_or(CatalogError::UnknownRepresentation)?;
        let mut chain = Vec::new();
        let mut seen = Vec::new();
        for _ in 0..=self.representations.len() {
            if seen.contains(&current.representation_id) {
                return Err(CatalogError::FallbackCycle);
            }
            seen.push(current.representation_id);
            let Some(link) = current.fallback else {
                return Ok(chain);
            };
            chain.push((current, link.equivalence));
            current = self
                .representations
                .iter()
                .find(|entry| entry.representation_id == link.representation_id)
                .ok_or(CatalogError::UnknownFallback)?;
        }
        Err(CatalogError::FallbackCycle)
    }
}

impl RepresentationRecord {
    fn canonical_len(&self) -> Result<usize, CatalogError> {
        let base = 16_usize
            .checked_add(4)
            .and_then(|n| n.checked_add(1 + 1 + 2))
            .and_then(|n| n.checked_add(32))
            .and_then(|n| n.checked_add(8))
            .and_then(|n| n.checked_add(32 * 2))
            .and_then(|n| n.checked_add(8 * 2 + 1 + 8 * 2))
            .ok_or(CatalogError::SizeOverflow)?;
        // Both optional-presence markers are always serialized.
        let optional_markers = 2_usize;
        let fallback_payload = if self.fallback.is_some() { 16 + 1 } else { 0 };
        let measured_payload = if self.measured_quality.is_some() {
            32 * 3 + 4 * 2 + 8 * 2 + 4
        } else {
            0
        };
        base.checked_add(optional_markers)
            .and_then(|n| n.checked_add(fallback_payload))
            .and_then(|n| n.checked_add(measured_payload))
            .ok_or(CatalogError::SizeOverflow)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CatalogError {
    UnsupportedSchema,
    InvalidCatalogIdentity,
    RequiredVisualFeaturesUnavailable,
    RequiredAnchorsUnavailable,
    InvalidRepresentationCount,
    InvalidRepresentation,
    UnknownCapabilityOrSemantic,
    InvalidRoleFormat,
    MissingRequiredFormatCapability,
    InvalidSemanticRig,
    SemanticBindingMismatch,
    NonCanonicalOrdering,
    InvalidFallback,
    UnknownFallback,
    UnknownRepresentation,
    FallbackRoleMismatch,
    FallbackEquivalenceMismatch,
    FallbackCycle,
    InvalidMeasurementEvidence,
    AllocationFailed,
    SizeOverflow,
    CatalogTooLarge,
}

impl fmt::Display for CatalogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for CatalogError {}

fn role_matches_format(role: RepresentationRole, format: RepresentationFormat) -> bool {
    match format {
        RepresentationFormat::GltfMesh => matches!(
            role,
            RepresentationRole::VisualAppearance
                | RepresentationRole::EditableGeometry
                | RepresentationRole::CollisionProxy
        ),
        RepresentationFormat::GltfGaussianSplat | RepresentationFormat::GltfPointCloud => {
            role == RepresentationRole::VisualAppearance
        }
        RepresentationFormat::OuroSdfGrid => role == RepresentationRole::ImplicitField,
        RepresentationFormat::LandmarkSidecar => role == RepresentationRole::SemanticRig,
        RepresentationFormat::CpuRaster => role == RepresentationRole::ReferenceCapture,
    }
}

fn is_zero(value: &[u8]) -> bool {
    value.iter().all(|byte| *byte == 0)
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(id: u8, format: RepresentationFormat, role: RepresentationRole) -> RepresentationRecord {
        let (required_capabilities, required_extensions, anchor_fidelity, anchor_mask) = match format {
            RepresentationFormat::GltfMesh => (CAP_GLTF_2, 0, AnchorFidelity::Exact, ANCHOR_KNOWN_MASK),
            RepresentationFormat::GltfGaussianSplat => (
                CAP_GLTF_2 | CAP_GAUSSIAN_SPLATTING,
                EXT_KHR_GAUSSIAN_SPLATTING,
                AnchorFidelity::Approximate,
                ANCHOR_KNOWN_MASK,
            ),
            RepresentationFormat::GltfPointCloud => (CAP_GLTF_2, 0, AnchorFidelity::Approximate, 0),
            RepresentationFormat::OuroSdfGrid => (
                CAP_SDF_GRID,
                EXT_OURO_SDF_GRID,
                AnchorFidelity::Exact,
                ANCHOR_HEAD_ROOT | ANCHOR_NOSE_TIP,
            ),
            RepresentationFormat::LandmarkSidecar => (
                CAP_CPU,
                EXT_OURO_LANDMARK_SIDECAR,
                AnchorFidelity::Exact,
                ANCHOR_KNOWN_MASK,
            ),
            RepresentationFormat::CpuRaster => (
                CAP_RASTER_IMAGE,
                0,
                AnchorFidelity::Unavailable,
                0,
            ),
        };
        RepresentationRecord {
            representation_id: [id; 16],
            representation_revision: 1,
            format,
            role,
            lod_rank: id as u16,
            payload_digest: [id.wrapping_add(10); 32],
            payload_bytes: 4096 + u64::from(id),
            coordinate_frame_digest: [4; 32],
            landmark_digest: [3; 32],
            semantic_feature_mask: FEATURE_KNOWN_MASK,
            anchor_mask,
            anchor_fidelity,
            required_capabilities,
            required_extensions,
            fallback: None,
            measured_quality: None,
        }
    }

    fn catalog(mut representations: Vec<RepresentationRecord>) -> RepresentationCatalog {
        representations.sort_by_key(|entry| entry.representation_id);
        RepresentationCatalog {
            schema_version: CATALOG_SCHEMA_VERSION,
            entity_id: [1; 16],
            entity_revision_root: [2; 32],
            landmark_digest: [3; 32],
            coordinate_frame_digest: [4; 32],
            required_visual_features: FEATURE_HEAD_SILHOUETTE | FEATURE_EYE_PAIR,
            required_anchors: ANCHOR_HEAD_ROOT | ANCHOR_LEFT_EYE | ANCHOR_RIGHT_EYE,
            representations,
        }
    }

    #[test]
    fn one_entity_can_bind_mesh_splat_sdf_rig_and_raster_representations() {
        let value = catalog(vec![
            record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance),
            record(2, RepresentationFormat::GltfGaussianSplat, RepresentationRole::VisualAppearance),
            record(3, RepresentationFormat::OuroSdfGrid, RepresentationRole::ImplicitField),
            record(4, RepresentationFormat::LandmarkSidecar, RepresentationRole::SemanticRig),
            record(5, RepresentationFormat::CpuRaster, RepresentationRole::ReferenceCapture),
        ]);
        assert_eq!(value.validate(), Ok(()));
        let bytes = value.canonical_bytes().unwrap();
        assert!(bytes.starts_with(&CATALOG_MAGIC));
    }

    #[test]
    fn entity_identity_is_independent_of_representation_and_payload() {
        let first = catalog(vec![
            record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance),
        ]);
        let mut second = catalog(vec![
            record(2, RepresentationFormat::GltfGaussianSplat, RepresentationRole::VisualAppearance),
        ]);
        assert_eq!(first.entity_id, second.entity_id);
        assert_eq!(first.entity_revision_root, second.entity_revision_root);
        second.representations[0].payload_digest = [99; 32];
        assert_eq!(first.entity_id, second.entity_id);
        assert_ne!(
            first.canonical_bytes().unwrap(),
            second.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn catalog_requires_declared_identity_features_and_exact_anchor_sidecar() {
        let visual = record(
            1,
            RepresentationFormat::GltfMesh,
            RepresentationRole::VisualAppearance,
        );
        let missing_rig = catalog(vec![visual.clone()]);
        assert_eq!(
            missing_rig.validate(),
            Err(CatalogError::RequiredAnchorsUnavailable)
        );
        let mut rig = record(
            2,
            RepresentationFormat::LandmarkSidecar,
            RepresentationRole::SemanticRig,
        );
        rig.anchor_mask = ANCHOR_HEAD_ROOT;
        let missing_anchor = catalog(vec![visual.clone(), rig.clone()]);
        assert_eq!(
            missing_anchor.validate(),
            Err(CatalogError::RequiredAnchorsUnavailable)
        );
        rig.anchor_mask = ANCHOR_KNOWN_MASK;
        let valid = catalog(vec![visual, rig]);
        assert_eq!(valid.validate(), Ok(()));
    }

    #[test]
    fn visual_compatibility_cannot_weaken_catalog_required_features() {
        let mut visual = record(
            1,
            RepresentationFormat::GltfMesh,
            RepresentationRole::VisualAppearance,
        );
        visual.semantic_feature_mask = FEATURE_HEAD_SILHOUETTE;
        let rig = record(
            2,
            RepresentationFormat::LandmarkSidecar,
            RepresentationRole::SemanticRig,
        );
        let value = catalog(vec![visual, rig]);
        let host = HostRepresentationCapabilities {
            capability_mask: CAP_GLTF_2,
            extension_mask: 0,
        };
        assert_eq!(
            value.compatible_entries(
                host,
                RepresentationRole::VisualAppearance,
                0,
                0,
                false
            ),
            Ok(Vec::new())
        );
    }

    #[test]
    fn fallback_cycles_are_rejected_during_catalog_validation() {
        let mut first = record(
            1,
            RepresentationFormat::GltfMesh,
            RepresentationRole::VisualAppearance,
        );
        let mut second = record(
            2,
            RepresentationFormat::GltfMesh,
            RepresentationRole::VisualAppearance,
        );
        first.fallback = Some(FallbackLink {
            representation_id: [2; 16],
            equivalence: FallbackEquivalence::ExactEquivalent,
        });
        second.fallback = Some(FallbackLink {
            representation_id: [1; 16],
            equivalence: FallbackEquivalence::ExactEquivalent,
        });
        assert_eq!(
            catalog(vec![first, second]).validate(),
            Err(CatalogError::FallbackCycle)
        );
    }

    #[test]
    fn canonical_record_length_matches_encoded_record_bytes() {
        let record = record(
            1,
            RepresentationFormat::GltfMesh,
            RepresentationRole::VisualAppearance,
        );
        let mut bytes = Vec::new();
        record.canonical_fields(&mut bytes).unwrap();
        assert_eq!(record.canonical_len().unwrap(), bytes.len());
        let with_fallback = {
            let mut value = record.clone();
            value.fallback = Some(FallbackLink {
                representation_id: [2; 16],
                equivalence: FallbackEquivalence::ExactEquivalent,
            });
            value
        };
        let mut bytes = Vec::new();
        with_fallback.canonical_fields(&mut bytes).unwrap();
        assert_eq!(with_fallback.canonical_len().unwrap(), bytes.len());
        let mut measured = record;
        measured.measured_quality = Some(MeasuredQuality {
            suite_digest: [5; 32],
            host_profile_digest: [6; 32],
            raw_result_digest: [7; 32],
            silhouette_error_microunits: 1,
            landmark_error_microunits: 2,
            p95_build_time_ns: 3,
            peak_memory_bytes: 4,
            topology_flags: 5,
        });
        let mut bytes = Vec::new();
        measured.canonical_fields(&mut bytes).unwrap();
        assert_eq!(measured.canonical_len().unwrap(), bytes.len());
    }

    #[test]
    fn catalog_rejects_unsorted_or_duplicate_representation_ids() {
        let a = record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance);
        let b = record(2, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance);
        let unsorted = RepresentationCatalog {
            representations: vec![b.clone(), a.clone()],
            ..catalog(vec![a.clone(), b.clone()])
        };
        assert_eq!(unsorted.validate(), Err(CatalogError::NonCanonicalOrdering));
        let duplicate = RepresentationCatalog {
            representations: vec![a.clone(), a],
            ..catalog(vec![record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance)])
        };
        assert_eq!(duplicate.validate(), Err(CatalogError::NonCanonicalOrdering));
    }

    #[test]
    fn representation_payloads_must_bind_catalog_landmarks_and_frame() {
        let mut entry = record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance);
        entry.landmark_digest = [8; 32];
        assert_eq!(
            catalog(vec![entry]).validate(),
            Err(CatalogError::SemanticBindingMismatch)
        );
        let mut entry = record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance);
        entry.coordinate_frame_digest = [8; 32];
        assert_eq!(
            catalog(vec![entry]).validate(),
            Err(CatalogError::SemanticBindingMismatch)
        );
    }

    #[test]
    fn splat_is_appearance_only_and_requires_ratified_extension_capability() {
        let mut invalid_role = record(
            1,
            RepresentationFormat::GltfGaussianSplat,
            RepresentationRole::CollisionProxy,
        );
        assert_eq!(invalid_role.validate(), Err(CatalogError::InvalidRoleFormat));
        invalid_role.role = RepresentationRole::VisualAppearance;
        invalid_role.required_extensions = 0;
        assert_eq!(
            invalid_role.validate(),
            Err(CatalogError::MissingRequiredFormatCapability)
        );
        let mut invalid_cap = record(
            2,
            RepresentationFormat::GltfGaussianSplat,
            RepresentationRole::VisualAppearance,
        );
        invalid_cap.required_capabilities &= !CAP_GAUSSIAN_SPLATTING;
        assert_eq!(
            invalid_cap.validate(),
            Err(CatalogError::MissingRequiredFormatCapability)
        );
    }

    #[test]
    fn sdf_rig_and_raster_formats_cannot_claim_unrelated_roles() {
        assert_eq!(
            record(1, RepresentationFormat::OuroSdfGrid, RepresentationRole::VisualAppearance).validate(),
            Err(CatalogError::InvalidRoleFormat)
        );
        assert_eq!(
            record(2, RepresentationFormat::LandmarkSidecar, RepresentationRole::CollisionProxy).validate(),
            Err(CatalogError::InvalidRoleFormat)
        );
        assert_eq!(
            record(3, RepresentationFormat::CpuRaster, RepresentationRole::SemanticRig).validate(),
            Err(CatalogError::InvalidRoleFormat)
        );
    }

    #[test]
    fn compatible_entries_enforce_hard_capabilities_and_semantics_without_ranking() {
        let value = catalog(vec![
            record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance),
            record(2, RepresentationFormat::GltfGaussianSplat, RepresentationRole::VisualAppearance),
            record(3, RepresentationFormat::GltfPointCloud, RepresentationRole::VisualAppearance),
        ]);
        let cpu_gltf = HostRepresentationCapabilities {
            capability_mask: CAP_CPU | CAP_GLTF_2,
            extension_mask: 0,
        };
        let result = value
            .compatible_entries(
                cpu_gltf,
                RepresentationRole::VisualAppearance,
                FEATURE_HEAD_SILHOUETTE,
                ANCHOR_HEAD_ROOT,
                false,
            )
            .unwrap();
        assert_eq!(result.len(), 2);
        assert_eq!(result[0].representation_id, [1; 16]);
        assert_eq!(result[1].representation_id, [3; 16]);

        let splat_host = HostRepresentationCapabilities {
            capability_mask: CAP_GLTF_2 | CAP_GAUSSIAN_SPLATTING,
            extension_mask: EXT_KHR_GAUSSIAN_SPLATTING,
        };
        let result = value
            .compatible_entries(
                splat_host,
                RepresentationRole::VisualAppearance,
                FEATURE_EYE_PAIR,
                0,
                false,
            )
            .unwrap();
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn exact_anchor_requirement_excludes_approximate_splats() {
        let value = catalog(vec![
            record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance),
            record(2, RepresentationFormat::GltfGaussianSplat, RepresentationRole::VisualAppearance),
        ]);
        let host = HostRepresentationCapabilities {
            capability_mask: CAP_GLTF_2 | CAP_GAUSSIAN_SPLATTING,
            extension_mask: EXT_KHR_GAUSSIAN_SPLATTING,
        };
        let result = value
            .compatible_entries(
                host,
                RepresentationRole::VisualAppearance,
                FEATURE_HEAD_SILHOUETTE,
                ANCHOR_HEAD_ROOT,
                true,
            )
            .unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].representation_id, [1; 16]);
    }

    #[test]
    fn fallback_requires_existing_same_role_and_declared_equivalence() {
        let mut splat = record(
            1,
            RepresentationFormat::GltfGaussianSplat,
            RepresentationRole::VisualAppearance,
        );
        splat.fallback = Some(FallbackLink {
            representation_id: [2; 16],
            equivalence: FallbackEquivalence::NonEquivalent,
        });
        let point_cloud = record(
            2,
            RepresentationFormat::GltfPointCloud,
            RepresentationRole::VisualAppearance,
        );
        let value = catalog(vec![splat.clone(), point_cloud]);
        assert_eq!(value.validate(), Ok(()));
        assert_eq!(value.fallback_chain([1; 16]).unwrap().len(), 1);

        splat.fallback = Some(FallbackLink {
            representation_id: [9; 16],
            equivalence: FallbackEquivalence::NonEquivalent,
        });
        assert_eq!(
            catalog(vec![splat]).validate(),
            Err(CatalogError::UnknownFallback)
        );
    }

    #[test]
    fn exact_fallback_must_preserve_features_and_anchor_fidelity() {
        let mut mesh = record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance);
        mesh.fallback = Some(FallbackLink {
            representation_id: [2; 16],
            equivalence: FallbackEquivalence::ExactEquivalent,
        });
        let mut reduced = record(2, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance);
        reduced.semantic_feature_mask &= !FEATURE_MOUTH;
        assert_eq!(
            catalog(vec![mesh, reduced]).validate(),
            Err(CatalogError::FallbackEquivalenceMismatch)
        );
    }

    #[test]
    fn measured_quality_requires_raw_suite_and_host_provenance() {
        let mut entry = record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance);
        entry.measured_quality = Some(MeasuredQuality {
            suite_digest: [5; 32],
            host_profile_digest: [6; 32],
            raw_result_digest: [0; 32],
            silhouette_error_microunits: 10,
            landmark_error_microunits: 5,
            p95_build_time_ns: 100,
            peak_memory_bytes: 200,
            topology_flags: 1,
        });
        assert_eq!(
            catalog(vec![entry]).validate(),
            Err(CatalogError::InvalidMeasurementEvidence)
        );
    }

    #[test]
    fn canonical_bytes_are_deterministic_and_bind_measurements_fallbacks_and_semantics() {
        let mut splat = record(1, RepresentationFormat::GltfGaussianSplat, RepresentationRole::VisualAppearance);
        splat.fallback = Some(FallbackLink {
            representation_id: [2; 16],
            equivalence: FallbackEquivalence::NonEquivalent,
        });
        let point = record(2, RepresentationFormat::GltfPointCloud, RepresentationRole::VisualAppearance);
        let first = catalog(vec![splat.clone(), point.clone()]);
        let second = catalog(vec![point, splat]);
        let a = first.canonical_bytes().unwrap();
        let b = second.canonical_bytes().unwrap();
        assert_eq!(a, b);
        let mut changed = first.clone();
        changed.representations[0].fallback.as_mut().unwrap().equivalence =
            FallbackEquivalence::SemanticallyReduced;
        assert_ne!(a, changed.canonical_bytes().unwrap());
    }

    #[test]
    fn catalog_rejects_unknown_schema_zero_roots_and_unknown_capabilities() {
        let mut value = catalog(vec![
            record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance),
        ]);
        value.schema_version = 99;
        assert_eq!(value.validate(), Err(CatalogError::UnsupportedSchema));
        value = catalog(vec![
            record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance),
        ]);
        value.landmark_digest = [0; 32];
        assert_eq!(value.validate(), Err(CatalogError::InvalidCatalogIdentity));
        value = catalog(vec![
            record(1, RepresentationFormat::GltfMesh, RepresentationRole::VisualAppearance),
        ]);
        value.representations[0].required_capabilities = 1 << 60;
        assert_eq!(value.validate(), Err(CatalogError::UnknownCapabilityOrSemantic));
    }
}
