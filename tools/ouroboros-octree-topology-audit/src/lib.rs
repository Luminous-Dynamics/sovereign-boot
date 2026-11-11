#![forbid(unsafe_code)]
//! Independent structural and 26-neighborhood qualification for adaptive octrees.
//!
//! This crate does not generate a mesh. Its strongest status means that the
//! exact-sample octree has a complete dyadic partition, cached-corner parity,
//! no unresolved surface leaf, and 2:1 balance across face, edge and corner
//! contacts. A future transition-template mesher still needs independent
//! topology, degeneracy and self-intersection qualification.

use std::{error::Error, fmt};

use ouroboros_adaptive_sampling::{
    AdaptiveOctreeError, AdaptiveOctreeReport, BalanceStatus, BudgetStopReason, LeafStop,
    MortonKey, OctreeLeaf, MAX_LEAF_CELLS, MAX_RECEIPT_BYTES as MAX_TREE_RECEIPT_BYTES,
};
use ouroboros_cell_intervals::CellStatus;
use ouroboros_scalar_grid::{ScalarGrid, ScalarGridError};

pub const TOPOLOGY_AUDIT_SCHEMA_VERSION: u16 = 1;
pub const TOPOLOGY_AUDIT_POLICY_VERSION: u16 = 1;
pub const TOPOLOGY_AUDIT_MAGIC: [u8; 8] = *b"OUROTCF1";
pub const MAX_TOPOLOGY_PAIR_CHECKS: u64 = 8_400_000;
pub const MAX_TOPOLOGY_AUDIT_RECEIPT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_TOPOLOGY_AUDIT_WORKING_BYTES: u64 = 256 * 1024 * 1024;
pub const UNKNOWN_COUNT: u64 = u64::MAX;

