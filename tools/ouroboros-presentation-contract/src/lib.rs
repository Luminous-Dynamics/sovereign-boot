#![forbid(unsafe_code)]
//! Dependency-free conformance oracle for Ouroboros persistent identity and
//! negotiated presentation quality.
//!
//! This crate deliberately does not hash, sign, render, access the OS, or
//! query a device. Callers hash the canonical bytes with the project's chosen
//! digest implementation (currently BLAKE3) and attach host/compositor
//! observations at the appropriate trust boundary.

use std::{collections::BTreeMap, fmt};

pub const ENTITY_SCHEMA_VERSION: u16 = 1;
pub const QUALITY_SCHEMA_VERSION: u16 = 1;
pub const CAPABILITY_SCHEMA_VERSION: u16 = 2;
pub const DECISION_SCHEMA_VERSION: u16 = 2;
pub const FRAME_RECEIPT_SCHEMA_VERSION: u16 = 2;

const MAX_FEATURE_IDS: usize = 128;
const MAX_REPRESENTATION_ROOTS: usize = 128;
const MAX_CANDIDATES: usize = 128;
const QUALITY_CANDIDATE_ENCODED_LEN: usize = 128;
pub const MAX_FEEDBACK_LEDGER_ENTRIES: usize = 4_096;

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

/// Conservative absolute error bounds for estimated candidate metrics.
/// These are declared planning bounds, not observed telemetry or statistical
/// confidence intervals unless the host provides a defensible calibration.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct QualityUncertainty {
    pub vertices: u32,
    pub indices: u32,
    pub geometry_bytes: u32,
    pub frame_time_ns: u64,
    pub cpu_time_ns: u64,
    pub gpu_time_ns: u64,
    pub cpu_memory_bytes: u64,
    pub gpu_memory_bytes: u64,
    pub feature_diameter_millipixels: u32,
    pub silhouette_error_microunits: u32,
}

