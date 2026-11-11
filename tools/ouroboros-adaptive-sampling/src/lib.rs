#![forbid(unsafe_code)]
//! Deterministic, budgeted, sample-cached octree over verified exact SDF fields.
//!
//! This crate refines a conservative interval frontier. It does not extract a
//! surface mesh. A budget-stopped or balance-incomplete tree is an explicit
//! partial result; unresolved space is never silently reclassified as empty.

use std::{
    cmp::Ordering,
    collections::{BTreeMap, BTreeSet},
    error::Error,
    fmt,
};

use ouroboros_cell_intervals::{
    CellClassification, CellStatus, IntervalError, VerifiedExactField,
};
use ouroboros_scalar_grid::{ScalarGrid, ScalarGridError, MAX_SAMPLES};

pub const ADAPTIVE_OCTREE_SCHEMA_VERSION: u16 = 1;
pub const ADAPTIVE_OCTREE_MAGIC: [u8; 8] = *b"OUROOCT1";
pub const MAX_ADAPTIVE_DEPTH: u8 = 18;
pub const MAX_LEAF_CELLS: u32 = 4_096;
pub const MAX_RECEIPT_BYTES: u64 = 64 * 1024 * 1024;
pub const MAX_WORKING_PAYLOAD_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_BALANCE_PAIR_CHECKS: u64 = 67_108_864;
const SAMPLE_CACHE_ENTRY_ESTIMATE_BYTES: u64 = 192;
const LEAF_ENTRY_ESTIMATE_BYTES: u64 = 160;
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdaptiveOctreeConfig {
    pub max_depth: u8,
    pub max_unique_samples: u64,
    pub max_leaf_cells: u32,
    pub max_working_payload_bytes: u64,
    pub max_receipt_bytes: u64,
    pub max_balance_pair_checks: u64,
}

impl Default for AdaptiveOctreeConfig {
    fn default() -> Self {
        Self {
            max_depth: 8,
            max_unique_samples: 65_536,
            max_leaf_cells: 4_096,
            max_working_payload_bytes: 64 * 1024 * 1024,
            max_receipt_bytes: 32 * 1024 * 1024,
            max_balance_pair_checks: MAX_BALANCE_PAIR_CHECKS,
        }
    }
}

