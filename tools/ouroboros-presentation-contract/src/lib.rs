#![forbid(unsafe_code)]
//! Dependency-free conformance oracle for Ouroboros persistent identity and
//! negotiated presentation quality.
//!
//! This crate deliberately does not hash, sign, render, access the OS, or
//! query a device. Callers hash the canonical bytes with the project's chosen
//! digest implementation (currently BLAKE3) and attach host/compositor
//! observations at the appropriate trust boundary.

use std::fmt;

pub const ENTITY_SCHEMA_VERSION: u16 = 1;
pub const QUALITY_SCHEMA_VERSION: u16 = 1;
pub const CAPABILITY_SCHEMA_VERSION: u16 = 1;
pub const DECISION_SCHEMA_VERSION: u16 = 1;
pub const FRAME_RECEIPT_SCHEMA_VERSION: u16 = 2;

const MAX_FEATURE_IDS: usize = 128;
const MAX_REPRESENTATION_ROOTS: usize = 128;
const MAX_CANDIDATES: usize = 128;
const QUALITY_CANDIDATE_ENCODED_LEN: usize = 67;

pub type Digest = [u8; 32];

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct EntityId(pub [u8; 16]);

impl EntityId {
    pub fn is_zero(self) -> bool {
        self.0.iter().all(|byte| *byte == 0)
    }
}

/// A stable entity identity is intentionally separate from revision digests.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EntityDescriptor {
    pub schema_version: u16,
    pub entity_id: EntityId,
    pub semantic_contract_version: u16,
    /// Sorted, unique semantic feature IDs required to preserve identity.
    pub required_feature_ids: Vec<u16>,
    /// Sorted, unique content roots for immutable representation assets.
    pub representation_roots: Vec<Digest>,
    /// Optional prior revision root for an explicit entity fork/lineage.
    pub parent_revision_root: Option<Digest>,
}

