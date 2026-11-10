//! Semantic-distortion-bounded LOD selection.
//!
//! Resource feasibility remains governed by the parent presentation contract.
//! This module adds per-feature presence, projected-size and screen-error
//! constraints so a globally plausible silhouette cannot silently erase an
//! identity-critical feature. All decisions use integer arithmetic and
//! canonical source inputs. This is a structural proxy, not a human-recognition
//! guarantee.

use super::{
    CapabilitySnapshot, ContractError, QualityCandidate, QualityRequest,
    put_u16, put_u32, put_u64,
};

pub const SEMANTIC_LOD_SCHEMA_VERSION: u16 = 1;
pub const MAX_SEMANTIC_FEATURES: usize = 32;
pub const MAX_SEMANTIC_CANDIDATES: usize = 128;

const PROFILE_MAGIC: [u8; 8] = *b"OUROSEM1";
const CANDIDATE_SET_MAGIC: [u8; 8] = *b"OUROSCN1";
const DECISION_MAGIC: [u8; 8] = *b"OUROSLD1";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u16)]
pub enum SemanticFeatureId {
    HeadSilhouette = 1,
    LeftEye = 2,
    RightEye = 3,
    Nose = 4,
    Mouth = 5,
}

impl SemanticFeatureId {
    fn is_identity_anchor(self) -> bool {
        matches!(self, Self::LeftEye | Self::RightEye | Self::Nose | Self::Mouth)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticFeatureConstraint {
    pub feature_id: SemanticFeatureId,
    pub required: bool,
    /// Minimum projected diameter in millipixels. 1000 = 1 pixel.
    pub min_projected_diameter_millipixels: u32,
    /// Maximum projected anchor/contour deviation in microunits.
    pub max_screen_error_microunits: u32,
    /// Fixed integer importance weight, 1000 = 1.0.
    pub importance_weight_milli: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticLodProfile {
    pub schema_version: u16,
    /// Constraints must be strictly increasing by stable feature ID.
    pub features: Vec<SemanticFeatureConstraint>,
    /// Penalty applied to every absent optional feature before its weight.
    pub missing_optional_penalty_microunits: u32,
    /// Maximum sum(error_microunits * importance_weight_milli).
    pub max_weighted_distortion_milliunits: u64,
}

impl SemanticLodProfile {
    pub fn validate(&self) -> Result<(), SemanticLodError> {
        if self.schema_version != SEMANTIC_LOD_SCHEMA_VERSION {
            return Err(SemanticLodError::UnsupportedSchema);
        }
        if self.features.is_empty() || self.features.len() > MAX_SEMANTIC_FEATURES {
            return Err(SemanticLodError::InvalidFeatureCount);
        }
        if self
            .features
            .windows(2)
            .any(|pair| pair[0].feature_id >= pair[1].feature_id)
        {
            return Err(SemanticLodError::NonCanonicalFeatureOrdering);
        }
        if self.features.iter().any(|feature| {
            feature.importance_weight_milli == 0
                || (feature.required && feature.min_projected_diameter_millipixels == 0)
        }) {
            return Err(SemanticLodError::InvalidFeatureConstraint);
        }
        let has_required_head = self.features.iter().any(|feature| {
            feature.feature_id == SemanticFeatureId::HeadSilhouette && feature.required
        });
        let required_anchors = self.features.iter().filter(|feature| {
            feature.required && feature.feature_id.is_identity_anchor()
        }).count();
        let has_optional_feature = self.features.iter().any(|feature| !feature.required);
        if has_optional_feature && self.missing_optional_penalty_microunits == 0 {
            return Err(SemanticLodError::InvalidFeatureConstraint);
        }
        if !has_required_head || required_anchors < 2 {
            return Err(SemanticLodError::InsufficientIdentityAnchors);
        }
        Ok(())
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SemanticLodError> {
        self.validate()?;
        let mut out = Vec::with_capacity(8 + 2 + 2 + 4 + 8 + self.features.len() * 13);
        out.extend_from_slice(&PROFILE_MAGIC);
        put_u16(&mut out, self.schema_version);
        put_u16(
            &mut out,
            u16::try_from(self.features.len()).map_err(|_| SemanticLodError::SizeOverflow)?,
        );
        put_u32(&mut out, self.missing_optional_penalty_microunits);
        put_u64(&mut out, self.max_weighted_distortion_milliunits);
        for feature in &self.features {
            put_u16(&mut out, feature.feature_id as u16);
            out.push(u8::from(feature.required));
            put_u32(&mut out, feature.min_projected_diameter_millipixels);
            put_u32(&mut out, feature.max_screen_error_microunits);
            put_u16(&mut out, feature.importance_weight_milli);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SemanticFeatureObservation {
    pub feature_id: SemanticFeatureId,
    pub present: bool,
    pub projected_diameter_millipixels: u32,
    pub max_screen_error_microunits: u32,
}

impl SemanticFeatureObservation {
    fn validate(&self) -> Result<(), SemanticLodError> {
        if !self.present
            && (self.projected_diameter_millipixels != 0
                || self.max_screen_error_microunits != 0)
        {
            return Err(SemanticLodError::InvalidMissingFeatureObservation);
        }
        if self.present && self.projected_diameter_millipixels == 0 {
            return Err(SemanticLodError::InvalidPresentFeatureObservation);
        }
        Ok(())
    }

    fn encode_into(&self, out: &mut Vec<u8>) {
        put_u16(out, self.feature_id as u16);
        out.push(u8::from(self.present));
        put_u32(out, self.projected_diameter_millipixels);
        put_u32(out, self.max_screen_error_microunits);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticLodCandidate {
    pub candidate_id: u16,
    /// Exactly one observation per profile feature, in profile order.
    pub observations: Vec<SemanticFeatureObservation>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticLodCandidateSet {
    pub candidates: Vec<SemanticLodCandidate>,
}

impl SemanticLodCandidateSet {
    pub fn validate(
        &self,
        profile: &SemanticLodProfile,
        capabilities: &CapabilitySnapshot,
    ) -> Result<(), SemanticLodError> {
        profile.validate()?;
        capabilities.validate().map_err(SemanticLodError::ResourceContract)?;
        if self.candidates.len() != capabilities.candidates.len()
            || self.candidates.len() > MAX_SEMANTIC_CANDIDATES
        {
            return Err(SemanticLodError::CandidateSetMismatch);
        }
        for (semantic, resource) in self.candidates.iter().zip(&capabilities.candidates) {
            if semantic.candidate_id == 0 || semantic.candidate_id != resource.candidate_id {
                return Err(SemanticLodError::CandidateSetMismatch);
            }
            if semantic.observations.len() != profile.features.len() {
                return Err(SemanticLodError::ObservationSetMismatch);
            }
            for (observation, constraint) in semantic.observations.iter().zip(&profile.features) {
                observation.validate()?;
                if observation.feature_id != constraint.feature_id {
                    return Err(SemanticLodError::ObservationSetMismatch);
                }
            }
        }
        Ok(())
    }

    pub fn canonical_bytes(
        &self,
        profile: &SemanticLodProfile,
        capabilities: &CapabilitySnapshot,
    ) -> Result<Vec<u8>, SemanticLodError> {
        self.validate(profile, capabilities)?;
        let observation_count = profile.features.len();
        let mut out = Vec::new();
        let capacity = 8_usize
            .checked_add(2)
            .and_then(|v| v.checked_add(2))
            .and_then(|v| {
                v.checked_add(self.candidates.len().checked_mul(2 + observation_count * 11)?)
            })
            .ok_or(SemanticLodError::SizeOverflow)?;
        out.try_reserve_exact(capacity)
            .map_err(|_| SemanticLodError::AllocationFailed)?;
        out.extend_from_slice(&CANDIDATE_SET_MAGIC);
        put_u16(&mut out, SEMANTIC_LOD_SCHEMA_VERSION);
        put_u16(
            &mut out,
            u16::try_from(self.candidates.len()).map_err(|_| SemanticLodError::SizeOverflow)?,
        );
        for candidate in &self.candidates {
            put_u16(&mut out, candidate.candidate_id);
            for observation in &candidate.observations {
                observation.encode_into(&mut out);
            }
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum SemanticLodInfeasibleReason {
    NoResourceFeasibleCandidate = 1,
    NoCandidatePreservesSemanticConstraints = 2,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticLodDecision {
    schema_version: u16,
    resource_request_bytes: Vec<u8>,
    capability_snapshot_bytes: Vec<u8>,
    semantic_profile_bytes: Vec<u8>,
    semantic_candidate_bytes: Vec<u8>,
    selected_candidate_id: Option<u16>,
    weighted_distortion_milliunits: Option<u64>,
    /// Nondominated candidate IDs in the capability snapshot's canonical order.
    pareto_candidate_ids: Vec<u16>,
    infeasible_reason: Option<SemanticLodInfeasibleReason>,
}

impl SemanticLodDecision {
    pub fn selected_candidate_id(&self) -> Option<u16> {
        self.selected_candidate_id
    }

    pub fn weighted_distortion_milliunits(&self) -> Option<u64> {
        self.weighted_distortion_milliunits
    }

    pub fn pareto_candidate_ids(&self) -> &[u16] {
        &self.pareto_candidate_ids
    }

    pub fn infeasible_reason(&self) -> Option<SemanticLodInfeasibleReason> {
        self.infeasible_reason
    }

    pub fn is_feasible(&self) -> bool {
        self.selected_candidate_id.is_some() && self.infeasible_reason.is_none()
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SemanticLodError> {
        if self.schema_version != SEMANTIC_LOD_SCHEMA_VERSION
            || self.resource_request_bytes.is_empty()
            || self.capability_snapshot_bytes.is_empty()
            || self.semantic_profile_bytes.is_empty()
            || self.semantic_candidate_bytes.is_empty()
            || (self.selected_candidate_id.is_some()
                != self.weighted_distortion_milliunits.is_some())
            || (self.selected_candidate_id.is_some() == self.infeasible_reason.is_some())
            || self.pareto_candidate_ids.len() > MAX_SEMANTIC_CANDIDATES
            || (self.is_feasible() && self.pareto_candidate_ids.is_empty())
            || (!self.is_feasible() && !self.pareto_candidate_ids.is_empty())
            || self.pareto_candidate_ids.iter().enumerate().any(|(index, candidate_id)| {
                self.pareto_candidate_ids[..index].contains(candidate_id)
            })
            || self
                .selected_candidate_id
                .is_some_and(|selected| !self.pareto_candidate_ids.contains(&selected))
        {
            return Err(SemanticLodError::InvalidDecision);
        }
        let mut out = Vec::new();
        let capacity = 8_usize
            .checked_add(2 + 4 * 4 + 2 + 8 + 1 + 1)
            .and_then(|v| v.checked_add(self.resource_request_bytes.len()))
            .and_then(|v| v.checked_add(self.capability_snapshot_bytes.len()))
            .and_then(|v| v.checked_add(self.semantic_profile_bytes.len()))
            .and_then(|v| v.checked_add(self.semantic_candidate_bytes.len()))
            .and_then(|v| v.checked_add(2 + self.pareto_candidate_ids.len() * 2))
            .ok_or(SemanticLodError::SizeOverflow)?;
        out.try_reserve_exact(capacity)
            .map_err(|_| SemanticLodError::AllocationFailed)?;
        out.extend_from_slice(&DECISION_MAGIC);
        put_u16(&mut out, self.schema_version);
        let resource_request_bytes_len = u32::try_from(self.resource_request_bytes.len())
            .map_err(|_| SemanticLodError::SizeOverflow)?;
        put_u32(&mut out, resource_request_bytes_len);
        out.extend_from_slice(&self.resource_request_bytes);
        let capability_snapshot_bytes_len = u32::try_from(self.capability_snapshot_bytes.len())
            .map_err(|_| SemanticLodError::SizeOverflow)?;
        put_u32(&mut out, capability_snapshot_bytes_len);
        out.extend_from_slice(&self.capability_snapshot_bytes);
        let semantic_profile_bytes_len = u32::try_from(self.semantic_profile_bytes.len())
            .map_err(|_| SemanticLodError::SizeOverflow)?;
        put_u32(&mut out, semantic_profile_bytes_len);
        out.extend_from_slice(&self.semantic_profile_bytes);
        let semantic_candidate_bytes_len = u32::try_from(self.semantic_candidate_bytes.len())
            .map_err(|_| SemanticLodError::SizeOverflow)?;
        put_u32(&mut out, semantic_candidate_bytes_len);
        out.extend_from_slice(&self.semantic_candidate_bytes);
        match self.selected_candidate_id {
            Some(candidate_id) => {
                out.push(1);
                put_u16(&mut out, candidate_id);
                put_u64(
                    &mut out,
                    self.weighted_distortion_milliunits
                        .ok_or(SemanticLodError::InvalidDecision)?,
                );
            }
            None => out.push(0),
        }
        put_u16(
            &mut out,
            u16::try_from(self.pareto_candidate_ids.len())
                .map_err(|_| SemanticLodError::SizeOverflow)?,
        );
        for candidate_id in &self.pareto_candidate_ids {
            put_u16(&mut out, *candidate_id);
        }
        match self.infeasible_reason {
            Some(reason) => {
                out.push(1);
                out.push(reason as u8);
            }
            None => out.push(0),
        }
        Ok(out)
    }
}

/// Pareto objective vector. All values are minimized; projected feature
/// preservation is represented by semantic distortion and global silhouette
/// error, while projected feature diameter is inverted to a minimization cost.
fn candidate_objective(
    candidate: &QualityCandidate,
    distortion: u64,
) -> [u64; 11] {
    [
        distortion,
        u64::from(candidate.estimated_vertices),
        u64::from(candidate.estimated_indices),
        u64::from(candidate.estimated_geometry_bytes),
        candidate.estimated_frame_time_ns,
        candidate.estimated_cpu_time_ns,
        candidate.estimated_gpu_time_ns,
        candidate.estimated_cpu_memory_bytes,
        candidate.estimated_gpu_memory_bytes,
        u64::from(candidate.silhouette_error_microunits),
        u64::from(u32::MAX - candidate.feature_diameter_millipixels),
    ]
}

fn objective_dominates(left: &[u64; 11], right: &[u64; 11]) -> bool {
    let no_worse = left.iter().zip(right).all(|(a, b)| a <= b);
    let strictly_better = left.iter().zip(right).any(|(a, b)| a < b);
    no_worse && strictly_better
}

/// Select the minimum semantic distortion among resource-feasible candidates.
/// Tie-breaks are stable: lower resource preference rank, then candidate ID.
pub fn negotiate_semantic_lod(
    resource_request: &QualityRequest,
    capabilities: &CapabilitySnapshot,
    profile: &SemanticLodProfile,
    semantic_candidates: &SemanticLodCandidateSet,
) -> Result<SemanticLodDecision, SemanticLodError> {
    resource_request.validate().map_err(SemanticLodError::ResourceContract)?;
    capabilities.validate().map_err(SemanticLodError::ResourceContract)?;
    let profile_bytes = profile.canonical_bytes()?;
    let semantic_candidate_bytes = semantic_candidates.canonical_bytes(profile, capabilities)?;
    let resource_request_bytes = resource_request
        .canonical_bytes()
        .map_err(SemanticLodError::ResourceContract)?;
    let capability_snapshot_bytes = capabilities
        .canonical_bytes()
        .map_err(SemanticLodError::ResourceContract)?;

    let mut resource_feasible_count = 0usize;
    let mut feasible = Vec::new();
    for (resource, semantic) in capabilities
        .candidates
        .iter()
        .zip(&semantic_candidates.candidates)
    {
        if !resource.is_feasible_for(resource_request) {
            continue;
        }
        resource_feasible_count += 1;
        let Some(distortion) = semantic_distortion(profile, semantic)? else {
            continue;
        };
        if distortion > profile.max_weighted_distortion_milliunits {
            continue;
        }
        feasible.push((
            resource,
            distortion,
            candidate_objective(resource, distortion),
        ));
    }

    // A candidate is on the frontier when no other feasible candidate is at
    // least as good in every declared objective and strictly better in one.
    let mut pareto_candidate_ids = Vec::new();
    let mut selected: Option<(u64, u16, u16)> = None;
    for (index, (candidate, distortion, objective)) in feasible.iter().enumerate() {
        let dominated = feasible.iter().enumerate().any(|(other_index, other)| {
            index != other_index && objective_dominates(&other.2, objective)
        });
        if dominated {
            continue;
        }
        pareto_candidate_ids.push(candidate.candidate_id);
        let key = (*distortion, candidate.preference_rank, candidate.candidate_id);
        if selected.is_none_or(|old| key < old) {
            selected = Some(key);
        }
    }

    let (selected_candidate_id, weighted_distortion_milliunits, infeasible_reason) =
        match selected {
            Some((distortion, _, candidate_id)) => (Some(candidate_id), Some(distortion), None),
            None if resource_feasible_count == 0 => (
                None,
                None,
                Some(SemanticLodInfeasibleReason::NoResourceFeasibleCandidate),
            ),
            None => (
                None,
                None,
                Some(SemanticLodInfeasibleReason::NoCandidatePreservesSemanticConstraints),
            ),
        };

    Ok(SemanticLodDecision {
        schema_version: SEMANTIC_LOD_SCHEMA_VERSION,
        resource_request_bytes,
        capability_snapshot_bytes,
        semantic_profile_bytes: profile_bytes,
        semantic_candidate_bytes,
        selected_candidate_id,
        weighted_distortion_milliunits,
        pareto_candidate_ids,
        infeasible_reason,
    })
}

/// Recompute from the original inputs; never trust a host/renderer score.
pub fn verify_semantic_lod_decision(
    resource_request: &QualityRequest,
    capabilities: &CapabilitySnapshot,
    profile: &SemanticLodProfile,
    semantic_candidates: &SemanticLodCandidateSet,
    supplied: &SemanticLodDecision,
) -> Result<(), SemanticLodError> {
    let expected = negotiate_semantic_lod(
        resource_request,
        capabilities,
        profile,
        semantic_candidates,
    )?;
    if expected.canonical_bytes()? != supplied.canonical_bytes()? {
        return Err(SemanticLodError::DecisionMismatch);
    }
    Ok(())
}

fn semantic_distortion(
    profile: &SemanticLodProfile,
    candidate: &SemanticLodCandidate,
) -> Result<Option<u64>, SemanticLodError> {
    let mut total = 0u64;
    for (constraint, observation) in profile.features.iter().zip(&candidate.observations) {
        let error = if observation.present {
            if observation.projected_diameter_millipixels
                < constraint.min_projected_diameter_millipixels
                || observation.max_screen_error_microunits
                    > constraint.max_screen_error_microunits
            {
                return Ok(None);
            }
            observation.max_screen_error_microunits
        } else if constraint.required {
            return Ok(None);
        } else {
            profile.missing_optional_penalty_microunits
        };
        let weighted = u64::from(error)
            .checked_mul(u64::from(constraint.importance_weight_milli))
            .ok_or(SemanticLodError::DistortionOverflow)?;
        total = total
            .checked_add(weighted)
            .ok_or(SemanticLodError::DistortionOverflow)?;
    }
    Ok(Some(total))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SemanticLodError {
    UnsupportedSchema,
    InvalidFeatureCount,
    NonCanonicalFeatureOrdering,
    InvalidFeatureConstraint,
    InsufficientIdentityAnchors,
    InvalidMissingFeatureObservation,
    InvalidPresentFeatureObservation,
    CandidateSetMismatch,
    ObservationSetMismatch,
    DistortionOverflow,
    SizeOverflow,
    AllocationFailed,
    InvalidDecision,
    DecisionMismatch,
    ResourceContract(ContractError),
}

impl std::fmt::Display for SemanticLodError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{self:?}")
    }
}

impl std::error::Error for SemanticLodError {}

#[cfg(test)]
mod tests {
    use super::*;

    fn quality_request(max_geometry_bytes: u32) -> QualityRequest {
        QualityRequest {
            schema_version: super::super::QUALITY_SCHEMA_VERSION,
            hard: super::super::HardConstraints {
                max_vertices: 10_000,
                max_indices: 30_000,
                max_geometry_bytes,
                max_frame_time_ns: 16_700_000,
                max_cpu_time_ns: 4_000_000,
                max_gpu_time_ns: 10_000_000,
                max_cpu_memory_bytes: 500_000,
                max_gpu_memory_bytes: 1_000_000,
                min_feature_diameter_millipixels: 500,
                max_silhouette_error_microunits: 100_000,
            },
            soft: super::super::SoftPreferences {
                preferred_feature_diameter_millipixels: 1_000,
                preferred_max_silhouette_error_microunits: 50_000,
            },
            reduced_motion: false,
        }
    }

    fn profile() -> SemanticLodProfile {
        SemanticLodProfile {
            schema_version: SEMANTIC_LOD_SCHEMA_VERSION,
            features: vec![
                SemanticFeatureConstraint {
                    feature_id: SemanticFeatureId::HeadSilhouette,
                    required: true,
                    min_projected_diameter_millipixels: 2_000,
                    max_screen_error_microunits: 20_000,
                    importance_weight_milli: 2_000,
                },
                SemanticFeatureConstraint {
                    feature_id: SemanticFeatureId::LeftEye,
                    required: true,
                    min_projected_diameter_millipixels: 500,
                    max_screen_error_microunits: 5_000,
                    importance_weight_milli: 3_000,
                },
                SemanticFeatureConstraint {
                    feature_id: SemanticFeatureId::RightEye,
                    required: true,
                    min_projected_diameter_millipixels: 500,
                    max_screen_error_microunits: 5_000,
                    importance_weight_milli: 3_000,
                },
                SemanticFeatureConstraint {
                    feature_id: SemanticFeatureId::Nose,
                    required: true,
                    min_projected_diameter_millipixels: 500,
                    max_screen_error_microunits: 8_000,
                    importance_weight_milli: 2_000,
                },
                SemanticFeatureConstraint {
                    feature_id: SemanticFeatureId::Mouth,
                    required: false,
                    min_projected_diameter_millipixels: 0,
                    max_screen_error_microunits: 12_000,
                    importance_weight_milli: 1_000,
                },
            ],
            missing_optional_penalty_microunits: 12_000,
            max_weighted_distortion_milliunits: 150_000_000,
        }
    }

    fn resource_candidate(id: u16, rank: u16, bytes: u32) -> QualityCandidate {
        QualityCandidate {
            candidate_id: id,
            preference_rank: rank,
            target_format_id: 1,
            estimated_vertices: 5_000,
            estimated_indices: 15_000,
            estimated_geometry_bytes: bytes,
            estimated_frame_time_ns: 8_000_000,
            estimated_cpu_time_ns: 2_000_000,
            estimated_gpu_time_ns: 5_000_000,
            estimated_cpu_memory_bytes: 300_000,
            estimated_gpu_memory_bytes: 400_000,
            feature_diameter_millipixels: 3_000,
            silhouette_error_microunits: 10_000,
            animated: true,
        }
    }

    fn capabilities() -> CapabilitySnapshot {
        CapabilitySnapshot {
            schema_version: super::super::CAPABILITY_SCHEMA_VERSION,
            revision: 3,
            candidates: vec![
                resource_candidate(1, 0, 800_000),
                resource_candidate(2, 1, 500_000),
                resource_candidate(3, 2, 300_000),
            ],
        }
    }

    fn candidate(
        id: u16,
        eye_error: u32,
        mouth_present: bool,
    ) -> SemanticLodCandidate {
        SemanticLodCandidate {
            candidate_id: id,
            observations: vec![
                SemanticFeatureObservation {
                    feature_id: SemanticFeatureId::HeadSilhouette,
                    present: true,
                    projected_diameter_millipixels: 8_000,
                    max_screen_error_microunits: 5_000,
                },
                SemanticFeatureObservation {
                    feature_id: SemanticFeatureId::LeftEye,
                    present: true,
                    projected_diameter_millipixels: 900,
                    max_screen_error_microunits: eye_error,
                },
                SemanticFeatureObservation {
                    feature_id: SemanticFeatureId::RightEye,
                    present: true,
                    projected_diameter_millipixels: 900,
                    max_screen_error_microunits: eye_error,
                },
                SemanticFeatureObservation {
                    feature_id: SemanticFeatureId::Nose,
                    present: true,
                    projected_diameter_millipixels: 700,
                    max_screen_error_microunits: 6_000,
                },
                SemanticFeatureObservation {
                    feature_id: SemanticFeatureId::Mouth,
                    present: mouth_present,
                    projected_diameter_millipixels: if mouth_present { 400 } else { 0 },
                    max_screen_error_microunits: if mouth_present { 8_000 } else { 0 },
                },
            ],
        }
    }

    fn candidates() -> SemanticLodCandidateSet {
        SemanticLodCandidateSet {
            candidates: vec![
                candidate(1, 4_000, true),
                candidate(2, 2_000, true),
                candidate(3, 1_000, false),
            ],
        }
    }

    #[test]
    fn profile_requires_head_and_two_independent_identity_anchors() {
        let valid = profile();
        assert!(valid.validate().is_ok());

        let mut invalid = valid;
        invalid.features[0].required = false;
        assert_eq!(
            invalid.validate(),
            Err(SemanticLodError::InsufficientIdentityAnchors)
        );

        let mut invalid = profile();
        invalid.features[2].required = false;
        invalid.features[3].required = false;
        invalid.features[4].required = false;
        assert_eq!(
            invalid.validate(),
            Err(SemanticLodError::InsufficientIdentityAnchors)
        );
    }

    #[test]
    fn profile_rejects_noncanonical_feature_order_and_zero_weight() {
        let mut invalid = profile();
        invalid.features.swap(1, 2);
        assert_eq!(
            invalid.validate(),
            Err(SemanticLodError::NonCanonicalFeatureOrdering)
        );

        let mut invalid = profile();
        invalid.features[1].importance_weight_milli = 0;
        assert_eq!(
            invalid.validate(),
            Err(SemanticLodError::InvalidFeatureConstraint)
        );
    }

    #[test]
    fn pareto_frontier_preserves_quality_resource_tradeoffs() {
        let caps = CapabilitySnapshot {
            schema_version: super::super::CAPABILITY_SCHEMA_VERSION,
            revision: 4,
            candidates: vec![
                QualityCandidate {
                    candidate_id: 10,
                    preference_rank: 0,
                    estimated_geometry_bytes: 800_000,
                    estimated_vertices: 8_000,
                    estimated_indices: 24_000,
                    estimated_frame_time_ns: 9_000_000,
                    estimated_cpu_time_ns: 3_000_000,
                    estimated_gpu_time_ns: 6_000_000,
                    estimated_cpu_memory_bytes: 400_000,
                    estimated_gpu_memory_bytes: 700_000,
                    ..resource_candidate(10, 0, 800_000)
                },
                QualityCandidate {
                    candidate_id: 11,
                    preference_rank: 1,
                    estimated_geometry_bytes: 300_000,
                    estimated_vertices: 3_000,
                    estimated_indices: 9_000,
                    estimated_frame_time_ns: 4_000_000,
                    estimated_cpu_time_ns: 1_000_000,
                    estimated_gpu_time_ns: 2_000_000,
                    estimated_cpu_memory_bytes: 200_000,
                    estimated_gpu_memory_bytes: 300_000,
                    ..resource_candidate(11, 1, 300_000)
                },
                QualityCandidate {
                    candidate_id: 12,
                    preference_rank: 2,
                    estimated_geometry_bytes: 900_000,
                    estimated_vertices: 9_000,
                    estimated_indices: 27_000,
                    estimated_frame_time_ns: 10_000_000,
                    estimated_cpu_time_ns: 3_500_000,
                    estimated_gpu_time_ns: 7_000_000,
                    estimated_cpu_memory_bytes: 450_000,
                    estimated_gpu_memory_bytes: 800_000,
                    ..resource_candidate(12, 2, 900_000)
                },
            ],
        };
        let high_fidelity = candidate(10, 1_000, true);
        let efficient = candidate(11, 2_500, false);
        let dominated = candidate(12, 4_000, true);
        let semantic = SemanticLodCandidateSet {
            candidates: vec![high_fidelity, efficient, dominated],
        };
        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &caps,
            &profile(),
            &semantic,
        )
        .unwrap();
        assert_eq!(decision.pareto_candidate_ids(), &[10, 11]);
        assert_eq!(decision.selected_candidate_id(), Some(10));
    }

    #[test]
    fn pareto_frontier_is_bound_into_canonical_decision_bytes() {
        let request = quality_request(1_000_000);
        let caps = capabilities();
        let semantic_profile = profile();
        let semantic_candidates = candidates();
        let decision = negotiate_semantic_lod(
            &request,
            &caps,
            &semantic_profile,
            &semantic_candidates,
        )
        .unwrap();
        assert!(!decision.canonical_bytes().unwrap().is_empty());
        let mut tampered = decision;
        tampered.pareto_candidate_ids.clear();
        assert_eq!(
            tampered.canonical_bytes(),
            Err(SemanticLodError::InvalidDecision)
        );
    }

    #[test]
    fn missing_required_eye_is_semantically_infeasible_even_when_global_silhouette_is_good() {
        let mut set = candidates();
        set.candidates[0].observations[1].present = false;
        set.candidates[0].observations[1].projected_diameter_millipixels = 0;
        set.candidates[0].observations[1].max_screen_error_microunits = 0;

        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &profile(),
            &set,
        )
        .unwrap();
        assert_ne!(decision.selected_candidate_id(), Some(1));
    }

    #[test]
    fn feature_error_above_its_per_feature_limit_is_infeasible() {
        let mut set = candidates();
        set.candidates[0].observations[1].max_screen_error_microunits = 5_001;

        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &profile(),
            &set,
        )
        .unwrap();
        assert_eq!(decision.selected_candidate_id(), Some(3));
    }

    #[test]
    fn missing_required_nose_and_undersized_eye_are_semantically_infeasible() {
        let mut set = candidates();
        let nose = &mut set.candidates[0].observations[3];
        nose.present = false;
        nose.projected_diameter_millipixels = 0;
        nose.max_screen_error_microunits = 0;

        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &profile(),
            &set,
        )
        .unwrap();
        assert_ne!(decision.selected_candidate_id(), Some(1));

        let mut set = candidates();
        set.candidates[0].observations[1].projected_diameter_millipixels = 499;
        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &profile(),
            &set,
        )
        .unwrap();
        assert_ne!(decision.selected_candidate_id(), Some(1));
    }

    #[test]
    fn weighted_distortion_ceiling_is_a_hard_semantic_constraint() {
        let mut strict = profile();
        strict.max_weighted_distortion_milliunits = 1;

        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &strict,
            &candidates(),
        )
        .unwrap();
        assert_eq!(
            decision.infeasible_reason(),
            Some(SemanticLodInfeasibleReason::NoCandidatePreservesSemanticConstraints)
        );
    }

    #[test]
    fn optional_feature_requires_a_nonzero_missing_penalty() {
        let mut invalid = profile();
        invalid.missing_optional_penalty_microunits = 0;
        assert_eq!(
            invalid.validate(),
            Err(SemanticLodError::InvalidFeatureConstraint)
        );
    }

    #[test]
    fn equal_distortion_uses_resource_preference_rank_then_candidate_id() {
        let caps = CapabilitySnapshot {
            schema_version: super::super::CAPABILITY_SCHEMA_VERSION,
            revision: 3,
            candidates: vec![
                resource_candidate(2, 0, 500_000),
                resource_candidate(1, 1, 800_000),
                resource_candidate(3, 2, 300_000),
            ],
        };
        let same_observations = candidate(1, 4_000, true).observations;
        let set = SemanticLodCandidateSet {
            candidates: vec![
                SemanticLodCandidate {
                    candidate_id: 2,
                    observations: same_observations.clone(),
                },
                SemanticLodCandidate {
                    candidate_id: 1,
                    observations: same_observations.clone(),
                },
                SemanticLodCandidate {
                    candidate_id: 3,
                    observations: same_observations,
                },
            ],
        };

        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &caps,
            &profile(),
            &set,
        )
        .unwrap();
        assert_eq!(decision.selected_candidate_id(), Some(2));
    }

    #[test]
    fn verifier_rejects_decision_replayed_with_changed_semantic_profile() {
        let request = quality_request(1_000_000);
        let caps = capabilities();
        let profile_a = profile();
        let candidates = candidates();
        let decision =
            negotiate_semantic_lod(&request, &caps, &profile_a, &candidates).unwrap();

        let mut profile_b = profile_a.clone();
        profile_b.features[1].importance_weight_milli += 1;
        assert_eq!(
            verify_semantic_lod_decision(
                &request,
                &caps,
                &profile_b,
                &candidates,
                &decision
            ),
            Err(SemanticLodError::DecisionMismatch)
        );
    }

    #[test]
    fn optional_missing_mouth_is_penalized_but_can_remain_feasible() {
        let mut set = candidates();
        set.candidates[0].observations[4].present = false;
        set.candidates[0].observations[4].projected_diameter_millipixels = 0;
        set.candidates[0].observations[4].max_screen_error_microunits = 0;

        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &profile(),
            &set,
        )
        .unwrap();
        assert!(decision.is_feasible());
        assert!(decision.weighted_distortion_milliunits().unwrap() >= 12_000_000);
    }

    #[test]
    fn selection_minimizes_semantic_distortion_not_triangle_preference_rank() {
        let decision = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &profile(),
            &candidates(),
        )
        .unwrap();
        assert_eq!(decision.selected_candidate_id(), Some(3));
        assert_eq!(
            decision.weighted_distortion_milliunits(),
            Some(40_000_000)
        );
    }

    #[test]
    fn tighter_resource_budget_reselects_the_best_remaining_semantic_candidate() {
        let loose = negotiate_semantic_lod(
            &quality_request(1_000_000),
            &capabilities(),
            &profile(),
            &candidates(),
        )
        .unwrap();
        let tight = negotiate_semantic_lod(
            &quality_request(500_000),
            &capabilities(),
            &profile(),
            &candidates(),
        )
        .unwrap();
        assert_eq!(loose.selected_candidate_id(), Some(3));
        assert_eq!(tight.selected_candidate_id(), Some(3));

        let very_tight = negotiate_semantic_lod(
            &quality_request(250_000),
            &capabilities(),
            &profile(),
            &candidates(),
        )
        .unwrap();
        assert_eq!(
            very_tight.infeasible_reason(),
            Some(SemanticLodInfeasibleReason::NoResourceFeasibleCandidate)
        );
    }

    #[test]
    fn candidate_ids_and_feature_observation_order_must_match_the_frozen_inputs() {
        let mut set = candidates();
        set.candidates[1].candidate_id = 99;
        assert_eq!(
            negotiate_semantic_lod(
                &quality_request(1_000_000),
                &capabilities(),
                &profile(),
                &set
            ),
            Err(SemanticLodError::CandidateSetMismatch)
        );

        let mut set = candidates();
        set.candidates[0].observations.swap(1, 2);
        assert_eq!(
            negotiate_semantic_lod(
                &quality_request(1_000_000),
                &capabilities(),
                &profile(),
                &set
            ),
            Err(SemanticLodError::ObservationSetMismatch)
        );
    }

    #[test]
    fn absent_optional_feature_must_not_smuggle_nonzero_measurements() {
        let mut set = candidates();
        set.candidates[0].observations[4].present = false;
        set.candidates[0].observations[4].max_screen_error_microunits = 1;
        assert_eq!(
            set.validate(&profile(), &capabilities()),
            Err(SemanticLodError::InvalidMissingFeatureObservation)
        );
    }

    #[test]
    fn decision_is_deterministic_canonical_and_recomputed_at_verification() {
        let request = quality_request(1_000_000);
        let capabilities = capabilities();
        let profile = profile();
        let candidates = candidates();
        let first =
            negotiate_semantic_lod(&request, &capabilities, &profile, &candidates).unwrap();
        let second =
            negotiate_semantic_lod(&request, &capabilities, &profile, &candidates).unwrap();
        assert_eq!(
            first.canonical_bytes().unwrap(),
            second.canonical_bytes().unwrap()
        );
        assert!(verify_semantic_lod_decision(
            &request,
            &capabilities,
            &profile,
            &candidates,
            &first
        )
        .is_ok());

        let mut tampered = first;
        tampered.selected_candidate_id = Some(1);
        assert_eq!(
            verify_semantic_lod_decision(
                &request,
                &capabilities,
                &profile,
                &candidates,
                &tampered
            ),
            Err(SemanticLodError::DecisionMismatch)
        );
    }

    #[test]
    fn more_permissive_resource_budget_cannot_select_worse_distortion() {
        let mut caps = capabilities();
        caps.candidates.reverse();
        caps.candidates
            .sort_by_key(|candidate| candidate.preference_rank);
        let set = candidates();

        let tight = negotiate_semantic_lod(&quality_request(500_000), &caps, &profile(), &set)
            .unwrap();
        let loose = negotiate_semantic_lod(&quality_request(1_000_000), &caps, &profile(), &set)
            .unwrap();
        assert!(
            loose.weighted_distortion_milliunits().unwrap()
                <= tight.weighted_distortion_milliunits().unwrap()
        );
    }
}
