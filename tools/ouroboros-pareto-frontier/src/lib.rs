#![forbid(unsafe_code)]
//! Explainable, deterministic, uncertainty-aware Pareto frontier oracle.
//!
//! Hard resource feasibility is evaluated before soft-objective dominance.
//! Unknown values remain unknown; estimates/declared values remain explicitly
//! provisional. This crate does not invent missing telemetry or collapse
//! incomparable objectives into an undocumented scalar score.

use std::{error::Error, fmt};

pub const FRONTIER_SCHEMA_VERSION: u16 = 1;
pub const MAX_CANDIDATES: usize = 128;
pub const MAX_OBJECTIVES: usize = 16;
pub const MAX_RESOURCE_OBSERVATIONS: usize = 16;
pub const MAX_CANONICAL_BYTES: usize = 4 * 1024 * 1024;
const REQUEST_MAGIC: [u8; 8] = *b"OUROREQ1";
const CANDIDATE_MAGIC: [u8; 8] = *b"OUROCAN1";
const RESULT_MAGIC: [u8; 8] = *b"OUROPRF1";

pub type Digest = [u8; 32];
pub type CandidateId = [u8; 16];

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ObjectiveKind {
    VisualFidelityPpm = 1,
    IdentityRecognitionPpm = 2,
    MotionContinuityPpm = 3,
    SilhouetteErrorMicrounits = 4,
    LandmarkErrorMicrounits = 5,
    FrameTimeP95Ns = 6,
    PresentationLatencyP95Ns = 7,
    EnergyPerPresentedFrameNanojoules = 8,
    DroppedFramesPerMillion = 9,
}