impl EntityDescriptor {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != ENTITY_SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        if self.entity_id.is_zero() || self.semantic_contract_version == 0 {
            return Err(ContractError::InvalidEntity);
        }
        if self.required_feature_ids.len() > MAX_FEATURE_IDS
            || self.representation_roots.len() > MAX_REPRESENTATION_ROOTS
        {
            return Err(ContractError::CollectionTooLarge);
        }
        if !strictly_increasing(&self.required_feature_ids) {
            return Err(ContractError::NonCanonicalOrdering);
        }
        if !self
            .representation_roots
            .windows(2)
            .all(|pair| pair[0] < pair[1])
        {
            return Err(ContractError::NonCanonicalOrdering);
        }
        Ok(())
    }

    /// Canonical little-endian bytes. Hash these bytes externally to obtain a
    /// revision root; never use that mutable root as the stable entity ID.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut out = Vec::with_capacity(
            8 + 2 + 16 + 2 + 2 + self.required_feature_ids.len() * 2
                + 2 + self.representation_roots.len() * 32 + 1 + 32,
        );
        out.extend_from_slice(b"OUROENT1");
        put_u16(&mut out, self.schema_version);
        out.extend_from_slice(&self.entity_id.0);
        put_u16(&mut out, self.semantic_contract_version);
        put_u16(&mut out, self.required_feature_ids.len() as u16);
        for id in &self.required_feature_ids {
            put_u16(&mut out, *id);
        }
        put_u16(&mut out, self.representation_roots.len() as u16);
        for root in &self.representation_roots {
            out.extend_from_slice(root);
        }
        match self.parent_revision_root {
            Some(root) => {
                out.push(1);
                out.extend_from_slice(&root);
            }
            None => out.push(0),
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardConstraints {
    pub max_vertices: u32,
    pub max_indices: u32,
    pub max_geometry_bytes: u32,
    /// Maximum estimated render work for one frame; observed time is separate.
    pub max_frame_time_ns: u64,
    pub max_cpu_time_ns: u64,
    pub max_gpu_time_ns: u64,
    pub max_cpu_memory_bytes: u64,
    pub max_gpu_memory_bytes: u64,
    pub min_feature_diameter_millipixels: u32,
    pub max_silhouette_error_microunits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoftPreferences {
    pub preferred_feature_diameter_millipixels: u32,
    pub preferred_max_silhouette_error_microunits: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityRequest {
    pub schema_version: u16,
    pub hard: HardConstraints,
    pub soft: SoftPreferences,
    pub reduced_motion: bool,
}

impl QualityRequest {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != QUALITY_SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        let h = self.hard;
        if h.max_vertices == 0
            || h.max_indices == 0
            || h.max_geometry_bytes == 0
            || h.max_frame_time_ns == 0
            || h.max_cpu_time_ns == 0
            || h.max_cpu_memory_bytes == 0
            || h.min_feature_diameter_millipixels == 0
            || self.soft.preferred_feature_diameter_millipixels
                < h.min_feature_diameter_millipixels
            || self.soft.preferred_max_silhouette_error_microunits
                > h.max_silhouette_error_microunits
        {
            return Err(ContractError::InvalidConstraints);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut out = Vec::with_capacity(8 + 2 + 4 * 7 + 8 * 5 + 1);
        out.extend_from_slice(b"OUROQRE1");
        put_u16(&mut out, self.schema_version);
        put_u32(&mut out, self.hard.max_vertices);
        put_u32(&mut out, self.hard.max_indices);
        put_u32(&mut out, self.hard.max_geometry_bytes);
        put_u64(&mut out, self.hard.max_frame_time_ns);
        put_u64(&mut out, self.hard.max_cpu_time_ns);
        put_u64(&mut out, self.hard.max_gpu_time_ns);
        put_u64(&mut out, self.hard.max_cpu_memory_bytes);
        put_u64(&mut out, self.hard.max_gpu_memory_bytes);
        put_u32(&mut out, self.hard.min_feature_diameter_millipixels);
        put_u32(&mut out, self.hard.max_silhouette_error_microunits);
        put_u32(&mut out, self.soft.preferred_feature_diameter_millipixels);
        put_u32(
            &mut out,
            self.soft.preferred_max_silhouette_error_microunits,
        );
        out.push(u8::from(self.reduced_motion));
        Ok(out)
    }
}

/// One host-advertised candidate. Estimate fields are planning inputs, not
/// telemetry and must never be copied into a delivered-frame observation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityCandidate {
    pub candidate_id: u16,
    /// Lower rank is preferred. Ranks must be unique in one snapshot.
    pub preference_rank: u16,
    pub target_format_id: u16,
    pub estimated_vertices: u32,
    pub estimated_indices: u32,
    pub estimated_geometry_bytes: u32,
    pub estimated_frame_time_ns: u64,
    pub estimated_cpu_time_ns: u64,
    pub estimated_gpu_time_ns: u64,
    pub estimated_cpu_memory_bytes: u64,
    pub estimated_gpu_memory_bytes: u64,
    pub feature_diameter_millipixels: u32,
    pub silhouette_error_microunits: u32,
    pub animated: bool,
}

impl QualityCandidate {
    fn validate(&self) -> Result<(), ContractError> {
        if self.candidate_id == 0
            || self.target_format_id == 0
            || self.estimated_vertices == 0
            || self.estimated_indices == 0
            || self.estimated_geometry_bytes == 0
            || self.estimated_frame_time_ns == 0
            || self.estimated_cpu_time_ns == 0
            || self.estimated_frame_time_ns < self.estimated_cpu_time_ns
            || self.estimated_frame_time_ns < self.estimated_gpu_time_ns
            || self.estimated_cpu_memory_bytes == 0
            || self.feature_diameter_millipixels == 0
        {
            return Err(ContractError::InvalidCandidate);
        }
        Ok(())
    }

    fn is_feasible_for(&self, request: &QualityRequest) -> bool {
        let h = request.hard;
        self.estimated_vertices <= h.max_vertices
            && self.estimated_indices <= h.max_indices
            && self.estimated_geometry_bytes <= h.max_geometry_bytes
            && self.estimated_frame_time_ns <= h.max_frame_time_ns
            && self.estimated_cpu_time_ns <= h.max_cpu_time_ns
            && self.estimated_gpu_time_ns <= h.max_gpu_time_ns
            && self.estimated_cpu_memory_bytes <= h.max_cpu_memory_bytes
            && self.estimated_gpu_memory_bytes <= h.max_gpu_memory_bytes
            && self.feature_diameter_millipixels >= h.min_feature_diameter_millipixels
            && self.silhouette_error_microunits <= h.max_silhouette_error_microunits
            && (!request.reduced_motion || !self.animated)
    }

    fn degradation_flags(&self, request: &QualityRequest) -> u8 {
        let mut flags = 0;
        if self.feature_diameter_millipixels
            < request.soft.preferred_feature_diameter_millipixels
        {
            flags |= Degradation::FeatureResolution as u8;
        }
        if self.silhouette_error_microunits
            > request.soft.preferred_max_silhouette_error_microunits
        {
            flags |= Degradation::SilhouetteError as u8;
        }
        flags
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        put_u16(out, self.candidate_id);
        put_u16(out, self.preference_rank);
        put_u16(out, self.target_format_id);
        put_u32(out, self.estimated_vertices);
        put_u32(out, self.estimated_indices);
        put_u32(out, self.estimated_geometry_bytes);
        put_u64(out, self.estimated_frame_time_ns);
        put_u64(out, self.estimated_cpu_time_ns);
        put_u64(out, self.estimated_gpu_time_ns);
        put_u64(out, self.estimated_cpu_memory_bytes);
        put_u64(out, self.estimated_gpu_memory_bytes);
        put_u32(out, self.feature_diameter_millipixels);
        put_u32(out, self.silhouette_error_microunits);
        out.push(u8::from(self.animated));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySnapshot {
    pub schema_version: u16,
    pub revision: u64,
    /// Sorted by ascending preference_rank; one rank per candidate.
    pub candidates: Vec<QualityCandidate>,
}

impl CapabilitySnapshot {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != CAPABILITY_SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        if self.revision == 0 || self.candidates.len() > MAX_CANDIDATES {
            return Err(ContractError::InvalidCapabilities);
        }
        for (index, candidate) in self.candidates.iter().enumerate() {
            candidate.validate()?;
            if self.candidates[..index]
                .iter()
                .any(|previous| previous.candidate_id == candidate.candidate_id)
            {
                return Err(ContractError::InvalidCapabilities);
            }
        }
        if self.candidates.windows(2).any(|pair| {
            pair[0].preference_rank >= pair[1].preference_rank
        }) {
            return Err(ContractError::NonCanonicalOrdering);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut out = Vec::with_capacity(8 + 2 + 8 + 2 + self.candidates.len() * QUALITY_CANDIDATE_ENCODED_LEN);
        out.extend_from_slice(b"OUROCAP1");
        put_u16(&mut out, self.schema_version);
        put_u64(&mut out, self.revision);
        put_u16(&mut out, self.candidates.len() as u16);
        for candidate in &self.candidates {
            candidate.encode_into(&mut out);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Degradation {
    FeatureResolution = 1,
    SilhouetteError = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum InfeasibleReason {
    NoCandidateSatisfiesAllHardConstraints = 1,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityDecision {
    schema_version: u16,
    policy_version: u16,
    capability_revision: u64,
    /// Exact canonical inputs are embedded so the decision digest binds them.
    request_bytes: Vec<u8>,
    capability_bytes: Vec<u8>,
    selected_candidate: Option<QualityCandidate>,
    degradation_flags: u8,
    infeasible_reason: Option<InfeasibleReason>,
}

impl QualityDecision {
    pub fn selected_candidate(&self) -> Option<&QualityCandidate> {
        self.selected_candidate.as_ref()
    }

    pub fn capability_revision(&self) -> u64 {
        self.capability_revision
    }

    pub fn is_degraded(&self, degradation: Degradation) -> bool {
        self.degradation_flags & (degradation as u8) != 0
    }

    pub fn degradation_flags(&self) -> u8 {
        self.degradation_flags
    }

    pub fn infeasible_reason(&self) -> Option<InfeasibleReason> {
        self.infeasible_reason
    }

    pub fn is_feasible(&self) -> bool {
        self.selected_candidate.is_some() && self.infeasible_reason.is_none()
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        if self.schema_version != DECISION_SCHEMA_VERSION
            || self.policy_version == 0
            || self.capability_revision == 0
            || (self.selected_candidate.is_some() == self.infeasible_reason.is_some())
            || self.degradation_flags & !0b11 != 0
        {
            return Err(ContractError::InvalidDecision);
        }
        let request_len = u32::try_from(self.request_bytes.len())
            .map_err(|_| ContractError::CollectionTooLarge)?;
        let capability_len = u32::try_from(self.capability_bytes.len())
            .map_err(|_| ContractError::CollectionTooLarge)?;
        let mut out = Vec::with_capacity(
            8 + 2 + 2 + 8 + 4 + self.request_bytes.len()
                + 4 + self.capability_bytes.len() + 1 + QUALITY_CANDIDATE_ENCODED_LEN + 1 + 1,
        );
        out.extend_from_slice(b"OURODEC1");
        put_u16(&mut out, self.schema_version);
        put_u16(&mut out, self.policy_version);
        put_u64(&mut out, self.capability_revision);
        put_u32(&mut out, request_len);
        out.extend_from_slice(&self.request_bytes);
        put_u32(&mut out, capability_len);
        out.extend_from_slice(&self.capability_bytes);
        match self.selected_candidate {
            Some(candidate) => {
                out.push(1);
                candidate.encode_into(&mut out);
            }
            None => out.push(0),
        }
        out.push(self.degradation_flags);
        out.push(match self.infeasible_reason {
            Some(reason) => reason as u8,
            None => 0,
        });
        Ok(out)
    }
}

/// Deterministic, fail-closed negotiation. The caller hashes the canonical
/// request, capability snapshot and returned decision bytes externally.
pub fn negotiate(
    request: &QualityRequest,
    capabilities: &CapabilitySnapshot,
    policy_version: u16,
) -> Result<QualityDecision, ContractError> {
    request.validate()?;
    capabilities.validate()?;
    if policy_version == 0 {
        return Err(ContractError::InvalidDecision);
    }
    let request_bytes = request.canonical_bytes()?;
    let capability_bytes = capabilities.canonical_bytes()?;
    let selected_candidate = capabilities
        .candidates
        .iter()
        .copied()
        .find(|candidate| candidate.is_feasible_for(request));
    let degradation_flags = selected_candidate
        .map(|candidate| candidate.degradation_flags(request))
        .unwrap_or(0);
    let infeasible_reason = if selected_candidate.is_none() {
        Some(InfeasibleReason::NoCandidateSatisfiesAllHardConstraints)
    } else {
        None
    };
    Ok(QualityDecision {
        schema_version: DECISION_SCHEMA_VERSION,
        policy_version,
        capability_revision: capabilities.revision,
        request_bytes,
        capability_bytes,
        selected_candidate,
        degradation_flags,
        infeasible_reason,
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PresentationStatus {
    Rendered = 1,
    Submitted = 2,
    Presented = 3,
    Discarded = 4,
    Unknown = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum EvidenceLevel {
    RendererReported = 0,
    HostObserved = 1,
    CompositorFeedback = 2,
    HardwareObserved = 3,
    ExternalInstrument = 4,
}

/// Presentation protocol identifiers are explicit so adapters cannot silently
/// reinterpret source-specific flags as universal evidence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum PresentationSource {
    WaylandPresentationTimeV2 = 1,
}

pub const WAYLAND_PRESENTATION_VSYNC: u32 = 0x1;
pub const WAYLAND_PRESENTATION_HW_CLOCK: u32 = 0x2;
pub const WAYLAND_PRESENTATION_HW_COMPLETION: u32 = 0x4;
pub const WAYLAND_PRESENTATION_ZERO_COPY: u32 = 0x8;
const WAYLAND_PRESENTATION_KNOWN_FLAGS: u32 = 0x0f;

/// Protocol-preserving presentation observation.
///
/// The optional output identity is a host-stable digest mapped from the
/// Wayland sync_output object. It is optional because a client may not have
/// bound the corresponding wl_output. Refresh interval and refresh sequence
/// are kept even when zero: the protocol uses zero when prediction/retrace
/// information is unavailable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PresentationObservation {
    pub source: PresentationSource,
    pub clock_id: u32,
    pub presentation_time_ns: u64,
    pub refresh_interval_ns: u32,
    pub refresh_sequence: u64,
    pub output_identity_digest: Option<Digest>,
    /// Source-protocol flags. For Wayland v2 these are vsync, hw_clock,
    /// hw_completion and zero_copy. They remain compositor-reported claims.
    pub source_flags: u32,
}

impl PresentationObservation {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.source_flags & !WAYLAND_PRESENTATION_KNOWN_FLAGS != 0
            || self
                .output_identity_digest
                .is_some_and(|digest| digest.iter().all(|byte| *byte == 0))
        {
            return Err(ContractError::InvalidPresentationObservation);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut out = Vec::with_capacity(2 + 4 + 8 + 4 + 8 + 1 + 32 + 4);
        put_u16(&mut out, self.source as u16);
        put_u32(&mut out, self.clock_id);
        put_u64(&mut out, self.presentation_time_ns);
        put_u32(&mut out, self.refresh_interval_ns);
        put_u64(&mut out, self.refresh_sequence);
        match self.output_identity_digest {
            Some(digest) => {
                out.push(1);
                out.extend_from_slice(&digest);
            }
            None => out.push(0),
        }
        put_u32(&mut out, self.source_flags);
        Ok(out)
    }
}

/// Raw fields from wp_presentation_feedback.presented in Wayland
/// presentation-time protocol v2. Split 32-bit values are retained until
/// checked conversion so timestamp/sequence overflow cannot be hidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WaylandPresentedEvent {
    pub seconds_hi: u32,
    pub seconds_lo: u32,
    pub nanoseconds: u32,
    pub refresh_interval_ns: u32,
    pub sequence_hi: u32,
    pub sequence_lo: u32,
    pub flags: u32,
}

impl WaylandPresentedEvent {
    pub fn into_observation(
        self,
        clock_id: u32,
        output_identity_digest: Option<Digest>,
    ) -> Result<PresentationObservation, ContractError> {
        if self.nanoseconds >= 1_000_000_000
            || self.flags & !WAYLAND_PRESENTATION_KNOWN_FLAGS != 0
            || output_identity_digest
                .is_some_and(|digest| digest.iter().all(|byte| *byte == 0))
        {
            return Err(ContractError::InvalidPresentationObservation);
        }
        let seconds = (u64::from(self.seconds_hi) << 32) | u64::from(self.seconds_lo);
        let presentation_time_ns = seconds
            .checked_mul(1_000_000_000)
            .and_then(|base| base.checked_add(u64::from(self.nanoseconds)))
            .ok_or(ContractError::PresentationTimestampOverflow)?;
        let refresh_sequence =
            (u64::from(self.sequence_hi) << 32) | u64::from(self.sequence_lo);
        Ok(PresentationObservation {
            source: PresentationSource::WaylandPresentationTimeV2,
            clock_id,
            presentation_time_ns,
            refresh_interval_ns: self.refresh_interval_ns,
            refresh_sequence,
            output_identity_digest,
            source_flags: self.flags,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameReceipt {
    pub schema_version: u16,
    pub entity_id: EntityId,
    pub entity_revision_root: Digest,
    pub backend_build_digest: Digest,
    pub session_id: [u8; 16],
    pub state_digest: Digest,
    pub decision_digest: Digest,
    pub frame_sequence: u64,
    pub buffer_generation: u64,
    pub target_format_id: u16,
    pub width: u32,
    pub height: u32,
    pub status: PresentationStatus,
    pub evidence_level: EvidenceLevel,
    /// Identifies the protocol that reported a terminal presentation outcome,
    /// including Discarded outcomes which have no presentation timestamp.
    pub presentation_source: Option<PresentationSource>,
    /// Present only when the compositor supplied a Presented outcome.
    pub presentation: Option<PresentationObservation>,
}

impl FrameReceipt {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != FRAME_RECEIPT_SCHEMA_VERSION {
            return Err(ContractError::UnsupportedSchema);
        }
        if self.entity_id.is_zero()
            || self.backend_build_digest.iter().all(|byte| *byte == 0)
            || self.session_id.iter().all(|byte| *byte == 0)
            || self.frame_sequence == 0
            || self.buffer_generation == 0
            || self.target_format_id == 0
            || self.width == 0
            || self.height == 0
        {
            return Err(ContractError::InvalidFrameReceipt);
        }
        match self.status {
            PresentationStatus::Rendered => {
                if self.evidence_level > EvidenceLevel::HostObserved
                    || self.presentation_source.is_some()
                    || self.presentation.is_some()
                {
                    return Err(ContractError::UnsupportedPresentationClaim);
                }
            }
            PresentationStatus::Submitted => {
                if self.evidence_level < EvidenceLevel::HostObserved
                    || self.presentation_source.is_some()
                    || self.presentation.is_some()
                {
                    return Err(ContractError::UnsupportedPresentationClaim);
                }
            }
            PresentationStatus::Presented => {
                if self.evidence_level != EvidenceLevel::CompositorFeedback
                    || self.presentation_source != Some(PresentationSource::WaylandPresentationTimeV2)
                    || self.presentation.is_none()
                    || self.presentation.map(|value| value.source) != self.presentation_source
                {
                    return Err(ContractError::UnsupportedPresentationClaim);
                }
            }
            PresentationStatus::Discarded => {
                if self.evidence_level != EvidenceLevel::CompositorFeedback
                    || self.presentation_source != Some(PresentationSource::WaylandPresentationTimeV2)
                    || self.presentation.is_some()
                {
                    return Err(ContractError::UnsupportedPresentationClaim);
                }
            }
            PresentationStatus::Unknown => {
                if self.presentation_source.is_some() || self.presentation.is_some() {
                    return Err(ContractError::UnsupportedPresentationClaim);
                }
            }
        }
        if let Some(presentation) = self.presentation {
            presentation.validate()?;
        }
        Ok(())
    }

    /// This returns success only for compositor-or-stronger evidence that a
    /// presentation timestamp exists. Submitted/rendered/unknown are not shown.
    pub fn verify_presented(&self) -> Result<u64, ContractError> {
        self.validate()?;
        if self.status != PresentationStatus::Presented {
            return Err(ContractError::FrameNotPresented);
        }
        self.presentation
            .map(|presentation| presentation.presentation_time_ns)
            .ok_or(ContractError::UnsupportedPresentationClaim)
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut out = Vec::with_capacity(8 + 2 + 16 + 32 * 4 + 16 + 8 * 2 + 2 + 4 * 2 + 3 + 61);
        out.extend_from_slice(b"OUROFRM2");
        put_u16(&mut out, self.schema_version);
        out.extend_from_slice(&self.entity_id.0);
        out.extend_from_slice(&self.entity_revision_root);
        out.extend_from_slice(&self.backend_build_digest);
        out.extend_from_slice(&self.session_id);
        out.extend_from_slice(&self.state_digest);
        out.extend_from_slice(&self.decision_digest);
        put_u64(&mut out, self.frame_sequence);
        put_u64(&mut out, self.buffer_generation);
        put_u16(&mut out, self.target_format_id);
        put_u32(&mut out, self.width);
        put_u32(&mut out, self.height);
        out.push(self.status as u8);
        out.push(self.evidence_level as u8);
        match self.presentation_source {
            Some(source) => {
                out.push(1);
                put_u16(&mut out, source as u16);
            }
            None => out.push(0),
        }
        match self.presentation {
            Some(presentation) => {
                out.push(1);
                out.extend_from_slice(&presentation.canonical_bytes()?);
            }
            None => out.push(0),
        }
        Ok(out)
    }
}

/// Immutable identity of one presentation session. Output properties may
/// change only through a strictly newer buffer-pool generation.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SessionBinding {
    pub entity_id: EntityId,
    pub entity_revision_root: Digest,
    pub backend_build_digest: Digest,
    pub session_id: [u8; 16],
    pub decision_digest: Digest,
    /// Wayland presentation clock received for this display connection, when known.
    pub presentation_clock_id: Option<u32>,
    pub target_format_id: u16,
    pub width: u32,
    pub height: u32,
}

impl SessionBinding {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.entity_id.is_zero()
            || self.entity_revision_root.iter().all(|byte| *byte == 0)
            || self.backend_build_digest.iter().all(|byte| *byte == 0)
            || self.session_id.iter().all(|byte| *byte == 0)
            || self.decision_digest.iter().all(|byte| *byte == 0)
            || self.target_format_id == 0
            || self.width == 0
            || self.height == 0
        {
            return Err(ContractError::InvalidSessionBinding);
        }
        Ok(())
    }

    fn matches(&self, receipt: &FrameReceipt) -> bool {
        self.entity_id == receipt.entity_id
            && self.entity_revision_root == receipt.entity_revision_root
            && self.backend_build_digest == receipt.backend_build_digest
            && self.session_id == receipt.session_id
            && self.decision_digest == receipt.decision_digest
            && self.target_format_id == receipt.target_format_id
            && self.width == receipt.width
            && self.height == receipt.height
    }
}

/// Per-session verifier. It fences frames to the entity revision, backend
/// build, negotiated decision, output format/extent and exact pool generation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrameSequenceGuard {
    binding: SessionBinding,
    expected_generation: u64,
    last_sequence: u64,
    observed_presentation_clock_id: Option<u32>,
}

impl FrameSequenceGuard {
    pub fn new(binding: SessionBinding, expected_generation: u64) -> Result<Self, ContractError> {
        binding.validate()?;
        if expected_generation == 0 {
            return Err(ContractError::InvalidFrameReceipt);
        }
        Ok(Self {
            observed_presentation_clock_id: binding.presentation_clock_id,
            binding,
            expected_generation,
            last_sequence: 0,
        })
    }

    pub fn observe(&mut self, receipt: &FrameReceipt) -> Result<(), ContractError> {
        receipt.validate()?;
        if receipt.buffer_generation != self.expected_generation {
            return Err(ContractError::StaleBufferGeneration);
        }
        if !self.binding.matches(receipt) {
            return Err(ContractError::SessionBindingMismatch);
        }
        if let Some(observation) = receipt.presentation {
            match self.observed_presentation_clock_id {
                Some(clock_id) if clock_id != observation.clock_id => {
                    return Err(ContractError::PresentationClockMismatch);
                }
                None => self.observed_presentation_clock_id = Some(observation.clock_id),
                _ => {}
            }
        }
        if receipt.frame_sequence <= self.last_sequence {
            return Err(ContractError::NonMonotonicFrameSequence);
        }
        self.last_sequence = receipt.frame_sequence;
        Ok(())
    }

    /// Reconfiguration may alter target format/extent only when the host
    /// publishes a strictly newer pool generation. The frame sequence remains
    /// monotonic across reconfiguration.
    pub fn reconfigure(
        &mut self,
        new_generation: u64,
        target_format_id: u16,
        width: u32,
        height: u32,
    ) -> Result<(), ContractError> {
        if new_generation <= self.expected_generation {
            return Err(ContractError::StaleBufferGeneration);
        }
        if target_format_id == 0 || width == 0 || height == 0 {
            return Err(ContractError::InvalidSessionBinding);
        }
        self.expected_generation = new_generation;
        self.binding.target_format_id = target_format_id;
        self.binding.width = width;
        self.binding.height = height;
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ContractError {
    UnsupportedSchema,
    InvalidEntity,
    CollectionTooLarge,
    NonCanonicalOrdering,
    InvalidConstraints,
    InvalidCandidate,
    InvalidCapabilities,
    InvalidDecision,
    InvalidFrameReceipt,
    UnsupportedPresentationClaim,
    FrameNotPresented,
    StaleBufferGeneration,
    NonMonotonicFrameSequence,
    SessionMismatch,
    InvalidSessionBinding,
    SessionBindingMismatch,
    PresentationClockMismatch,
    InvalidPresentationObservation,
    PresentationTimestampOverflow,
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl std::error::Error for ContractError {}

fn strictly_increasing(values: &[u16]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
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

    fn request(max_gpu_memory_bytes: u64, reduced_motion: bool) -> QualityRequest {
        QualityRequest {
            schema_version: QUALITY_SCHEMA_VERSION,
            hard: HardConstraints {
                max_vertices: 10_000,
                max_indices: 30_000,
                max_geometry_bytes: 1_000_000,
                max_frame_time_ns: 16_700_000,
                max_cpu_time_ns: 4_000_000,
                max_gpu_time_ns: 10_000_000,
                max_cpu_memory_bytes: 500_000,
                max_gpu_memory_bytes,
                min_feature_diameter_millipixels: 1_000,
                max_silhouette_error_microunits: 50_000,
            },
            soft: SoftPreferences {
                preferred_feature_diameter_millipixels: 2_000,
                preferred_max_silhouette_error_microunits: 10_000,
            },
            reduced_motion,
        }
    }

    fn candidate(
        id: u16,
        rank: u16,
        gpu_bytes: u64,
        animated: bool,
        feature_px: u32,
        silhouette_error: u32,
    ) -> QualityCandidate {
        QualityCandidate {
            candidate_id: id,
            preference_rank: rank,
            target_format_id: 1,
            estimated_vertices: 5_000,
            estimated_indices: 15_000,
            estimated_geometry_bytes: 500_000,
            estimated_frame_time_ns: 8_000_000,
            estimated_cpu_time_ns: 2_000_000,
            estimated_gpu_time_ns: 5_000_000,
            estimated_cpu_memory_bytes: 300_000,
            estimated_gpu_memory_bytes: gpu_bytes,
            feature_diameter_millipixels: feature_px,
            silhouette_error_microunits: silhouette_error,
            animated,
        }
    }

    fn capabilities(candidates: Vec<QualityCandidate>) -> CapabilitySnapshot {
        CapabilitySnapshot {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            revision: 7,
            candidates,
        }
    }

    fn receipt(
        sequence: u64,
        generation: u64,
        status: PresentationStatus,
        evidence: EvidenceLevel,
        time: Option<u64>,
    ) -> FrameReceipt {
        FrameReceipt {
            schema_version: FRAME_RECEIPT_SCHEMA_VERSION,
            entity_id: EntityId([1; 16]),
            entity_revision_root: [2; 32],
            backend_build_digest: [6; 32],
            session_id: [3; 16],
            state_digest: [4; 32],
            decision_digest: [5; 32],
            frame_sequence: sequence,
            buffer_generation: generation,
            target_format_id: 1,
            width: 1920,
            height: 1080,
            status,
            evidence_level: evidence,
            presentation_source: time.map(|_| PresentationSource::WaylandPresentationTimeV2),
            presentation: time.map(|presentation_time_ns| PresentationObservation {
                source: PresentationSource::WaylandPresentationTimeV2,
                clock_id: 1,
                presentation_time_ns,
                refresh_interval_ns: 16_666_667,
                refresh_sequence: 9,
                output_identity_digest: Some([7; 32]),
                source_flags: WAYLAND_PRESENTATION_VSYNC,
            }),
        }
    }

    #[test]
    fn entity_identity_is_not_the_revision_digest() {
        let first = EntityDescriptor {
            schema_version: ENTITY_SCHEMA_VERSION,
            entity_id: EntityId([8; 16]),
            semantic_contract_version: 1,
            required_feature_ids: vec![1, 2, 3],
            representation_roots: vec![[1; 32]],
            parent_revision_root: None,
        };
        let mut second = first.clone();
        second.representation_roots = vec![[2; 32]];
        assert_eq!(first.entity_id, second.entity_id);
        assert_ne!(first.canonical_bytes().unwrap(), second.canonical_bytes().unwrap());
    }

    #[test]
    fn descriptor_rejects_noncanonical_feature_order() {
        let descriptor = EntityDescriptor {
            schema_version: ENTITY_SCHEMA_VERSION,
            entity_id: EntityId([8; 16]),
            semantic_contract_version: 1,
            required_feature_ids: vec![2, 1],
            representation_roots: Vec::new(),
            parent_revision_root: None,
        };
        assert_eq!(
            descriptor.canonical_bytes(),
            Err(ContractError::NonCanonicalOrdering)
        );
    }

    #[test]
    fn capability_snapshot_rejects_nonadjacent_duplicate_candidate_ids() {
        let caps = capabilities(vec![
            candidate(1, 0, 800_000, true, 3_000, 5_000),
            candidate(2, 1, 600_000, true, 2_000, 8_000),
            candidate(1, 2, 400_000, true, 1_500, 20_000),
        ]);
        assert_eq!(caps.validate(), Err(ContractError::InvalidCapabilities));
    }

    #[test]
    fn negotiation_is_deterministic_and_records_soft_degradation() {
        let req = request(1_000_000, false);
        let caps = capabilities(vec![
            candidate(1, 0, 900_000, true, 1_500, 15_000),
            candidate(2, 1, 600_000, true, 2_200, 8_000),
        ]);
        let a = negotiate(&req, &caps, 1).unwrap();
        let b = negotiate(&req, &caps, 1).unwrap();
        assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
        assert_eq!(a.selected_candidate().unwrap().candidate_id, 1);
        assert_eq!(
            a.degradation_flags(),
            Degradation::FeatureResolution as u8
                | Degradation::SilhouetteError as u8
        );
    }

    #[test]
    fn candidate_rejects_total_frame_estimate_below_cpu_or_gpu_component() {
        let mut inconsistent = candidate(1, 0, 500_000, true, 2_000, 5_000);
        inconsistent.estimated_frame_time_ns = 4_000_000;
        inconsistent.estimated_gpu_time_ns = 5_000_000;
        let caps = capabilities(vec![inconsistent]);
        assert_eq!(caps.validate(), Err(ContractError::InvalidCandidate));
    }

    #[test]
    fn hard_budget_violation_is_not_reported_as_degradation() {
        let req = request(500_000, false);
        let caps = capabilities(vec![candidate(1, 0, 600_000, true, 3_000, 1_000)]);
        let decision = negotiate(&req, &caps, 1).unwrap();
        assert!(!decision.is_feasible());
        assert_eq!(
            decision.infeasible_reason(),
            Some(InfeasibleReason::NoCandidateSatisfiesAllHardConstraints)
        );
    }

    #[test]
    fn cpu_only_renderer_is_feasible_with_zero_gpu_budget() {
        let mut req = request(1_000_000, false);
        req.hard.max_gpu_time_ns = 0;
        req.hard.max_gpu_memory_bytes = 0;
        let mut cpu_only = candidate(1, 0, 0, true, 3_000, 1_000);
        cpu_only.estimated_gpu_time_ns = 0;
        let caps = capabilities(vec![cpu_only]);
        let decision = negotiate(&req, &caps, 1).unwrap();
        assert!(decision.is_feasible());
        assert_eq!(decision.selected_candidate().unwrap().estimated_gpu_memory_bytes, 0);
    }

    #[test]
    fn cpu_budget_is_enforced_independently_of_gpu_budget() {
        let req = request(1_000_000, false);
        let mut over_cpu = candidate(1, 0, 400_000, true, 3_000, 1_000);
        over_cpu.estimated_cpu_time_ns = 5_000_000;
        let caps = capabilities(vec![over_cpu]);
        let decision = negotiate(&req, &caps, 1).unwrap();
        assert!(!decision.is_feasible());
    }

    #[test]
    fn reduced_motion_rejects_animated_candidates() {
        let req = request(1_000_000, true);
        let caps = capabilities(vec![candidate(1, 0, 500_000, true, 3_000, 1_000)]);
        let decision = negotiate(&req, &caps, 1).unwrap();
        assert!(!decision.is_feasible());
    }

    #[test]
    fn more_permissive_budget_does_not_choose_a_worse_rank() {
        let caps = capabilities(vec![
            candidate(1, 0, 900_000, true, 3_000, 5_000),
            candidate(2, 1, 400_000, true, 1_500, 20_000),
        ]);
        let tight = negotiate(&request(500_000, false), &caps, 1).unwrap();
        let expanded = negotiate(&request(1_000_000, false), &caps, 1).unwrap();
        assert_eq!(tight.selected_candidate().unwrap().preference_rank, 1);
        assert_eq!(expanded.selected_candidate().unwrap().preference_rank, 0);
    }

    #[test]
    fn presented_requires_compositor_or_stronger_evidence() {
        let false_claim = receipt(
            1,
            1,
            PresentationStatus::Presented,
            EvidenceLevel::RendererReported,
            Some(10),
        );
        assert_eq!(
            false_claim.verify_presented(),
            Err(ContractError::UnsupportedPresentationClaim)
        );
        let valid = receipt(
            1,
            1,
            PresentationStatus::Presented,
            EvidenceLevel::CompositorFeedback,
            Some(10),
        );
        assert_eq!(valid.verify_presented(), Ok(10));
    }

    #[test]
    fn presentation_receipt_requires_full_observation_and_backend_identity() {
        let mut missing_clock = receipt(
            1,
            1,
            PresentationStatus::Presented,
            EvidenceLevel::CompositorFeedback,
            Some(10),
        );
        missing_clock.presentation_source = None;
        missing_clock.presentation = None;
        assert_eq!(
            missing_clock.validate(),
            Err(ContractError::UnsupportedPresentationClaim)
        );

        let mut missing_backend = receipt(
            2,
            1,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        missing_backend.backend_build_digest = [0; 32];
        assert_eq!(
            missing_backend.validate(),
            Err(ContractError::InvalidFrameReceipt)
        );
    }

    #[test]
    fn wayland_feedback_combines_split_timestamp_and_refresh_sequence() {
        let event = WaylandPresentedEvent {
            seconds_hi: 1,
            seconds_lo: 2,
            nanoseconds: 999_999_999,
            refresh_interval_ns: 0,
            sequence_hi: 3,
            sequence_lo: 4,
            flags: WAYLAND_PRESENTATION_VSYNC | WAYLAND_PRESENTATION_HW_CLOCK,
        };
        let observation = event.into_observation(1, None).unwrap();
        let seconds = (1_u64 << 32) | 2;
        assert_eq!(
            observation.presentation_time_ns,
            seconds * 1_000_000_000 + 999_999_999
        );
        assert_eq!(observation.refresh_sequence, (3_u64 << 32) | 4);
        assert_eq!(observation.refresh_interval_ns, 0);
        assert_eq!(observation.output_identity_digest, None);
        assert_eq!(
            observation.source_flags,
            WAYLAND_PRESENTATION_VSYNC | WAYLAND_PRESENTATION_HW_CLOCK
        );
    }

    #[test]
    fn wayland_feedback_rejects_invalid_nanoseconds_flags_and_zero_output_digest() {
        let mut event = WaylandPresentedEvent {
            seconds_hi: 0,
            seconds_lo: 5,
            nanoseconds: 1_000_000_000,
            refresh_interval_ns: 16_666_667,
            sequence_hi: 0,
            sequence_lo: 1,
            flags: 0,
        };
        assert_eq!(
            event.into_observation(1, None),
            Err(ContractError::InvalidPresentationObservation)
        );
        event.nanoseconds = 0;
        event.flags = 0x10;
        assert_eq!(
            event.into_observation(1, None),
            Err(ContractError::InvalidPresentationObservation)
        );
        event.flags = 0;
        assert_eq!(
            event.into_observation(1, Some([0; 32])),
            Err(ContractError::InvalidPresentationObservation)
        );
    }

    #[test]
    fn wayland_feedback_rejects_timestamp_overflow() {
        let event = WaylandPresentedEvent {
            seconds_hi: u32::MAX,
            seconds_lo: u32::MAX,
            nanoseconds: 999_999_999,
            refresh_interval_ns: 0,
            sequence_hi: 0,
            sequence_lo: 0,
            flags: 0,
        };
        assert_eq!(
            event.into_observation(1, None),
            Err(ContractError::PresentationTimestampOverflow)
        );
    }

    #[test]
    fn compositor_hardware_flags_do_not_upgrade_evidence_level() {
        let observation = WaylandPresentedEvent {
            seconds_hi: 0,
            seconds_lo: 1,
            nanoseconds: 2,
            refresh_interval_ns: 0,
            sequence_hi: 0,
            sequence_lo: 0,
            flags: WAYLAND_PRESENTATION_HW_CLOCK
                | WAYLAND_PRESENTATION_HW_COMPLETION
                | WAYLAND_PRESENTATION_ZERO_COPY,
        }
        .into_observation(1, None)
        .unwrap();
        let mut frame = receipt(
            1,
            1,
            PresentationStatus::Presented,
            EvidenceLevel::CompositorFeedback,
            None,
        );
        frame.presentation_source = Some(PresentationSource::WaylandPresentationTimeV2);
        frame.presentation = Some(observation);
        frame.validate().unwrap();
        assert_eq!(frame.evidence_level, EvidenceLevel::CompositorFeedback);
    }

    #[test]
    fn wayland_source_cannot_claim_hardware_or_external_instrument_evidence() {
        let observation = WaylandPresentedEvent {
            seconds_hi: 0,
            seconds_lo: 1,
            nanoseconds: 2,
            refresh_interval_ns: 0,
            sequence_hi: 0,
            sequence_lo: 0,
            flags: WAYLAND_PRESENTATION_HW_CLOCK | WAYLAND_PRESENTATION_HW_COMPLETION,
        }
        .into_observation(1, None)
        .unwrap();
        for level in [EvidenceLevel::HardwareObserved, EvidenceLevel::ExternalInstrument] {
            let mut frame = receipt(
                1,
                1,
                PresentationStatus::Presented,
                level,
                None,
            );
            frame.presentation_source = Some(PresentationSource::WaylandPresentationTimeV2);
            frame.presentation = Some(observation);
            assert_eq!(
                frame.validate(),
                Err(ContractError::UnsupportedPresentationClaim)
            );
        }
        let mut discarded = receipt(
            2,
            1,
            PresentationStatus::Discarded,
            EvidenceLevel::ExternalInstrument,
            None,
        );
        discarded.presentation_source = Some(PresentationSource::WaylandPresentationTimeV2);
        assert_eq!(
            discarded.validate(),
            Err(ContractError::UnsupportedPresentationClaim)
        );
    }

    #[test]
    fn discarded_wayland_feedback_is_source_bound_without_present_time() {
        let mut frame = receipt(
            1,
            1,
            PresentationStatus::Discarded,
            EvidenceLevel::CompositorFeedback,
            None,
        );
        frame.presentation_source = Some(PresentationSource::WaylandPresentationTimeV2);
        assert!(frame.validate().is_ok());
        assert!(frame.canonical_bytes().is_ok());
    }

    #[test]
    fn presentation_observation_canonical_bytes_bind_protocol_feedback() {
        let base = WaylandPresentedEvent {
            seconds_hi: 0,
            seconds_lo: 10,
            nanoseconds: 20,
            refresh_interval_ns: 16_666_667,
            sequence_hi: 0,
            sequence_lo: 30,
            flags: WAYLAND_PRESENTATION_VSYNC,
        }
        .into_observation(1, Some([8; 32]))
        .unwrap();
        let canonical = base.canonical_bytes().unwrap();
        let mut changed = base;
        changed.source_flags |= WAYLAND_PRESENTATION_ZERO_COPY;
        assert_ne!(canonical, changed.canonical_bytes().unwrap());
        changed = base;
        changed.refresh_sequence += 1;
        assert_ne!(canonical, changed.canonical_bytes().unwrap());
        changed = base;
        changed.output_identity_digest = Some([9; 32]);
        assert_ne!(canonical, changed.canonical_bytes().unwrap());
    }

    #[test]
    fn submitted_is_not_presented() {
        let submitted = receipt(
            1,
            1,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        assert_eq!(
            submitted.verify_presented(),
            Err(ContractError::FrameNotPresented)
        );
    }

    #[test]
    fn discarded_frame_cannot_carry_a_presentation_time() {
        let mut invalid = receipt(
            1,
            1,
            PresentationStatus::Discarded,
            EvidenceLevel::CompositorFeedback,
            None,
        );
        invalid.presentation_source = Some(PresentationSource::WaylandPresentationTimeV2);
        invalid.presentation = Some(PresentationObservation {
            source: PresentationSource::WaylandPresentationTimeV2,
            clock_id: 1,
            presentation_time_ns: 10,
            refresh_interval_ns: 16_666_667,
            refresh_sequence: 1,
            output_identity_digest: Some([7; 32]),
            source_flags: WAYLAND_PRESENTATION_VSYNC,
        });
        assert_eq!(
            invalid.validate(),
            Err(ContractError::UnsupportedPresentationClaim)
        );
    }

    fn session_binding() -> SessionBinding {
        SessionBinding {
            entity_id: EntityId([1; 16]),
            entity_revision_root: [2; 32],
            backend_build_digest: [6; 32],
            session_id: [3; 16],
            decision_digest: [5; 32],
            presentation_clock_id: Some(1),
            target_format_id: 1,
            width: 1920,
            height: 1080,
        }
    }

    #[test]
    fn frame_guard_binds_entity_revision_backend_decision_and_output() {
        let mut guard = FrameSequenceGuard::new(session_binding(), 4).unwrap();
        let mut wrong = receipt(
            1,
            4,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        wrong.entity_id = EntityId([9; 16]);
        assert_eq!(
            guard.observe(&wrong),
            Err(ContractError::SessionBindingMismatch)
        );
        wrong.entity_id = EntityId([1; 16]);
        wrong.entity_revision_root = [9; 32];
        assert_eq!(
            guard.observe(&wrong),
            Err(ContractError::SessionBindingMismatch)
        );
        wrong.entity_revision_root = [2; 32];
        wrong.backend_build_digest = [9; 32];
        assert_eq!(
            guard.observe(&wrong),
            Err(ContractError::SessionBindingMismatch)
        );
        wrong.backend_build_digest = [6; 32];
        wrong.decision_digest = [9; 32];
        assert_eq!(
            guard.observe(&wrong),
            Err(ContractError::SessionBindingMismatch)
        );
        wrong.decision_digest = [5; 32];
        wrong.target_format_id = 2;
        assert_eq!(
            guard.observe(&wrong),
            Err(ContractError::SessionBindingMismatch)
        );
        wrong.target_format_id = 1;
        wrong.width = 1280;
        assert_eq!(
            guard.observe(&wrong),
            Err(ContractError::SessionBindingMismatch)
        );
    }

    #[test]
    fn frame_guard_rejects_duplicate_sequence_stale_pool_and_foreign_session() {
        let mut guard = FrameSequenceGuard::new(session_binding(), 4).unwrap();
        let first = receipt(
            1,
            4,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        guard.observe(&first).unwrap();
        assert_eq!(
            guard.observe(&first),
            Err(ContractError::NonMonotonicFrameSequence)
        );
        let mut foreign_session = receipt(
            2,
            4,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        foreign_session.session_id = [9; 16];
        assert_eq!(
            guard.observe(&foreign_session),
            Err(ContractError::SessionBindingMismatch)
        );
        let stale = receipt(
            2,
            3,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        assert_eq!(
            guard.observe(&stale),
            Err(ContractError::StaleBufferGeneration)
        );
        guard.reconfigure(5, 1, 1920, 1080).unwrap();
        let next = receipt(
            2,
            5,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        guard.observe(&next).unwrap();
    }

    #[test]
    fn frame_guard_rejects_mixed_presentation_clock_domains() {
        let mut guard = FrameSequenceGuard::new(session_binding(), 4).unwrap();
        let mut first = receipt(
            1,
            4,
            PresentationStatus::Presented,
            EvidenceLevel::CompositorFeedback,
            Some(10),
        );
        first.presentation_clock_id = None;
        first.presentation.as_mut().unwrap().clock_id = 1;
        guard.observe(&first).unwrap();

        let mut changed_clock = receipt(
            2,
            4,
            PresentationStatus::Presented,
            EvidenceLevel::CompositorFeedback,
            Some(20),
        );
        changed_clock.presentation_clock_id = None;
        changed_clock.presentation.as_mut().unwrap().clock_id = 2;
        assert_eq!(
            guard.observe(&changed_clock),
            Err(ContractError::PresentationClockMismatch)
        );
    }

    #[test]
    fn frame_guard_reconfiguration_requires_new_generation_and_updates_extent() {
        let mut guard = FrameSequenceGuard::new(session_binding(), 4).unwrap();
        assert_eq!(
            guard.reconfigure(4, 1, 1280, 720),
            Err(ContractError::StaleBufferGeneration)
        );
        guard.reconfigure(5, 2, 1280, 720).unwrap();
        let mut next = receipt(
            1,
            5,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        next.target_format_id = 2;
        next.width = 1280;
        next.height = 720;
        guard.observe(&next).unwrap();
    }
}