const CELL_CORNERS: [[u32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];
const CELL_EDGES: [[usize; 2]; 12] = [
    [0, 1], [1, 2], [2, 3], [3, 0],
    [4, 5], [5, 6], [6, 7], [7, 4],
    [0, 4], [1, 5], [2, 6], [3, 7],
];
const METRIC_U64_FIELDS: usize = 21;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologyAuditConfig {
    pub max_pair_checks: u64,
    pub max_working_payload_bytes: u64,
    pub max_receipt_bytes: u64,
}

impl Default for TopologyAuditConfig {
    fn default() -> Self {
        Self {
            max_pair_checks: MAX_TOPOLOGY_PAIR_CHECKS,
            max_working_payload_bytes: 128 * 1024 * 1024,
            max_receipt_bytes: 32 * 1024 * 1024,
        }
    }
}

impl TopologyAuditConfig {
    pub fn validate(self) -> Result<(), TopologyAuditError> {
        if self.max_pair_checks == 0
            || self.max_pair_checks > MAX_TOPOLOGY_PAIR_CHECKS
            || self.max_working_payload_bytes == 0
            || self.max_working_payload_bytes > MAX_TOPOLOGY_AUDIT_WORKING_BYTES
            || self.max_receipt_bytes == 0
            || self.max_receipt_bytes > MAX_TOPOLOGY_AUDIT_RECEIPT_BYTES
        {
            return Err(TopologyAuditError::InvalidConfig);
        }
        Ok(())
    }

    fn encode_into(self, out: &mut Vec<u8>) {
        put_u64(out, self.max_pair_checks);
        put_u64(out, self.max_working_payload_bytes);
        put_u64(out, self.max_receipt_bytes);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CheckStatus {
    Pass = 1,
    Fail = 2,
    NotEvaluated = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ReadinessStatus {
    AuditIncomplete = 1,
    TreeIncomplete = 2,
    SurfaceUnresolved = 3,
    ReadyForTransitionTemplateEvaluation = 4,
    NoSurface = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum ContactKind {
    Face = 1,
    Edge = 2,
    Corner = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologyAuditChecks {
    pub leaf_partition: CheckStatus,
    pub sample_cache_and_sign_mask: CheckStatus,
    pub face_two_to_one: CheckStatus,
    pub edge_two_to_one: CheckStatus,
    pub corner_two_to_one: CheckStatus,
    pub surface_leaf_extractability: CheckStatus,
    pub pair_scan: CheckStatus,
}

impl TopologyAuditChecks {
    fn encode_into(self, out: &mut Vec<u8>) {
        out.push(self.leaf_partition as u8);
        out.push(self.sample_cache_and_sign_mask as u8);
        out.push(self.face_two_to_one as u8);
        out.push(self.edge_two_to_one as u8);
        out.push(self.corner_two_to_one as u8);
        out.push(self.surface_leaf_extractability as u8);
        out.push(self.pair_scan as u8);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TopologyAuditMetrics {
    pub leaf_count: u64,
    pub surface_possible_leaf_count: u64,
    pub contourable_surface_leaf_count: u64,
    pub surface_possible_without_crossing_count: u64,
    pub surface_crossing_edge_count: u64,
    pub face_neighbor_pairs: u64,
    pub edge_neighbor_pairs: u64,
    pub corner_neighbor_pairs: u64,
    pub face_two_to_one_violations: u64,
    pub edge_two_to_one_violations: u64,
    pub corner_two_to_one_violations: u64,
    pub total_pair_count: u64,
    pub pair_comparisons_completed: u64,
    pub pair_comparison_budget: u64,
    pub partition_volume_ticks_cubed: u64,
    pub root_volume_ticks_cubed: u64,
    pub maximum_face_level_gap: u64,
    pub maximum_edge_level_gap: u64,
    pub maximum_corner_level_gap: u64,
    pub working_payload_estimate_bytes: u64,
    pub canonical_receipt_estimate_bytes: u64,
}

impl TopologyAuditMetrics {
    fn encode_into(self, out: &mut Vec<u8>) {
        for value in [
            self.leaf_count,
            self.surface_possible_leaf_count,
            self.contourable_surface_leaf_count,
            self.surface_possible_without_crossing_count,
            self.surface_crossing_edge_count,
            self.face_neighbor_pairs,
            self.edge_neighbor_pairs,
            self.corner_neighbor_pairs,
            self.face_two_to_one_violations,
            self.edge_two_to_one_violations,
            self.corner_two_to_one_violations,
            self.total_pair_count,
            self.pair_comparisons_completed,
            self.pair_comparison_budget,
            self.partition_volume_ticks_cubed,
            self.root_volume_ticks_cubed,
            self.maximum_face_level_gap,
            self.maximum_edge_level_gap,
            self.maximum_corner_level_gap,
            self.working_payload_estimate_bytes,
            self.canonical_receipt_estimate_bytes,
        ] {
            put_u64(out, value);
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OctreeTopologyAudit {
    config: TopologyAuditConfig,
    source_tree_receipt: Vec<u8>,
    source_tree_balance_status: BalanceStatus,
    readiness: ReadinessStatus,
    checks: TopologyAuditChecks,
    metrics: TopologyAuditMetrics,
}

impl OctreeTopologyAudit {
    pub fn readiness(&self) -> ReadinessStatus {
        self.readiness
    }

    pub fn checks(&self) -> TopologyAuditChecks {
        self.checks
    }

    pub fn metrics(&self) -> TopologyAuditMetrics {
        self.metrics
    }

    pub fn source_tree_receipt(&self) -> &[u8] {
        &self.source_tree_receipt
    }

    /// Rebuilds the source octree and all topology checks before accepting
    /// this receipt. The comparison is exact, not a trust in stored metrics.
    pub fn verify(
        &self,
        grid: &ScalarGrid,
        tree: &AdaptiveOctreeReport,
    ) -> Result<(), TopologyAuditError> {
        let recomputed = audit_octree_topology(grid, tree, self.config)?;
        if recomputed != *self {
            return Err(TopologyAuditError::ReportMismatch);
        }
        Ok(())
    }

    /// Canonical OUROTCF1 bytes bind the complete OUROOCT1 receipt and every
    /// topology metric/status. No hash or signature is implied.
    pub fn canonical_bytes(
        &self,
        grid: &ScalarGrid,
        tree: &AdaptiveOctreeReport,
    ) -> Result<Vec<u8>, TopologyAuditError> {
        self.verify(grid, tree)?;
        let expected = canonical_receipt_estimate(self.source_tree_receipt.len() as u64)?;
        if expected != self.metrics.canonical_receipt_estimate_bytes
            || expected > self.config.max_receipt_bytes
            || expected > MAX_TOPOLOGY_AUDIT_RECEIPT_BYTES
        {
            return Err(TopologyAuditError::ReceiptBudgetExceeded);
        }
        let capacity = usize::try_from(expected).map_err(|_| TopologyAuditError::SizeOverflow)?;
        let mut out = Vec::new();
        out.try_reserve_exact(capacity).map_err(|_| TopologyAuditError::AllocationFailed)?;
        out.extend_from_slice(&TOPOLOGY_AUDIT_MAGIC);
        put_u16(&mut out, TOPOLOGY_AUDIT_SCHEMA_VERSION);
        put_u16(&mut out, TOPOLOGY_AUDIT_POLICY_VERSION);
        self.config.encode_into(&mut out);
        put_u32(
            &mut out,
            u32::try_from(self.source_tree_receipt.len())
                .map_err(|_| TopologyAuditError::SizeOverflow)?,
        );
        out.extend_from_slice(&self.source_tree_receipt);
        out.push(self.readiness as u8);
        out.push(self.source_tree_balance_status as u8);
        self.checks.encode_into(&mut out);
        self.metrics.encode_into(&mut out);
        if out.len() as u64 != expected {
            return Err(TopologyAuditError::ReceiptLengthMismatch);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TopologyAuditError {
    InvalidConfig,
    SizeOverflow,
    AllocationFailed,
    WorkingBudgetExceeded,
    ReceiptBudgetExceeded,
    TreeReceiptUnavailable,
    InvalidLeafCount,
    InvalidMortonPath,
    LeafOutOfBounds,
    LeafSpanMismatch,
    DuplicateOrOverlappingLeaf,
    IncompletePartition,
    MissingCachedCorner,
    ClassificationMismatch,
    InvalidClassificationBounds,
    InvalidLeafStopState,
    ReportMismatch,
    ReceiptLengthMismatch,
    Grid(ScalarGridError),
    Adaptive(AdaptiveOctreeError),
}

impl fmt::Display for TopologyAuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for TopologyAuditError {}

impl From<ScalarGridError> for TopologyAuditError {
    fn from(value: ScalarGridError) -> Self {
        Self::Grid(value)
    }
}

impl From<AdaptiveOctreeError> for TopologyAuditError {
    fn from(value: AdaptiveOctreeError) -> Self {
        Self::Adaptive(value)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct PairMetrics {
    face_pairs: u64,
    edge_pairs: u64,
    corner_pairs: u64,
    face_violations: u64,
    edge_violations: u64,
    corner_violations: u64,
    max_face_gap: u64,
    max_edge_gap: u64,
    max_corner_gap: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct LeafMetrics {
    surface_possible: u64,
    contourable_surface: u64,
    surface_without_edges: u64,
    crossing_edges: u64,
}

pub fn audit_octree_topology(
    grid: &ScalarGrid,
    tree: &AdaptiveOctreeReport,
    config: TopologyAuditConfig,
) -> Result<OctreeTopologyAudit, TopologyAuditError> {
    config.validate()?;
    grid.validate()?;
    let leaves_count = usize::try_from(tree.metrics().leaf_count)
        .map_err(|_| TopologyAuditError::SizeOverflow)?;
    if leaves_count == 0 || leaves_count > MAX_LEAF_CELLS as usize {
        return Err(TopologyAuditError::InvalidLeafCount);
    }
    let total_pair_count = (leaves_count as u64)
        .checked_mul(leaves_count.saturating_sub(1) as u64)
        .and_then(|value| value.checked_div(2))
        .ok_or(TopologyAuditError::SizeOverflow)?;
    let tree_receipt_estimate = tree.metrics().canonical_receipt_estimate_bytes;
    let working_preflight = estimate_working_payload(
        tree.metrics().working_payload_estimate_bytes,
        tree_receipt_estimate,
        leaves_count as u64,
    )?;
    if working_preflight > config.max_working_payload_bytes {
        return Err(TopologyAuditError::WorkingBudgetExceeded);
    }
    let receipt_preflight = canonical_receipt_estimate(tree_receipt_estimate)?;
    if receipt_preflight > config.max_receipt_bytes {
        return Err(TopologyAuditError::ReceiptBudgetExceeded);
    }

    if tree_receipt_estimate > MAX_TREE_RECEIPT_BYTES {
        return Err(TopologyAuditError::ReceiptBudgetExceeded);
    }
    // Validate the source grid against the sampler's deterministic rebuild
    // only after this audit's leaf, working and receipt budgets are known.
    tree.verify(grid)?;
    let source_tree_receipt = tree.canonical_bytes()?;
    if source_tree_receipt.len() as u64 != tree_receipt_estimate {
        return Err(TopologyAuditError::TreeReceiptUnavailable);
    }
    let mut leaves = Vec::new();
    leaves
        .try_reserve_exact(leaves_count)
        .map_err(|_| TopologyAuditError::AllocationFailed)?;
    leaves.extend(tree.leaves().copied());
    if leaves.len() != leaves_count {
        return Err(TopologyAuditError::InvalidLeafCount);
    }

    let maximum_depth = tree.config().max_depth;
    let scale = 1_u32
        .checked_shl(u32::from(maximum_depth))
        .ok_or(TopologyAuditError::SizeOverflow)?;
    let root_volume = u64::from(scale)
        .checked_mul(u64::from(scale))
        .and_then(|value| value.checked_mul(u64::from(scale)))
        .ok_or(TopologyAuditError::SizeOverflow)?;

    let mut keys = Vec::<(u8, u64)>::new();
    keys.try_reserve_exact(leaves.len())
        .map_err(|_| TopologyAuditError::AllocationFailed)?;
    let mut partition_volume = 0_u64;
    for leaf in &leaves {
        validate_leaf_path(*leaf, maximum_depth, scale)?;
        keys.push((leaf.key.level, leaf.key.path_code));
        let volume = u64::from(leaf.span_ticks)
            .checked_mul(u64::from(leaf.span_ticks))
            .and_then(|value| value.checked_mul(u64::from(leaf.span_ticks)))
            .ok_or(TopologyAuditError::SizeOverflow)?;
        partition_volume = partition_volume
            .checked_add(volume)
            .ok_or(TopologyAuditError::SizeOverflow)?;
        validate_leaf_samples_and_classification(*leaf, grid, tree)?;
    }
    keys.sort_unstable();
    if keys.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(TopologyAuditError::DuplicateOrOverlappingLeaf);
    }
    for (level, path_code) in &keys {
        for ancestor_level in 0..*level {
            let shift = 3_u32
                .checked_mul(u32::from(*level - ancestor_level))
                .ok_or(TopologyAuditError::SizeOverflow)?;
            let ancestor_path = path_code >> shift;
            if keys.binary_search(&(ancestor_level, ancestor_path)).is_ok() {
                return Err(TopologyAuditError::DuplicateOrOverlappingLeaf);
            }
        }
    }
    if partition_volume != root_volume {
        return Err(TopologyAuditError::IncompletePartition);
    }

    let leaf_metrics = count_surface_leaves(&leaves, maximum_depth)?;
    let pair_scan_complete = total_pair_count <= config.max_pair_checks;
    let pair_metrics = if pair_scan_complete {
        scan_all_neighbor_pairs(&leaves)?
    } else {
        PairMetrics::default()
    };

    let checks = TopologyAuditChecks {
        leaf_partition: CheckStatus::Pass,
        sample_cache_and_sign_mask: CheckStatus::Pass,
        face_two_to_one: if !pair_scan_complete {
            CheckStatus::NotEvaluated
        } else if pair_metrics.face_violations == 0 {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        edge_two_to_one: if !pair_scan_complete {
            CheckStatus::NotEvaluated
        } else if pair_metrics.edge_violations == 0 {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        corner_two_to_one: if !pair_scan_complete {
            CheckStatus::NotEvaluated
        } else if pair_metrics.corner_violations == 0 {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        surface_leaf_extractability: if leaf_metrics.surface_possible > 0
            && leaf_metrics.contourable_surface == leaf_metrics.surface_possible
        {
            CheckStatus::Pass
        } else if leaf_metrics.surface_without_edges > 0 {
            CheckStatus::Fail
        } else {
            CheckStatus::NotEvaluated
        },
        pair_scan: if pair_scan_complete {
            CheckStatus::Pass
        } else {
            CheckStatus::NotEvaluated
        },
    };

    let source_metrics = tree.metrics();
    let readiness = if !pair_scan_complete {
        ReadinessStatus::AuditIncomplete
    } else if source_metrics.leaves_budget_stopped > 0
        || source_metrics.leaves_balance_stopped > 0
        || source_metrics.remaining_2_to_1_violations != 0
        || tree.balance_status() != BalanceStatus::Balanced
        || leaves.iter().any(|leaf| {
            matches!(leaf.stop, LeafStop::SurfacePending | LeafStop::BudgetStopped | LeafStop::BalanceStopped)
        })
    {
        ReadinessStatus::TreeIncomplete
    } else if leaf_metrics.surface_possible == 0 {
        ReadinessStatus::NoSurface
    } else if leaf_metrics.surface_without_edges > 0
        || pair_metrics.face_violations > 0
        || pair_metrics.edge_violations > 0
        || pair_metrics.corner_violations > 0
    {
        ReadinessStatus::SurfaceUnresolved
    } else {
        ReadinessStatus::ReadyForTransitionTemplateEvaluation
    };

    let working = estimate_working_payload(
        source_metrics.working_payload_estimate_bytes,
        source_tree_receipt.len() as u64,
        leaves.len() as u64,
    )?;
    if working > config.max_working_payload_bytes {
        return Err(TopologyAuditError::WorkingBudgetExceeded);
    }
    let receipt_estimate = canonical_receipt_estimate(source_tree_receipt.len() as u64)?;
    if receipt_estimate > config.max_receipt_bytes {
        return Err(TopologyAuditError::ReceiptBudgetExceeded);
    }

    let metrics = TopologyAuditMetrics {
        leaf_count: leaves.len() as u64,
        surface_possible_leaf_count: leaf_metrics.surface_possible,
        contourable_surface_leaf_count: leaf_metrics.contourable_surface,
        surface_possible_without_crossing_count: leaf_metrics.surface_without_edges,
        surface_crossing_edge_count: leaf_metrics.crossing_edges,
        face_neighbor_pairs: if pair_scan_complete { pair_metrics.face_pairs } else { UNKNOWN_COUNT },
        edge_neighbor_pairs: if pair_scan_complete { pair_metrics.edge_pairs } else { UNKNOWN_COUNT },
        corner_neighbor_pairs: if pair_scan_complete { pair_metrics.corner_pairs } else { UNKNOWN_COUNT },
        face_two_to_one_violations: if pair_scan_complete { pair_metrics.face_violations } else { UNKNOWN_COUNT },
        edge_two_to_one_violations: if pair_scan_complete { pair_metrics.edge_violations } else { UNKNOWN_COUNT },
        corner_two_to_one_violations: if pair_scan_complete { pair_metrics.corner_violations } else { UNKNOWN_COUNT },
        total_pair_count,
        pair_comparisons_completed: if pair_scan_complete { total_pair_count } else { 0 },
        pair_comparison_budget: config.max_pair_checks,
        partition_volume_ticks_cubed: partition_volume,
        root_volume_ticks_cubed: root_volume,
        maximum_face_level_gap: if pair_scan_complete { pair_metrics.max_face_gap } else { UNKNOWN_COUNT },
        maximum_edge_level_gap: if pair_scan_complete { pair_metrics.max_edge_gap } else { UNKNOWN_COUNT },
        maximum_corner_level_gap: if pair_scan_complete { pair_metrics.max_corner_gap } else { UNKNOWN_COUNT },
        working_payload_estimate_bytes: working,
        canonical_receipt_estimate_bytes: receipt_estimate,
    };
    Ok(OctreeTopologyAudit {
        config,
        source_tree_receipt,
        source_tree_balance_status: tree.balance_status(),
        readiness,
        checks,
        metrics,
    })
}

fn validate_leaf_path(
    leaf: OctreeLeaf,
    maximum_depth: u8,
    scale: u32,
) -> Result<(), TopologyAuditError> {
    let level = leaf.key.level;
    if level > maximum_depth || level > 18 {
        return Err(TopologyAuditError::InvalidMortonPath);
    }
    let bits = 3_u32
        .checked_mul(u32::from(level))
        .ok_or(TopologyAuditError::SizeOverflow)?;
    if bits < 64 && leaf.key.path_code >= (1_u64 << bits) {
        return Err(TopologyAuditError::InvalidMortonPath);
    }
    let remaining_depth = maximum_depth - level;
    let expected_span = 1_u32
        .checked_shl(u32::from(remaining_depth))
        .ok_or(TopologyAuditError::SizeOverflow)?;
    if leaf.span_ticks != expected_span {
        return Err(TopologyAuditError::LeafSpanMismatch);
    }
    let mut cell_coordinate = [0_u32; 3];
    for depth in 0..level {
        let shift = 3_u32
            .checked_mul(u32::from(level - depth - 1))
            .ok_or(TopologyAuditError::SizeOverflow)?;
        let octant = ((leaf.key.path_code >> shift) & 7) as u32;
        for axis in 0..3 {
            cell_coordinate[axis] = (cell_coordinate[axis] << 1) | ((octant >> axis) & 1);
        }
    }
    let mut expected_minimum = [0_u32; 3];
    for axis in 0..3 {
        expected_minimum[axis] = cell_coordinate[axis]
            .checked_mul(expected_span)
            .ok_or(TopologyAuditError::SizeOverflow)?;
    }
    if leaf.minimum_tick != expected_minimum {
        return Err(TopologyAuditError::InvalidMortonPath);
    }
    let maximum_tick = [
        leaf.minimum_tick[0].checked_add(leaf.span_ticks).ok_or(TopologyAuditError::SizeOverflow)?,
        leaf.minimum_tick[1].checked_add(leaf.span_ticks).ok_or(TopologyAuditError::SizeOverflow)?,
        leaf.minimum_tick[2].checked_add(leaf.span_ticks).ok_or(TopologyAuditError::SizeOverflow)?,
    ];
    if (0..3).any(|axis| leaf.minimum_tick[axis] >= maximum_tick[axis] || maximum_tick[axis] > scale) {
        return Err(TopologyAuditError::LeafOutOfBounds);
    }
    Ok(())
}

fn validate_leaf_samples_and_classification(
    leaf: OctreeLeaf,
    grid: &ScalarGrid,
    tree: &AdaptiveOctreeReport,
) -> Result<(), TopologyAuditError> {
    let spec = grid.spec();
    let minimum = spec.minimum_microunits;
    let maximum = spec.maximum_microunits;
    let scale = 1_u32
        .checked_shl(u32::from(tree.config().max_depth))
        .ok_or(TopologyAuditError::SizeOverflow)?;
    let mut values = [0_i32; 8];
    for (index, corner) in CELL_CORNERS.iter().enumerate() {
        let tick = [
            leaf.minimum_tick[0] + corner[0] * leaf.span_ticks,
            leaf.minimum_tick[1] + corner[1] * leaf.span_ticks,
            leaf.minimum_tick[2] + corner[2] * leaf.span_ticks,
        ];
        let point = [
            tick_coordinate(minimum[0], maximum[0], tick[0], scale)?,
            tick_coordinate(minimum[1], maximum[1], tick[1], scale)?,
            tick_coordinate(minimum[2], maximum[2], tick[2], scale)?,
        ];
        values[index] = *tree.samples().get(&point).ok_or(TopologyAuditError::MissingCachedCorner)?;
    }
    let minimum_sample = *values.iter().min().ok_or(TopologyAuditError::ClassificationMismatch)?;
    let maximum_sample = *values.iter().max().ok_or(TopologyAuditError::ClassificationMismatch)?;
    if minimum_sample != leaf.classification.minimum_corner_sample_microunits
        || maximum_sample != leaf.classification.maximum_corner_sample_microunits
    {
        return Err(TopologyAuditError::ClassificationMismatch);
    }
    let mut edge_mask = 0_u16;
    for (index, edge) in CELL_EDGES.iter().enumerate() {
        if (values[edge[0]] < 0) != (values[edge[1]] < 0) {
            edge_mask |= 1_u16 << index;
        }
    }
    if edge_mask != leaf.classification.sign_changing_edge_mask {
        return Err(TopologyAuditError::ClassificationMismatch);
    }
    match leaf.classification.status {
        CellStatus::CertifiedInside => {
            if maximum_sample >= 0 || leaf.classification.upper_bound_microunits >= 0 {
                return Err(TopologyAuditError::InvalidClassificationBounds);
            }
        }
        CellStatus::CertifiedOutside => {
            if minimum_sample <= 0 || leaf.classification.lower_bound_microunits <= 0 {
                return Err(TopologyAuditError::InvalidClassificationBounds);
            }
        }
        CellStatus::SurfacePossible => {
            if leaf.classification.lower_bound_microunits > 0
                || leaf.classification.upper_bound_microunits < 0
            {
                return Err(TopologyAuditError::InvalidClassificationBounds);
            }
        }
        CellStatus::UncertifiedField => {
            return Err(TopologyAuditError::InvalidClassificationBounds);
        }
    }
    let maximum_depth = tree.config().max_depth;
    let stop_valid = match leaf.stop {
        LeafStop::Certified => {
            matches!(leaf.classification.status, CellStatus::CertifiedInside | CellStatus::CertifiedOutside)
                && leaf.stop_reason == BudgetStopReason::None
        }
        LeafStop::SurfacePending => {
            leaf.classification.status == CellStatus::SurfacePossible
                && leaf.key.level < maximum_depth
                && leaf.stop_reason == BudgetStopReason::None
        }
        LeafStop::SurfaceAtMaximumDepth => {
            leaf.classification.status == CellStatus::SurfacePossible
                && leaf.key.level == maximum_depth
                && leaf.stop_reason == BudgetStopReason::None
        }
        LeafStop::BudgetStopped => {
            leaf.classification.status == CellStatus::SurfacePossible
                && leaf.stop_reason != BudgetStopReason::None
        }
        // Balancing may split a certified coarse neighbor as well as a
        // surface leaf. Its field classification remains unchanged.
        LeafStop::BalanceStopped => leaf.stop_reason != BudgetStopReason::None,
    };
    if !stop_valid {
        return Err(TopologyAuditError::InvalidLeafStopState);
    }
    Ok(())
}

fn tick_coordinate(
    minimum: i64,
    maximum: i64,
    tick: u32,
    scale: u32,
) -> Result<i64, TopologyAuditError> {
    if tick > scale || maximum <= minimum {
        return Err(TopologyAuditError::LeafOutOfBounds);
    }
    let span = i128::from(maximum) - i128::from(minimum);
    let offset = span
        .checked_mul(i128::from(tick))
        .ok_or(TopologyAuditError::SizeOverflow)?
        / i128::from(scale);
    i64::try_from(
        i128::from(minimum)
            .checked_add(offset)
            .ok_or(TopologyAuditError::SizeOverflow)?,
    )
    .map_err(|_| TopologyAuditError::SizeOverflow)
}

fn count_surface_leaves(
    leaves: &[OctreeLeaf],
    maximum_depth: u8,
) -> Result<LeafMetrics, TopologyAuditError> {
    let mut metrics = LeafMetrics {
        surface_possible: 0,
        contourable_surface: 0,
        surface_without_edges: 0,
        crossing_edges: 0,
    };
    for leaf in leaves {
        if leaf.classification.status != CellStatus::SurfacePossible {
            continue;
        }
        metrics.surface_possible = metrics.surface_possible
            .checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?;
        let edge_count = u64::from(leaf.classification.sign_changing_edge_mask.count_ones());
        metrics.crossing_edges = metrics.crossing_edges
            .checked_add(edge_count).ok_or(TopologyAuditError::SizeOverflow)?;
        if leaf.key.level != maximum_depth || leaf.stop != LeafStop::SurfaceAtMaximumDepth {
            continue;
        }
        if edge_count == 0 {
            metrics.surface_without_edges = metrics.surface_without_edges
                .checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?;
        } else {
            metrics.contourable_surface = metrics.contourable_surface
                .checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?;
        }
    }
    Ok(metrics)
}

fn scan_all_neighbor_pairs(leaves: &[OctreeLeaf]) -> Result<PairMetrics, TopologyAuditError> {
    let mut metrics = PairMetrics::default();
    for i in 0..leaves.len() {
        for j in (i + 1)..leaves.len() {
            let Some(kind) = contact_kind(leaves[i], leaves[j]) else {
                continue;
            };
            let gap = u64::from(leaves[i].key.level.abs_diff(leaves[j].key.level));
            match kind {
                ContactKind::Face => {
                    metrics.face_pairs = metrics.face_pairs.checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?;
                    metrics.max_face_gap = metrics.max_face_gap.max(gap);
                    if gap > 1 { metrics.face_violations = metrics.face_violations.checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?; }
                }
                ContactKind::Edge => {
                    metrics.edge_pairs = metrics.edge_pairs.checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?;
                    metrics.max_edge_gap = metrics.max_edge_gap.max(gap);
                    if gap > 1 { metrics.edge_violations = metrics.edge_violations.checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?; }
                }
                ContactKind::Corner => {
                    metrics.corner_pairs = metrics.corner_pairs.checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?;
                    metrics.max_corner_gap = metrics.max_corner_gap.max(gap);
                    if gap > 1 { metrics.corner_violations = metrics.corner_violations.checked_add(1).ok_or(TopologyAuditError::SizeOverflow)?; }
                }
            }
        }
    }
    Ok(metrics)
}

fn maximum_tick(leaf: OctreeLeaf) -> [u32; 3] {
    [
        leaf.minimum_tick[0].saturating_add(leaf.span_ticks),
        leaf.minimum_tick[1].saturating_add(leaf.span_ticks),
        leaf.minimum_tick[2].saturating_add(leaf.span_ticks),
    ]
}

fn contact_kind(a: OctreeLeaf, b: OctreeLeaf) -> Option<ContactKind> {
    let a_max = maximum_tick(a);
    let b_max = maximum_tick(b);
    let mut touching_axes = 0_u8;
    for axis in 0..3 {
        if a_max[axis] < b.minimum_tick[axis] || b_max[axis] < a.minimum_tick[axis] {
            return None;
        }
        if a_max[axis] == b.minimum_tick[axis] || b_max[axis] == a.minimum_tick[axis] {
            touching_axes += 1;
        } else if a.minimum_tick[axis].max(b.minimum_tick[axis])
            >= a_max[axis].min(b_max[axis])
        {
            return None;
        }
    }
    match touching_axes {
        1 => Some(ContactKind::Face),
        2 => Some(ContactKind::Edge),
        3 => Some(ContactKind::Corner),
        _ => None,
    }
}

fn estimate_working_payload(
    source_tree_working: u64,
    source_tree_receipt: u64,
    leaf_count: u64,
) -> Result<u64, TopologyAuditError> {
    // The source report remains live, report.verify rebuilds an equivalent
    // report, and the audit keeps a leaf vector plus an ordering vector.
    let two_reports = source_tree_working
        .checked_mul(2)
        .ok_or(TopologyAuditError::SizeOverflow)?;
    let leaf_workspace = leaf_count
        .checked_mul((std::mem::size_of::<OctreeLeaf>() + std::mem::size_of::<(u8, u64)>()) as u64)
        .ok_or(TopologyAuditError::SizeOverflow)?;
    two_reports
        .checked_add(source_tree_receipt)
        .and_then(|value| value.checked_add(leaf_workspace))
        .ok_or(TopologyAuditError::SizeOverflow)
}

fn canonical_receipt_estimate(source_tree_receipt: u64) -> Result<u64, TopologyAuditError> {
    // Magic/schema/policy/config/source length + readiness/tree status/checks
    // + 21 fixed-width u64 metrics + the complete OUROOCT1 source receipt.
    8_u64
        .checked_add(2 + 2 + 24 + 4 + 1 + 1 + 7 + (METRIC_U64_FIELDS as u64) * 8)
        .and_then(|value| value.checked_add(source_tree_receipt))
        .ok_or(TopologyAuditError::SizeOverflow)
}

fn put_u16(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u64(out: &mut Vec<u8>, value: u64) { out.extend_from_slice(&value.to_le_bytes()); }

#[cfg(test)]
mod tests {
    use super::*;
    use ouroboros_adaptive_sampling::{
        build_adaptive_octree, AdaptiveOctreeConfig, MAX_BALANCE_PAIR_CHECKS,
    };
    use ouroboros_scalar_grid::{
        AnalyticField, GridSpec, MAX_CANONICAL_BYTES, MAX_SAMPLES,
    };

    fn sphere_fixture(max_depth: u8) -> (ScalarGrid, AdaptiveOctreeReport) {
        let grid = ScalarGrid::sample_exact(
            GridSpec {
                minimum_microunits: [-2_000_000; 3],
                maximum_microunits: [2_000_000; 3],
                dimensions: [2, 2, 2],
                max_samples: MAX_SAMPLES,
                max_descriptor_bytes: 1_048_576,
                max_canonical_bytes: MAX_CANONICAL_BYTES,
                max_working_payload_bytes: 32 * 1024 * 1024,
            },
            AnalyticField::Sphere {
                center_microunits: [0, 0, 0],
                radius_microunits: 1_500_000,
            },
            0,
        )
        .unwrap();
        let tree = build_adaptive_octree(
            &grid,
            AdaptiveOctreeConfig {
                max_depth,
                max_unique_samples: 32_768,
                max_leaf_cells: 4_096,
                max_working_payload_bytes: 64 * 1024 * 1024,
                max_receipt_bytes: 32 * 1024 * 1024,
                max_balance_pair_checks: MAX_BALANCE_PAIR_CHECKS,
            },
        )
        .unwrap();
        (grid, tree)
    }

    #[test]
    fn exact_sphere_tree_is_independently_partition_and_sample_checked() {
        let (grid, tree) = sphere_fixture(4);
        let audit = audit_octree_topology(&grid, &tree, TopologyAuditConfig::default()).unwrap();
        assert_eq!(audit.checks().leaf_partition, CheckStatus::Pass);
        assert_eq!(audit.checks().sample_cache_and_sign_mask, CheckStatus::Pass);
        assert_eq!(audit.metrics().partition_volume_ticks_cubed, audit.metrics().root_volume_ticks_cubed);
        assert!(audit.metrics().surface_possible_leaf_count > 0);
    }

    #[test]
    fn face_edge_and_corner_contacts_are_classified_independently() {
        let unit = |key: u8, minimum_tick: [u32; 3]| OctreeLeaf {
            key: MortonKey { level: 3, path_code: u64::from(key) },
            minimum_tick,
            span_ticks: 1,
            classification: dummy_surface_classification(),
            stop: LeafStop::SurfaceAtMaximumDepth,
            stop_reason: BudgetStopReason::None,
        };
        let origin = unit(0, [0, 0, 0]);
        let face = unit(1, [1, 0, 0]);
        let edge = unit(2, [1, 1, 0]);
        let corner = unit(3, [1, 1, 1]);
        assert_eq!(contact_kind(origin, face), Some(ContactKind::Face));
        assert_eq!(contact_kind(origin, edge), Some(ContactKind::Edge));
        assert_eq!(contact_kind(origin, corner), Some(ContactKind::Corner));
    }

    fn dummy_surface_classification() -> ouroboros_cell_intervals::CellClassification {
        ouroboros_cell_intervals::CellClassification {
            status: CellStatus::SurfacePossible,
            sign_changing_edge_mask: 1,
            minimum_corner_sample_microunits: -1,
            maximum_corner_sample_microunits: 1,
            lower_bound_microunits: -1,
            upper_bound_microunits: 1,
        }
    }

    fn leaf(key: MortonKey, minimum_tick: [u32; 3], span_ticks: u32) -> OctreeLeaf {
        OctreeLeaf {
            key,
            minimum_tick,
            span_ticks,
            classification: dummy_surface_classification(),
            stop: LeafStop::SurfaceAtMaximumDepth,
            stop_reason: BudgetStopReason::None,
        }
    }

    #[test]
    fn corner_only_two_to_one_violation_is_visible_to_strict_audit() {
        let mut leaves = Vec::new();
        // Six face/edge-adjacent root octants are refined to level 2.
        for root_octant in 1..7_u8 {
            let root = MortonKey::root().child(root_octant).unwrap();
            for child_octant in 0..8_u8 {
                let child = root.child(child_octant).unwrap();
                let root_min = [
                    u32::from(root_octant & 1) * 4,
                    u32::from((root_octant >> 1) & 1) * 4,
                    u32::from((root_octant >> 2) & 1) * 4,
                ];
                let child_min = [
                    root_min[0] + u32::from(child_octant & 1) * 2,
                    root_min[1] + u32::from((child_octant >> 1) & 1) * 2,
                    root_min[2] + u32::from((child_octant >> 2) & 1) * 2,
                ];
                leaves.push(leaf(child, child_min, 2));
            }
        }
        // Root octant 0 is split to level 2; its upper corner child is split
        // again to level 3. The untouched root octant 7 meets it at one corner.
        let root0 = MortonKey::root().child(0).unwrap();
        for child_octant in 0..7_u8 {
            let key = root0.child(child_octant).unwrap();
            let minimum = [
                u32::from(child_octant & 1) * 2,
                u32::from((child_octant >> 1) & 1) * 2,
                u32::from((child_octant >> 2) & 1) * 2,
            ];
            leaves.push(leaf(key, minimum, 2));
        }
        let upper = root0.child(7).unwrap();
        for child_octant in 0..8_u8 {
            let key = upper.child(child_octant).unwrap();
            let minimum = [
                2 + u32::from(child_octant & 1),
                2 + u32::from((child_octant >> 1) & 1),
                2 + u32::from((child_octant >> 2) & 1),
            ];
            leaves.push(leaf(key, minimum, 1));
        }
        leaves.push(leaf(MortonKey::root().child(7).unwrap(), [4, 4, 4], 4));

        let pairs = scan_all_neighbor_pairs(&leaves).unwrap();
        assert_eq!(pairs.face_violations, 0);
        assert_eq!(pairs.edge_violations, 0);
        assert_eq!(pairs.corner_violations, 1);
        assert_eq!(pairs.max_corner_gap, 2);
    }

    #[test]
    fn max_depth_surface_without_crossing_edges_is_not_extractable() {
        let mut leaf = leaf(MortonKey { level: 4, path_code: 0 }, [0, 0, 0], 1);
        leaf.classification.sign_changing_edge_mask = 0;
        let metrics = count_surface_leaves(&[leaf], 4).unwrap();
        assert_eq!(metrics.surface_possible, 1);
        assert_eq!(metrics.contourable_surface, 0);
        assert_eq!(metrics.surface_without_edges, 1);
    }

    #[test]
    fn receipt_estimate_matches_fixed_layout() {
        assert_eq!(canonical_receipt_estimate(0).unwrap(), 217);
    }

    #[test]
    fn pair_scan_budget_does_not_turn_unknown_into_zero() {
        let pair_count = 100_u64;
        let config = TopologyAuditConfig {
            max_pair_checks: 10,
            ..TopologyAuditConfig::default()
        };
        assert!(pair_count > config.max_pair_checks);
        assert_eq!(UNKNOWN_COUNT, u64::MAX);
    }

    #[test]
    fn pair_budget_exhaustion_is_unknown_not_zero_violations() {
        let (grid, tree) = sphere_fixture(3);
        assert!(tree.metrics().leaf_count > 2);
        let audit = audit_octree_topology(
            &grid,
            &tree,
            TopologyAuditConfig {
                max_pair_checks: 1,
                ..TopologyAuditConfig::default()
            },
        )
        .unwrap();
        assert_eq!(audit.readiness(), ReadinessStatus::AuditIncomplete);
        assert_eq!(audit.checks().pair_scan, CheckStatus::NotEvaluated);
        assert_eq!(audit.metrics().face_neighbor_pairs, UNKNOWN_COUNT);
        assert_eq!(audit.metrics().face_two_to_one_violations, UNKNOWN_COUNT);
        assert_eq!(audit.metrics().pair_comparisons_completed, 0);
    }

    #[test]
    fn complete_tree_with_no_surface_is_not_a_mesh_candidate() {
        let grid = ScalarGrid::sample_exact(
            GridSpec {
                minimum_microunits: [-2_000_000; 3],
                maximum_microunits: [2_000_000; 3],
                dimensions: [2, 2, 2],
                max_samples: MAX_SAMPLES,
                max_descriptor_bytes: 1_048_576,
                max_canonical_bytes: MAX_CANONICAL_BYTES,
                max_working_payload_bytes: 32 * 1024 * 1024,
            },
            AnalyticField::Sphere {
                center_microunits: [12_000_000; 3],
                radius_microunits: 1_000_000,
            },
            0,
        )
        .unwrap();
        let tree = build_adaptive_octree(
            &grid,
            AdaptiveOctreeConfig {
                max_depth: 3,
                max_unique_samples: 128,
                max_leaf_cells: 64,
                max_working_payload_bytes: 16 * 1024 * 1024,
                max_receipt_bytes: 8 * 1024 * 1024,
                max_balance_pair_checks: MAX_BALANCE_PAIR_CHECKS,
            },
        )
        .unwrap();
        let audit = audit_octree_topology(&grid, &tree, TopologyAuditConfig::default()).unwrap();
        assert_eq!(audit.metrics().surface_possible_leaf_count, 0);
        assert_eq!(audit.readiness(), ReadinessStatus::NoSurface);
    }

    #[test]
    fn morton_path_decode_rejects_a_forged_leaf_origin() {
        let good = leaf(MortonKey::root().child(1).unwrap(), [4, 0, 0], 4);
        validate_leaf_path(good, 3, 8).unwrap();
        let forged = leaf(MortonKey::root().child(1).unwrap(), [0, 0, 0], 4);
        assert_eq!(
            validate_leaf_path(forged, 3, 8),
            Err(TopologyAuditError::InvalidMortonPath)
        );
    }

    #[test]
    fn repeated_exact_tree_audit_is_byte_deterministic() {
        let (grid, tree) = sphere_fixture(3);
        let first = audit_octree_topology(&grid, &tree, TopologyAuditConfig::default()).unwrap();
        let second = audit_octree_topology(&grid, &tree, TopologyAuditConfig::default()).unwrap();
        assert_eq!(first, second);
        let first_bytes = first.canonical_bytes(&grid, &tree).unwrap();
        assert_eq!(first_bytes, second.canonical_bytes(&grid, &tree).unwrap());
        assert_eq!(
            first_bytes.len() as u64,
            first.metrics().canonical_receipt_estimate_bytes
        );
    }
}