impl ObjectiveKind {
    fn higher_is_better(self) -> bool {
        matches!(
            self,
            Self::VisualFidelityPpm
                | Self::IdentityRecognitionPpm
                | Self::MotionContinuityPpm
        )
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum ResourceKind {
    GeometryBytes = 1,
    VertexCount = 2,
    IndexCount = 3,
    FrameTimeP95Ns = 4,
    CpuComputeTimeP95Ns = 5,
    GpuComputeTimeP95Ns = 6,
    CpuMemoryBytes = 7,
    GpuMemoryBytes = 8,
    EnergyPerPresentedFrameNanojoules = 9,
    InputToPresentedLatencyNs = 10,
    DroppedFramesPerMillion = 11,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ProvenanceKind {
    Measured = 1,
    Estimated = 2,
    Declared = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricProvenance {
    pub kind: ProvenanceKind,
    /// Digest of the raw measurement, estimator model or declaration source.
    pub source_digest: Digest,
}

impl MetricProvenance {
    pub fn validate(self) -> Result<(), FrontierError> {
        if is_zero(&self.source_digest) {
            return Err(FrontierError::InvalidMetricProvenance);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricValue {
    pub value: u64,
    /// Symmetric uncertainty envelope in the same declared unit as value.
    pub uncertainty: u64,
    pub provenance: MetricProvenance,
}

impl MetricValue {
    pub fn validate(self) -> Result<(), FrontierError> {
        self.provenance.validate()?;
        self.value
            .checked_add(self.uncertainty)
            .ok_or(FrontierError::MetricRangeOverflow)?;
        Ok(())
    }

    pub fn lower_bound(self) -> u64 {
        self.value.saturating_sub(self.uncertainty)
    }

    pub fn upper_bound(self) -> Result<u64, FrontierError> {
        self.value
            .checked_add(self.uncertainty)
            .ok_or(FrontierError::MetricRangeOverflow)
    }

    fn is_measured(self) -> bool {
        self.provenance.kind == ProvenanceKind::Measured
    }

    fn canonical_fields(self, out: &mut Vec<u8>) {
        put_u64(out, self.value);
        put_u64(out, self.uncertainty);
        out.push(self.provenance.kind as u8);
        out.extend_from_slice(&self.provenance.source_digest);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ObjectiveObservation {
    pub kind: ObjectiveKind,
    pub metric: MetricValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResourceObservation {
    pub kind: ResourceKind,
    pub metric: MetricValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct HardLimit {
    pub kind: ResourceKind,
    /// Resource metrics are costs; lower is better.
    pub maximum: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontierRequest {
    pub schema_version: u16,
    pub policy_revision: u64,
    pub policy_digest: Digest,
    /// Canonically sorted, unique objective dimensions; no implicit weights.
    pub objectives: Vec<ObjectiveKind>,
    /// Canonically sorted, unique hard resource ceilings.
    pub hard_limits: Vec<HardLimit>,
}

impl FrontierRequest {
    pub fn validate(&self) -> Result<(), FrontierError> {
        if self.schema_version != FRONTIER_SCHEMA_VERSION {
            return Err(FrontierError::UnsupportedSchema);
        }
        if self.policy_revision == 0 || is_zero(&self.policy_digest) {
            return Err(FrontierError::InvalidPolicyIdentity);
        }
        if self.objectives.is_empty() || self.objectives.len() > MAX_OBJECTIVES {
            return Err(FrontierError::InvalidObjectiveSet);
        }
        if !strictly_increasing(&self.objectives) {
            return Err(FrontierError::NonCanonicalObjectiveOrder);
        }
        if self.hard_limits.len() > MAX_RESOURCE_OBSERVATIONS {
            return Err(FrontierError::TooManyHardLimits);
        }
        if !self
            .hard_limits
            .windows(2)
            .all(|pair| pair[0].kind < pair[1].kind)
        {
            return Err(FrontierError::NonCanonicalHardLimitOrder);
        }
        Ok(())
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, FrontierError> {
        self.validate()?;
        let mut out = Vec::new();
        out.extend_from_slice(&REQUEST_MAGIC);
        put_u16(&mut out, self.schema_version);
        put_u64(&mut out, self.policy_revision);
        out.extend_from_slice(&self.policy_digest);
        put_u16(
            &mut out,
            u16::try_from(self.objectives.len()).map_err(|_| FrontierError::SizeOverflow)?,
        );
        for objective in &self.objectives {
            out.push(*objective as u8);
        }
        put_u16(
            &mut out,
            u16::try_from(self.hard_limits.len()).map_err(|_| FrontierError::SizeOverflow)?,
        );
        for limit in &self.hard_limits {
            out.push(limit.kind as u8);
            put_u64(&mut out, limit.maximum);
        }
        Ok(out)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct QualityCandidate {
    pub candidate_id: CandidateId,
    /// Partial observations are allowed; missing requested objectives result
    /// in Unverified, never an implicit zero.
    pub objectives: Vec<ObjectiveObservation>,
    /// Partial resource observations are allowed; a missing hard-limited
    /// resource results in Unverified.
    pub resources: Vec<ResourceObservation>,
}

impl QualityCandidate {
    pub fn validate(&self, request: &FrontierRequest) -> Result<(), FrontierError> {
        if is_zero(&self.candidate_id) {
            return Err(FrontierError::InvalidCandidateIdentity);
        }
        if self.objectives.len() > request.objectives.len()
            || self.resources.len() > MAX_RESOURCE_OBSERVATIONS
        {
            return Err(FrontierError::TooManyCandidateMetrics);
        }
        if !self
            .objectives
            .windows(2)
            .all(|pair| pair[0].kind < pair[1].kind)
        {
            return Err(FrontierError::NonCanonicalObjectiveObservationOrder);
        }
        if !self
            .resources
            .windows(2)
            .all(|pair| pair[0].kind < pair[1].kind)
        {
            return Err(FrontierError::NonCanonicalResourceObservationOrder);
        }
        for observation in &self.objectives {
            if request.objectives.binary_search(&observation.kind).is_err() {
                return Err(FrontierError::UnexpectedObjectiveObservation);
            }
            observation.metric.validate()?;
        }
        for observation in &self.resources {
            observation.metric.validate()?;
        }
        Ok(())
    }

    fn objective(&self, kind: ObjectiveKind) -> Option<MetricValue> {
        self.objectives
            .binary_search_by_key(&kind, |observation| observation.kind)
            .ok()
            .map(|index| self.objectives[index].metric)
    }

    fn resource(&self, kind: ResourceKind) -> Option<MetricValue> {
        self.resources
            .binary_search_by_key(&kind, |observation| observation.kind)
            .ok()
            .map(|index| self.resources[index].metric)
    }

    fn canonical_bytes(&self) -> Result<Vec<u8>, FrontierError> {
        let mut out = Vec::new();
        out.extend_from_slice(&CANDIDATE_MAGIC);
        out.extend_from_slice(&self.candidate_id);
        put_u16(
            &mut out,
            u16::try_from(self.objectives.len()).map_err(|_| FrontierError::SizeOverflow)?,
        );
        for observation in &self.objectives {
            out.push(observation.kind as u8);
            observation.metric.canonical_fields(&mut out);
        }
        put_u16(
            &mut out,
            u16::try_from(self.resources.len()).map_err(|_| FrontierError::SizeOverflow)?,
        );
        for observation in &self.resources {
            out.push(observation.kind as u8);
            observation.metric.canonical_fields(&mut out);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum Feasibility {
    FeasibleMeasured = 1,
    FeasibleProvisional = 2,
    Infeasible = 3,
    Unverified = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AssessmentReason {
    MissingHardMetric(ResourceKind),
    DefiniteHardLimitViolation(ResourceKind),
    UncertaintyCrossesHardLimit(ResourceKind),
    MissingObjective(ObjectiveKind),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CandidateAssessment {
    pub candidate_id: CandidateId,
    pub feasibility: Feasibility,
    /// Empty only when no feasible candidate dominates this candidate.
    pub dominated_by: Vec<CandidateId>,
    pub reasons: Vec<AssessmentReason>,
}

impl CandidateAssessment {
    pub fn is_in_frontier(&self) -> bool {
        matches!(
            self.feasibility,
            Feasibility::FeasibleMeasured | Feasibility::FeasibleProvisional
        ) && self.dominated_by.is_empty()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FrontierResult {
    pub request: FrontierRequest,
    /// Candidates are sorted by stable ID before storage/serialization.
    pub candidates: Vec<QualityCandidate>,
    /// Assessments are in the same stable-ID order.
    pub assessments: Vec<CandidateAssessment>,
}

impl FrontierResult {
    pub fn frontier_ids(&self) -> Vec<CandidateId> {
        self.assessments
            .iter()
            .filter(|assessment| assessment.is_in_frontier())
            .map(|assessment| assessment.candidate_id)
            .collect()
    }

    pub fn feasible_ids(&self) -> Vec<CandidateId> {
        self.assessments
            .iter()
            .filter(|assessment| {
                matches!(
                    assessment.feasibility,
                    Feasibility::FeasibleMeasured | Feasibility::FeasibleProvisional
                )
            })
            .map(|assessment| assessment.candidate_id)
            .collect()
    }

    pub fn canonical_bytes(&self) -> Result<Vec<u8>, FrontierError> {
        self.request.validate()?;
        if self.candidates.len() != self.assessments.len()
            || self.candidates.len() > MAX_CANDIDATES
        {
            return Err(FrontierError::InvalidResultShape);
        }
        let request = self.request.canonical_bytes()?;
        let mut out = Vec::new();
        out.extend_from_slice(&RESULT_MAGIC);
        put_u32(
            &mut out,
            u32::try_from(request.len()).map_err(|_| FrontierError::SizeOverflow)?,
        );
        out.extend_from_slice(&request);
        put_u16(
            &mut out,
            u16::try_from(self.candidates.len()).map_err(|_| FrontierError::SizeOverflow)?,
        );
        for (index, candidate) in self.candidates.iter().enumerate() {
            if index > 0
                && self.candidates[index - 1].candidate_id >= candidate.candidate_id
            {
                return Err(FrontierError::NonCanonicalCandidateOrder);
            }
            let candidate_bytes = candidate.canonical_bytes()?;
            put_u32(
                &mut out,
                u32::try_from(candidate_bytes.len()).map_err(|_| FrontierError::SizeOverflow)?,
            );
            out.extend_from_slice(&candidate_bytes);
        }
        put_u16(
            &mut out,
            u16::try_from(self.assessments.len()).map_err(|_| FrontierError::SizeOverflow)?,
        );
        for (index, assessment) in self.assessments.iter().enumerate() {
            if assessment.candidate_id != self.candidates[index].candidate_id {
                return Err(FrontierError::InvalidResultShape);
            }
            out.extend_from_slice(&assessment.candidate_id);
            out.push(assessment.feasibility as u8);
            out.push(u8::from(assessment.is_in_frontier()));
            put_u16(
                &mut out,
                u16::try_from(assessment.dominated_by.len())
                    .map_err(|_| FrontierError::SizeOverflow)?,
            );
            for dominated in &assessment.dominated_by {
                out.extend_from_slice(dominated);
            }
            put_u16(
                &mut out,
                u16::try_from(assessment.reasons.len()).map_err(|_| FrontierError::SizeOverflow)?,
            );
            for reason in &assessment.reasons {
                match reason {
                    AssessmentReason::MissingHardMetric(kind) => {
                        out.push(1);
                        out.push(*kind as u8);
                    }
                    AssessmentReason::DefiniteHardLimitViolation(kind) => {
                        out.push(2);
                        out.push(*kind as u8);
                    }
                    AssessmentReason::UncertaintyCrossesHardLimit(kind) => {
                        out.push(3);
                        out.push(*kind as u8);
                    }
                    AssessmentReason::MissingObjective(kind) => {
                        out.push(4);
                        out.push(*kind as u8);
                    }
                }
            }
        }
        if out.len() > MAX_CANONICAL_BYTES {
            return Err(FrontierError::ResultTooLarge);
        }
        Ok(out)
    }
}

pub fn evaluate_frontier(
    request: &FrontierRequest,
    candidates: &[QualityCandidate],
) -> Result<FrontierResult, FrontierError> {
    request.validate()?;
    if candidates.is_empty() || candidates.len() > MAX_CANDIDATES {
        return Err(FrontierError::InvalidCandidateCount);
    }
    let mut sorted = candidates.to_vec();
    sorted.sort_by_key(|candidate| candidate.candidate_id);
    for (index, candidate) in sorted.iter().enumerate() {
        candidate.validate(request)?;
        if index > 0 && sorted[index - 1].candidate_id == candidate.candidate_id {
            return Err(FrontierError::DuplicateCandidateId);
        }
    }

    let mut assessments = Vec::with_capacity(sorted.len());
    for candidate in &sorted {
        assessments.push(assess_candidate(request, candidate)?);
    }

    for target_index in 0..sorted.len() {
        if !is_feasible(assessments[target_index].feasibility) {
            continue;
        }
        let mut dominators = Vec::new();
        for source_index in 0..sorted.len() {
            if source_index == target_index
                || !is_feasible(assessments[source_index].feasibility)
            {
                continue;
            }
            if dominates(
                &sorted[source_index],
                &sorted[target_index],
                &request.objectives,
            )? {
                dominators.push(sorted[source_index].candidate_id);
            }
        }
        dominators.sort();
        assessments[target_index].dominated_by = dominators;
    }

    Ok(FrontierResult {
        request: request.clone(),
        candidates: sorted,
        assessments,
    })
}

/// Recompute rather than trusting a stored frontier result.
pub fn verify_frontier_result(result: &FrontierResult) -> Result<(), FrontierError> {
    let recomputed = evaluate_frontier(&result.request, &result.candidates)?;
    if recomputed != *result {
        return Err(FrontierError::DecisionMismatch);
    }
    Ok(())
}

fn assess_candidate(
    request: &FrontierRequest,
    candidate: &QualityCandidate,
) -> Result<CandidateAssessment, FrontierError> {
    let mut reasons = Vec::new();
    let mut definite_violation = false;
    let mut unverified = false;
    let mut provisional = false;

    for limit in &request.hard_limits {
        match candidate.resource(limit.kind) {
            None => {
                reasons.push(AssessmentReason::MissingHardMetric(limit.kind));
                unverified = true;
            }
            Some(metric) => {
                let lower = metric.lower_bound();
                let upper = metric.upper_bound()?;
                if lower > limit.maximum {
                    reasons.push(AssessmentReason::DefiniteHardLimitViolation(limit.kind));
                    definite_violation = true;
                } else if upper > limit.maximum {
                    reasons.push(AssessmentReason::UncertaintyCrossesHardLimit(limit.kind));
                    unverified = true;
                }
                if !metric.is_measured() {
                    provisional = true;
                }
            }
        }
    }

    for objective in &request.objectives {
        match candidate.objective(*objective) {
            None => {
                reasons.push(AssessmentReason::MissingObjective(*objective));
                unverified = true;
            }
            Some(metric) => {
                metric.upper_bound()?;
                if !metric.is_measured() {
                    provisional = true;
                }
            }
        }
    }

    let feasibility = if definite_violation {
        Feasibility::Infeasible
    } else if unverified {
        Feasibility::Unverified
    } else if provisional {
        Feasibility::FeasibleProvisional
    } else {
        Feasibility::FeasibleMeasured
    };

    Ok(CandidateAssessment {
        candidate_id: candidate.candidate_id,
        feasibility,
        dominated_by: Vec::new(),
        reasons,
    })
}

fn dominates(
    a: &QualityCandidate,
    b: &QualityCandidate,
    objectives: &[ObjectiveKind],
) -> Result<bool, FrontierError> {
    let mut strictly_better = false;
    let mut identical = true;

    for objective in objectives {
        let left = a
            .objective(*objective)
            .ok_or(FrontierError::MissingObjectiveDuringDominance)?;
        let right = b
            .objective(*objective)
            .ok_or(FrontierError::MissingObjectiveDuringDominance)?;
        if left != right {
            identical = false;
        }

        if objective.higher_is_better() {
            let left_worst = left.lower_bound();
            let right_best = right.upper_bound()?;
            if left_worst < right_best {
                return Ok(false);
            }
            if left_worst > right_best {
                strictly_better = true;
            }
        } else {
            let left_worst = left.upper_bound()?;
            let right_best = right.lower_bound();
            if left_worst > right_best {
                return Ok(false);
            }
            if left_worst < right_best {
                strictly_better = true;
            }
        }
    }

    // Exact duplicate vectors resolve by stable candidate ID, never input order.
    Ok(strictly_better || (identical && a.candidate_id < b.candidate_id))
}

fn is_feasible(value: Feasibility) -> bool {
    matches!(
        value,
        Feasibility::FeasibleMeasured | Feasibility::FeasibleProvisional
    )
}

fn strictly_increasing<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FrontierError {
    UnsupportedSchema,
    InvalidPolicyIdentity,
    InvalidObjectiveSet,
    NonCanonicalObjectiveOrder,
    TooManyHardLimits,
    NonCanonicalHardLimitOrder,
    InvalidCandidateCount,
    InvalidCandidateIdentity,
    TooManyCandidateMetrics,
    NonCanonicalObjectiveObservationOrder,
    NonCanonicalResourceObservationOrder,
    UnexpectedObjectiveObservation,
    InvalidMetricProvenance,
    MetricRangeOverflow,
    DuplicateCandidateId,
    MissingObjectiveDuringDominance,
    InvalidResultShape,
    NonCanonicalCandidateOrder,
    SizeOverflow,
    ResultTooLarge,
    DecisionMismatch,
}

impl fmt::Display for FrontierError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for FrontierError {}

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

    fn provenance(kind: ProvenanceKind, seed: u8) -> MetricProvenance {
        MetricProvenance {
            kind,
            source_digest: [seed; 32],
        }
    }

    fn measured(value: u64, uncertainty: u64, seed: u8) -> MetricValue {
        MetricValue {
            value,
            uncertainty,
            provenance: provenance(ProvenanceKind::Measured, seed),
        }
    }

    fn estimated(value: u64, uncertainty: u64, seed: u8) -> MetricValue {
        MetricValue {
            value,
            uncertainty,
            provenance: provenance(ProvenanceKind::Estimated, seed),
        }
    }

    fn request(hard_limits: Vec<HardLimit>) -> FrontierRequest {
        FrontierRequest {
            schema_version: FRONTIER_SCHEMA_VERSION,
            policy_revision: 1,
            policy_digest: [90; 32],
            objectives: vec![
                ObjectiveKind::VisualFidelityPpm,
                ObjectiveKind::EnergyPerPresentedFrameNanojoules,
            ],
            hard_limits,
        }
    }

    fn candidate(
        id: u8,
        fidelity: MetricValue,
        energy: MetricValue,
        frame_time: MetricValue,
    ) -> QualityCandidate {
        QualityCandidate {
            candidate_id: [id; 16],
            objectives: vec![
                ObjectiveObservation {
                    kind: ObjectiveKind::VisualFidelityPpm,
                    metric: fidelity,
                },
                ObjectiveObservation {
                    kind: ObjectiveKind::EnergyPerPresentedFrameNanojoules,
                    metric: energy,
                },
            ],
            resources: vec![ResourceObservation {
                kind: ResourceKind::FrameTimeP95Ns,
                metric: frame_time,
            }],
        }
    }

    fn frame_limit(maximum: u64) -> Vec<HardLimit> {
        vec![HardLimit {
            kind: ResourceKind::FrameTimeP95Ns,
            maximum,
        }]
    }

    #[test]
    fn better_on_every_objective_dominates() {
        let req = request(frame_limit(20));
        let a = candidate(
            1,
            measured(900_000, 0, 1),
            measured(100, 0, 2),
            measured(10, 0, 3),
        );
        let b = candidate(
            2,
            measured(800_000, 0, 4),
            measured(120, 0, 5),
            measured(11, 0, 6),
        );
        let result = evaluate_frontier(&req, &[b, a]).unwrap();
        assert_eq!(result.frontier_ids(), vec![[1; 16]]);
        assert_eq!(
            result.assessments[1].dominated_by,
            vec![[1; 16]]
        );
    }

    #[test]
    fn incomparable_quality_energy_tradeoff_keeps_both_frontier_points() {
        let req = request(frame_limit(20));
        let high_visual = candidate(
            1,
            measured(950_000, 0, 1),
            measured(150, 0, 2),
            measured(10, 0, 3),
        );
        let low_energy = candidate(
            2,
            measured(850_000, 0, 4),
            measured(80, 0, 5),
            measured(10, 0, 6),
        );
        let result = evaluate_frontier(&req, &[high_visual, low_energy]).unwrap();
        assert_eq!(result.frontier_ids(), vec![[1; 16], [2; 16]]);
    }

    #[test]
    fn overlapping_uncertainty_intervals_remain_incomparable() {
        let req = request(frame_limit(20));
        let a = candidate(
            1,
            measured(900_000, 20_000, 1),
            measured(100, 10, 2),
            measured(10, 0, 3),
        );
        let b = candidate(
            2,
            measured(910_000, 20_000, 4),
            measured(105, 10, 5),
            measured(10, 0, 6),
        );
        let result = evaluate_frontier(&req, &[a, b]).unwrap();
        assert_eq!(result.frontier_ids(), vec![[1; 16], [2; 16]]);
    }

    #[test]
    fn exact_metric_vector_ties_resolve_by_candidate_id() {
        let req = request(frame_limit(20));
        let low = candidate(
            1,
            measured(900_000, 0, 1),
            measured(100, 0, 2),
            measured(10, 0, 3),
        );
        let high = candidate(
            2,
            measured(900_000, 0, 1),
            measured(100, 0, 2),
            measured(10, 0, 3),
        );
        let result = evaluate_frontier(&req, &[high, low]).unwrap();
        assert_eq!(result.frontier_ids(), vec![[1; 16]]);
        assert_eq!(result.assessments[1].dominated_by, vec![[1; 16]]);
    }

    #[test]
    fn definite_hard_budget_violation_is_excluded_before_dominance() {
        let req = request(frame_limit(10));
        let a = candidate(
            1,
            measured(1_000_000, 0, 1),
            measured(50, 0, 2),
            measured(12, 0, 3),
        );
        let b = candidate(
            2,
            measured(800_000, 0, 4),
            measured(100, 0, 5),
            measured(8, 0, 6),
        );
        let result = evaluate_frontier(&req, &[a, b]).unwrap();
        assert_eq!(result.frontier_ids(), vec![[2; 16]]);
        assert_eq!(
            result.assessments[0].feasibility,
            Feasibility::Infeasible
        );
        assert_eq!(
            result.assessments[0].reasons,
            vec![AssessmentReason::DefiniteHardLimitViolation(
                ResourceKind::FrameTimeP95Ns
            )]
        );
    }

    #[test]
    fn uncertainty_crossing_hard_limit_is_unverified_not_feasible() {
        let req = request(frame_limit(10));
        let value = candidate(
            1,
            measured(900_000, 0, 1),
            measured(100, 0, 2),
            measured(9, 2, 3),
        );
        let result = evaluate_frontier(&req, &[value]).unwrap();
        assert_eq!(result.frontier_ids(), Vec::<CandidateId>::new());
        assert_eq!(
            result.assessments[0].feasibility,
            Feasibility::Unverified
        );
        assert_eq!(
            result.assessments[0].reasons,
            vec![AssessmentReason::UncertaintyCrossesHardLimit(
                ResourceKind::FrameTimeP95Ns
            )]
        );
    }

    #[test]
    fn missing_energy_or_hard_metrics_stays_unknown_not_zero() {
        let req = request(vec![
            HardLimit {
                kind: ResourceKind::FrameTimeP95Ns,
                maximum: 20,
            },
            HardLimit {
                kind: ResourceKind::GpuMemoryBytes,
                maximum: 1_000_000,
            },
        ]);
        let mut item = candidate(
            1,
            measured(900_000, 0, 1),
            measured(100, 0, 2),
            measured(10, 0, 3),
        );
        item.objectives.pop();
        let result = evaluate_frontier(&req, &[item]).unwrap();
        assert_eq!(result.assessments[0].feasibility, Feasibility::Unverified);
        assert_eq!(
            result.assessments[0].reasons,
            vec![
                AssessmentReason::MissingHardMetric(ResourceKind::GpuMemoryBytes),
                AssessmentReason::MissingObjective(
                    ObjectiveKind::EnergyPerPresentedFrameNanojoules
                )
            ]
        );
        assert!(result.frontier_ids().is_empty());
    }

    #[test]
    fn estimates_are_provisional_and_never_mislabeled_as_measurements() {
        let req = request(frame_limit(20));
        let item = candidate(
            1,
            measured(900_000, 0, 1),
            estimated(100, 0, 2),
            estimated(10, 0, 3),
        );
        let result = evaluate_frontier(&req, &[item]).unwrap();
        assert_eq!(
            result.assessments[0].feasibility,
            Feasibility::FeasibleProvisional
        );
        assert_eq!(result.frontier_ids(), vec![[1; 16]]);
    }

    #[test]
    fn input_permutation_does_not_change_frontier_or_canonical_result() {
        let req = request(frame_limit(20));
        let candidates = vec![
            candidate(3, measured(800_000, 0, 1), measured(80, 0, 2), measured(10, 0, 3)),
            candidate(1, measured(900_000, 0, 4), measured(100, 0, 5), measured(9, 0, 6)),
            candidate(2, measured(850_000, 0, 7), measured(120, 0, 8), measured(8, 0, 9)),
        ];
        let mut reversed = candidates.clone();
        reversed.reverse();
        let a = evaluate_frontier(&req, &candidates).unwrap();
        let b = evaluate_frontier(&req, &reversed).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
    }

    #[test]
    fn relaxing_hard_budget_never_removes_previously_feasible_candidates() {
        let candidates = vec![
            candidate(1, measured(900_000, 0, 1), measured(100, 0, 2), measured(10, 0, 3)),
            candidate(2, measured(850_000, 0, 4), measured(90, 0, 5), measured(15, 0, 6)),
            candidate(3, measured(800_000, 0, 7), measured(80, 0, 8), measured(30, 0, 9)),
        ];
        let tight = evaluate_frontier(&request(frame_limit(20)), &candidates).unwrap();
        let relaxed = evaluate_frontier(&request(frame_limit(40)), &candidates).unwrap();
        let tight_set = tight.feasible_ids();
        let relaxed_set = relaxed.feasible_ids();
        assert!(tight_set.iter().all(|id| relaxed_set.contains(id)));
    }

    #[test]
    fn frontier_can_shrink_when_newly_feasible_candidate_dominates() {
        let candidates = vec![
            candidate(1, measured(800_000, 0, 1), measured(120, 0, 2), measured(10, 0, 3)),
            candidate(2, measured(900_000, 0, 4), measured(100, 0, 5), measured(30, 0, 6)),
        ];
        let tight = evaluate_frontier(&request(frame_limit(20)), &candidates).unwrap();
        let relaxed = evaluate_frontier(&request(frame_limit(40)), &candidates).unwrap();
        assert_eq!(tight.frontier_ids(), vec![[1; 16]]);
        assert!(relaxed.feasible_ids().contains(&[1; 16]));
        assert_eq!(relaxed.frontier_ids(), vec![[2; 16]]);
        assert_eq!(relaxed.assessments[0].dominated_by, vec![[2; 16]]);
    }

    #[test]
    fn duplicate_ids_and_noncanonical_metric_orders_are_rejected() {
        let req = request(frame_limit(20));
        let a = candidate(1, measured(900_000, 0, 1), measured(100, 0, 2), measured(10, 0, 3));
        let mut b = a.clone();
        assert_eq!(
            evaluate_frontier(&req, &[a.clone(), b.clone()]),
            Err(FrontierError::DuplicateCandidateId)
        );
        b.objectives.reverse();
        assert_eq!(
            evaluate_frontier(&req, &[a, b]),
            Err(FrontierError::NonCanonicalObjectiveObservationOrder)
        );
    }

    #[test]
    fn source_digests_and_metric_ranges_are_required() {
        assert_eq!(
            MetricProvenance {
                kind: ProvenanceKind::Measured,
                source_digest: [0; 32]
            }
            .validate(),
            Err(FrontierError::InvalidMetricProvenance)
        );
        let overflow = MetricValue {
            value: u64::MAX,
            uncertainty: 1,
            provenance: provenance(ProvenanceKind::Measured, 1),
        };
        assert_eq!(overflow.validate(), Err(FrontierError::MetricRangeOverflow));
    }

    #[test]
    fn policy_and_provenance_changes_bind_canonical_result_bytes() {
        let req = request(frame_limit(20));
        let item = candidate(
            1,
            measured(900_000, 0, 1),
            measured(100, 0, 2),
            measured(10, 0, 3),
        );
        let first = evaluate_frontier(&req, &[item.clone()]).unwrap();
        let mut changed = item;
        changed.objectives[0].metric.provenance.source_digest = [99; 32];
        let second = evaluate_frontier(&req, &[changed]).unwrap();
        assert_ne!(
            first.canonical_bytes().unwrap(),
            second.canonical_bytes().unwrap()
        );
    }

    #[test]
    fn frontier_result_is_recomputable_and_forgery_is_rejected() {
        let req = request(frame_limit(20));
        let item = candidate(
            1,
            measured(900_000, 0, 1),
            measured(100, 0, 2),
            measured(10, 0, 3),
        );
        let mut result = evaluate_frontier(&req, &[item]).unwrap();
        assert_eq!(verify_frontier_result(&result), Ok(()));
        result.assessments[0].dominated_by.push([8; 16]);
        assert_eq!(
            verify_frontier_result(&result),
            Err(FrontierError::DecisionMismatch)
        );
    }

    #[test]
    fn request_objectives_and_limits_must_be_canonical() {
        let mut req = request(frame_limit(20));
        req.objectives.reverse();
        assert_eq!(req.validate(), Err(FrontierError::NonCanonicalObjectiveOrder));
        req = request(vec![
            HardLimit {
                kind: ResourceKind::GpuMemoryBytes,
                maximum: 100,
            },
            HardLimit {
                kind: ResourceKind::FrameTimeP95Ns,
                maximum: 10,
            },
        ]);
        assert_eq!(
            req.validate(),
            Err(FrontierError::NonCanonicalHardLimitOrder)
        );
    }
}
