#![forbid(unsafe_code)]
//! Conservative cell classification over canonical OUROSGD1 samples.
//!
//! This crate does not generate a mesh or resample an uncertified field.
//! For an ExactAnalyticSdf grid it decodes the bound OUROFLD1 descriptor,
//! reproduces every canonical sample, checks adjacent-sample Lipschitz
//! consistency, and then applies the unit-Lipschitz cell interval bound.
//! Approximate, mesh-derived and unknown grids remain UncertifiedField.

use std::{error::Error, fmt};

use ouroboros_scalar_grid::{
    AnalyticField, FieldSemantics, ScalarGrid, ScalarGridError, FIELD_MAGIC,
    FIELD_SCHEMA_VERSION,
};

pub const INTERVAL_SCHEMA_VERSION: u16 = 1;
pub const INTERVAL_MAGIC: [u8; 8] = *b"OUROIV1\0";
pub const POLICY_MAGIC: [u8; 8] = *b"OUROIP1\0";
pub const MAX_CLASSIFIED_CELLS: u64 = 2_048_383;
pub const MAX_RECEIPT_BYTES: u64 = 96 * 1024 * 1024;
pub const MAX_WORKING_PAYLOAD_BYTES: u64 = 192 * 1024 * 1024;
pub const CELL_CLASSIFICATION_ENCODED_LEN: u64 = 27;
const Q32_ONE: u64 = 1_u64 << 32;
const DISTANCE_QUANTIZATION: f64 = 1_000_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum BoundAuthority {
    /// The OUROFLD1 descriptor is a built-in exact analytic primitive and
    /// every quantized sample was recomputed from that descriptor.
    VerifiedExactAnalyticSampler = 1,
    /// No mathematical bound has been verified for this source.
    Uncertified = 2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CellStatus {
    CertifiedInside = 1,
    CertifiedOutside = 2,
    SurfacePossible = 3,
    UncertifiedField = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntervalLimits {
    pub max_cells: u64,
    pub max_working_payload_bytes: u64,
    pub max_receipt_bytes: u64,
}

impl Default for IntervalLimits {
    fn default() -> Self {
        Self {
            max_cells: MAX_CLASSIFIED_CELLS,
            max_working_payload_bytes: MAX_WORKING_PAYLOAD_BYTES,
            max_receipt_bytes: MAX_RECEIPT_BYTES,
        }
    }
}

impl IntervalLimits {
    fn validate(self) -> Result<(), IntervalError> {
        if self.max_cells == 0
            || self.max_cells > MAX_CLASSIFIED_CELLS
            || self.max_working_payload_bytes == 0
            || self.max_working_payload_bytes > MAX_WORKING_PAYLOAD_BYTES
            || self.max_receipt_bytes == 0
            || self.max_receipt_bytes > MAX_RECEIPT_BYTES
        {
            return Err(IntervalError::InvalidLimits);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntervalPolicy {
    pub schema_version: u16,
    pub policy_version: u16,
    pub authority: BoundAuthority,
    /// Q32.32 Lipschitz upper bound. Zero means no bound is available.
    pub lipschitz_q32: u64,
    /// One microunit covers nearest-integer sample quantization uncertainty.
    pub quantization_guard_microunits: u32,
    /// Maximum cell-corner to cell-point distance, rounded upward.
    pub cell_diagonal_microunits: u64,
}

impl IntervalPolicy {
    fn canonical_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(33);
        out.extend_from_slice(&POLICY_MAGIC);
        put_u16(&mut out, self.schema_version);
        put_u16(&mut out, self.policy_version);
        out.push(self.authority as u8);
        put_u64(&mut out, self.lipschitz_q32);
        put_u32(&mut out, self.quantization_guard_microunits);
        put_u64(&mut out, self.cell_diagonal_microunits);
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CellClassification {
    pub status: CellStatus,
    /// One bit for each of the 12 cube edges. Negative is inside; zero is
    /// treated as outside for this diagnostic mask.
    pub sign_changing_edge_mask: u16,
    pub minimum_corner_sample_microunits: i32,
    pub maximum_corner_sample_microunits: i32,
    /// Valid only for CertifiedInside, CertifiedOutside or SurfacePossible.
    /// They are zero for UncertifiedField and must not be interpreted there.
    pub lower_bound_microunits: i64,
    pub upper_bound_microunits: i64,
}

/// A verified evaluator for new samples from one canonical exact-analytic
/// grid. Construction decodes the descriptor and reproduces every existing
/// sample; subsequent coordinates must remain inside the source grid bounds.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VerifiedExactField {
    field: AnalyticField,
    isovalue_microunits: i32,
    minimum_microunits: [i64; 3],
    maximum_microunits: [i64; 3],
    descriptor_bytes: Vec<u8>,
}

impl VerifiedExactField {
    pub fn from_grid(grid: &ScalarGrid) -> Result<Self, IntervalError> {
        grid.validate()?;
        if grid.semantics() != FieldSemantics::ExactAnalyticSdf {
            return Err(IntervalError::FieldNotCertifiable);
        }
        let field = decode_exact_analytic_descriptor(grid.descriptor_bytes())?;
        verify_exact_samples(grid, field)?;
        Ok(Self {
            field,
            isovalue_microunits: grid.isovalue_microunits(),
            minimum_microunits: grid.spec().minimum_microunits,
            maximum_microunits: grid.spec().maximum_microunits,
            descriptor_bytes: grid.descriptor_bytes().to_vec(),
        })
    }

    pub fn descriptor_bytes(&self) -> &[u8] {
        &self.descriptor_bytes
    }

    pub fn isovalue_microunits(&self) -> i32 {
        self.isovalue_microunits
    }

    pub fn bounds_microunits(&self) -> ([i64; 3], [i64; 3]) {
        (self.minimum_microunits, self.maximum_microunits)
    }

    pub fn sample_at_microunits(&self, coordinate: [i64; 3]) -> Result<i32, IntervalError> {
        for axis in 0..3 {
            if coordinate[axis] < self.minimum_microunits[axis]
                || coordinate[axis] > self.maximum_microunits[axis]
            {
                return Err(IntervalError::SampleOutsideSourceBounds);
            }
        }
        let point = coordinate.map(|value| value as f64 / DISTANCE_QUANTIZATION);
        let value = self.field.evaluate(point)?
            - f64::from(self.isovalue_microunits) / DISTANCE_QUANTIZATION;
        quantize_distance_microunits(value)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IntervalMetrics {
    pub cell_dimensions: [u32; 3],
    pub total_cells: u64,
    pub certified_inside_cells: u64,
    pub certified_outside_cells: u64,
    pub surface_possible_cells: u64,
    pub uncertified_cells: u64,
    pub sign_changing_cells: u64,
    pub verified_source_samples: u64,
    pub adjacent_sample_checks: u64,
    pub working_payload_estimate_bytes: u64,
    pub receipt_bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IntervalReport {
    source_grid_canonical_bytes: Vec<u8>,
    policy: IntervalPolicy,
    metrics: IntervalMetrics,
    cells: Vec<CellClassification>,
}

impl IntervalReport {
    pub fn policy(&self) -> IntervalPolicy {
        self.policy
    }

    pub fn metrics(&self) -> IntervalMetrics {
        self.metrics
    }

    pub fn cells(&self) -> &[CellClassification] {
        &self.cells
    }

    pub fn source_grid_canonical_bytes(&self) -> &[u8] {
        &self.source_grid_canonical_bytes
    }

    /// Deterministically recompute the report from the exact input grid.
    pub fn verify(&self, grid: &ScalarGrid) -> Result<(), IntervalError> {
        let recomputed = classify(grid, IntervalLimits::default())?;
        if recomputed != *self {
            return Err(IntervalError::ReportMismatch);
        }
        Ok(())
    }

    /// Canonical receipt embeds the exact OUROSGD1 bytes and every cell result.
    /// This identifies evidence; it does not authenticate the grid producer.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, IntervalError> {
        let expected = expected_receipt_len(
            self.source_grid_canonical_bytes.len() as u64,
            self.cells.len() as u64,
        )?;
        if expected > MAX_RECEIPT_BYTES || expected != self.metrics.receipt_bytes {
            return Err(IntervalError::ReceiptLengthMismatch);
        }
        let mut out = Vec::new();
        out.try_reserve_exact(usize::try_from(expected).map_err(|_| IntervalError::SizeOverflow)?)
            .map_err(|_| IntervalError::AllocationFailed)?;
        out.extend_from_slice(&INTERVAL_MAGIC);
        put_u16(&mut out, INTERVAL_SCHEMA_VERSION);
        put_u32(
            &mut out,
            u32::try_from(self.source_grid_canonical_bytes.len())
                .map_err(|_| IntervalError::SizeOverflow)?,
        );
        out.extend_from_slice(&self.source_grid_canonical_bytes);
        out.extend_from_slice(&self.policy.canonical_bytes());
        put_u32(&mut out, self.metrics.cell_dimensions[0]);
        put_u32(&mut out, self.metrics.cell_dimensions[1]);
        put_u32(&mut out, self.metrics.cell_dimensions[2]);
        put_u64(&mut out, self.metrics.total_cells);
        put_u64(&mut out, self.metrics.certified_inside_cells);
        put_u64(&mut out, self.metrics.certified_outside_cells);
        put_u64(&mut out, self.metrics.surface_possible_cells);
        put_u64(&mut out, self.metrics.uncertified_cells);
        put_u64(&mut out, self.metrics.sign_changing_cells);
        put_u64(&mut out, self.metrics.verified_source_samples);
        put_u64(&mut out, self.metrics.adjacent_sample_checks);
        put_u64(&mut out, self.metrics.working_payload_estimate_bytes);
        for cell in &self.cells {
            out.push(cell.status as u8);
            put_u16(&mut out, cell.sign_changing_edge_mask);
            put_i32(&mut out, cell.minimum_corner_sample_microunits);
            put_i32(&mut out, cell.maximum_corner_sample_microunits);
            put_i64(&mut out, cell.lower_bound_microunits);
            put_i64(&mut out, cell.upper_bound_microunits);
        }
        if out.len() as u64 != expected {
            return Err(IntervalError::ReceiptLengthMismatch);
        }
        Ok(out)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IntervalError {
    InvalidLimits,
    InvalidGrid,
    SizeOverflow,
    CellBudgetExceeded,
    WorkingBudgetExceeded,
    ReceiptBudgetExceeded,
    AllocationFailed,
    ExactDescriptorMalformed,
    ExactDescriptorUnsupported,
    FieldNotCertifiable,
    SampleOutsideSourceBounds,
    SourceSampleMismatch,
    InconsistentSamples,
    ReportMismatch,
    ReceiptLengthMismatch,
    ScalarGrid(ScalarGridError),
}

impl fmt::Display for IntervalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for IntervalError {}

impl From<ScalarGridError> for IntervalError {
    fn from(value: ScalarGridError) -> Self {
        Self::ScalarGrid(value)
    }
}

const CORNERS: [[u32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [1, 1, 0],
    [0, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [1, 1, 1],
    [0, 1, 1],
];

const EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [1, 2],
    [2, 3],
    [3, 0],
    [4, 5],
    [5, 6],
    [6, 7],
    [7, 4],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

pub fn classify(grid: &ScalarGrid, limits: IntervalLimits) -> Result<IntervalReport, IntervalError> {
    limits.validate()?;
    let grid_limits = grid.validate()?;
    let spec = grid.spec();
    let dimensions = spec.dimensions;
    if dimensions.iter().any(|dimension| *dimension < 2) {
        return Err(IntervalError::InvalidGrid);
    }
    let cell_dimensions = [dimensions[0] - 1, dimensions[1] - 1, dimensions[2] - 1];
    let total_cells = u64::from(cell_dimensions[0])
        .checked_mul(u64::from(cell_dimensions[1]))
        .and_then(|value| value.checked_mul(u64::from(cell_dimensions[2])))
        .ok_or(IntervalError::SizeOverflow)?;
    if total_cells > limits.max_cells {
        return Err(IntervalError::CellBudgetExceeded);
    }

    // Keep the verified field handle available to downstream adaptive
    // samplers; an exact semantic label alone never grants certification.
    let verified_field = if grid.semantics() == FieldSemantics::ExactAnalyticSdf {
        Some(VerifiedExactField::from_grid(grid)?)
    } else {
        None
    };
    let (authority, verified_source_samples, lipschitz_q32) =
        if verified_field.is_some() {
            (BoundAuthority::VerifiedExactAnalyticSampler, grid_limits.sample_count, Q32_ONE)
        } else {
            (BoundAuthority::Uncertified, 0, 0)
        };

    let axis_steps = [
        ceil_div_u128(
            (spec.maximum_microunits[0] - spec.minimum_microunits[0]) as u128,
            u128::from(dimensions[0] - 1),
        )?,
        ceil_div_u128(
            (spec.maximum_microunits[1] - spec.minimum_microunits[1]) as u128,
            u128::from(dimensions[1] - 1),
        )?,
        ceil_div_u128(
            (spec.maximum_microunits[2] - spec.minimum_microunits[2]) as u128,
            u128::from(dimensions[2] - 1),
        )?,
    ];
    let diagonal_squared = axis_steps.iter().try_fold(0_u128, |sum, value| {
        sum.checked_add(value.checked_mul(*value).ok_or(IntervalError::SizeOverflow)?)
            .ok_or(IntervalError::SizeOverflow)
    })?;
    let diagonal = ceil_sqrt_u128(diagonal_squared)?;
    let diagonal = u64::try_from(diagonal).map_err(|_| IntervalError::SizeOverflow)?;
    let quantization_guard = 1_u32;
    let interval_margin = if authority == BoundAuthority::VerifiedExactAnalyticSampler {
        diagonal.checked_add(u64::from(quantization_guard)).ok_or(IntervalError::SizeOverflow)?
    } else {
        0
    };

    let in_memory_cell_bytes = total_cells
        .checked_mul(std::mem::size_of::<CellClassification>() as u64)
        .ok_or(IntervalError::SizeOverflow)?;
    let receipt_bytes = expected_receipt_len(grid_limits.canonical_bytes, total_cells)?;
    if receipt_bytes > limits.max_receipt_bytes {
        return Err(IntervalError::ReceiptBudgetExceeded);
    }
    // Includes the source sample vector, canonical source bytes, fixed cell
    // records and a second copy of source+records while emitting the receipt.
    let working_payload_estimate_bytes = grid_limits.sample_bytes
        .checked_add(grid_limits.canonical_bytes)
        .and_then(|value| value.checked_add(in_memory_cell_bytes))
        .and_then(|value| value.checked_add(receipt_bytes))
        .ok_or(IntervalError::SizeOverflow)?;
    if working_payload_estimate_bytes > limits.max_working_payload_bytes {
        return Err(IntervalError::WorkingBudgetExceeded);
    }

    // Verify discrete 1-Lipschitz consistency on every grid edge. This is a
    // necessary conformance check; source re-evaluation above is what binds
    // these samples to a built-in exact analytic field.
    let adjacent_sample_checks = if authority == BoundAuthority::VerifiedExactAnalyticSampler {
        verify_adjacent_sample_consistency(grid, axis_steps)?
    } else {
        0
    };

    let source_grid_canonical_bytes = grid.canonical_bytes()?;
    let cell_count = usize::try_from(total_cells).map_err(|_| IntervalError::SizeOverflow)?;
    let mut cells = Vec::new();
    cells.try_reserve_exact(cell_count).map_err(|_| IntervalError::AllocationFailed)?;

    let mut certified_inside_cells = 0_u64;
    let mut certified_outside_cells = 0_u64;
    let mut surface_possible_cells = 0_u64;
    let mut uncertified_cells = 0_u64;
    let mut sign_changing_cells = 0_u64;

    for z in 0..cell_dimensions[2] {
        for y in 0..cell_dimensions[1] {
            for x in 0..cell_dimensions[0] {
                let mut samples = [0_i32; 8];
                for (corner, offset) in CORNERS.iter().enumerate() {
                    samples[corner] = grid.value_at(
                        x + offset[0],
                        y + offset[1],
                        z + offset[2],
                    )?;
                }
                let minimum = *samples.iter().min().ok_or(IntervalError::InvalidGrid)?;
                let maximum = *samples.iter().max().ok_or(IntervalError::InvalidGrid)?;
                let mut edge_mask = 0_u16;
                for (edge_index, edge) in EDGES.iter().enumerate() {
                    if (samples[edge[0]] < 0) != (samples[edge[1]] < 0) {
                        edge_mask |= 1_u16 << edge_index;
                    }
                }
                if edge_mask != 0 {
                    sign_changing_cells += 1;
                }

                let classification = if authority == BoundAuthority::Uncertified {
                    uncertified_cells += 1;
                    CellClassification {
                        status: CellStatus::UncertifiedField,
                        sign_changing_edge_mask: edge_mask,
                        minimum_corner_sample_microunits: minimum,
                        maximum_corner_sample_microunits: maximum,
                        lower_bound_microunits: 0,
                        upper_bound_microunits: 0,
                    }
                } else {
                    let lower = i64::from(maximum)
                        .checked_sub(i64::try_from(interval_margin).map_err(|_| IntervalError::SizeOverflow)?)
                        .ok_or(IntervalError::SizeOverflow)?;
                    let upper = i64::from(minimum)
                        .checked_add(i64::try_from(interval_margin).map_err(|_| IntervalError::SizeOverflow)?)
                        .ok_or(IntervalError::SizeOverflow)?;
                    let status = if lower > 0 {
                        certified_outside_cells += 1;
                        CellStatus::CertifiedOutside
                    } else if upper < 0 {
                        certified_inside_cells += 1;
                        CellStatus::CertifiedInside
                    } else {
                        surface_possible_cells += 1;
                        CellStatus::SurfacePossible
                    };
                    CellClassification {
                        status,
                        sign_changing_edge_mask: edge_mask,
                        minimum_corner_sample_microunits: minimum,
                        maximum_corner_sample_microunits: maximum,
                        lower_bound_microunits: lower,
                        upper_bound_microunits: upper,
                    }
                };
                cells.push(classification);
            }
        }
    }

    let policy = IntervalPolicy {
        schema_version: INTERVAL_SCHEMA_VERSION,
        policy_version: 1,
        authority,
        lipschitz_q32,
        quantization_guard_microunits: quantization_guard,
        cell_diagonal_microunits: diagonal,
    };
    let metrics = IntervalMetrics {
        cell_dimensions,
        total_cells,
        certified_inside_cells,
        certified_outside_cells,
        surface_possible_cells,
        uncertified_cells,
        sign_changing_cells,
        verified_source_samples,
        adjacent_sample_checks,
        working_payload_estimate_bytes,
        receipt_bytes,
    };
    Ok(IntervalReport {
        source_grid_canonical_bytes,
        policy,
        metrics,
        cells,
    })
}

fn decode_exact_analytic_descriptor(bytes: &[u8]) -> Result<AnalyticField, IntervalError> {
    if bytes.len() < 12
        || bytes[0..8] != FIELD_MAGIC
        || read_u16(bytes, 8)? != FIELD_SCHEMA_VERSION
        || bytes[10] != FieldSemantics::ExactAnalyticSdf as u8
    {
        return Err(IntervalError::ExactDescriptorMalformed);
    }
    let kind = bytes[11];
    let expected_len = match kind {
        1 | 7 => 44,
        2 => 60,
        3 => 92,
        4 => 68,
        5 | 6 => 52,
        _ => return Err(IntervalError::ExactDescriptorUnsupported),
    };
    if bytes.len() != expected_len {
        return Err(IntervalError::ExactDescriptorMalformed);
    }
    let mut cursor = 12_usize;
    let field = match kind {
        1 => AnalyticField::Sphere {
            center_microunits: read_i64_array3(bytes, &mut cursor)?,
            radius_microunits: read_i64(bytes, &mut cursor)?,
        },
        2 => AnalyticField::Box {
            center_microunits: read_i64_array3(bytes, &mut cursor)?,
            half_extents_microunits: read_i64_array3(bytes, &mut cursor)?,
        },
        3 => {
            let center_microunits = read_i64_array3(bytes, &mut cursor)?;
            let half_extents_microunits = read_i64_array3(bytes, &mut cursor)?;
            let mut rotation_q30 = [0_i64; 4];
            for value in &mut rotation_q30 {
                *value = read_i64(bytes, &mut cursor)?;
            }
            AnalyticField::OrientedBox {
                center_microunits,
                half_extents_microunits,
                rotation_q30,
            }
        }
        4 => AnalyticField::Capsule {
            endpoint_a_microunits: read_i64_array3(bytes, &mut cursor)?,
            endpoint_b_microunits: read_i64_array3(bytes, &mut cursor)?,
            radius_microunits: read_i64(bytes, &mut cursor)?,
        },
        5 => AnalyticField::Torus {
            center_microunits: read_i64_array3(bytes, &mut cursor)?,
            major_radius_microunits: read_i64(bytes, &mut cursor)?,
            minor_radius_microunits: read_i64(bytes, &mut cursor)?,
        },
        6 => AnalyticField::CappedCylinder {
            center_microunits: read_i64_array3(bytes, &mut cursor)?,
            radius_microunits: read_i64(bytes, &mut cursor)?,
            half_length_microunits: read_i64(bytes, &mut cursor)?,
        },
        7 => {
            let mut normal_q30 = [0_i64; 3];
            for value in &mut normal_q30 {
                *value = read_i64(bytes, &mut cursor)?;
            }
            AnalyticField::Plane {
                normal_q30,
                offset_microunits: read_i64(bytes, &mut cursor)?,
            }
        }
        _ => return Err(IntervalError::ExactDescriptorUnsupported),
    };
    if cursor != bytes.len() {
        return Err(IntervalError::ExactDescriptorMalformed);
    }
    field.validate()?;
    Ok(field)
}

fn quantize_distance_microunits(value: f64) -> Result<i32, IntervalError> {
    let scaled = value * DISTANCE_QUANTIZATION;
    if !scaled.is_finite() || scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return Err(IntervalError::SourceSampleMismatch);
    }
    Ok(scaled.round() as i32)
}

fn verify_exact_samples(grid: &ScalarGrid, field: AnalyticField) -> Result<(), IntervalError> {
    let spec = grid.spec();
    let iso = f64::from(grid.isovalue_microunits()) / DISTANCE_QUANTIZATION;
    for (index, stored) in grid.quantized_samples().iter().enumerate() {
        let point = spec.point_at_linear_index(index)?;
        let sampled = field.evaluate(point)? - iso;
        let expected = quantize_distance_microunits(sampled)?;
        if expected != *stored {
            return Err(IntervalError::SourceSampleMismatch);
        }
    }
    Ok(())
}

fn verify_adjacent_sample_consistency(
    grid: &ScalarGrid,
    axis_steps: [u128; 3],
) -> Result<u64, IntervalError> {
    verify_adjacent_values(
        grid.spec().dimensions,
        grid.quantized_samples(),
        axis_steps,
    )
}

fn verify_adjacent_values(
    dims: [u32; 3],
    samples: &[i32],
    axis_steps: [u128; 3],
) -> Result<u64, IntervalError> {
    let expected = u64::from(dims[0])
        .checked_mul(u64::from(dims[1]))
        .and_then(|value| value.checked_mul(u64::from(dims[2])))
        .ok_or(IntervalError::SizeOverflow)?;
    if samples.len() as u64 != expected {
        return Err(IntervalError::InvalidGrid);
    }
    let mut checks = 0_u64;
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let index = usize::try_from(
                    u64::from(x)
                        + u64::from(dims[0])
                            * (u64::from(y) + u64::from(dims[1]) * u64::from(z)),
                )
                .map_err(|_| IntervalError::SizeOverflow)?;
                let value = i64::from(*samples.get(index).ok_or(IntervalError::InvalidGrid)?);
                for axis in 0..3 {
                    let mut next = [x, y, z];
                    if next[axis] + 1 >= dims[axis] {
                        continue;
                    }
                    next[axis] += 1;
                    let other_index = usize::try_from(
                        u64::from(next[0])
                            + u64::from(dims[0])
                                * (u64::from(next[1]) + u64::from(dims[1]) * u64::from(next[2])),
                    )
                    .map_err(|_| IntervalError::SizeOverflow)?;
                    let other = i64::from(*samples.get(other_index).ok_or(IntervalError::InvalidGrid)?);
                    let difference = value.abs_diff(other) as u128;
                    let allowed = axis_steps[axis]
                        .checked_add(1)
                        .ok_or(IntervalError::SizeOverflow)?;
                    if difference > allowed {
                        return Err(IntervalError::InconsistentSamples);
                    }
                    checks = checks.checked_add(1).ok_or(IntervalError::SizeOverflow)?;
                }
            }
        }
    }
    Ok(checks)
}

fn expected_receipt_len(source_len: u64, cell_count: u64) -> Result<u64, IntervalError> {
    8_u64
        .checked_add(2 + 4)
        .and_then(|value| value.checked_add(source_len))
        .and_then(|value| value.checked_add(33))
        .and_then(|value| value.checked_add(3 * 4 + 8 * 9))
        .and_then(|value| value.checked_add(cell_count.checked_mul(CELL_CLASSIFICATION_ENCODED_LEN)?))
        .ok_or(IntervalError::SizeOverflow)
}

fn ceil_div_u128(numerator: u128, denominator: u128) -> Result<u128, IntervalError> {
    if denominator == 0 {
        return Err(IntervalError::InvalidGrid);
    }
    numerator
        .checked_add(denominator - 1)
        .ok_or(IntervalError::SizeOverflow)
        .map(|value| value / denominator)
}

fn ceil_sqrt_u128(value: u128) -> Result<u128, IntervalError> {
    if value < 2 {
        return Ok(value);
    }
    let mut x = value;
    let mut y = x
        .checked_add(1)
        .ok_or(IntervalError::SizeOverflow)?
        / 2;
    while y < x {
        x = y;
        y = x
            .checked_add(value / x)
            .ok_or(IntervalError::SizeOverflow)?
            / 2;
    }
    let square = x.checked_mul(x).ok_or(IntervalError::SizeOverflow)?;
    if square == value {
        Ok(x)
    } else {
        x.checked_add(1).ok_or(IntervalError::SizeOverflow)
    }
}

fn read_u16(bytes: &[u8], offset: usize) -> Result<u16, IntervalError> {
    let slice = bytes.get(offset..offset + 2).ok_or(IntervalError::ExactDescriptorMalformed)?;
    Ok(u16::from_le_bytes(slice.try_into().map_err(|_| IntervalError::ExactDescriptorMalformed)?))
}

fn read_i64(bytes: &[u8], offset: &mut usize) -> Result<i64, IntervalError> {
    let end = (*offset).checked_add(8).ok_or(IntervalError::SizeOverflow)?;
    let slice = bytes.get(*offset..end).ok_or(IntervalError::ExactDescriptorMalformed)?;
    *offset = end;
    Ok(i64::from_le_bytes(slice.try_into().map_err(|_| IntervalError::ExactDescriptorMalformed)?))
}

fn read_i64_array3(bytes: &[u8], offset: &mut usize) -> Result<[i64; 3], IntervalError> {
    Ok([
        read_i64(bytes, offset)?,
        read_i64(bytes, offset)?,
        read_i64(bytes, offset)?,
    ])
}

fn put_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_le_bytes());
}
fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;
    use ouroboros_scalar_grid::{GridSpec, ScalarGrid, MAX_CANONICAL_BYTES, MAX_SAMPLES};

    fn spec(dimensions: [u32; 3]) -> GridSpec {
        GridSpec {
            minimum_microunits: [-2_000_000, -2_000_000, -2_000_000],
            maximum_microunits: [2_000_000, 2_000_000, 2_000_000],
            dimensions,
            max_samples: MAX_SAMPLES,
            max_descriptor_bytes: 1_048_576,
            max_canonical_bytes: MAX_CANONICAL_BYTES,
            max_working_payload_bytes: 32 * 1024 * 1024,
        }
    }

    fn sphere_grid(dimensions: [u32; 3], radius: i64) -> ScalarGrid {
        ScalarGrid::sample_exact(
            spec(dimensions),
            AnalyticField::Sphere {
                center_microunits: [0, 0, 0],
                radius_microunits: radius,
            },
            0,
        )
        .unwrap()
    }

    #[test]
    fn verified_exact_field_samples_inside_bounds_and_rejects_outside() {
        let grid = sphere_grid([9, 9, 9], 1_000_000);
        let verified = VerifiedExactField::from_grid(&grid).unwrap();
        let spec = grid.spec();
        let center_index = spec.linear_index(4, 4, 4).unwrap();
        let center = [
            0_i64,
            0_i64,
            0_i64,
        ];
        assert_eq!(
            verified.sample_at_microunits(center).unwrap(),
            grid.quantized_samples()[center_index]
        );
        assert_eq!(
            verified.sample_at_microunits([2_000_001, 0, 0]),
            Err(IntervalError::SampleOutsideSourceBounds)
        );
        let approximate = ScalarGrid::from_samples(
            grid.spec(),
            FieldSemantics::ApproximateAnalyticField,
            grid.descriptor_bytes().to_vec(),
            grid.isovalue_microunits(),
            grid.quantized_samples().to_vec(),
        )
        .unwrap();
        assert_eq!(
            VerifiedExactField::from_grid(&approximate),
            Err(IntervalError::FieldNotCertifiable)
        );
    }

    #[test]
    fn exact_sphere_produces_inside_outside_and_surface_possible_cells() {
        let grid = sphere_grid([17, 17, 17], 1_500_000);
        let report = classify(&grid, IntervalLimits::default()).unwrap();
        let metrics = report.metrics();
        assert!(metrics.certified_inside_cells > 0);
        assert!(metrics.certified_outside_cells > 0);
        assert!(metrics.surface_possible_cells > 0);
        assert_eq!(metrics.uncertified_cells, 0);
        assert_eq!(metrics.total_cells, 16 * 16 * 16);
        assert_eq!(metrics.verified_source_samples, 17 * 17 * 17);
        assert_eq!(report.policy().authority, BoundAuthority::VerifiedExactAnalyticSampler);
        assert_eq!(report.policy().lipschitz_q32, Q32_ONE);
    }

    #[test]
    fn certified_bounds_match_dense_ground_truth_for_every_certified_cell() {
        let grid = sphere_grid([17, 17, 17], 1_500_000);
        let report = classify(&grid, IntervalLimits::default()).unwrap();
        let spec = grid.spec();
        let dims = spec.dimensions;
        for (linear, cell) in report.cells().iter().enumerate() {
            if cell.status != CellStatus::CertifiedInside && cell.status != CellStatus::CertifiedOutside {
                continue;
            }
            let x = (linear as u32) % (dims[0] - 1);
            let y = ((linear as u32) / (dims[0] - 1)) % (dims[1] - 1);
            let z = (linear as u32) / ((dims[0] - 1) * (dims[1] - 1));
            let field = AnalyticField::Sphere {
                center_microunits: [0, 0, 0],
                radius_microunits: 1_500_000,
            };
            for iz in 0..=3 {
                for iy in 0..=3 {
                    for ix in 0..=3 {
                        let point = [
                            cell_coordinate(spec.minimum_microunits[0], spec.maximum_microunits[0], x, ix, dims[0]),
                            cell_coordinate(spec.minimum_microunits[1], spec.maximum_microunits[1], y, iy, dims[1]),
                            cell_coordinate(spec.minimum_microunits[2], spec.maximum_microunits[2], z, iz, dims[2]),
                        ];
                        let value = field.evaluate(point).unwrap();
                        match cell.status {
                            CellStatus::CertifiedInside => assert!(value < 0.0, "inside certificate contains {value}"),
                            CellStatus::CertifiedOutside => assert!(value > 0.0, "outside certificate contains {value}"),
                            _ => unreachable!(),
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn exact_zero_lattice_vertices_remain_surface_possible() {
        let field = AnalyticField::Plane {
            normal_q30: [0, 0, 1_i64 << 30],
            offset_microunits: 0,
        };
        let grid = ScalarGrid::sample_exact(spec([9, 9, 9]), field, 0).unwrap();
        let report = classify(&grid, IntervalLimits::default()).unwrap();
        let cell_dims = report.metrics().cell_dimensions;
        for z in [3_u32, 4_u32] {
            for y in 0..cell_dims[1] {
                for x in 0..cell_dims[0] {
                    let index = x as usize
                        + cell_dims[0] as usize
                            * (y as usize + cell_dims[1] as usize * z as usize);
                    assert_eq!(report.cells()[index].status, CellStatus::SurfacePossible);
                }
            }
        }
    }

    #[test]
    fn approximate_field_never_receives_an_exact_lipschitz_certificate() {
        let exact = sphere_grid([9, 9, 9], 1_000_000);
        let approximate = ScalarGrid::from_samples(
            exact.spec(),
            FieldSemantics::ApproximateAnalyticField,
            exact.descriptor_bytes().to_vec(),
            exact.isovalue_microunits(),
            exact.quantized_samples().to_vec(),
        )
        .unwrap();
        let report = classify(&approximate, IntervalLimits::default()).unwrap();
        assert_eq!(report.policy().authority, BoundAuthority::Uncertified);
        assert_eq!(report.policy().lipschitz_q32, 0);
        assert_eq!(report.metrics().uncertified_cells, report.metrics().total_cells);
        assert_eq!(report.metrics().certified_inside_cells, 0);
        assert_eq!(report.metrics().certified_outside_cells, 0);
    }

    #[test]
    fn forged_exact_semantics_with_modified_samples_is_rejected() {
        let exact = sphere_grid([9, 9, 9], 1_000_000);
        let mut samples = exact.quantized_samples().to_vec();
        samples[exact.spec().linear_index(4, 4, 4).unwrap()] += 500_000;
        let forged = ScalarGrid::from_samples(
            exact.spec(),
            FieldSemantics::ExactAnalyticSdf,
            exact.descriptor_bytes().to_vec(),
            exact.isovalue_microunits(),
            samples,
        )
        .unwrap();
        assert_eq!(
            classify(&forged, IntervalLimits::default()),
            Err(IntervalError::SourceSampleMismatch)
        );
    }

    #[test]
    fn malformed_or_unknown_exact_descriptors_fail_closed() {
        let exact = sphere_grid([9, 9, 9], 1_000_000);
        let mut descriptor = exact.descriptor_bytes().to_vec();
        descriptor[11] = 99;
        let invalid = ScalarGrid::from_samples(
            exact.spec(),
            FieldSemantics::ExactAnalyticSdf,
            descriptor,
            0,
            exact.quantized_samples().to_vec(),
        )
        .unwrap();
        assert!(matches!(
            classify(&invalid, IntervalLimits::default()),
            Err(IntervalError::ExactDescriptorUnsupported)
        ));
    }

    #[test]
    fn adjacent_sample_lipschitz_check_rejects_a_discontinuous_grid() {
        let samples = [0_i32, 0, 0, 0, 0, 10_000, 0, 0];
        assert_eq!(
            verify_adjacent_values([2, 2, 2], &samples, [100, 100, 100]),
            Err(IntervalError::InconsistentSamples)
        );
        assert_eq!(
            verify_adjacent_values([2, 2, 2], &[0_i32; 8], [100, 100, 100]),
            Ok(12)
        );
    }

    #[test]
    fn every_builtin_exact_field_descriptor_decodes_and_reproduces_samples() {
        const Q30: i64 = 1_i64 << 30;
        let fields = [
            AnalyticField::Sphere {
                center_microunits: [0, 0, 0],
                radius_microunits: 1_000_000,
            },
            AnalyticField::Box {
                center_microunits: [0, 0, 0],
                half_extents_microunits: [1_000_000; 3],
            },
            AnalyticField::OrientedBox {
                center_microunits: [0, 0, 0],
                half_extents_microunits: [1_000_000; 3],
                rotation_q30: [0, 0, 0, Q30],
            },
            AnalyticField::Capsule {
                endpoint_a_microunits: [0, -500_000, 0],
                endpoint_b_microunits: [0, 500_000, 0],
                radius_microunits: 250_000,
            },
            AnalyticField::Torus {
                center_microunits: [0, 0, 0],
                major_radius_microunits: 1_000_000,
                minor_radius_microunits: 250_000,
            },
            AnalyticField::CappedCylinder {
                center_microunits: [0, 0, 0],
                radius_microunits: 750_000,
                half_length_microunits: 1_000_000,
            },
            AnalyticField::Plane {
                normal_q30: [0, 0, Q30],
                offset_microunits: 0,
            },
        ];
        for field in fields {
            let descriptor = field.canonical_descriptor().unwrap();
            assert_eq!(decode_exact_analytic_descriptor(&descriptor).unwrap(), field);
            let grid = ScalarGrid::sample_exact(spec([5, 5, 5]), field, 0).unwrap();
            let report = classify(&grid, IntervalLimits::default()).unwrap();
            assert_eq!(report.policy().authority, BoundAuthority::VerifiedExactAnalyticSampler);
            assert_eq!(report.metrics().verified_source_samples, 125);
        }
    }

    #[test]
    fn canonical_receipt_binds_source_grid_policy_and_cell_results() {
        let grid = sphere_grid([9, 9, 9], 1_000_000);
        let first = classify(&grid, IntervalLimits::default()).unwrap();
        let second = classify(&grid, IntervalLimits::default()).unwrap();
        let bytes = first.canonical_bytes().unwrap();
        assert_eq!(bytes, second.canonical_bytes().unwrap());
        assert_eq!(bytes.len() as u64, first.metrics().receipt_bytes);
        assert!(bytes.starts_with(&INTERVAL_MAGIC));
        first.verify(&grid).unwrap();
        let mut mutated = first.clone();
        mutated.cells[0].lower_bound_microunits += 1;
        assert_eq!(mutated.verify(&grid), Err(IntervalError::ReportMismatch));
    }

    #[test]
    fn first_axis_fastest_cell_order_is_stable() {
        let grid = sphere_grid([5, 5, 5], 1_000_000);
        let report = classify(&grid, IntervalLimits::default()).unwrap();
        assert_eq!(report.cells().len(), 4 * 4 * 4);
        // Cell (1,0,0) follows cell (0,0,0) in first-axis-fastest ordering.
        let a = report.cells()[0];
        let b = report.cells()[1];
        assert_ne!(a.minimum_corner_sample_microunits, b.minimum_corner_sample_microunits);
    }

    #[test]
    fn cell_budget_rejects_before_classification_allocation() {
        let grid = sphere_grid([17, 17, 17], 1_500_000);
        let mut limits = IntervalLimits::default();
        limits.max_cells = 100;
        assert_eq!(
            classify(&grid, limits),
            Err(IntervalError::CellBudgetExceeded)
        );
    }

    #[test]
    fn receipt_and_working_payload_ceilings_are_enforced() {
        let grid = sphere_grid([17, 17, 17], 1_500_000);
        let mut limits = IntervalLimits::default();
        limits.max_receipt_bytes = 1_000;
        assert_eq!(
            classify(&grid, limits),
            Err(IntervalError::ReceiptBudgetExceeded)
        );
        let mut limits = IntervalLimits::default();
        limits.max_working_payload_bytes = 1_000;
        assert_eq!(
            classify(&grid, limits),
            Err(IntervalError::WorkingBudgetExceeded)
        );
    }

    fn cell_coordinate(minimum: i64, maximum: i64, cell: u32, subdivision: u32, count: u32) -> f64 {
        let span = (maximum - minimum) as f64 / 1_000_000.0;
        let base = minimum as f64 / 1_000_000.0 + span * f64::from(cell) / f64::from(count - 1);
        base + span / f64::from(count - 1) * f64::from(subdivision) / 3.0
    }
}