impl AdaptiveOctreeConfig {
    pub fn validate(self) -> Result<(), AdaptiveOctreeError> {
        if self.max_depth == 0
            || self.max_depth > MAX_ADAPTIVE_DEPTH
            || self.max_unique_samples < 8
            || self.max_unique_samples > MAX_SAMPLES
            || self.max_leaf_cells == 0
            || self.max_leaf_cells > MAX_LEAF_CELLS
            || self.max_working_payload_bytes == 0
            || self.max_working_payload_bytes > MAX_WORKING_PAYLOAD_BYTES
            || self.max_receipt_bytes == 0
            || self.max_receipt_bytes > MAX_RECEIPT_BYTES
            || self.max_balance_pair_checks == 0
            || self.max_balance_pair_checks > MAX_BALANCE_PAIR_CHECKS
        {
            return Err(AdaptiveOctreeError::InvalidConfig);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MortonKey {
    pub level: u8,
    pub path_code: u64,
}

impl MortonKey {
    fn spatial_order_code(self) -> u64 {
        if self.level > MAX_ADAPTIVE_DEPTH {
            return u64::MAX;
        }
        self.path_code << (3 * u32::from(MAX_ADAPTIVE_DEPTH - self.level))
    }
}

impl Ord for MortonKey {
    fn cmp(&self, other: &Self) -> Ordering {
        self.spatial_order_code()
            .cmp(&other.spatial_order_code())
            .then_with(|| self.level.cmp(&other.level))
            .then_with(|| self.path_code.cmp(&other.path_code))
    }
}

impl PartialOrd for MortonKey {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl MortonKey {
    pub fn root() -> Self {
        Self { level: 0, path_code: 0 }
    }

    pub fn child(self, octant: u8) -> Result<Self, AdaptiveOctreeError> {
        if octant > 7 || self.level >= MAX_ADAPTIVE_DEPTH {
            return Err(AdaptiveOctreeError::MortonKeyOverflow);
        }
        let path_code = self.path_code
            .checked_shl(3)
            .and_then(|value| value.checked_add(u64::from(octant)))
            .ok_or(AdaptiveOctreeError::MortonKeyOverflow)?;
        Ok(Self {
            level: self.level + 1,
            path_code,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum LeafStop {
    Certified = 1,
    SurfacePending = 2,
    SurfaceAtMaximumDepth = 3,
    BudgetStopped = 4,
    BalanceStopped = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BudgetStopReason {
    None = 0,
    SampleBudget = 1,
    LeafBudget = 2,
    WorkingPayloadBudget = 3,
    ReceiptBudget = 4,
    BalanceCheckBudget = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BalanceStatus {
    Balanced = 1,
    BalanceIncomplete = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OctreeLeaf {
    pub key: MortonKey,
    /// Cell minimum in the max-depth dyadic tick lattice.
    pub minimum_tick: [u32; 3],
    /// Cell edge length in max-depth dyadic ticks.
    pub span_ticks: u32,
    pub classification: CellClassification,
    pub stop: LeafStop,
    pub stop_reason: BudgetStopReason,
}

impl OctreeLeaf {
    fn maximum_tick(self) -> [u32; 3] {
        [
            self.minimum_tick[0] + self.span_ticks,
            self.minimum_tick[1] + self.span_ticks,
            self.minimum_tick[2] + self.span_ticks,
        ]
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AdaptiveOctreeMetrics {
    pub maximum_depth: u8,
    pub leaf_count: u64,
    pub leaves_certified_inside: u64,
    pub leaves_certified_outside: u64,
    pub leaves_surface_possible: u64,
    pub leaves_budget_stopped: u64,
    pub leaves_balance_stopped: u64,
    pub unique_sample_count: u64,
    pub sample_cache_hits: u64,
    pub sample_evaluations: u64,
    pub split_count: u64,
    pub balance_rounds: u64,
    pub balance_pair_checks: u64,
    pub remaining_2_to_1_violations: u64,
    pub working_payload_estimate_bytes: u64,
    pub canonical_receipt_estimate_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AdaptiveOctreeReport {
    config: AdaptiveOctreeConfig,
    source_grid_canonical_bytes: Vec<u8>,
    source_descriptor_bytes: Vec<u8>,
    isovalue_microunits: i32,
    source_bounds_microunits: [[i64; 3]; 2],
    samples: BTreeMap<[i64; 3], i32>,
    leaves: BTreeMap<MortonKey, OctreeLeaf>,
    balance_status: BalanceStatus,
    metrics: AdaptiveOctreeMetrics,
}

impl AdaptiveOctreeReport {
    pub fn config(&self) -> AdaptiveOctreeConfig {
        self.config
    }

    pub fn balance_status(&self) -> BalanceStatus {
        self.balance_status
    }

    pub fn metrics(&self) -> AdaptiveOctreeMetrics {
        self.metrics
    }

    pub fn leaves(&self) -> impl Iterator<Item = &OctreeLeaf> {
        self.leaves.values()
    }

    pub fn samples(&self) -> &BTreeMap<[i64; 3], i32> {
        &self.samples
    }

    pub fn source_grid_canonical_bytes(&self) -> &[u8] {
        &self.source_grid_canonical_bytes
    }

    /// Rebuild from the exact source grid and compare every leaf/cache record.
    pub fn verify(&self, grid: &ScalarGrid) -> Result<(), AdaptiveOctreeError> {
        let recomputed = build_adaptive_octree(grid, self.config)?;
        if recomputed != *self {
            return Err(AdaptiveOctreeError::ReportMismatch);
        }
        Ok(())
    }

    /// Canonical receipt. The full source grid, ordered sample cache, leaves,
    /// statuses and budget outcomes are embedded. No signature is implied.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, AdaptiveOctreeError> {
        let expected = canonical_receipt_estimate(
            self.source_grid_canonical_bytes.len() as u64,
            self.source_descriptor_bytes.len() as u64,
            self.samples.len() as u64,
            self.leaves.len() as u64,
        )?;
        if expected > self.config.max_receipt_bytes
            || expected > MAX_RECEIPT_BYTES
            || expected != self.metrics.canonical_receipt_estimate_bytes
        {
            return Err(AdaptiveOctreeError::ReceiptBudgetExceeded);
        }
        let capacity = usize::try_from(expected).map_err(|_| AdaptiveOctreeError::SizeOverflow)?;
        let mut out = Vec::new();
        out.try_reserve_exact(capacity).map_err(|_| AdaptiveOctreeError::AllocationFailed)?;
        out.extend_from_slice(&ADAPTIVE_OCTREE_MAGIC);
        put_u16(&mut out, ADAPTIVE_OCTREE_SCHEMA_VERSION);
        put_u8(&mut out, self.config.max_depth);
        put_u64(&mut out, self.config.max_unique_samples);
        put_u32(&mut out, self.config.max_leaf_cells);
        put_u64(&mut out, self.config.max_working_payload_bytes);
        put_u64(&mut out, self.config.max_receipt_bytes);
        put_u64(&mut out, self.config.max_balance_pair_checks);
        put_u32(
            &mut out,
            u32::try_from(self.source_grid_canonical_bytes.len())
                .map_err(|_| AdaptiveOctreeError::SizeOverflow)?,
        );
        out.extend_from_slice(&self.source_grid_canonical_bytes);
        put_u32(
            &mut out,
            u32::try_from(self.source_descriptor_bytes.len())
                .map_err(|_| AdaptiveOctreeError::SizeOverflow)?,
        );
        out.extend_from_slice(&self.source_descriptor_bytes);
        put_i32(&mut out, self.isovalue_microunits);
        for axis in 0..3 {
            put_i64(&mut out, self.source_bounds_microunits[0][axis]);
        }
        for axis in 0..3 {
            put_i64(&mut out, self.source_bounds_microunits[1][axis]);
        }
        put_u8(&mut out, self.balance_status as u8);
        put_u64(&mut out, self.metrics.leaf_count);
        put_u64(&mut out, self.metrics.leaves_certified_inside);
        put_u64(&mut out, self.metrics.leaves_certified_outside);
        put_u64(&mut out, self.metrics.leaves_surface_possible);
        put_u64(&mut out, self.metrics.leaves_budget_stopped);
        put_u64(&mut out, self.metrics.leaves_balance_stopped);
        put_u64(&mut out, self.metrics.unique_sample_count);
        put_u64(&mut out, self.metrics.sample_cache_hits);
        put_u64(&mut out, self.metrics.sample_evaluations);
        put_u64(&mut out, self.metrics.split_count);
        put_u64(&mut out, self.metrics.balance_rounds);
        put_u64(&mut out, self.metrics.balance_pair_checks);
        put_u64(&mut out, self.metrics.remaining_2_to_1_violations);
        put_u64(&mut out, self.metrics.working_payload_estimate_bytes);
        put_u64(&mut out, self.metrics.canonical_receipt_estimate_bytes);
        put_u64(&mut out, self.samples.len() as u64);
        for (coordinate, sample) in &self.samples {
            for value in coordinate {
                put_i64(&mut out, *value);
            }
            put_i32(&mut out, *sample);
        }
        put_u64(&mut out, self.leaves.len() as u64);
        for leaf in self.leaves.values() {
            put_u8(&mut out, leaf.key.level);
            put_u64(&mut out, leaf.key.path_code);
            for value in leaf.minimum_tick {
                put_u32(&mut out, value);
            }
            put_u32(&mut out, leaf.span_ticks);
            put_u8(&mut out, leaf.classification.status as u8);
            put_u16(&mut out, leaf.classification.sign_changing_edge_mask);
            put_i32(&mut out, leaf.classification.minimum_corner_sample_microunits);
            put_i32(&mut out, leaf.classification.maximum_corner_sample_microunits);
            put_i64(&mut out, leaf.classification.lower_bound_microunits);
            put_i64(&mut out, leaf.classification.upper_bound_microunits);
            put_u8(&mut out, leaf.stop as u8);
            put_u8(&mut out, leaf.stop_reason as u8);
        }
        if out.len() as u64 != expected {
            return Err(AdaptiveOctreeError::ReceiptLengthMismatch);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AdaptiveOctreeError {
    InvalidConfig,
    NonDyadicDomain,
    DepthOverflow,
    MortonKeyOverflow,
    SizeOverflow,
    SampleBudgetTooSmall,
    LeafBudgetTooSmall,
    WorkingPayloadBudgetExceeded,
    ReceiptBudgetExceeded,
    BalanceCheckBudgetExceeded,
    AllocationFailed,
    MissingCachedSample,
    InvalidCell,
    ReportMismatch,
    ReceiptLengthMismatch,
    ScalarGrid(ScalarGridError),
    Interval(IntervalError),
}

impl fmt::Display for AdaptiveOctreeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for AdaptiveOctreeError {}

impl From<ScalarGridError> for AdaptiveOctreeError {
    fn from(value: ScalarGridError) -> Self {
        Self::ScalarGrid(value)
    }
}

impl From<IntervalError> for AdaptiveOctreeError {
    fn from(value: IntervalError) -> Self {
        Self::Interval(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SplitFailure {
    SampleBudget,
    LeafBudget,
    WorkingPayloadBudget,
    ReceiptBudget,
}

pub fn build_adaptive_octree(
    source_grid: &ScalarGrid,
    config: AdaptiveOctreeConfig,
) -> Result<AdaptiveOctreeReport, AdaptiveOctreeError> {
    config.validate()?;
    let source_limits = source_grid.validate()?;
    let spec = source_grid.spec();
    let minimum = spec.minimum_microunits;
    let maximum = spec.maximum_microunits;
    let descriptor_len = u64::try_from(source_grid.descriptor_bytes().len())
        .map_err(|_| AdaptiveOctreeError::SizeOverflow)?;
    if config.max_unique_samples < 8 {
        return Err(AdaptiveOctreeError::SampleBudgetTooSmall);
    }
    let scale = 1_u32
        .checked_shl(u32::from(config.max_depth))
        .ok_or(AdaptiveOctreeError::DepthOverflow)?;
    let mut span_micro = [0_i64; 3];
    for axis in 0..3 {
        let span = i128::from(maximum[axis]) - i128::from(minimum[axis]);
        // Every max-depth tick must advance by at least one microunit.
        // Non-divisible spans are supported: tick coordinates are floored,
        // while each cell's interval diagonal rounds its extent upward.
        if span < i128::from(scale) {
            return Err(AdaptiveOctreeError::NonDyadicDomain);
        }
        span_micro[axis] = i64::try_from(span).map_err(|_| AdaptiveOctreeError::SizeOverflow)?;
    }
    // Check the minimum viable tree, duplicate source descriptor and receipt
    // ceilings before cloning the descriptor, verifying the source stream, or
    // allocating the canonical source-grid copy.
    let initial_working = estimate_working_payload(
        8,
        1,
        source_limits.canonical_bytes,
        descriptor_len,
    )?;
    if initial_working > config.max_working_payload_bytes {
        return Err(AdaptiveOctreeError::WorkingPayloadBudgetExceeded);
    }
    let initial_receipt = canonical_receipt_estimate(
        source_limits.canonical_bytes,
        descriptor_len,
        8,
        1,
    )?;
    if initial_receipt > config.max_receipt_bytes {
        return Err(AdaptiveOctreeError::ReceiptBudgetExceeded);
    }
    let verified = VerifiedExactField::from_grid(source_grid)?;
    let source_grid_canonical_bytes = source_grid.canonical_bytes()?;
    let source_descriptor_bytes = verified.descriptor_bytes().to_vec();
    let source_bounds_microunits = [minimum, maximum];

    let mut samples = BTreeMap::<[i64; 3], i32>::new();
    let mut sample_cache_hits = 0_u64;
    let mut sample_evaluations = 0_u64;
    let root_key = MortonKey::root();
    let root_min = [0_u32; 3];
    let root_span = scale;
    let mut root_points = BTreeSet::new();
    for corner in CELL_CORNERS {
        root_points.insert(point_at_tick(
            minimum,
            span_micro,
            scale,
            [
                root_min[0] + corner[0] * root_span,
                root_min[1] + corner[1] * root_span,
                root_min[2] + corner[2] * root_span,
            ],
        )?);
    }
    if root_points.len() > config.max_unique_samples as usize {
        return Err(AdaptiveOctreeError::SampleBudgetTooSmall);
    }
    let root_pending = evaluate_missing_points(
        &verified,
        &samples,
        root_points,
        config,
        source_grid_canonical_bytes.len() as u64,
        1,
    )?;
    commit_pending(&mut samples, root_pending, &mut sample_evaluations, &mut sample_cache_hits);
    let root_classification = classify_cell(
        &samples,
        minimum,
        span_micro,
        scale,
        root_min,
        root_span,
    )?;
    let root = OctreeLeaf {
        key: root_key,
        minimum_tick: root_min,
        span_ticks: root_span,
        classification: root_classification,
        stop: stop_for_classification(root_classification, 0, config.max_depth),
        stop_reason: BudgetStopReason::None,
    };
    let mut leaves = BTreeMap::new();
    leaves.insert(root_key, root);
    let mut split_count = 0_u64;
    let mut balance_rounds = 0_u64;
    let mut balance_pair_checks = 0_u64;
    let mut balance_status = BalanceStatus::Balanced;
    let mut tree_budget_stopped = false;
    let mut balance_check_exhausted = false;
    let mut remaining_2_to_1_violations = 0_u64;

    // Surface refinement and balance are iterated together. A balance split
    // can create new surface-possible children, so they re-enter this loop.
    loop {
        let mut changed = false;
        let refine_keys: Vec<MortonKey> = leaves
            .values()
            .filter(|leaf| {
                leaf.classification.status == CellStatus::SurfacePossible
                    && leaf.key.level < config.max_depth
                    && leaf.stop == LeafStop::SurfacePending
            })
            .map(|leaf| leaf.key)
            .collect();
        for key in refine_keys {
            match split_leaf(
                key,
                &mut leaves,
                &verified,
                &mut samples,
                &mut sample_evaluations,
                &mut sample_cache_hits,
                &mut split_count,
                minimum,
                span_micro,
                scale,
                config,
                source_grid_canonical_bytes.len() as u64,
            ) {
                Ok(()) => changed = true,
                Err(failure) => {
                    tree_budget_stopped = true;
                    mark_split_failure(&mut leaves, key, failure, false)?;
                }
            }
        }

        balance_rounds = balance_rounds
            .checked_add(1)
            .ok_or(AdaptiveOctreeError::SizeOverflow)?;
        let (coarse_keys, comparisons, violations) = match find_2_to_1_violations(
            &leaves,
            config.max_balance_pair_checks.saturating_sub(balance_pair_checks),
        ) {
            Ok(result) => result,
            Err(AdaptiveOctreeError::BalanceCheckBudgetExceeded) => {
                balance_status = BalanceStatus::BalanceIncomplete;
                tree_budget_stopped = true;
                balance_check_exhausted = true;
                break;
            }
            Err(error) => return Err(error),
        };
        balance_pair_checks = balance_pair_checks
            .checked_add(comparisons)
            .ok_or(AdaptiveOctreeError::SizeOverflow)?;
        remaining_2_to_1_violations = violations;
        if coarse_keys.is_empty() {
            balance_status = BalanceStatus::Balanced;
            if !changed {
                break;
            }
            continue;
        }

        for key in coarse_keys {
            match split_leaf(
                key,
                &mut leaves,
                &verified,
                &mut samples,
                &mut sample_evaluations,
                &mut sample_cache_hits,
                &mut split_count,
                minimum,
                span_micro,
                scale,
                config,
                source_grid_canonical_bytes.len() as u64,
            ) {
                Ok(()) => changed = true,
                Err(failure) => {
                    tree_budget_stopped = true;
                    balance_status = BalanceStatus::BalanceIncomplete;
                    mark_split_failure(&mut leaves, key, failure, true)?;
                }
            }
        }
        if !changed && balance_status == BalanceStatus::BalanceIncomplete {
            break;
        }
    }

    if balance_check_exhausted {
        // u64::MAX means the pairwise check budget was exhausted; the exact
        // remaining violation count is unknown, never implicitly zero.
        remaining_2_to_1_violations = u64::MAX;
    }
    if remaining_2_to_1_violations > 0 {
        balance_status = BalanceStatus::BalanceIncomplete;
    }
    if tree_budget_stopped && remaining_2_to_1_violations > 0 {
        balance_status = BalanceStatus::BalanceIncomplete;
    }

    let metrics = calculate_metrics(
        &leaves,
        &samples,
        sample_cache_hits,
        sample_evaluations,
        split_count,
        balance_rounds,
        balance_pair_checks,
        remaining_2_to_1_violations,
        balance_status,
        config,
        source_grid_canonical_bytes.len() as u64,
        source_descriptor_bytes.len() as u64,
    )?;
    Ok(AdaptiveOctreeReport {
        config,
        source_grid_canonical_bytes,
        source_descriptor_bytes,
        isovalue_microunits: verified.isovalue_microunits(),
        source_bounds_microunits,
        samples,
        leaves,
        balance_status,
        metrics,
    })
}

fn split_leaf(
    key: MortonKey,
    leaves: &mut BTreeMap<MortonKey, OctreeLeaf>,
    verified: &VerifiedExactField,
    samples: &mut BTreeMap<[i64; 3], i32>,
    sample_evaluations: &mut u64,
    sample_cache_hits: &mut u64,
    split_count: &mut u64,
    minimum: [i64; 3],
    span_micro: [i64; 3],
    scale: u32,
    config: AdaptiveOctreeConfig,
    source_bytes: u64,
) -> Result<(), SplitFailure> {
    let parent = *leaves.get(&key).ok_or(SplitFailure::LeafBudget)?;
    if parent.key.level >= config.max_depth || parent.span_ticks < 2 {
        return Err(SplitFailure::LeafBudget);
    }
    let half = parent.span_ticks / 2;
    if half == 0 {
        return Err(SplitFailure::LeafBudget);
    }
    let mut lattice_points = BTreeSet::new();
    for z in 0..=2_u32 {
        for y in 0..=2_u32 {
            for x in 0..=2_u32 {
                lattice_points.insert(
                    point_at_tick(
                        minimum,
                        span_micro,
                        scale,
                        [
                            parent.minimum_tick[0] + x * half,
                            parent.minimum_tick[1] + y * half,
                            parent.minimum_tick[2] + z * half,
                        ],
                    )
                    .map_err(|_| SplitFailure::WorkingPayloadBudget)?,
                );
            }
        }
    }
    let missing: Vec<[i64; 3]> = lattice_points
        .iter()
        .copied()
        .filter(|point| !samples.contains_key(point))
        .collect();
    let projected_samples = samples.len()
        .checked_add(missing.len())
        .ok_or(SplitFailure::WorkingPayloadBudget)?;
    if projected_samples as u64 > config.max_unique_samples {
        return Err(SplitFailure::SampleBudget);
    }
    if leaves.len().saturating_add(7) > config.max_leaf_cells as usize {
        return Err(SplitFailure::LeafBudget);
    }
    let projected_working = estimate_working_payload(
        projected_samples as u64,
        (leaves.len() as u64).saturating_add(7),
        source_bytes,
        verified.descriptor_bytes().len() as u64,
    )
    .map_err(|_| SplitFailure::WorkingPayloadBudget)?;
    if projected_working > config.max_working_payload_bytes {
        return Err(SplitFailure::WorkingPayloadBudget);
    }
    let projected_receipt = canonical_receipt_estimate(
        source_bytes,
        verified.descriptor_bytes().len() as u64,
        projected_samples as u64,
        (leaves.len() as u64).saturating_add(7),
    )
    .map_err(|_| SplitFailure::ReceiptBudget)?;
    if projected_receipt > config.max_receipt_bytes {
        return Err(SplitFailure::ReceiptBudget);
    }

    // Stage every new sample and all eight child records before mutating the
    // parent partition or the shared cache. A failed split is atomic.
    let mut pending = Vec::<([i64; 3], i32)>::new();
    pending.try_reserve_exact(missing.len()).map_err(|_| SplitFailure::WorkingPayloadBudget)?;
    for point in &missing {
        let value = verified
            .sample_at_microunits(*point)
            .map_err(|_| SplitFailure::WorkingPayloadBudget)?;
        pending.push((*point, value));
    }
    let mut children = Vec::new();
    children.try_reserve_exact(8).map_err(|_| SplitFailure::WorkingPayloadBudget)?;
    for octant in 0..8_u8 {
        let offset = [
            u32::from(octant & 1),
            u32::from((octant >> 1) & 1),
            u32::from((octant >> 2) & 1),
        ];
        let minimum_tick = [
            parent.minimum_tick[0] + offset[0] * half,
            parent.minimum_tick[1] + offset[1] * half,
            parent.minimum_tick[2] + offset[2] * half,
        ];
        let child_key = parent.key.child(octant).map_err(|_| SplitFailure::WorkingPayloadBudget)?;
        let mut child_samples = [0_i32; 8];
        for (index, corner) in CELL_CORNERS.iter().enumerate() {
            let tick = [
                minimum_tick[0] + corner[0] * half,
                minimum_tick[1] + corner[1] * half,
                minimum_tick[2] + corner[2] * half,
            ];
            let point = point_at_tick(minimum, span_micro, scale, tick)
                .map_err(|_| SplitFailure::WorkingPayloadBudget)?;
            child_samples[index] = lookup_with_pending(samples, &pending, point)
                .ok_or(SplitFailure::WorkingPayloadBudget)?;
        }
        let child_spans = spans_for_ticks(span_micro, half, scale)
            .map_err(|_| SplitFailure::WorkingPayloadBudget)?;
        let classification = classify_corner_samples(child_samples, child_spans)
            .map_err(|_| SplitFailure::WorkingPayloadBudget)?;
        children.push(OctreeLeaf {
            key: child_key,
            minimum_tick,
            span_ticks: half,
            classification,
            stop: stop_for_classification(classification, child_key.level, config.max_depth),
            stop_reason: BudgetStopReason::None,
        });
    }

    for (point, value) in pending {
        if samples.insert(point, value).is_none() {
            *sample_evaluations = sample_evaluations
                .checked_add(1)
                .ok_or(SplitFailure::WorkingPayloadBudget)?;
        }
    }
    // The 3x3x3 lattice contains 27 unique coordinates. Existing entries were
    // hits; missing entries were evaluated exactly once.
    *sample_cache_hits = sample_cache_hits
        .checked_add((27_usize.saturating_sub(missing.len())) as u64)
        .ok_or(SplitFailure::WorkingPayloadBudget)?;
    leaves.remove(&key);
    for child in children {
        leaves.insert(child.key, child);
    }
    *split_count = split_count.checked_add(1).ok_or(SplitFailure::WorkingPayloadBudget)?;
    Ok(())
}

fn classify_cell(
    samples: &BTreeMap<[i64; 3], i32>,
    minimum: [i64; 3],
    span_micro: [i64; 3],
    scale: u32,
    minimum_tick: [u32; 3],
    span_ticks: u32,
) -> Result<CellClassification, AdaptiveOctreeError> {
    let mut values = [0_i32; 8];
    for (index, corner) in CELL_CORNERS.iter().enumerate() {
        let tick = [
            minimum_tick[0] + corner[0] * span_ticks,
            minimum_tick[1] + corner[1] * span_ticks,
            minimum_tick[2] + corner[2] * span_ticks,
        ];
        let point = point_at_tick(minimum, span_micro, scale, tick)?;
        values[index] = *samples.get(&point).ok_or(AdaptiveOctreeError::MissingCachedSample)?;
    }
    let spans = spans_for_ticks(span_micro, span_ticks, scale)?;
    classify_corner_samples(values, spans)
}

fn classify_corner_samples(
    values: [i32; 8],
    spans_microunits: [i64; 3],
) -> Result<CellClassification, AdaptiveOctreeError> {
    if spans_microunits.iter().any(|span| *span <= 0) {
        return Err(AdaptiveOctreeError::InvalidCell);
    }
    let mut edge_mask = 0_u16;
    for (index, edge) in CELL_EDGES.iter().enumerate() {
        if (values[edge[0]] < 0) != (values[edge[1]] < 0) {
            edge_mask |= 1_u16 << index;
        }
    }
    let minimum = *values.iter().min().ok_or(AdaptiveOctreeError::InvalidCell)?;
    let maximum = *values.iter().max().ok_or(AdaptiveOctreeError::InvalidCell)?;
    let diagonal_squared = spans_microunits.iter().try_fold(0_u128, |sum, span| {
        let magnitude = span.unsigned_abs() as u128;
        sum.checked_add(magnitude.checked_mul(magnitude).ok_or(AdaptiveOctreeError::SizeOverflow)?)
            .ok_or(AdaptiveOctreeError::SizeOverflow)
    })?;
    let diagonal = ceil_sqrt_u128(diagonal_squared)?;
    let margin = i64::try_from(diagonal)
        .map_err(|_| AdaptiveOctreeError::SizeOverflow)?
        .checked_add(1)
        .ok_or(AdaptiveOctreeError::SizeOverflow)?;
    let lower = i64::from(maximum).checked_sub(margin).ok_or(AdaptiveOctreeError::SizeOverflow)?;
    let upper = i64::from(minimum).checked_add(margin).ok_or(AdaptiveOctreeError::SizeOverflow)?;
    let status = if lower > 0 {
        CellStatus::CertifiedOutside
    } else if upper < 0 {
        CellStatus::CertifiedInside
    } else {
        CellStatus::SurfacePossible
    };
    Ok(CellClassification {
        status,
        sign_changing_edge_mask: edge_mask,
        minimum_corner_sample_microunits: minimum,
        maximum_corner_sample_microunits: maximum,
        lower_bound_microunits: lower,
        upper_bound_microunits: upper,
    })
}

fn stop_for_classification(classification: CellClassification, level: u8, maximum_depth: u8) -> LeafStop {
    match classification.status {
        CellStatus::CertifiedInside | CellStatus::CertifiedOutside => LeafStop::Certified,
        CellStatus::SurfacePossible if level >= maximum_depth => LeafStop::SurfaceAtMaximumDepth,
        CellStatus::SurfacePossible | CellStatus::UncertifiedField => LeafStop::SurfacePending,
    }
}

fn mark_split_failure(
    leaves: &mut BTreeMap<MortonKey, OctreeLeaf>,
    key: MortonKey,
    failure: SplitFailure,
    balance: bool,
) -> Result<(), AdaptiveOctreeError> {
    let leaf = leaves.get_mut(&key).ok_or(AdaptiveOctreeError::InvalidCell)?;
    if balance {
        leaf.stop = LeafStop::BalanceStopped;
    } else {
        leaf.stop = LeafStop::BudgetStopped;
    }
    leaf.stop_reason = match failure {
        SplitFailure::SampleBudget => BudgetStopReason::SampleBudget,
        SplitFailure::LeafBudget => BudgetStopReason::LeafBudget,
        SplitFailure::WorkingPayloadBudget => BudgetStopReason::WorkingPayloadBudget,
        SplitFailure::ReceiptBudget => BudgetStopReason::ReceiptBudget,
    };
    Ok(())
}

fn find_2_to_1_violations(
    leaves: &BTreeMap<MortonKey, OctreeLeaf>,
    remaining_pair_budget: u64,
) -> Result<(BTreeSet<MortonKey>, u64, u64), AdaptiveOctreeError> {
    let values: Vec<OctreeLeaf> = leaves.values().copied().collect();
    let pair_count = (values.len() as u64)
        .checked_mul(values.len().saturating_sub(1) as u64)
        .and_then(|value| value.checked_div(2))
        .ok_or(AdaptiveOctreeError::SizeOverflow)?;
    if pair_count > remaining_pair_budget {
        return Err(AdaptiveOctreeError::BalanceCheckBudgetExceeded);
    }
    let mut coarse = BTreeSet::new();
    let mut violation_count = 0_u64;
    for i in 0..values.len() {
        for j in (i + 1)..values.len() {
            let a = values[i];
            let b = values[j];
            if !share_face(a, b) {
                continue;
            }
            let difference = a.key.level.abs_diff(b.key.level);
            if difference > 1 {
                violation_count = violation_count
                    .checked_add(1)
                    .ok_or(AdaptiveOctreeError::SizeOverflow)?;
                coarse.insert(if a.key.level < b.key.level { a.key } else { b.key });
            }
        }
    }
    Ok((coarse, pair_count, violation_count))
}

fn share_face(a: OctreeLeaf, b: OctreeLeaf) -> bool {
    let a_max = a.maximum_tick();
    let b_max = b.maximum_tick();
    for axis in 0..3 {
        if a_max[axis] != b.minimum_tick[axis] && b_max[axis] != a.minimum_tick[axis] {
            continue;
        }
        let first = (axis + 1) % 3;
        let second = (axis + 2) % 3;
        if a.minimum_tick[first].max(b.minimum_tick[first]) < a_max[first].min(b_max[first])
            && a.minimum_tick[second].max(b.minimum_tick[second])
                < a_max[second].min(b_max[second])
        {
            return true;
        }
    }
    false
}

fn evaluate_missing_points(
    verified: &VerifiedExactField,
    existing: &BTreeMap<[i64; 3], i32>,
    points: BTreeSet<[i64; 3]>,
    config: AdaptiveOctreeConfig,
    source_bytes: u64,
    projected_leaves: u64,
) -> Result<Vec<([i64; 3], i32)>, AdaptiveOctreeError> {
    let missing: Vec<[i64; 3]> = points
        .into_iter()
        .filter(|point| !existing.contains_key(point))
        .collect();
    if existing.len().saturating_add(missing.len()) as u64 > config.max_unique_samples {
        return Err(AdaptiveOctreeError::SampleBudgetTooSmall);
    }
    let working = estimate_working_payload(
        existing.len().saturating_add(missing.len()) as u64,
        projected_leaves,
        source_bytes,
        verified.descriptor_bytes().len() as u64,
    )?;
    if working > config.max_working_payload_bytes {
        return Err(AdaptiveOctreeError::WorkingPayloadBudgetExceeded);
    }
    let receipt = canonical_receipt_estimate(
        source_bytes,
        verified.descriptor_bytes().len() as u64,
        existing.len().saturating_add(missing.len()) as u64,
        projected_leaves,
    )?;
    if receipt > config.max_receipt_bytes {
        return Err(AdaptiveOctreeError::ReceiptBudgetExceeded);
    }
    let mut pending = Vec::new();
    pending.try_reserve_exact(missing.len()).map_err(|_| AdaptiveOctreeError::AllocationFailed)?;
    for point in missing {
        pending.push((
            point,
            verified.sample_at_microunits(point)?,
        ));
    }
    Ok(pending)
}

fn commit_pending(
    target: &mut BTreeMap<[i64; 3], i32>,
    pending: Vec<([i64; 3], i32)>,
    evaluations: &mut u64,
    hits: &mut u64,
) {
    for (point, value) in pending {
        if target.insert(point, value).is_none() {
            *evaluations = evaluations.saturating_add(1);
        } else {
            *hits = hits.saturating_add(1);
        }
    }
}

fn lookup_with_pending(
    existing: &BTreeMap<[i64; 3], i32>,
    pending: &[([i64; 3], i32)],
    coordinate: [i64; 3],
) -> Option<i32> {
    existing.get(&coordinate).copied().or_else(|| {
        pending.iter().find(|(point, _)| *point == coordinate).map(|(_, value)| *value)
    })
}

fn spans_for_ticks(
    span_microunits: [i64; 3],
    span_ticks: u32,
    scale: u32,
) -> Result<[i64; 3], AdaptiveOctreeError> {
    if scale == 0 || span_ticks == 0 || span_ticks > scale {
        return Err(AdaptiveOctreeError::InvalidCell);
    }
    let mut result = [0_i64; 3];
    for axis in 0..3 {
        if span_microunits[axis] <= 0 {
            return Err(AdaptiveOctreeError::InvalidCell);
        }
        let numerator = u128::try_from(span_microunits[axis])
            .map_err(|_| AdaptiveOctreeError::SizeOverflow)?
            .checked_mul(u128::from(span_ticks))
            .ok_or(AdaptiveOctreeError::SizeOverflow)?;
        let denominator = u128::from(scale);
        let rounded_up = numerator
            .checked_add(denominator - 1)
            .ok_or(AdaptiveOctreeError::SizeOverflow)?
            / denominator;
        result[axis] = i64::try_from(rounded_up)
            .map_err(|_| AdaptiveOctreeError::SizeOverflow)?;
        if result[axis] <= 0 {
            return Err(AdaptiveOctreeError::InvalidCell);
        }
    }
    Ok(result)
}

fn point_at_tick(
    minimum: [i64; 3],
    span_micro: [i64; 3],
    scale: u32,
    tick: [u32; 3],
) -> Result<[i64; 3], AdaptiveOctreeError> {
    let mut point = [0_i64; 3];
    for axis in 0..3 {
        if tick[axis] > scale {
            return Err(AdaptiveOctreeError::InvalidCell);
        }
        let offset = i128::from(span_micro[axis])
            .checked_mul(i128::from(tick[axis]))
            .ok_or(AdaptiveOctreeError::SizeOverflow)?
            / i128::from(scale);
        point[axis] = i64::try_from(
            i128::from(minimum[axis])
                .checked_add(offset)
                .ok_or(AdaptiveOctreeError::SizeOverflow)?,
        )
        .map_err(|_| AdaptiveOctreeError::SizeOverflow)?;
    }
    Ok(point)
}

fn calculate_metrics(
    leaves: &BTreeMap<MortonKey, OctreeLeaf>,
    samples: &BTreeMap<[i64; 3], i32>,
    sample_cache_hits: u64,
    sample_evaluations: u64,
    split_count: u64,
    balance_rounds: u64,
    balance_pair_checks: u64,
    remaining_2_to_1_violations: u64,
    balance_status: BalanceStatus,
    config: AdaptiveOctreeConfig,
    source_bytes: u64,
    descriptor_bytes: u64,
) -> Result<AdaptiveOctreeMetrics, AdaptiveOctreeError> {
    let mut metrics = AdaptiveOctreeMetrics {
        maximum_depth: 0,
        leaf_count: leaves.len() as u64,
        leaves_certified_inside: 0,
        leaves_certified_outside: 0,
        leaves_surface_possible: 0,
        leaves_budget_stopped: 0,
        leaves_balance_stopped: 0,
        unique_sample_count: samples.len() as u64,
        sample_cache_hits,
        sample_evaluations,
        split_count,
        balance_rounds,
        balance_pair_checks,
        remaining_2_to_1_violations,
        working_payload_estimate_bytes: 0,
        canonical_receipt_estimate_bytes: 0,
    };
    for leaf in leaves.values() {
        metrics.maximum_depth = metrics.maximum_depth.max(leaf.key.level);
        match leaf.classification.status {
            CellStatus::CertifiedInside => metrics.leaves_certified_inside += 1,
            CellStatus::CertifiedOutside => metrics.leaves_certified_outside += 1,
            CellStatus::SurfacePossible | CellStatus::UncertifiedField => metrics.leaves_surface_possible += 1,
        }
        match leaf.stop {
            LeafStop::BudgetStopped => metrics.leaves_budget_stopped += 1,
            LeafStop::BalanceStopped => metrics.leaves_balance_stopped += 1,
            _ => {}
        }
    }
    metrics.working_payload_estimate_bytes = estimate_working_payload(
        metrics.unique_sample_count,
        metrics.leaf_count,
        source_bytes,
        descriptor_bytes,
    )?;
    metrics.canonical_receipt_estimate_bytes = canonical_receipt_estimate(
        source_bytes,
        descriptor_bytes,
        metrics.unique_sample_count,
        metrics.leaf_count,
    )?;
    if metrics.working_payload_estimate_bytes > config.max_working_payload_bytes {
        return Err(AdaptiveOctreeError::WorkingPayloadBudgetExceeded);
    }
    if metrics.canonical_receipt_estimate_bytes > config.max_receipt_bytes {
        return Err(AdaptiveOctreeError::ReceiptBudgetExceeded);
    }
    let _ = balance_status;
    Ok(metrics)
}

fn estimate_working_payload(
    sample_count: u64,
    leaf_count: u64,
    source_bytes: u64,
    descriptor_bytes: u64,
) -> Result<u64, AdaptiveOctreeError> {
    let receipt_bytes = canonical_receipt_estimate(
        source_bytes,
        descriptor_bytes,
        sample_count,
        leaf_count,
    )?;
    sample_count
        .checked_mul(SAMPLE_CACHE_ENTRY_ESTIMATE_BYTES)
        .and_then(|value| value.checked_add(leaf_count.checked_mul(LEAF_ENTRY_ESTIMATE_BYTES)?))
        .and_then(|value| value.checked_add(source_bytes))
        // VerifiedExactField and the report retain separate descriptor copies.
        .and_then(|value| value.checked_add(descriptor_bytes.checked_mul(2)?))
        // The canonical serialization buffer coexists with the in-memory tree.
        .and_then(|value| value.checked_add(receipt_bytes))
        .ok_or(AdaptiveOctreeError::SizeOverflow)
}

fn canonical_receipt_estimate(
    source_bytes: u64,
    descriptor_bytes: u64,
    sample_count: u64,
    leaf_count: u64,
) -> Result<u64, AdaptiveOctreeError> {
    8_u64
        .checked_add(2 + 1 + 8 + 4 + 8 + 8 + 8)
        .and_then(|value| value.checked_add(4 + source_bytes))
        .and_then(|value| value.checked_add(4 + descriptor_bytes))
        .and_then(|value| value.checked_add(4 + 6 * 8 + 1 + 8 * 15))
        .and_then(|value| value.checked_add(sample_count.checked_mul(28)?))
        .and_then(|value| value.checked_add(8 + leaf_count.checked_mul(54)?))
        .ok_or(AdaptiveOctreeError::SizeOverflow)
}

fn ceil_sqrt_u128(value: u128) -> Result<u128, AdaptiveOctreeError> {
    if value < 2 {
        return Ok(value);
    }
    let mut x = value;
    let mut y = x.checked_add(1).ok_or(AdaptiveOctreeError::SizeOverflow)? / 2;
    while y < x {
        x = y;
        y = x.checked_add(value / x).ok_or(AdaptiveOctreeError::SizeOverflow)? / 2;
    }
    if x.checked_mul(x).ok_or(AdaptiveOctreeError::SizeOverflow)? == value {
        Ok(x)
    } else {
        x.checked_add(1).ok_or(AdaptiveOctreeError::SizeOverflow)
    }
}

fn put_u8(out: &mut Vec<u8>, value: u8) { out.push(value); }
fn put_u16(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u64(out: &mut Vec<u8>, value: u64) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_i32(out: &mut Vec<u8>, value: i32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_i64(out: &mut Vec<u8>, value: i64) { out.extend_from_slice(&value.to_le_bytes()); }

#[cfg(test)]
mod tests {
    use super::*;
    use ouroboros_scalar_grid::{AnalyticField, GridSpec, MAX_CANONICAL_BYTES};

    fn grid(dimensions: [u32; 3], minimum: [i64; 3], maximum: [i64; 3]) -> ScalarGrid {
        ScalarGrid::sample_exact(
            GridSpec {
                minimum_microunits: minimum,
                maximum_microunits: maximum,
                dimensions,
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
        .unwrap()
    }

    fn small_config() -> AdaptiveOctreeConfig {
        AdaptiveOctreeConfig {
            max_depth: 3,
            max_unique_samples: 1_000,
            max_leaf_cells: 512,
            max_working_payload_bytes: 32 * 1024 * 1024,
            max_receipt_bytes: 16 * 1024 * 1024,
            max_balance_pair_checks: 16_000_000,
        }
    }

    #[test]
    fn enclosed_sphere_with_all_root_corners_outside_is_discovered() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let report = build_adaptive_octree(&grid, small_config()).unwrap();
        let root = report.samples();
        assert_eq!(root.len(), report.metrics().unique_sample_count as usize);
        assert!(root.values().all(|value| *value > 0));
        assert!(report.metrics().split_count > 0);
        assert!(report.metrics().leaves_certified_inside > 0);
        assert!(report.metrics().leaves_surface_possible > 0);
    }

    #[test]
    fn shared_sample_cache_has_one_entry_per_coordinate() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let report = build_adaptive_octree(&grid, small_config()).unwrap();
        assert_eq!(report.samples().len(), report.metrics().unique_sample_count as usize);
        assert_eq!(report.metrics().sample_evaluations, report.metrics().unique_sample_count);
        assert!(report.metrics().sample_cache_hits > 0);
    }

    #[test]
    fn initial_working_and_receipt_budgets_reject_before_sampling_tree() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let mut working_limited = small_config();
        working_limited.max_working_payload_bytes = 1;
        assert_eq!(
            build_adaptive_octree(&grid, working_limited),
            Err(AdaptiveOctreeError::WorkingPayloadBudgetExceeded)
        );

        let mut receipt_limited = small_config();
        receipt_limited.max_receipt_bytes = 1;
        assert_eq!(
            build_adaptive_octree(&grid, receipt_limited),
            Err(AdaptiveOctreeError::ReceiptBudgetExceeded)
        );
    }

    #[test]
    fn sample_budget_stops_before_atomic_split_and_preserves_parent() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let mut config = small_config();
        config.max_unique_samples = 8;
        let report = build_adaptive_octree(&grid, config).unwrap();
        assert_eq!(report.leaves.len(), 1);
        assert_eq!(report.samples.len(), 8);
        assert_eq!(report.metrics().split_count, 0);
        let root = report.leaves.values().next().unwrap();
        assert_eq!(root.key, MortonKey::root());
        assert_eq!(root.stop, LeafStop::BudgetStopped);
        assert_eq!(root.stop_reason, BudgetStopReason::SampleBudget);
    }

    #[test]
    fn nondivisible_fixed_point_spans_use_conservative_ceil_cell_widths() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_001; 3]);
        let report = build_adaptive_octree(&grid, small_config()).unwrap();
        assert!(report.metrics().split_count > 0);
        assert!(report.samples().contains_key(&[-2_000_000; 3]));
        assert!(report.samples().contains_key(&[2_000_001; 3]));
        for leaf in report.leaves() {
            assert!(leaf.classification.lower_bound_microunits
                <= leaf.classification.upper_bound_microunits);
        }
    }

    #[test]
    fn domain_smaller_than_one_max_depth_tick_rejects() {
        let grid = grid([2, 2, 2], [0; 3], [7; 3]);
        assert_eq!(
            build_adaptive_octree(&grid, small_config()),
            Err(AdaptiveOctreeError::NonDyadicDomain)
        );
    }

    #[test]
    fn canonical_receipt_estimate_matches_actual_serialized_bytes() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let report = build_adaptive_octree(&grid, small_config()).unwrap();
        let bytes = report.canonical_bytes().unwrap();
        assert_eq!(bytes.len() as u64, report.metrics().canonical_receipt_estimate_bytes);
        assert!(bytes.starts_with(&ADAPTIVE_OCTREE_MAGIC));
    }

    #[test]
    fn morton_keys_sort_spatially_across_different_depths() {
        let root = MortonKey::root();
        let early_subtree = root.child(0).unwrap().child(7).unwrap();
        let later_sibling = root.child(1).unwrap();
        assert!(early_subtree < later_sibling);
    }

    #[test]
    fn balance_check_budget_exhaustion_is_explicit_and_not_zero_violations() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let mut config = small_config();
        config.max_balance_pair_checks = 1;
        let report = build_adaptive_octree(&grid, config).unwrap();
        assert_eq!(report.balance_status(), BalanceStatus::BalanceIncomplete);
        assert_eq!(report.metrics().remaining_2_to_1_violations, u64::MAX);
    }

    #[test]
    fn unresolved_surface_remains_visible_when_sample_budget_stops_refinement() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let mut config = small_config();
        config.max_unique_samples = 27;
        let report = build_adaptive_octree(&grid, config).unwrap();
        assert!(report.metrics().leaves_budget_stopped > 0);
        assert!(report.leaves().any(|leaf| {
            leaf.stop == LeafStop::BudgetStopped
                && leaf.classification.status == CellStatus::SurfacePossible
        }));
    }

    #[test]
    fn approximate_field_label_cannot_authorize_adaptive_sampling() {
        let spec = GridSpec {
            minimum_microunits: [-2_000_000; 3],
            maximum_microunits: [2_000_000; 3],
            dimensions: [2, 2, 2],
            max_samples: MAX_SAMPLES,
            max_descriptor_bytes: 1_048_576,
            max_canonical_bytes: MAX_CANONICAL_BYTES,
            max_working_payload_bytes: 32 * 1024 * 1024,
        };
        let approximate = ScalarGrid::from_samples(
            spec,
            ouroboros_scalar_grid::FieldSemantics::ApproximateAnalyticField,
            vec![1],
            0,
            vec![1; 8],
        )
        .unwrap();
        assert_eq!(
            build_adaptive_octree(&approximate, small_config()),
            Err(AdaptiveOctreeError::Interval(IntervalError::FieldNotCertifiable))
        );
    }

    #[test]
    fn exact_zero_lattice_vertices_never_become_certified_empty_space() {
        let spec = GridSpec {
            minimum_microunits: [-2_000_000, -2_000_000, 0],
            maximum_microunits: [2_000_000, 2_000_000, 2_000_000],
            dimensions: [2, 2, 2],
            max_samples: MAX_SAMPLES,
            max_descriptor_bytes: 1_048_576,
            max_canonical_bytes: MAX_CANONICAL_BYTES,
            max_working_payload_bytes: 32 * 1024 * 1024,
        };
        let plane = ScalarGrid::sample_exact(
            spec,
            AnalyticField::Plane {
                normal_q30: [0, 0, 1_i64 << 30],
                offset_microunits: 0,
            },
            0,
        )
        .unwrap();
        let report = build_adaptive_octree(&plane, small_config()).unwrap();
        assert!(report.samples().values().any(|value| *value == 0));
        let boundary_leaves: Vec<&OctreeLeaf> = report
            .leaves()
            .filter(|leaf| leaf.minimum_tick[2] == 0)
            .collect();
        assert!(!boundary_leaves.is_empty());
        assert!(boundary_leaves
            .iter()
            .all(|leaf| leaf.classification.status == CellStatus::SurfacePossible));
    }

    #[test]
    fn completed_tree_is_two_to_one_balanced() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let mut config = small_config();
        config.max_leaf_cells = 4_096;
        config.max_unique_samples = 10_000;
        let report = build_adaptive_octree(&grid, config).unwrap();
        assert_eq!(report.balance_status(), BalanceStatus::Balanced);
        assert_eq!(report.metrics().remaining_2_to_1_violations, 0);
    }

    #[test]
    fn deterministic_receipt_recomputes_and_rejects_mutated_leaf() {
        let grid = grid([2, 2, 2], [-2_000_000; 3], [2_000_000; 3]);
        let report = build_adaptive_octree(&grid, small_config()).unwrap();
        let bytes = report.canonical_bytes().unwrap();
        let second = build_adaptive_octree(&grid, small_config()).unwrap();
        assert_eq!(bytes, second.canonical_bytes().unwrap());
        report.verify(&grid).unwrap();
        let mut altered = report.clone();
        let first_key = *altered.leaves.keys().next().unwrap();
        altered.leaves.get_mut(&first_key).unwrap().span_ticks += 1;
        assert_eq!(altered.verify(&grid), Err(AdaptiveOctreeError::ReportMismatch));
    }
}