impl QualityUncertainty {
    fn encode_into(&self, out: &mut Vec<u8>) {
        put_u32(out, self.vertices);
        put_u32(out, self.indices);
        put_u32(out, self.geometry_bytes);
        put_u64(out, self.frame_time_ns);
        put_u64(out, self.cpu_time_ns);
        put_u64(out, self.gpu_time_ns);
        put_u64(out, self.cpu_memory_bytes);
        put_u64(out, self.gpu_memory_bytes);
        put_u32(out, self.feature_diameter_millipixels);
        put_u32(out, self.silhouette_error_microunits);
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
    /// None means uncertainty is unknown. Point-estimate policy may still
    /// compare the nominal values; conservative-interval policy will not.
    pub uncertainty: Option<QualityUncertainty>,
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
        match self.uncertainty {
            Some(uncertainty) => {
                out.push(1);
                uncertainty.encode_into(out);
            }
            None => out.push(0),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CapabilitySnapshot {
    pub schema_version: u16,
    pub revision: u64,
    /// Host array order is not semantic. Candidate IDs and preference ranks
    /// must be unique; canonical bytes sort by stable candidate_id.
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
        let mut candidate_ids = std::collections::BTreeSet::new();
        let mut preference_ranks = std::collections::BTreeSet::new();
        for candidate in &self.candidates {
            candidate.validate()?;
            if !candidate_ids.insert(candidate.candidate_id)
                || !preference_ranks.insert(candidate.preference_rank)
            {
                return Err(ContractError::InvalidCapabilities);
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut out = Vec::with_capacity(8 + 2 + 8 + 2 + self.candidates.len() * QUALITY_CANDIDATE_ENCODED_LEN);
        out.extend_from_slice(b"OUROCAP2");
        put_u16(&mut out, self.schema_version);
        put_u64(&mut out, self.revision);
        put_u16(&mut out, self.candidates.len() as u16);
        let mut candidates: Vec<&QualityCandidate> = self.candidates.iter().collect();
        candidates.sort_by_key(|candidate| candidate.candidate_id);
        for candidate in candidates {
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
        out.extend_from_slice(b"OURODEC2");
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
        .filter(|candidate| candidate.is_feasible_for(request))
        .min_by_key(|candidate| (candidate.preference_rank, candidate.candidate_id));
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

/// Recompute a decision from the original request/capability snapshot and
/// compare every canonical field. Use this at a host/renderer trust boundary
/// rather than accepting a serialized selected candidate at face value.
pub fn verify_quality_decision(
    request: &QualityRequest,
    capabilities: &CapabilitySnapshot,
    policy_version: u16,
    decision: &QualityDecision,
) -> Result<(), ContractError> {
    let expected = negotiate(request, capabilities, policy_version)?;
    let expected_bytes = expected.canonical_bytes()?;
    let supplied_bytes = decision.canonical_bytes()?;
    if expected_bytes != supplied_bytes {
        return Err(ContractError::DecisionMismatch);
    }
    Ok(())
}

/// Objective comparison mode for Pareto policy. Point estimates are
/// explicitly nominal. ConservativeIntervals requires both candidates to
/// declare uncertainty bounds; unknown uncertainty is never silently treated
/// as zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DominanceMode {
    PointEstimate = 1,
    ConservativeIntervals = 2,
    EpsilonGrid = 3,
}

/// Additive bucket widths in the exact units of each objective. These are
/// comparison tolerances, not uncertainty estimates and not weights.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParetoEpsilon {
    pub vertices: u32,
    pub indices: u32,
    pub geometry_bytes: u32,
    pub frame_time_ns: u64,
    pub cpu_time_ns: u64,
    pub gpu_time_ns: u64,
    pub cpu_memory_bytes: u64,
    pub gpu_memory_bytes: u64,
    pub feature_diameter_millipixels: u32,
    pub silhouette_error_microunits: u32,
}

impl ParetoEpsilon {
    fn validate(&self) -> Result<(), ContractError> {
        if self.vertices == 0
            || self.indices == 0
            || self.geometry_bytes == 0
            || self.frame_time_ns == 0
            || self.cpu_time_ns == 0
            || self.gpu_time_ns == 0
            || self.cpu_memory_bytes == 0
            || self.gpu_memory_bytes == 0
            || self.feature_diameter_millipixels == 0
            || self.silhouette_error_microunits == 0
        {
            return Err(ContractError::InvalidParetoPolicy);
        }
        Ok(())
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        put_u32(out, self.vertices);
        put_u32(out, self.indices);
        put_u32(out, self.geometry_bytes);
        put_u64(out, self.frame_time_ns);
        put_u64(out, self.cpu_time_ns);
        put_u64(out, self.gpu_time_ns);
        put_u64(out, self.cpu_memory_bytes);
        put_u64(out, self.gpu_memory_bytes);
        put_u32(out, self.feature_diameter_millipixels);
        put_u32(out, self.silhouette_error_microunits);
    }
}

pub const PARETO_POLICY_SCHEMA_VERSION: u16 = 2;
pub const PARETO_DECISION_SCHEMA_VERSION: u16 = 2;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParetoPolicy {
    pub schema_version: u16,
    pub policy_version: u16,
    pub dominance_mode: DominanceMode,
    /// Required exactly for EpsilonGrid; omitted for point/interval modes.
    pub epsilon: Option<ParetoEpsilon>,
}

impl ParetoPolicy {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.schema_version != PARETO_POLICY_SCHEMA_VERSION || self.policy_version == 0 {
            return Err(ContractError::InvalidParetoPolicy);
        }
        match (self.dominance_mode, self.epsilon) {
            (DominanceMode::EpsilonGrid, Some(epsilon)) => epsilon.validate()?,
            (DominanceMode::EpsilonGrid, None) => return Err(ContractError::InvalidParetoPolicy),
            (_, Some(_)) => return Err(ContractError::InvalidParetoPolicy),
            (_, None) => {}
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        self.validate()?;
        let mut out = Vec::with_capacity(8 + 2 + 2 + 1 + 1 + 60);
        out.extend_from_slice(b"OUROPOL2");
        put_u16(&mut out, self.schema_version);
        put_u16(&mut out, self.policy_version);
        out.push(self.dominance_mode as u8);
        match self.epsilon {
            Some(epsilon) => {
                out.push(1);
                epsilon.encode_into(&mut out);
            }
            None => out.push(0),
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CandidateDisposition {
    Infeasible = 1,
    Indeterminate = 2,
    Dominated = 3,
    Frontier = 4,
    Selected = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum DominanceReason {
    StrictlyBetterObjectiveVector = 1,
    EquivalentVectorStableIdTieBreak = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ParetoInfeasibleReason {
    NoCandidateSatisfiesHardConstraints = 1,
    NoCandidateHasRequiredUncertainty = 2,
}

pub const REJECT_VERTICES: u16 = 1 << 0;
pub const REJECT_INDICES: u16 = 1 << 1;
pub const REJECT_GEOMETRY_BYTES: u16 = 1 << 2;
pub const REJECT_FRAME_TIME: u16 = 1 << 3;
pub const REJECT_CPU_TIME: u16 = 1 << 4;
pub const REJECT_GPU_TIME: u16 = 1 << 5;
pub const REJECT_CPU_MEMORY: u16 = 1 << 6;
pub const REJECT_GPU_MEMORY: u16 = 1 << 7;
pub const REJECT_FEATURE_DIAMETER: u16 = 1 << 8;
pub const REJECT_SILHOUETTE_ERROR: u16 = 1 << 9;
pub const REJECT_REDUCED_MOTION: u16 = 1 << 10;

/// One reproducible explanation for every candidate in the snapshot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CandidateEvaluation {
    pub candidate_id: u16,
    pub disposition: CandidateDisposition,
    pub hard_rejection_flags: u16,
    /// True means conservative comparison is unavailable because this
    /// candidate has no declared uncertainty vector.
    pub uncertainty_unknown: bool,
    pub dominated_by_candidate_id: Option<u16>,
    pub dominance_reason: Option<DominanceReason>,
    pub degradation_flags: u8,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParetoDecision {
    schema_version: u16,
    policy_version: u16,
    capability_revision: u64,
    request_bytes: Vec<u8>,
    capability_bytes: Vec<u8>,
    policy_bytes: Vec<u8>,
    /// Sorted by candidate_id, independent of host input ordering.
    evaluations: Vec<CandidateEvaluation>,
    /// Frontier IDs are sorted ascending, not by host preference.
    frontier_candidate_ids: Vec<u16>,
    selected_candidate_id: Option<u16>,
    infeasible_reason: Option<ParetoInfeasibleReason>,
    degradation_flags: u8,
}

impl ParetoDecision {
    pub fn evaluations(&self) -> &[CandidateEvaluation] {
        &self.evaluations
    }

    pub fn frontier_candidate_ids(&self) -> &[u16] {
        &self.frontier_candidate_ids
    }

    pub fn selected_candidate_id(&self) -> Option<u16> {
        self.selected_candidate_id
    }

    pub fn infeasible_reason(&self) -> Option<ParetoInfeasibleReason> {
        self.infeasible_reason
    }

    pub fn degradation_flags(&self) -> u8 {
        self.degradation_flags
    }

    pub fn is_feasible(&self) -> bool {
        self.selected_candidate_id.is_some() && self.infeasible_reason.is_none()
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ContractError> {
        if self.schema_version != PARETO_DECISION_SCHEMA_VERSION
            || self.policy_version == 0
            || self.capability_revision == 0
            || (self.selected_candidate_id.is_some() == self.infeasible_reason.is_some())
            || self.degradation_flags & !0b11 != 0
            || self.evaluations.len() > MAX_CANDIDATES
            || self.frontier_candidate_ids.len() > self.evaluations.len()
            || self.evaluations.windows(2).any(|pair| {
                pair[0].candidate_id >= pair[1].candidate_id
            })
            || self.frontier_candidate_ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(ContractError::InvalidParetoDecision);
        }
        let request_len = u32::try_from(self.request_bytes.len())
            .map_err(|_| ContractError::CollectionTooLarge)?;
        let capability_len = u32::try_from(self.capability_bytes.len())
            .map_err(|_| ContractError::CollectionTooLarge)?;
        let policy_len = u32::try_from(self.policy_bytes.len())
            .map_err(|_| ContractError::CollectionTooLarge)?;
        let mut out = Vec::new();
        out.try_reserve_exact(
            8 + 2 + 2 + 8 + 4 + self.request_bytes.len()
                + 4 + self.capability_bytes.len()
                + 4 + self.policy_bytes.len()
                + 2 + self.evaluations.len() * 13
                + 2 + self.frontier_candidate_ids.len() * 2
                + 1 + 2,
        )
        .map_err(|_| ContractError::CollectionTooLarge)?;
        out.extend_from_slice(b"OUROPDE2");
        put_u16(&mut out, self.schema_version);
        put_u16(&mut out, self.policy_version);
        put_u64(&mut out, self.capability_revision);
        put_u32(&mut out, request_len);
        out.extend_from_slice(&self.request_bytes);
        put_u32(&mut out, capability_len);
        out.extend_from_slice(&self.capability_bytes);
        put_u32(&mut out, policy_len);
        out.extend_from_slice(&self.policy_bytes);
        put_u16(&mut out, self.evaluations.len() as u16);
        for evaluation in &self.evaluations {
            put_u16(&mut out, evaluation.candidate_id);
            out.push(evaluation.disposition as u8);
            put_u16(&mut out, evaluation.hard_rejection_flags);
            out.push(u8::from(evaluation.uncertainty_unknown));
            match evaluation.dominated_by_candidate_id {
                Some(id) => {
                    out.push(1);
                    put_u16(&mut out, id);
                }
                None => out.push(0),
            }
            out.push(match evaluation.dominance_reason {
                Some(reason) => reason as u8,
                None => 0,
            });
            out.push(evaluation.degradation_flags);
        }
        put_u16(&mut out, self.frontier_candidate_ids.len() as u16);
        for candidate_id in &self.frontier_candidate_ids {
            put_u16(&mut out, *candidate_id);
        }
        match self.selected_candidate_id {
            Some(id) => {
                out.push(1);
                put_u16(&mut out, id);
            }
            None => {
                out.push(0);
                put_u16(&mut out, 0);
            }
        }
        out.push(match self.infeasible_reason {
            Some(reason) => reason as u8,
            None => 0,
        });
        out.push(self.degradation_flags);
        Ok(out)
    }
}

fn rejection_flags(
    candidate: &QualityCandidate,
    request: &QualityRequest,
    mode: DominanceMode,
) -> (u16, bool) {
    if mode == DominanceMode::ConservativeIntervals && candidate.uncertainty.is_none() {
        let reduced_motion = request.reduced_motion && candidate.animated;
        return (
            if reduced_motion { REJECT_REDUCED_MOTION } else { 0 },
            true,
        );
    }
    let uncertainty = candidate.uncertainty.unwrap_or(QualityUncertainty {
        vertices: 0,
        indices: 0,
        geometry_bytes: 0,
        frame_time_ns: 0,
        cpu_time_ns: 0,
        gpu_time_ns: 0,
        cpu_memory_bytes: 0,
        gpu_memory_bytes: 0,
        feature_diameter_millipixels: 0,
        silhouette_error_microunits: 0,
    });
    let conservative = mode == DominanceMode::ConservativeIntervals;
    let upper = |value: u64, margin: u64| {
        if conservative { value as u128 + margin as u128 } else { value as u128 }
    };
    let lower = |value: u64, margin: u64| {
        if conservative { (value as u128).saturating_sub(margin as u128) } else { value as u128 }
    };
    let h = request.hard;
    let mut flags = 0;
    if upper(candidate.estimated_vertices as u64, uncertainty.vertices as u64) > h.max_vertices as u128 {
        flags |= REJECT_VERTICES;
    }
    if upper(candidate.estimated_indices as u64, uncertainty.indices as u64) > h.max_indices as u128 {
        flags |= REJECT_INDICES;
    }
    if upper(candidate.estimated_geometry_bytes as u64, uncertainty.geometry_bytes as u64) > h.max_geometry_bytes as u128 {
        flags |= REJECT_GEOMETRY_BYTES;
    }
    if upper(candidate.estimated_frame_time_ns, uncertainty.frame_time_ns) > h.max_frame_time_ns as u128 {
        flags |= REJECT_FRAME_TIME;
    }
    if upper(candidate.estimated_cpu_time_ns, uncertainty.cpu_time_ns) > h.max_cpu_time_ns as u128 {
        flags |= REJECT_CPU_TIME;
    }
    if upper(candidate.estimated_gpu_time_ns, uncertainty.gpu_time_ns) > h.max_gpu_time_ns as u128 {
        flags |= REJECT_GPU_TIME;
    }
    if upper(candidate.estimated_cpu_memory_bytes, uncertainty.cpu_memory_bytes) > h.max_cpu_memory_bytes as u128 {
        flags |= REJECT_CPU_MEMORY;
    }
    if upper(candidate.estimated_gpu_memory_bytes, uncertainty.gpu_memory_bytes) > h.max_gpu_memory_bytes as u128 {
        flags |= REJECT_GPU_MEMORY;
    }
    if lower(
        candidate.feature_diameter_millipixels as u64,
        uncertainty.feature_diameter_millipixels as u64,
    ) < h.min_feature_diameter_millipixels as u128
    {
        flags |= REJECT_FEATURE_DIAMETER;
    }
    if upper(
        candidate.silhouette_error_microunits as u64,
        uncertainty.silhouette_error_microunits as u64,
    ) > h.max_silhouette_error_microunits as u128
    {
        flags |= REJECT_SILHOUETTE_ERROR;
    }
    if request.reduced_motion && candidate.animated {
        flags |= REJECT_REDUCED_MOTION;
    }
    (flags, false)
}

fn same_nominal_objective_vector(a: &QualityCandidate, b: &QualityCandidate) -> bool {
    a.target_format_id == b.target_format_id
        && a.animated == b.animated
        && a.estimated_vertices == b.estimated_vertices
        && a.estimated_indices == b.estimated_indices
        && a.estimated_geometry_bytes == b.estimated_geometry_bytes
        && a.estimated_frame_time_ns == b.estimated_frame_time_ns
        && a.estimated_cpu_time_ns == b.estimated_cpu_time_ns
        && a.estimated_gpu_time_ns == b.estimated_gpu_time_ns
        && a.estimated_cpu_memory_bytes == b.estimated_cpu_memory_bytes
        && a.estimated_gpu_memory_bytes == b.estimated_gpu_memory_bytes
        && a.feature_diameter_millipixels == b.feature_diameter_millipixels
        && a.silhouette_error_microunits == b.silhouette_error_microunits
}

fn dominates(
    a: &QualityCandidate,
    b: &QualityCandidate,
    policy: &ParetoPolicy,
) -> Option<DominanceReason> {
    let mode = policy.dominance_mode;
    if a.candidate_id == b.candidate_id
        || a.target_format_id != b.target_format_id
        || a.animated != b.animated
    {
        return None;
    }
    if same_nominal_objective_vector(a, b)
        && (mode != DominanceMode::ConservativeIntervals || a.uncertainty == b.uncertainty)
    {
        return (a.candidate_id < b.candidate_id)
            .then_some(DominanceReason::EquivalentVectorStableIdTieBreak);
    }

    if mode == DominanceMode::EpsilonGrid {
        let epsilon = policy.epsilon?;
        let cost_pairs = [
            (a.estimated_vertices / epsilon.vertices, b.estimated_vertices / epsilon.vertices),
            (a.estimated_indices / epsilon.indices, b.estimated_indices / epsilon.indices),
            (a.estimated_geometry_bytes / epsilon.geometry_bytes, b.estimated_geometry_bytes / epsilon.geometry_bytes),
            ((a.estimated_frame_time_ns / epsilon.frame_time_ns), (b.estimated_frame_time_ns / epsilon.frame_time_ns)),
            ((a.estimated_cpu_time_ns / epsilon.cpu_time_ns), (b.estimated_cpu_time_ns / epsilon.cpu_time_ns)),
            ((a.estimated_gpu_time_ns / epsilon.gpu_time_ns), (b.estimated_gpu_time_ns / epsilon.gpu_time_ns)),
            ((a.estimated_cpu_memory_bytes / epsilon.cpu_memory_bytes), (b.estimated_cpu_memory_bytes / epsilon.cpu_memory_bytes)),
            ((a.estimated_gpu_memory_bytes / epsilon.gpu_memory_bytes), (b.estimated_gpu_memory_bytes / epsilon.gpu_memory_bytes)),
            (a.silhouette_error_microunits / epsilon.silhouette_error_microunits, b.silhouette_error_microunits / epsilon.silhouette_error_microunits),
        ];
        let feature_a = a.feature_diameter_millipixels / epsilon.feature_diameter_millipixels;
        let feature_b = b.feature_diameter_millipixels / epsilon.feature_diameter_millipixels;
        if cost_pairs.iter().any(|(a_bucket, b_bucket)| a_bucket > b_bucket)
            || feature_a < feature_b
        {
            return None;
        }
        let strict = cost_pairs.iter().any(|(a_bucket, b_bucket)| a_bucket < b_bucket)
            || feature_a > feature_b;
        if strict {
            return Some(DominanceReason::StrictlyBetterObjectiveVector);
        }
        return (a.candidate_id < b.candidate_id)
            .then_some(DominanceReason::EquivalentVectorStableIdTieBreak);
    }

    if mode == DominanceMode::ConservativeIntervals
        && (a.uncertainty.is_none() || b.uncertainty.is_none())
    {
        return None;
    }
    let au = a.uncertainty.unwrap_or(QualityUncertainty {
        vertices: 0, indices: 0, geometry_bytes: 0, frame_time_ns: 0,
        cpu_time_ns: 0, gpu_time_ns: 0, cpu_memory_bytes: 0, gpu_memory_bytes: 0,
        feature_diameter_millipixels: 0, silhouette_error_microunits: 0,
    });
    let bu = b.uncertainty.unwrap_or(QualityUncertainty {
        vertices: 0, indices: 0, geometry_bytes: 0, frame_time_ns: 0,
        cpu_time_ns: 0, gpu_time_ns: 0, cpu_memory_bytes: 0, gpu_memory_bytes: 0,
        feature_diameter_millipixels: 0, silhouette_error_microunits: 0,
    });
    let conservative = mode == DominanceMode::ConservativeIntervals;
    let upper = |value: u64, margin: u64| {
        if conservative { value as u128 + margin as u128 } else { value as u128 }
    };
    let lower = |value: u64, margin: u64| {
        if conservative { (value as u128).saturating_sub(margin as u128) } else { value as u128 }
    };

    let cost_pairs = [
        (upper(a.estimated_vertices as u64, au.vertices as u64), lower(b.estimated_vertices as u64, bu.vertices as u64)),
        (upper(a.estimated_indices as u64, au.indices as u64), lower(b.estimated_indices as u64, bu.indices as u64)),
        (upper(a.estimated_geometry_bytes as u64, au.geometry_bytes as u64), lower(b.estimated_geometry_bytes as u64, bu.geometry_bytes as u64)),
        (upper(a.estimated_frame_time_ns, au.frame_time_ns), lower(b.estimated_frame_time_ns, bu.frame_time_ns)),
        (upper(a.estimated_cpu_time_ns, au.cpu_time_ns), lower(b.estimated_cpu_time_ns, bu.cpu_time_ns)),
        (upper(a.estimated_gpu_time_ns, au.gpu_time_ns), lower(b.estimated_gpu_time_ns, bu.gpu_time_ns)),
        (upper(a.estimated_cpu_memory_bytes, au.cpu_memory_bytes), lower(b.estimated_cpu_memory_bytes, bu.cpu_memory_bytes)),
        (upper(a.estimated_gpu_memory_bytes, au.gpu_memory_bytes), lower(b.estimated_gpu_memory_bytes, bu.gpu_memory_bytes)),
        (upper(a.silhouette_error_microunits as u64, au.silhouette_error_microunits as u64), lower(b.silhouette_error_microunits as u64, bu.silhouette_error_microunits as u64)),
    ];
    let feature_lower = lower(
        a.feature_diameter_millipixels as u64,
        au.feature_diameter_millipixels as u64,
    );
    let feature_upper_b = upper(
        b.feature_diameter_millipixels as u64,
        bu.feature_diameter_millipixels as u64,
    );
    if cost_pairs.iter().any(|(a_upper, b_lower)| a_upper > b_lower)
        || feature_lower < feature_upper_b
    {
        return None;
    }
    let strict = cost_pairs.iter().any(|(a_upper, b_lower)| a_upper < b_lower)
        || feature_lower > feature_upper_b;
    strict.then_some(DominanceReason::StrictlyBetterObjectiveVector)
}

pub fn negotiate_pareto(
    request: &QualityRequest,
    capabilities: &CapabilitySnapshot,
    policy: &ParetoPolicy,
) -> Result<ParetoDecision, ContractError> {
    request.validate()?;
    capabilities.validate()?;
    policy.validate()?;
    let request_bytes = request.canonical_bytes()?;
    let capability_bytes = capabilities.canonical_bytes()?;
    let policy_bytes = policy.canonical_bytes()?;

    let mut candidate_refs: Vec<&QualityCandidate> = capabilities.candidates.iter().collect();
    candidate_refs.sort_by_key(|candidate| candidate.candidate_id);

    let mut evaluations = Vec::with_capacity(candidate_refs.len());
    let mut eligible = Vec::new();
    let mut any_unknown = false;
    for candidate in &candidate_refs {
        let (hard_rejection_flags, uncertainty_unknown) =
            rejection_flags(candidate, request, policy.dominance_mode);
        if hard_rejection_flags != 0 {
            evaluations.push(CandidateEvaluation {
                candidate_id: candidate.candidate_id,
                disposition: CandidateDisposition::Infeasible,
                hard_rejection_flags,
                uncertainty_unknown,
                dominated_by_candidate_id: None,
                dominance_reason: None,
                degradation_flags: 0,
            });
        } else if uncertainty_unknown {
            any_unknown = true;
            evaluations.push(CandidateEvaluation {
                candidate_id: candidate.candidate_id,
                disposition: CandidateDisposition::Indeterminate,
                hard_rejection_flags: 0,
                uncertainty_unknown: true,
                dominated_by_candidate_id: None,
                dominance_reason: None,
                degradation_flags: 0,
            });
        } else {
            eligible.push(*candidate);
            evaluations.push(CandidateEvaluation {
                candidate_id: candidate.candidate_id,
                disposition: CandidateDisposition::Frontier,
                hard_rejection_flags: 0,
                uncertainty_unknown: false,
                dominated_by_candidate_id: None,
                dominance_reason: None,
                degradation_flags: candidate.degradation_flags(request),
            });
        }
    }

    for candidate in &eligible {
        let mut dominators = Vec::new();
        for other in &eligible {
            if let Some(reason) = dominates(other, candidate, policy) {
                dominators.push((other.candidate_id, reason));
            }
        }
        if let Some((dominator_id, reason)) = dominators.into_iter().min_by_key(|entry| entry.0) {
            let evaluation = evaluations
                .iter_mut()
                .find(|evaluation| evaluation.candidate_id == candidate.candidate_id)
                .ok_or(ContractError::InvalidParetoDecision)?;
            evaluation.disposition = CandidateDisposition::Dominated;
            evaluation.dominated_by_candidate_id = Some(dominator_id);
            evaluation.dominance_reason = Some(reason);
        }
    }

    let mut frontier_candidate_ids: Vec<u16> = evaluations
        .iter()
        .filter(|evaluation| evaluation.disposition == CandidateDisposition::Frontier)
        .map(|evaluation| evaluation.candidate_id)
        .collect();
    frontier_candidate_ids.sort_unstable();

    let selected_candidate = eligible
        .iter()
        .copied()
        .filter(|candidate| frontier_candidate_ids.binary_search(&candidate.candidate_id).is_ok())
        .min_by_key(|candidate| (candidate.preference_rank, candidate.candidate_id));
    let selected_candidate_id = selected_candidate.map(|candidate| candidate.candidate_id);
    let degradation_flags = selected_candidate
        .map(|candidate| candidate.degradation_flags(request))
        .unwrap_or(0);
    if let Some(selected_id) = selected_candidate_id {
        let evaluation = evaluations
            .iter_mut()
            .find(|evaluation| evaluation.candidate_id == selected_id)
            .ok_or(ContractError::InvalidParetoDecision)?;
        evaluation.disposition = CandidateDisposition::Selected;
    }
    let infeasible_reason = if selected_candidate_id.is_some() {
        None
    } else if any_unknown {
        Some(ParetoInfeasibleReason::NoCandidateHasRequiredUncertainty)
    } else {
        Some(ParetoInfeasibleReason::NoCandidateSatisfiesHardConstraints)
    };

    Ok(ParetoDecision {
        schema_version: PARETO_DECISION_SCHEMA_VERSION,
        policy_version: policy.policy_version,
        capability_revision: capabilities.revision,
        request_bytes,
        capability_bytes,
        policy_bytes,
        evaluations,
        frontier_candidate_ids,
        selected_candidate_id,
        infeasible_reason,
        degradation_flags,
    })
}

pub fn verify_pareto_decision(
    request: &QualityRequest,
    capabilities: &CapabilitySnapshot,
    policy: &ParetoPolicy,
    decision: &ParetoDecision,
) -> Result<(), ContractError> {
    let expected = negotiate_pareto(request, capabilities, policy)?;
    if expected.canonical_bytes()? != decision.canonical_bytes()? {
        return Err(ContractError::ParetoDecisionMismatch);
    }
    Ok(())
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
    /// Connection-level presentation clock. Present even on Submitted,
    /// Discarded or Unknown receipts when the host already knows the clock.
    pub presentation_clock_id: Option<u32>,
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
            || self.entity_revision_root.iter().all(|byte| *byte == 0)
            || self.backend_build_digest.iter().all(|byte| *byte == 0)
            || self.state_digest.iter().all(|byte| *byte == 0)
            || self.decision_digest.iter().all(|byte| *byte == 0)
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
            if self.presentation_clock_id != Some(presentation.clock_id) {
                return Err(ContractError::PresentationClockMismatch);
            }
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
        let mut out = Vec::with_capacity(8 + 2 + 16 + 32 * 4 + 16 + 8 * 2 + 2 + 4 * 2 + 8 + 61);
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
        match self.presentation_clock_id {
            Some(clock_id) => {
                out.push(1);
                put_u32(&mut out, clock_id);
            }
            None => out.push(0),
        }
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

/// One terminal compositor outcome for a submitted surface commit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalFeedback {
    Presented(PresentationObservation),
    Discarded(PresentationSource),
}

impl TerminalFeedback {
    fn validate(self) -> Result<(), ContractError> {
        match self {
            Self::Presented(observation) => observation.validate(),
            Self::Discarded(_) => Ok(()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FeedbackRecordResult {
    Recorded,
    DuplicateIdentical,
}

/// Bounded, session-scoped correlation ledger with explicit retirement.
///
/// A frame sequence is registered at the surface-commit boundary. Identical
/// terminal callbacks are idempotent; conflicting outcomes are rejected.
/// Completed entries may be retired only through an explicit watermark. The
/// watermark survives retirement, so a late callback for an old frame cannot
/// be rebound to a newer frame after its detail entry has been removed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PresentationFeedbackLedger {
    session_id: [u8; 16],
    capacity: usize,
    last_registered_sequence: u64,
    retired_through_sequence: u64,
    expected_clock_id: Option<u32>,
    entries: BTreeMap<u64, Option<TerminalFeedback>>,
}

impl PresentationFeedbackLedger {
    pub fn new(
        session_id: [u8; 16],
        presentation_clock_id: Option<u32>,
        capacity: usize,
    ) -> Result<Self, ContractError> {
        if session_id.iter().all(|byte| *byte == 0) {
            return Err(ContractError::SessionMismatch);
        }
        if capacity == 0 || capacity > MAX_FEEDBACK_LEDGER_ENTRIES {
            return Err(ContractError::InvalidFeedbackLedgerCapacity);
        }
        Ok(Self {
            session_id,
            capacity,
            last_registered_sequence: 0,
            retired_through_sequence: 0,
            expected_clock_id: presentation_clock_id,
            entries: BTreeMap::new(),
        })
    }

    fn validate_session(&self, session_id: [u8; 16]) -> Result<(), ContractError> {
        if session_id != self.session_id {
            return Err(ContractError::SessionMismatch);
        }
        Ok(())
    }

    /// Register one host-owned surface commit before accepting its feedback.
    pub fn register_commit(
        &mut self,
        session_id: [u8; 16],
        frame_sequence: u64,
    ) -> Result<(), ContractError> {
        self.validate_session(session_id)?;
        if frame_sequence == 0 || frame_sequence <= self.last_registered_sequence {
            return Err(ContractError::NonMonotonicFrameSequence);
        }
        if self.entries.len() >= self.capacity {
            return Err(ContractError::FeedbackLedgerCapacityExceeded);
        }
        self.entries.insert(frame_sequence, None);
        self.last_registered_sequence = frame_sequence;
        Ok(())
    }

    pub fn record_presented(
        &mut self,
        session_id: [u8; 16],
        frame_sequence: u64,
        observation: PresentationObservation,
    ) -> Result<FeedbackRecordResult, ContractError> {
        self.validate_session(session_id)?;
        observation.validate()?;
        if frame_sequence <= self.retired_through_sequence {
            return Err(ContractError::RetiredFrameCommit);
        }
        if !self.entries.contains_key(&frame_sequence) {
            return Err(ContractError::UnknownFrameCommit);
        }
        match self.expected_clock_id {
            Some(clock_id) if clock_id != observation.clock_id => {
                return Err(ContractError::PresentationClockMismatch);
            }
            None => self.expected_clock_id = Some(observation.clock_id),
            _ => {}
        }
        self.record_terminal(
            frame_sequence,
            TerminalFeedback::Presented(observation),
        )
    }

    pub fn record_discarded(
        &mut self,
        session_id: [u8; 16],
        frame_sequence: u64,
        source: PresentationSource,
    ) -> Result<FeedbackRecordResult, ContractError> {
        self.validate_session(session_id)?;
        self.record_terminal(frame_sequence, TerminalFeedback::Discarded(source))
    }

    fn record_terminal(
        &mut self,
        frame_sequence: u64,
        outcome: TerminalFeedback,
    ) -> Result<FeedbackRecordResult, ContractError> {
        outcome.validate()?;
        if frame_sequence <= self.retired_through_sequence {
            return Err(ContractError::RetiredFrameCommit);
        }
        let slot = self
            .entries
            .get_mut(&frame_sequence)
            .ok_or(ContractError::UnknownFrameCommit)?;
        match slot {
            Some(previous) if *previous == outcome => {
                Ok(FeedbackRecordResult::DuplicateIdentical)
            }
            Some(_) => Err(ContractError::ConflictingTerminalFeedback),
            None => {
                *slot = Some(outcome);
                Ok(FeedbackRecordResult::Recorded)
            }
        }
    }

    /// Returns Ok(None) for a registered commit whose compositor outcome has
    /// not arrived yet. Unknown and retired sequences are distinct failures.
    pub fn outcome(
        &self,
        session_id: [u8; 16],
        frame_sequence: u64,
    ) -> Result<Option<TerminalFeedback>, ContractError> {
        self.validate_session(session_id)?;
        if frame_sequence <= self.retired_through_sequence {
            return Err(ContractError::RetiredFrameCommit);
        }
        self.entries
            .get(&frame_sequence)
            .copied()
            .ok_or(ContractError::UnknownFrameCommit)
    }

    /// Retire a completed prefix to bound long-lived sessions. Every registered
    /// commit at or below the watermark must already have a terminal outcome.
    /// Sequence and clock watermarks are retained after the detailed entries
    /// are pruned. A late callback for a retired sequence is rejected.
    pub fn retire_through(
        &mut self,
        session_id: [u8; 16],
        frame_sequence: u64,
    ) -> Result<usize, ContractError> {
        self.validate_session(session_id)?;
        if frame_sequence <= self.retired_through_sequence {
            return Err(ContractError::RetiredFrameCommit);
        }
        if frame_sequence > self.last_registered_sequence {
            return Err(ContractError::UnknownFrameCommit);
        }
        if self
            .entries
            .range(..=frame_sequence)
            .any(|(_, outcome)| outcome.is_none())
        {
            return Err(ContractError::PendingFrameFeedback);
        }
        let before = self.entries.len();
        self.entries.retain(|sequence, _| *sequence > frame_sequence);
        self.retired_through_sequence = frame_sequence;
        Ok(before - self.entries.len())
    }

    pub fn session_id(&self) -> [u8; 16] {
        self.session_id
    }

    pub fn retired_through_sequence(&self) -> u64 {
        self.retired_through_sequence
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
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
        if let Some(expected_clock_id) = self.binding.presentation_clock_id {
            if receipt.presentation_clock_id != Some(expected_clock_id) {
                return Err(ContractError::PresentationClockMismatch);
            }
        }
        if let Some(receipt_clock_id) = receipt.presentation_clock_id {
            match self.observed_presentation_clock_id {
                Some(clock_id) if clock_id != receipt_clock_id => {
                    return Err(ContractError::PresentationClockMismatch);
                }
                None => self.observed_presentation_clock_id = Some(receipt_clock_id),
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
    DecisionMismatch,
    InvalidParetoPolicy,
    InvalidParetoDecision,
    ParetoDecisionMismatch,
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
    InvalidFeedbackLedgerCapacity,
    FeedbackLedgerCapacityExceeded,
    UnknownFrameCommit,
    ConflictingTerminalFeedback,
    RetiredFrameCommit,
    PendingFrameFeedback,
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
            uncertainty: None,
        }
    }

    fn capabilities(candidates: Vec<QualityCandidate>) -> CapabilitySnapshot {
        CapabilitySnapshot {
            schema_version: CAPABILITY_SCHEMA_VERSION,
            revision: 7,
            candidates,
        }
    }

    fn pareto_policy(dominance_mode: DominanceMode) -> ParetoPolicy {
        ParetoPolicy {
            schema_version: PARETO_POLICY_SCHEMA_VERSION,
            policy_version: 1,
            dominance_mode,
            epsilon: if dominance_mode == DominanceMode::EpsilonGrid {
                Some(ParetoEpsilon {
                    vertices: 1_000,
                    indices: 3_000,
                    geometry_bytes: 100_000,
                    frame_time_ns: 1_000_000,
                    cpu_time_ns: 500_000,
                    gpu_time_ns: 500_000,
                    cpu_memory_bytes: 100_000,
                    gpu_memory_bytes: 100_000,
                    feature_diameter_millipixels: 100,
                    silhouette_error_microunits: 1_000,
                })
            } else {
                None
            },
        }
    }

    fn zero_uncertainty() -> QualityUncertainty {
        QualityUncertainty {
            vertices: 0,
            indices: 0,
            geometry_bytes: 0,
            frame_time_ns: 0,
            cpu_time_ns: 0,
            gpu_time_ns: 0,
            cpu_memory_bytes: 0,
            gpu_memory_bytes: 0,
            feature_diameter_millipixels: 0,
            silhouette_error_microunits: 0,
        }
    }

    fn quality_candidate(
        id: u16,
        rank: u16,
        vertices: u32,
        indices: u32,
        geometry_bytes: u32,
        frame_time_ns: u64,
        cpu_time_ns: u64,
        gpu_time_ns: u64,
        cpu_memory_bytes: u64,
        gpu_memory_bytes: u64,
        feature_diameter: u32,
        silhouette_error: u32,
    ) -> QualityCandidate {
        QualityCandidate {
            candidate_id: id,
            preference_rank: rank,
            target_format_id: 1,
            estimated_vertices: vertices,
            estimated_indices: indices,
            estimated_geometry_bytes: geometry_bytes,
            estimated_frame_time_ns: frame_time_ns,
            estimated_cpu_time_ns: cpu_time_ns,
            estimated_gpu_time_ns: gpu_time_ns,
            estimated_cpu_memory_bytes: cpu_memory_bytes,
            estimated_gpu_memory_bytes: gpu_memory_bytes,
            feature_diameter_millipixels: feature_diameter,
            silhouette_error_microunits: silhouette_error,
            animated: true,
            uncertainty: None,
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
            presentation_clock_id: Some(1),
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
    fn decision_verifier_recomputes_candidate_and_degradation() {
        let req = request(1_000_000, false);
        let caps = capabilities(vec![
            candidate(1, 0, 600_000, true, 1_500, 15_000),
            candidate(2, 1, 500_000, true, 2_500, 5_000),
        ]);
        let decision = negotiate(&req, &caps, 1).unwrap();
        assert_eq!(
            verify_quality_decision(&req, &caps, 1, &decision),
            Ok(())
        );

        let mut forged = decision.clone();
        forged.selected_candidate = Some(caps.candidates[1]);
        assert_eq!(
            verify_quality_decision(&req, &caps, 1, &forged),
            Err(ContractError::DecisionMismatch)
        );

        let mut stale_policy = decision.clone();
        stale_policy.policy_version = 2;
        assert_eq!(
            verify_quality_decision(&req, &caps, 1, &stale_policy),
            Err(ContractError::DecisionMismatch)
        );
    }

    #[test]
    fn decision_verifier_rejects_changed_capability_revision() {
        let req = request(1_000_000, false);
        let caps = capabilities(vec![candidate(1, 0, 600_000, true, 2_000, 5_000)]);
        let decision = negotiate(&req, &caps, 1).unwrap();
        let mut changed = caps.clone();
        changed.revision += 1;
        assert_eq!(
            verify_quality_decision(&req, &changed, 1, &decision),
            Err(ContractError::DecisionMismatch)
        );
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
    fn pareto_tradeoff_keeps_resource_and_perceptual_extremes_on_frontier() {
        let resource_efficient = quality_candidate(
            1, 0, 4_000, 12_000, 400_000, 6_000_000, 1_500_000, 3_000_000,
            250_000, 400_000, 1_500, 15_000,
        );
        let perceptually_rich = quality_candidate(
            2, 1, 6_500, 20_000, 650_000, 10_000_000, 3_000_000, 6_000_000,
            400_000, 700_000, 2_700, 5_000,
        );
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![resource_efficient, perceptually_rich]),
            &pareto_policy(DominanceMode::PointEstimate),
        )
        .unwrap();
        assert_eq!(decision.frontier_candidate_ids(), &[1, 2]);
        assert_eq!(decision.selected_candidate_id(), Some(1));
        assert!(decision.is_feasible());
    }

    #[test]
    fn pareto_strictly_dominating_candidate_removes_weaker_candidate() {
        let better = quality_candidate(
            1, 1, 4_000, 12_000, 400_000, 6_000_000, 1_500_000, 3_000_000,
            250_000, 350_000, 2_800, 4_000,
        );
        let weaker = quality_candidate(
            2, 0, 6_000, 18_000, 600_000, 10_000_000, 3_000_000, 6_000_000,
            400_000, 650_000, 1_500, 20_000,
        );
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![weaker, better]),
            &pareto_policy(DominanceMode::PointEstimate),
        )
        .unwrap();
        assert_eq!(decision.frontier_candidate_ids(), &[1]);
        assert_eq!(decision.selected_candidate_id(), Some(1));
        let dominated = decision.evaluations().iter().find(|e| e.candidate_id == 2).unwrap();
        assert_eq!(dominated.disposition, CandidateDisposition::Dominated);
        assert_eq!(dominated.dominated_by_candidate_id, Some(1));
        assert_eq!(dominated.dominance_reason, Some(DominanceReason::StrictlyBetterObjectiveVector));
    }

    #[test]
    fn pareto_hard_constraints_filter_before_dominance() {
        let feasible = quality_candidate(
            1, 0, 5_000, 15_000, 500_000, 8_000_000, 2_000_000, 5_000_000,
            300_000, 400_000, 1_500, 15_000,
        );
        let over_budget_but_pretty = quality_candidate(
            2, 1, 12_000, 36_000, 1_200_000, 12_000_000, 3_000_000, 6_000_000,
            400_000, 700_000, 4_000, 1_000,
        );
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![feasible, over_budget_but_pretty]),
            &pareto_policy(DominanceMode::PointEstimate),
        )
        .unwrap();
        assert_eq!(decision.frontier_candidate_ids(), &[1]);
        let rejected = decision.evaluations().iter().find(|e| e.candidate_id == 2).unwrap();
        assert_eq!(rejected.disposition, CandidateDisposition::Infeasible);
        assert_ne!(rejected.hard_rejection_flags & REJECT_VERTICES, 0);
        assert_ne!(rejected.hard_rejection_flags & REJECT_INDICES, 0);
        assert_ne!(rejected.hard_rejection_flags & REJECT_GEOMETRY_BYTES, 0);
    }

    #[test]
    fn capability_snapshot_and_pareto_decision_are_permutation_invariant() {
        let a = quality_candidate(
            1, 1, 4_000, 12_000, 400_000, 6_000_000, 1_500_000, 3_000_000,
            250_000, 350_000, 2_000, 10_000,
        );
        let b = quality_candidate(
            2, 0, 6_000, 18_000, 600_000, 10_000_000, 3_000_000, 6_000_000,
            400_000, 650_000, 1_500, 20_000,
        );
        let first = capabilities(vec![a, b]);
        let reversed = capabilities(vec![b, a]);
        assert_eq!(first.canonical_bytes().unwrap(), reversed.canonical_bytes().unwrap());
        let request = request(1_000_000, false);
        let policy = pareto_policy(DominanceMode::PointEstimate);
        let one = negotiate_pareto(&request, &first, &policy).unwrap();
        let two = negotiate_pareto(&request, &reversed, &policy).unwrap();
        assert_eq!(one.canonical_bytes().unwrap(), two.canonical_bytes().unwrap());
        assert_eq!(
            negotiate(&request, &first, 1).unwrap().canonical_bytes().unwrap(),
            negotiate(&request, &reversed, 1).unwrap().canonical_bytes().unwrap()
        );
    }

    #[test]
    fn identical_metric_vectors_use_stable_candidate_id_not_host_rank() {
        let lower_id = quality_candidate(
            1, 1, 5_000, 15_000, 500_000, 8_000_000, 2_000_000, 5_000_000,
            300_000, 400_000, 2_000, 10_000,
        );
        let mut higher_id = lower_id;
        higher_id.candidate_id = 2;
        higher_id.preference_rank = 0;
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![higher_id, lower_id]),
            &pareto_policy(DominanceMode::PointEstimate),
        )
        .unwrap();
        assert_eq!(decision.frontier_candidate_ids(), &[1]);
        assert_eq!(decision.selected_candidate_id(), Some(1));
        let duplicate = decision.evaluations().iter().find(|e| e.candidate_id == 2).unwrap();
        assert_eq!(duplicate.dominance_reason, Some(DominanceReason::EquivalentVectorStableIdTieBreak));
    }

    #[test]
    fn conservative_interval_policy_does_not_treat_unknown_uncertainty_as_zero() {
        let mut known = quality_candidate(
            1, 0, 5_000, 15_000, 500_000, 8_000_000, 2_000_000, 5_000_000,
            300_000, 400_000, 2_000, 10_000,
        );
        known.uncertainty = Some(zero_uncertainty());
        let unknown_but_nominally_better = quality_candidate(
            2, 1, 1_000, 3_000, 100_000, 2_000_000, 500_000, 1_000_000,
            100_000, 100_000, 3_000, 1_000,
        );
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![known, unknown_but_nominally_better]),
            &pareto_policy(DominanceMode::ConservativeIntervals),
        )
        .unwrap();
        assert_eq!(decision.frontier_candidate_ids(), &[1]);
        let unknown = decision.evaluations().iter().find(|e| e.candidate_id == 2).unwrap();
        assert_eq!(unknown.disposition, CandidateDisposition::Indeterminate);
        assert!(unknown.uncertainty_unknown);
        assert_eq!(decision.selected_candidate_id(), Some(1));
    }

    #[test]
    fn conservative_intervals_require_separated_bounds_to_dominate() {
        let mut a = quality_candidate(
            1, 0, 4_000, 12_000, 400_000, 6_000_000, 1_500_000, 3_000_000,
            250_000, 350_000, 2_500, 5_000,
        );
        let mut b = quality_candidate(
            2, 1, 4_100, 12_100, 401_000, 6_100_000, 1_600_000, 3_100_000,
            251_000, 351_000, 2_450, 5_100,
        );
        let wide = QualityUncertainty {
            vertices: 200, indices: 500, geometry_bytes: 20_000, frame_time_ns: 200_000,
            cpu_time_ns: 100_000, gpu_time_ns: 100_000, cpu_memory_bytes: 20_000,
            gpu_memory_bytes: 20_000, feature_diameter_millipixels: 100,
            silhouette_error_microunits: 100,
        };
        a.uncertainty = Some(wide);
        b.uncertainty = Some(wide);
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![a, b]),
            &pareto_policy(DominanceMode::ConservativeIntervals),
        )
        .unwrap();
        assert_eq!(decision.frontier_candidate_ids(), &[1, 2]);
    }

    #[test]
    fn conservative_policy_returns_indeterminate_when_all_bounds_are_unknown() {
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![candidate(1, 0, 500_000, true, 2_000, 5_000)]),
            &pareto_policy(DominanceMode::ConservativeIntervals),
        )
        .unwrap();
        assert!(!decision.is_feasible());
        assert_eq!(
            decision.infeasible_reason(),
            Some(ParetoInfeasibleReason::NoCandidateHasRequiredUncertainty)
        );
    }

    #[test]
    fn relaxed_hard_budget_preserves_feasible_set_even_when_frontier_changes() {
        let a = quality_candidate(
            1, 0, 5_000, 15_000, 500_000, 8_000_000, 2_000_000, 5_000_000,
            300_000, 400_000, 1_500, 15_000,
        );
        let mut b = quality_candidate(
            2, 1, 6_000, 18_000, 600_000, 10_000_000, 3_000_000, 6_000_000,
            400_000, 600_000, 3_000, 5_000,
        );
        let caps = capabilities(vec![a, b]);
        let tight = negotiate_pareto(
            &request(500_000, false),
            &caps,
            &pareto_policy(DominanceMode::PointEstimate),
        )
        .unwrap();
        assert_eq!(tight.frontier_candidate_ids(), &[1]);
        b.estimated_gpu_memory_bytes = 600_000;
        let expanded = negotiate_pareto(
            &request(700_000, false),
            &capabilities(vec![a, b]),
            &pareto_policy(DominanceMode::PointEstimate),
        )
        .unwrap();
        let old_candidate = expanded.evaluations().iter().find(|e| e.candidate_id == 1).unwrap();
        assert_ne!(old_candidate.disposition, CandidateDisposition::Infeasible);
        assert_eq!(expanded.frontier_candidate_ids(), &[1, 2]);
    }

    #[test]
    fn pareto_decision_verifier_recomputes_frontier_and_rejects_mutation() {
        let request = request(1_000_000, false);
        let caps = capabilities(vec![
            quality_candidate(1, 0, 4_000, 12_000, 400_000, 6_000_000, 1_500_000, 3_000_000, 250_000, 350_000, 2_000, 10_000),
            quality_candidate(2, 1, 6_000, 18_000, 600_000, 10_000_000, 3_000_000, 6_000_000, 400_000, 650_000, 1_500, 20_000),
        ]);
        let policy = pareto_policy(DominanceMode::PointEstimate);
        let decision = negotiate_pareto(&request, &caps, &policy).unwrap();
        assert_eq!(verify_pareto_decision(&request, &caps, &policy, &decision), Ok(()));
        let mut forged = decision.clone();
        forged.selected_candidate_id = Some(2);
        assert_eq!(
            verify_pareto_decision(&request, &caps, &policy, &forged),
            Err(ContractError::ParetoDecisionMismatch)
        );
    }

    #[test]
    fn epsilon_grid_groups_close_metrics_and_uses_stable_candidate_id() {
        let nominally_weaker_lower_id = quality_candidate(
            1, 0, 5_500, 16_000, 550_000, 8_500_000, 2_200_000, 5_200_000,
            320_000, 420_000, 1_980, 10_500,
        );
        let nominally_better_higher_id = quality_candidate(
            2, 1, 5_000, 15_000, 500_000, 8_000_000, 2_000_000, 5_000_000,
            300_000, 400_000, 2_000, 10_000,
        );
        let decision = negotiate_pareto(
            &request(1_000_000, false),
            &capabilities(vec![nominally_better_higher_id, nominally_weaker_lower_id]),
            &pareto_policy(DominanceMode::EpsilonGrid),
        )
        .unwrap();
        assert_eq!(decision.frontier_candidate_ids(), &[1]);
        assert_eq!(decision.selected_candidate_id(), Some(1));
        let grouped = decision.evaluations().iter().find(|e| e.candidate_id == 2).unwrap();
        assert_eq!(grouped.disposition, CandidateDisposition::Dominated);
        assert_eq!(grouped.dominance_reason, Some(DominanceReason::EquivalentVectorStableIdTieBreak));
    }

    #[test]
    fn epsilon_grid_policy_requires_and_binds_positive_per_metric_widths() {
        let mut missing = pareto_policy(DominanceMode::EpsilonGrid);
        missing.epsilon = None;
        assert_eq!(missing.validate(), Err(ContractError::InvalidParetoPolicy));

        let mut zero_width = pareto_policy(DominanceMode::EpsilonGrid);
        zero_width.epsilon.as_mut().unwrap().gpu_memory_bytes = 0;
        assert_eq!(zero_width.validate(), Err(ContractError::InvalidParetoPolicy));

        let first = pareto_policy(DominanceMode::EpsilonGrid).canonical_bytes().unwrap();
        let mut second = pareto_policy(DominanceMode::EpsilonGrid);
        second.epsilon.as_mut().unwrap().gpu_memory_bytes += 1;
        assert_ne!(first, second.canonical_bytes().unwrap());
        assert!(first.starts_with(b"OUROPOL2"));
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
    fn frame_receipt_rejects_zero_revision_state_and_decision_digests() {
        let mut frame = receipt(
            1,
            1,
            PresentationStatus::Submitted,
            EvidenceLevel::HostObserved,
            None,
        );
        frame.entity_revision_root = [0; 32];
        assert_eq!(frame.validate(), Err(ContractError::InvalidFrameReceipt));
        frame.entity_revision_root = [2; 32];
        frame.state_digest = [0; 32];
        assert_eq!(frame.validate(), Err(ContractError::InvalidFrameReceipt));
        frame.state_digest = [4; 32];
        frame.decision_digest = [0; 32];
        assert_eq!(frame.validate(), Err(ContractError::InvalidFrameReceipt));
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
        missing_clock.presentation_clock_id = None;
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
    fn frame_receipt_rejects_clock_metadata_that_disagrees_with_presented_observation() {
        let mut frame = receipt(
            1,
            1,
            PresentationStatus::Presented,
            EvidenceLevel::CompositorFeedback,
            Some(10),
        );
        frame.presentation_clock_id = Some(2);
        assert_eq!(
            frame.validate(),
            Err(ContractError::PresentationClockMismatch)
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

    fn test_presentation_observation(clock_id: u32, timestamp: u64) -> PresentationObservation {
        PresentationObservation {
            source: PresentationSource::WaylandPresentationTimeV2,
            clock_id,
            presentation_time_ns: timestamp,
            refresh_interval_ns: 16_666_667,
            refresh_sequence: timestamp / 16_666_667,
            output_identity_digest: Some([7; 32]),
            source_flags: WAYLAND_PRESENTATION_VSYNC,
        }
    }

    fn ledger_session() -> [u8; 16] {
        [3; 16]
    }

    fn feedback_ledger(capacity: usize) -> PresentationFeedbackLedger {
        PresentationFeedbackLedger::new(ledger_session(), Some(1), capacity).unwrap()
    }

    #[test]
    fn feedback_ledger_deduplicates_identical_presented_callbacks() {
        let observation = test_presentation_observation(1, 10);
        let mut ledger = feedback_ledger(8);
        ledger.register_commit(ledger_session(), 1).unwrap();
        assert_eq!(
            ledger.record_presented(ledger_session(), 1, observation),
            Ok(FeedbackRecordResult::Recorded)
        );
        assert_eq!(
            ledger.record_presented(ledger_session(), 1, observation),
            Ok(FeedbackRecordResult::DuplicateIdentical)
        );
        assert_eq!(
            ledger.outcome(ledger_session(), 1),
            Ok(Some(TerminalFeedback::Presented(observation)))
        );
    }

    #[test]
    fn feedback_ledger_deduplicates_identical_discarded_callbacks() {
        let mut ledger = feedback_ledger(8);
        ledger.register_commit(ledger_session(), 1).unwrap();
        assert_eq!(
            ledger.record_discarded(
                ledger_session(),
                1,
                PresentationSource::WaylandPresentationTimeV2
            ),
            Ok(FeedbackRecordResult::Recorded)
        );
        assert_eq!(
            ledger.record_discarded(
                ledger_session(),
                1,
                PresentationSource::WaylandPresentationTimeV2
            ),
            Ok(FeedbackRecordResult::DuplicateIdentical)
        );
        assert_eq!(
            ledger.outcome(ledger_session(), 1),
            Ok(Some(TerminalFeedback::Discarded(
                PresentationSource::WaylandPresentationTimeV2
            )))
        );
    }

    #[test]
    fn feedback_ledger_rejects_conflicting_presented_and_discarded_outcomes() {
        let mut ledger = feedback_ledger(8);
        ledger.register_commit(ledger_session(), 1).unwrap();
        let observation = test_presentation_observation(1, 10);
        ledger
            .record_presented(ledger_session(), 1, observation)
            .unwrap();
        assert_eq!(
            ledger.record_discarded(
                ledger_session(),
                1,
                PresentationSource::WaylandPresentationTimeV2
            ),
            Err(ContractError::ConflictingTerminalFeedback)
        );
        assert_eq!(
            ledger.outcome(ledger_session(), 1),
            Ok(Some(TerminalFeedback::Presented(observation)))
        );
    }

    #[test]
    fn feedback_ledger_binds_session_and_rejects_unregistered_sequences() {
        let mut ledger = feedback_ledger(8);
        let observation = test_presentation_observation(1, 10);
        assert_eq!(
            ledger.register_commit([9; 16], 1),
            Err(ContractError::SessionMismatch)
        );
        assert_eq!(
            ledger.record_presented([9; 16], 1, observation),
            Err(ContractError::SessionMismatch)
        );
        assert_eq!(
            ledger.outcome([9; 16], 1),
            Err(ContractError::SessionMismatch)
        );
        assert_eq!(
            ledger.record_presented(ledger_session(), 1, observation),
            Err(ContractError::UnknownFrameCommit)
        );
        ledger.register_commit(ledger_session(), 2).unwrap();
        assert_eq!(
            ledger.register_commit(ledger_session(), 2),
            Err(ContractError::NonMonotonicFrameSequence)
        );
        assert_eq!(
            ledger.register_commit(ledger_session(), 1),
            Err(ContractError::NonMonotonicFrameSequence)
        );
    }

    #[test]
    fn unregistered_presented_feedback_cannot_poison_session_clock() {
        let mut ledger = PresentationFeedbackLedger::new(ledger_session(), None, 8).unwrap();
        assert_eq!(
            ledger.record_presented(
                ledger_session(),
                99,
                test_presentation_observation(99, 10)
            ),
            Err(ContractError::UnknownFrameCommit)
        );
        ledger.register_commit(ledger_session(), 1).unwrap();
        assert_eq!(
            ledger.record_presented(
                ledger_session(),
                1,
                test_presentation_observation(1, 20)
            ),
            Ok(FeedbackRecordResult::Recorded)
        );
    }

    #[test]
    fn feedback_ledger_locks_presentation_clock_for_the_session() {
        let mut ledger = feedback_ledger(8);
        ledger.register_commit(ledger_session(), 1).unwrap();
        assert_eq!(
            ledger.record_presented(
                ledger_session(),
                1,
                test_presentation_observation(2, 10)
            ),
            Err(ContractError::PresentationClockMismatch)
        );
        assert_eq!(ledger.outcome(ledger_session(), 1), Ok(None));
    }

    #[test]
    fn feedback_ledger_retires_only_terminal_prefix_and_rejects_late_duplicates() {
        let mut ledger = feedback_ledger(3);
        ledger.register_commit(ledger_session(), 1).unwrap();
        ledger.register_commit(ledger_session(), 2).unwrap();
        ledger.register_commit(ledger_session(), 3).unwrap();
        ledger
            .record_presented(
                ledger_session(),
                1,
                test_presentation_observation(1, 10)
            )
            .unwrap();
        ledger
            .record_discarded(
                ledger_session(),
                2,
                PresentationSource::WaylandPresentationTimeV2
            )
            .unwrap();
        assert_eq!(
            ledger.retire_through(ledger_session(), 2),
            Ok(2)
        );
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger.retired_through_sequence(), 2);
        assert_eq!(
            ledger.record_discarded(
                ledger_session(),
                1,
                PresentationSource::WaylandPresentationTimeV2
            ),
            Err(ContractError::RetiredFrameCommit)
        );
        assert_eq!(
            ledger.outcome(ledger_session(), 1),
            Err(ContractError::RetiredFrameCommit)
        );
        assert_eq!(
            ledger.retire_through(ledger_session(), 3),
            Err(ContractError::PendingFrameFeedback)
        );
        ledger
            .record_discarded(
                ledger_session(),
                3,
                PresentationSource::WaylandPresentationTimeV2
            )
            .unwrap();
        assert_eq!(
            ledger.retire_through(ledger_session(), 3),
            Ok(1)
        );
        assert!(ledger.is_empty());
        assert_eq!(
            ledger.register_commit(ledger_session(), 3),
            Err(ContractError::NonMonotonicFrameSequence)
        );
    }

    #[test]
    fn feedback_ledger_capacity_is_reusable_after_explicit_retirement() {
        let mut ledger = feedback_ledger(2);
        for sequence in 1..=8 {
            ledger.register_commit(ledger_session(), sequence).unwrap();
            ledger
                .record_discarded(
                    ledger_session(),
                    sequence,
                    PresentationSource::WaylandPresentationTimeV2
                )
                .unwrap();
            assert_eq!(
                ledger.retire_through(ledger_session(), sequence),
                Ok(1)
            );
            assert!(ledger.is_empty());
        }
    }

    #[test]
    fn feedback_ledger_rejects_zero_session_and_invalid_capacity() {
        assert_eq!(
            PresentationFeedbackLedger::new([0; 16], Some(1), 8).err(),
            Some(ContractError::SessionMismatch)
        );
        assert_eq!(
            PresentationFeedbackLedger::new(ledger_session(), None, 0).err(),
            Some(ContractError::InvalidFeedbackLedgerCapacity)
        );
        assert_eq!(
            PresentationFeedbackLedger::new(
                ledger_session(),
                None,
                MAX_FEEDBACK_LEDGER_ENTRIES + 1
            )
            .err(),
            Some(ContractError::InvalidFeedbackLedgerCapacity)
        );
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
        first.presentation.as_mut().unwrap().clock_id = 1;
        guard.observe(&first).unwrap();

        let mut changed_clock = receipt(
            2,
            4,
            PresentationStatus::Presented,
            EvidenceLevel::CompositorFeedback,
            Some(20),
        );
        changed_clock.presentation_clock_id = Some(2);
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
