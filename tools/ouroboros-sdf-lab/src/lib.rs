#![forbid(unsafe_code)]
//! Bounded CPU-only semantic SDF sampling oracle for Ouroboros.
//!
//! This crate samples the source-grounded head-shell + nose analytic field.
//! It does not extract meshes, use a GPU, call the OS, access the filesystem
//! (except in the example), or depend on Python. The ellipsoid distance
//! estimator is intentionally approximate and must not be described as an
//! exact Euclidean signed distance function.

use std::{error::Error, fmt};

pub const GRID_SCHEMA_VERSION: u16 = 1;
pub const FIELD_SCHEMA_VERSION: u16 = 1;
pub const DISTANCE_QUANTIZATION: i64 = 1_000_000;
pub const MAX_SAMPLES: u64 = 2_097_152;
pub const MAX_CANONICAL_BYTES: u64 = 16 * 1024 * 1024;
/// Maximum simultaneous sample-vector + canonical-output payload; excludes
/// allocator metadata and runtime/process overhead.
pub const MAX_WORKING_PAYLOAD_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_SMOOTH_UNION_WIDTH: f64 = 0.05;
pub const FIXTURE_ID: &str = "ouroboros-face-skin-v1";

const GRID_MAGIC: [u8; 8] = *b"OUROGRD1";
const FIELD_MAGIC: [u8; 8] = *b"OUROFLD1";
const FIELD_DESCRIPTOR_BYTES: u64 = 123;
const COORDINATE_QUANTIZATION: f64 = 1_000_000.0;
const DISTANCE_EPSILON: f64 = 1.0e-15;

/// Canonical coordinate/interpretation identifiers. Changing their meaning
/// requires a field/grid schema bump rather than an undocumented reinterpretation.
const AXIS_ORDER_FORWARD_LATERAL_UP: u8 = 1;
const SIGN_NEGATIVE_INSIDE_POSITIVE_OUTSIDE: u8 = 1;
const UNIT_NORMALIZED_MAJOR_RADIUS: u8 = 1;
const ESTIMATOR_IQ_ELLIPSOID_APPROX: u8 = 1;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u16)]
pub enum RegionId {
    HeadShell = 1,
    LeftEye = 2,
    RightEye = 3,
    Nose = 4,
    Mouth = 5,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidRegion {
    pub region_id: RegionId,
    /// Local coordinates are ordered forward, lateral, up.
    pub center: [f64; 3],
    /// Positive radii in forward, lateral, up axes.
    pub radii: [f64; 3],
}

impl EllipsoidRegion {
    pub fn validate(self) -> Result<(), SdfError> {
        if !self.center.into_iter().all(f64::is_finite)
            || !self
                .radii
                .into_iter()
                .all(|radius| radius.is_finite() && radius > 0.0)
        {
            return Err(SdfError::InvalidFeature);
        }
        for value in self.center.into_iter().chain(self.radii) {
            if !is_canonical_microunit(value)? {
                return Err(SdfError::NonCanonicalParameter);
            }
        }
        Ok(())
    }

    fn canonical_fields(self, out: &mut Vec<u8>) -> Result<(), SdfError> {
        self.validate()?;
        put_u16(out, self.region_id as u16);
        for value in self.center {
            put_i64(out, coordinate_to_microunits(value)?);
        }
        for value in self.radii {
            put_i64(out, coordinate_to_microunits(value)?);
        }
        Ok(())
    }
}

/// The exact five analytic regions authored by PR #15 at
/// d5a7f877d71757db49e63ad1b1582a661f6e4ef2, expressed in normalized
/// major-radius units and the OUROLND1 forward/lateral/up basis.
///
/// These are source-derived primitive parameters, not a finished face model.
pub fn source_face_features() -> [EllipsoidRegion; 5] {
    [
        EllipsoidRegion {
            region_id: RegionId::HeadShell,
            center: [0.030, 0.0, 0.0],
            radii: [0.120, 0.060, 0.110],
        },
        EllipsoidRegion {
            region_id: RegionId::LeftEye,
            center: [0.060, -0.050, 0.020],
            radii: [0.016, 0.018, 0.014],
        },
        EllipsoidRegion {
            region_id: RegionId::RightEye,
            center: [0.060, 0.050, 0.020],
            radii: [0.016, 0.018, 0.014],
        },
        EllipsoidRegion {
            region_id: RegionId::Nose,
            center: [0.160, 0.0, 0.005],
            radii: [0.020, 0.014, 0.018],
        },
        EllipsoidRegion {
            region_id: RegionId::Mouth,
            center: [0.140, 0.0, -0.035],
            radii: [0.008, 0.035, 0.006],
        },
    ]
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UnionMode {
    Sharp,
    PolynomialSmooth { width: f64 },
}

impl UnionMode {
    fn validate(self) -> Result<(), SdfError> {
        match self {
            Self::Sharp => Ok(()),
            Self::PolynomialSmooth { width }
                if width.is_finite() && width > 0.0 && width <= MAX_SMOOTH_UNION_WIDTH =>
            {
                if is_canonical_microunit(width)? {
                    Ok(())
                } else {
                    Err(SdfError::NonCanonicalParameter)
                }
            }
            Self::PolynomialSmooth { .. } => Err(SdfError::InvalidSmoothWidth),
        }
    }

    fn canonical_fields(self, out: &mut Vec<u8>) -> Result<(), SdfError> {
        self.validate()?;
        match self {
            Self::Sharp => {
                out.push(0);
                put_i64(out, 0);
            }
            Self::PolynomialSmooth { width } => {
                out.push(1);
                put_i64(out, coordinate_to_microunits(width)?);
            }
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FaceSkinField {
    head: EllipsoidRegion,
    nose: EllipsoidRegion,
    mode: UnionMode,
}

impl FaceSkinField {
    pub fn source_fixture(mode: UnionMode) -> Result<Self, SdfError> {
        mode.validate()?;
        let features = source_face_features();
        let field = Self {
            head: features[0],
            nose: features[3],
            mode,
        };
        field.validate()?;
        Ok(field)
    }

    pub fn new(
        head: EllipsoidRegion,
        nose: EllipsoidRegion,
        mode: UnionMode,
    ) -> Result<Self, SdfError> {
        let field = Self { head, nose, mode };
        field.validate()?;
        Ok(field)
    }

    pub fn validate(self) -> Result<(), SdfError> {
        self.head.validate()?;
        self.nose.validate()?;
        self.mode.validate()?;
        if self.head.region_id != RegionId::HeadShell
            || self.nose.region_id != RegionId::Nose
        {
            return Err(SdfError::InvalidFeature);
        }
        Ok(())
    }

    pub fn mode(self) -> UnionMode {
        self.mode
    }

    /// Approximate negative-inside, zero-on-surface, positive-outside field.
    pub fn evaluate(self, point: [f64; 3]) -> Result<f64, SdfError> {
        self.validate()?;
        self.evaluate_validated(point)
    }

    fn evaluate_validated(self, point: [f64; 3]) -> Result<f64, SdfError> {
        if !point.into_iter().all(f64::is_finite) {
            return Err(SdfError::NonFiniteInput);
        }
        let head = ellipsoid_distance_approx_validated(point, self.head)?;
        let nose = ellipsoid_distance_approx_validated(point, self.nose)?;
        let value = match self.mode {
            UnionMode::Sharp => head.min(nose),
            UnionMode::PolynomialSmooth { width } => {
                polynomial_smooth_min(head, nose, width)?
            }
        };
        if !value.is_finite() {
            return Err(SdfError::NonFiniteResult);
        }
        Ok(value)
    }

    fn canonical_descriptor(self) -> Result<Vec<u8>, SdfError> {
        self.validate()?;
        let mut out = Vec::with_capacity(FIELD_DESCRIPTOR_BYTES as usize);
        out.extend_from_slice(&FIELD_MAGIC);
        put_u16(&mut out, FIELD_SCHEMA_VERSION);
        out.push(AXIS_ORDER_FORWARD_LATERAL_UP);
        out.push(SIGN_NEGATIVE_INSIDE_POSITIVE_OUTSIDE);
        out.push(UNIT_NORMALIZED_MAJOR_RADIUS);
        out.push(ESTIMATOR_IQ_ELLIPSOID_APPROX);
        self.head.canonical_fields(&mut out)?;
        self.nose.canonical_fields(&mut out)?;
        self.mode.canonical_fields(&mut out)?;
        Ok(out)
    }
}

/// IQ-style ellipsoid distance estimator.
///
/// This is a useful approximate field for the initial analytic fixture. It is
/// not the exact Euclidean distance to an ellipsoid except in special cases.
pub fn ellipsoid_distance_approx(
    point: [f64; 3],
    ellipsoid: EllipsoidRegion,
) -> Result<f64, SdfError> {
    ellipsoid.validate()?;
    ellipsoid_distance_approx_validated(point, ellipsoid)
}

fn ellipsoid_distance_approx_validated(
    point: [f64; 3],
    ellipsoid: EllipsoidRegion,
) -> Result<f64, SdfError> {
    if !point.into_iter().all(f64::is_finite) {
        return Err(SdfError::NonFiniteInput);
    }

    let q = [
        (point[0] - ellipsoid.center[0]) / ellipsoid.radii[0],
        (point[1] - ellipsoid.center[1]) / ellipsoid.radii[1],
        (point[2] - ellipsoid.center[2]) / ellipsoid.radii[2],
    ];
    let k0 = length3(q);
    let k1 = length3([
        q[0] / ellipsoid.radii[0],
        q[1] / ellipsoid.radii[1],
        q[2] / ellipsoid.radii[2],
    ]);

    // The estimator's ratio is undefined exactly at the centre. The most
    // conservative local interior offset is the smallest positive radius.
    let distance = if k1 <= DISTANCE_EPSILON {
        -ellipsoid.radii.into_iter().fold(f64::INFINITY, f64::min)
    } else {
        k0 * (k0 - 1.0) / k1
    };
    if distance.is_finite() {
        Ok(distance)
    } else {
        Err(SdfError::NonFiniteResult)
    }
}

pub fn polynomial_smooth_min(a: f64, b: f64, width: f64) -> Result<f64, SdfError> {
    if !a.is_finite() || !b.is_finite() {
        return Err(SdfError::NonFiniteInput);
    }
    if !width.is_finite() || width <= 0.0 || width > MAX_SMOOTH_UNION_WIDTH {
        return Err(SdfError::InvalidSmoothWidth);
    }
    let h = ((width - (a - b).abs()).max(0.0)) / width;
    let result = a.min(b) - 0.25 * h * h * width;
    if result.is_finite() {
        Ok(result)
    } else {
        Err(SdfError::NonFiniteResult)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GridSpec {
    pub minimum: [f64; 3],
    pub maximum: [f64; 3],
    /// Number of grid vertices along forward, lateral and up, respectively.
    pub dimensions: [u32; 3],
    pub max_samples: u64,
    /// Maximum complete canonical-grid size, including header and samples.
    pub max_canonical_bytes: u64,
    /// Maximum simultaneous sample vector plus canonical output payload.
    /// Does not represent total process RSS or allocator overhead.
    pub max_working_payload_bytes: u64,
}

impl GridSpec {
    pub fn validate(self) -> Result<GridLimits, SdfError> {
        if !self.minimum.into_iter().all(f64::is_finite)
            || !self.maximum.into_iter().all(f64::is_finite)
            || (0..3).any(|axis| self.maximum[axis] <= self.minimum[axis])
            || self.dimensions.iter().any(|dimension| *dimension < 2)
        {
            return Err(SdfError::InvalidGrid);
        }
        let nx = u64::from(self.dimensions[0]);
        let ny = u64::from(self.dimensions[1]);
        let nz = u64::from(self.dimensions[2]);
        let sample_count = nx
            .checked_mul(ny)
            .and_then(|value| value.checked_mul(nz))
            .ok_or(SdfError::SizeOverflow)?;
        if sample_count > MAX_SAMPLES || sample_count > self.max_samples {
            return Err(SdfError::SampleBudgetExceeded);
        }
        let sample_bytes = sample_count
            .checked_mul(std::mem::size_of::<i32>() as u64)
            .ok_or(SdfError::SizeOverflow)?;
        let canonical_bytes = canonical_header_len()
            .checked_add(sample_bytes)
            .ok_or(SdfError::SizeOverflow)?;
        if canonical_bytes > MAX_CANONICAL_BYTES
            || canonical_bytes > self.max_canonical_bytes
        {
            return Err(SdfError::ByteBudgetExceeded);
        }
        let working_payload_bytes = sample_bytes
            .checked_add(canonical_bytes)
            .ok_or(SdfError::SizeOverflow)?;
        if working_payload_bytes > MAX_WORKING_PAYLOAD_BYTES
            || working_payload_bytes > self.max_working_payload_bytes
        {
            return Err(SdfError::WorkingPayloadBudgetExceeded);
        }
        let minimum_fixed = self.minimum.map(coordinate_to_microunits);
        let maximum_fixed = self.maximum.map(coordinate_to_microunits);
        for axis in 0..3 {
            let min = minimum_fixed[axis]?;
            let max = maximum_fixed[axis]?;
            if max <= min {
                return Err(SdfError::InvalidGrid);
            }
        }
        let count = usize::try_from(sample_count).map_err(|_| SdfError::SizeOverflow)?;
        Ok(GridLimits {
            sample_count,
            sample_bytes,
            canonical_bytes,
            working_payload_bytes,
            count,
        })
    }

    /// First-axis-fastest index, matching Gpytoolbox's SDF grid convention:
    /// i + nx * (j + ny * k).
    pub fn index(self, i: u32, j: u32, k: u32) -> Result<usize, SdfError> {
        self.validate()?;
        if i >= self.dimensions[0] || j >= self.dimensions[1] || k >= self.dimensions[2] {
            return Err(SdfError::GridIndexOutOfBounds);
        }
        let nx = u64::from(self.dimensions[0]);
        let ny = u64::from(self.dimensions[1]);
        let index = u64::from(i)
            .checked_add(
                nx.checked_mul(
                    u64::from(j)
                        .checked_add(ny.checked_mul(u64::from(k)).ok_or(SdfError::SizeOverflow)?)
                        .ok_or(SdfError::SizeOverflow)?,
                )
                .ok_or(SdfError::SizeOverflow)?,
            )
            .ok_or(SdfError::SizeOverflow)?;
        usize::try_from(index).map_err(|_| SdfError::SizeOverflow)
    }

    pub fn point_at_linear_index(self, index: usize) -> Result<[f64; 3], SdfError> {
        let limits = self.validate()?;
        self.point_at_linear_index_validated(index, limits.count)
    }

    fn point_at_linear_index_validated(
        self,
        index: usize,
        sample_count: usize,
    ) -> Result<[f64; 3], SdfError> {
        if index >= sample_count {
            return Err(SdfError::GridIndexOutOfBounds);
        }
        let nx = self.dimensions[0] as usize;
        let ny = self.dimensions[1] as usize;
        let i = index % nx;
        let j = (index / nx) % ny;
        let k = index / (nx * ny);
        Ok([
            grid_coordinate(self.minimum[0], self.maximum[0], i, nx)?,
            grid_coordinate(self.minimum[1], self.maximum[1], j, ny)?,
            grid_coordinate(self.minimum[2], self.maximum[2], k, self.dimensions[2] as usize)?,
        ])
    }

    pub fn sample(self, field: FaceSkinField) -> Result<SampledGrid, SdfError> {
        field.validate()?;
        let limits = self.validate()?;
        let mut samples = Vec::new();
        samples
            .try_reserve_exact(limits.count)
            .map_err(|_| SdfError::AllocationFailed)?;
        for linear_index in 0..limits.count {
            let point = self.point_at_linear_index_validated(linear_index, limits.count)?;
            let value = field.evaluate_validated(point)?;
            samples.push(quantize_distance(value)?);
        }
        Ok(SampledGrid {
            spec: self,
            field,
            quantized_samples: samples,
        })
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridLimits {
    pub sample_count: u64,
    pub sample_bytes: u64,
    pub canonical_bytes: u64,
    pub working_payload_bytes: u64,
    count: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SampledGrid {
    pub spec: GridSpec,
    pub field: FaceSkinField,
    /// Signed distance in microunits; negative is inside and positive outside.
    pub quantized_samples: Vec<i32>,
}

impl SampledGrid {
    pub fn validate(&self) -> Result<(), SdfError> {
        let limits = self.spec.validate()?;
        self.field.validate()?;
        if self.quantized_samples.len() != limits.count {
            return Err(SdfError::SampleCountMismatch);
        }
        Ok(())
    }

    pub fn value_at(&self, i: u32, j: u32, k: u32) -> Result<i32, SdfError> {
        self.validate()?;
        let index = self.spec.index(i, j, k)?;
        self.quantized_samples
            .get(index)
            .copied()
            .ok_or(SdfError::GridIndexOutOfBounds)
    }

    /// Canonical little-endian bytes. Hash this byte string externally (for
    /// example with BLAKE3); this crate deliberately has no hashing dependency.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, SdfError> {
        self.validate()?;
        let limits = self.spec.validate()?;
        let mut out = Vec::new();
        out.try_reserve_exact(limits.canonical_bytes as usize)
            .map_err(|_| SdfError::AllocationFailed)?;
        out.extend_from_slice(&GRID_MAGIC);
        put_u16(&mut out, GRID_SCHEMA_VERSION);
        let fixture = FIXTURE_ID.as_bytes();
        put_u16(
            &mut out,
            u16::try_from(fixture.len()).map_err(|_| SdfError::SizeOverflow)?,
        );
        out.extend_from_slice(fixture);
        let field_descriptor = self.field.canonical_descriptor()?;
        if field_descriptor.len() as u64 != FIELD_DESCRIPTOR_BYTES {
            return Err(SdfError::FieldDescriptorSizeMismatch);
        }
        put_u32(
            &mut out,
            u32::try_from(field_descriptor.len()).map_err(|_| SdfError::SizeOverflow)?,
        );
        out.extend_from_slice(&field_descriptor);
        put_u32(&mut out, self.spec.dimensions[0]);
        put_u32(&mut out, self.spec.dimensions[1]);
        put_u32(&mut out, self.spec.dimensions[2]);
        // Coordinate axes: forward, lateral, up. Layout: first-axis-fastest.
        out.extend_from_slice(&[1, 2, 3, 1]);
        for value in self.spec.minimum.into_iter().chain(self.spec.maximum) {
            put_i64(&mut out, coordinate_to_microunits(value)?);
        }
        put_i64(&mut out, DISTANCE_QUANTIZATION);
        put_u64(&mut out, limits.sample_count);
        for sample in &self.quantized_samples {
            put_i32(&mut out, *sample);
        }
        if out.len() as u64 > self.spec.max_canonical_bytes
            || out.len() as u64 > MAX_CANONICAL_BYTES
        {
            return Err(SdfError::ByteBudgetExceeded);
        }
        Ok(out)
    }

    /// Diagnostic little-endian f32 grid for external mesh extractors.
    ///
    /// Values are reconstructed from the canonical signed-microunit samples;
    /// this is a dequantized interchange view, not a second higher-precision
    /// field evaluation. Pair it with the canonical OUROGRD1 receipt.
    pub fn diagnostic_f32_bytes(&self) -> Result<Vec<u8>, SdfError> {
        self.validate()?;
        let limits = self.spec.validate()?;
        let mut out = Vec::new();
        out.try_reserve_exact(limits.canonical_bytes as usize)
            .map_err(|_| SdfError::AllocationFailed)?;
        out.extend_from_slice(b"OUROF32D");
        put_u16(&mut out, GRID_SCHEMA_VERSION);
        let fixture = FIXTURE_ID.as_bytes();
        put_u16(
            &mut out,
            u16::try_from(fixture.len()).map_err(|_| SdfError::SizeOverflow)?,
        );
        out.extend_from_slice(fixture);
        let field_descriptor = self.field.canonical_descriptor()?;
        if field_descriptor.len() as u64 != FIELD_DESCRIPTOR_BYTES {
            return Err(SdfError::FieldDescriptorSizeMismatch);
        }
        put_u32(
            &mut out,
            u32::try_from(field_descriptor.len()).map_err(|_| SdfError::SizeOverflow)?,
        );
        out.extend_from_slice(&field_descriptor);
        put_u32(&mut out, self.spec.dimensions[0]);
        put_u32(&mut out, self.spec.dimensions[1]);
        put_u32(&mut out, self.spec.dimensions[2]);
        // Coordinate axes: forward, lateral, up. Layout: first-axis-fastest.
        out.extend_from_slice(&[1, 2, 3, 1]);
        for value in self.spec.minimum.into_iter().chain(self.spec.maximum) {
            put_i64(&mut out, coordinate_to_microunits(value)?);
        }
        put_i64(&mut out, DISTANCE_QUANTIZATION);
        put_u64(&mut out, limits.sample_count);
        for sample in &self.quantized_samples {
            let value = *sample as f32 / DISTANCE_QUANTIZATION as f32;
            if !value.is_finite() {
                return Err(SdfError::NonFiniteResult);
            }
            out.extend_from_slice(&value.to_le_bytes());
        }
        if out.len() as u64 > self.spec.max_canonical_bytes
            || out.len() as u64 > MAX_CANONICAL_BYTES
        {
            return Err(SdfError::ByteBudgetExceeded);
        }
        Ok(out)
    }

    pub fn canonical_len(&self) -> Result<u64, SdfError> {
        self.validate()?;
        Ok(self.spec.validate()?.canonical_bytes)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SdfError {
    InvalidFeature,
    InvalidSmoothWidth,
    InvalidGrid,
    NonFiniteInput,
    NonFiniteResult,
    SizeOverflow,
    SampleBudgetExceeded,
    ByteBudgetExceeded,
    WorkingPayloadBudgetExceeded,
    AllocationFailed,
    GridIndexOutOfBounds,
    SampleCountMismatch,
    QuantizationOverflow,
    NonCanonicalParameter,
    FieldDescriptorSizeMismatch,
}

impl fmt::Display for SdfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for SdfError {}

fn canonical_header_len() -> u64 {
    // Magic + schema + fixture length/bytes + descriptor length/bytes +
    // dimensions + axis/layout IDs + six fixed-point bounds + quantization + sample count.
    8 + 2
        + 2
        + FIXTURE_ID.len() as u64
        + 4
        + FIELD_DESCRIPTOR_BYTES
        + 3 * 4
        + 4
        + 6 * 8
        + 8
        + 8
}

fn grid_coordinate(
    minimum: f64,
    maximum: f64,
    index: usize,
    count: usize,
) -> Result<f64, SdfError> {
    let min = coordinate_to_microunits(minimum)? as f64 / COORDINATE_QUANTIZATION;
    let max = coordinate_to_microunits(maximum)? as f64 / COORDINATE_QUANTIZATION;
    Ok(min + (max - min) * index as f64 / (count - 1) as f64)
}

fn is_canonical_microunit(value: f64) -> Result<bool, SdfError> {
    let quantized = coordinate_to_microunits(value)?;
    Ok(quantized as f64 / COORDINATE_QUANTIZATION == value)
}

fn coordinate_to_microunits(value: f64) -> Result<i64, SdfError> {
    if !value.is_finite() {
        return Err(SdfError::NonFiniteInput);
    }
    let scaled = value * COORDINATE_QUANTIZATION;
    if !scaled.is_finite()
        || scaled < i64::MIN as f64
        || scaled >= 9_223_372_036_854_775_808.0
    {
        return Err(SdfError::QuantizationOverflow);
    }
    Ok(scaled.round() as i64)
}

pub fn quantize_distance(value: f64) -> Result<i32, SdfError> {
    if !value.is_finite() {
        return Err(SdfError::NonFiniteResult);
    }
    let scaled = value * DISTANCE_QUANTIZATION as f64;
    if !scaled.is_finite()
        || scaled < i32::MIN as f64
        || scaled > i32::MAX as f64
    {
        return Err(SdfError::QuantizationOverflow);
    }
    Ok(scaled.round() as i32)
}

fn length3(value: [f64; 3]) -> f64 {
    (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt()
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

fn put_i32(out: &mut Vec<u8>, value: i32) {
    out.extend_from_slice(&value.to_le_bytes());
}

fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small_grid() -> GridSpec {
        GridSpec {
            minimum: [-0.18, -0.14, -0.16],
            maximum: [0.25, 0.14, 0.16],
            dimensions: [5, 4, 3],
            max_samples: 128,
            max_canonical_bytes: 4_096,
            max_working_payload_bytes: 8_192,
        }
    }

    #[test]
    fn source_fixture_matches_pr15_feature_contract() {
        let features = source_face_features();
        assert_eq!(features.map(|feature| feature.region_id as u16), [1, 2, 3, 4, 5]);
        assert_eq!(features[0].center, [0.030, 0.0, 0.0]);
        assert_eq!(features[0].radii, [0.120, 0.060, 0.110]);
        assert_eq!(features[1].center, [0.060, -0.050, 0.020]);
        assert_eq!(features[2].center, [0.060, 0.050, 0.020]);
        assert_eq!(features[3].center, [0.160, 0.0, 0.005]);
        assert_eq!(features[3].radii, [0.020, 0.014, 0.018]);
        assert_eq!(features[4].center, [0.140, 0.0, -0.035]);
    }

    #[test]
    fn source_field_descriptor_binds_axis_sign_units_and_estimator_schema() {
        let field = FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
            width: 0.004,
        })
        .unwrap();
        let bytes = field.canonical_descriptor().unwrap();
        assert_eq!(bytes.len() as u64, FIELD_DESCRIPTOR_BYTES);
        assert_eq!(
            &bytes[10..14],
            &[
                AXIS_ORDER_FORWARD_LATERAL_UP,
                SIGN_NEGATIVE_INSIDE_POSITIVE_OUTSIDE,
                UNIT_NORMALIZED_MAJOR_RADIUS,
                ESTIMATOR_IQ_ELLIPSOID_APPROX
            ]
        );
    }

    #[test]
    fn ellipsoid_estimator_has_expected_sign_and_axis_boundary() {
        let head = source_face_features()[0];
        let boundary = [
            head.center[0] + head.radii[0],
            head.center[1],
            head.center[2],
        ];
        assert!(ellipsoid_distance_approx(head.center, head).unwrap() < 0.0);
        assert!(ellipsoid_distance_approx(
            [head.center[0] + head.radii[0] * 2.0, head.center[1], head.center[2]],
            head
        )
        .unwrap()
            > 0.0);
        assert!(ellipsoid_distance_approx(boundary, head).unwrap().abs() < 1.0e-12);
    }

    #[test]
    fn sharp_union_is_the_minimum_of_the_two_fields() {
        let features = source_face_features();
        let point = [0.12, 0.0, 0.01];
        let head = ellipsoid_distance_approx(point, features[0]).unwrap();
        let nose = ellipsoid_distance_approx(point, features[3]).unwrap();
        let field = FaceSkinField::source_fixture(UnionMode::Sharp).unwrap();
        assert_eq!(field.evaluate(point).unwrap(), head.min(nose));
    }

    #[test]
    fn smooth_union_is_bounded_and_finite() {
        let smooth = polynomial_smooth_min(-0.01, 0.02, 0.004).unwrap();
        assert!(smooth <= -0.01);
        assert!(smooth >= -0.011);
        assert!(smooth.is_finite());
    }

    #[test]
    fn field_parameters_must_be_exactly_representable_in_canonical_units() {
        let mut head = source_face_features()[0];
        head.center[0] = 0.0300001;
        assert_eq!(
            FaceSkinField::new(
                head,
                source_face_features()[3],
                UnionMode::Sharp
            ),
            Err(SdfError::NonCanonicalParameter)
        );
        assert_eq!(
            FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
                width: 0.0040001
            }),
            Err(SdfError::NonCanonicalParameter)
        );
    }

    #[test]
    fn invalid_smooth_widths_fail_closed() {
        assert_eq!(
            FaceSkinField::source_fixture(UnionMode::PolynomialSmooth { width: 0.0 }),
            Err(SdfError::InvalidSmoothWidth)
        );
        assert_eq!(
            FaceSkinField::source_fixture(UnionMode::PolynomialSmooth { width: f64::NAN }),
            Err(SdfError::InvalidSmoothWidth)
        );
        assert_eq!(
            FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
                width: MAX_SMOOTH_UNION_WIDTH * 2.0,
            }),
            Err(SdfError::InvalidSmoothWidth)
        );
    }

    #[test]
    fn grid_index_uses_first_axis_fastest_order() {
        let grid = GridSpec {
            dimensions: [2, 3, 4],
            ..small_grid()
        };
        assert_eq!(grid.index(1, 2, 3).unwrap(), 1 + 2 * (2 + 3 * 3));
        assert_eq!(grid.index(0, 0, 0).unwrap(), 0);
        assert_eq!(grid.index(1, 0, 0).unwrap(), 1);
        assert_eq!(grid.index(0, 1, 0).unwrap(), 2);
        assert_eq!(grid.index(0, 0, 1).unwrap(), 6);
    }

    #[test]
    fn quantized_grid_coordinates_include_both_declared_bounds() {
        let grid = GridSpec {
            dimensions: [3, 2, 2],
            ..small_grid()
        };
        let first = grid.point_at_linear_index(0).unwrap();
        let last = grid.point_at_linear_index(11).unwrap();
        assert_eq!(first, [-0.18, -0.14, -0.16]);
        assert_eq!(last, [0.25, 0.14, 0.16]);
    }

    #[test]
    fn sample_sequence_matches_first_axis_fastest_coordinate_order() {
        let grid = GridSpec {
            dimensions: [3, 2, 2],
            ..small_grid()
        };
        let field = FaceSkinField::source_fixture(UnionMode::Sharp).unwrap();
        let samples = grid.sample(field).unwrap();
        for linear in 0..12 {
            let point = grid.point_at_linear_index(linear).unwrap();
            let expected = quantize_distance(field.evaluate(point).unwrap()).unwrap();
            assert_eq!(samples.quantized_samples[linear], expected);
        }
        assert_eq!(grid.index(1, 0, 0).unwrap(), 1);
        assert_eq!(grid.index(0, 1, 0).unwrap(), 3);
        assert_eq!(grid.index(0, 0, 1).unwrap(), 6);
    }

    #[test]
    fn grid_rejects_sample_count_and_byte_budget_violations() {
        let too_many = GridSpec {
            dimensions: [128, 128, 128],
            max_samples: 1_000,
            ..small_grid()
        };
        assert_eq!(too_many.validate(), Err(SdfError::SampleBudgetExceeded));

        let too_large = GridSpec {
            dimensions: [8, 8, 8],
            max_samples: 1_000,
            max_canonical_bytes: 100,
            ..small_grid()
        };
        assert_eq!(too_large.validate(), Err(SdfError::ByteBudgetExceeded));
    }

    #[test]
    fn grid_rejects_bounds_that_collapse_at_canonical_precision() {
        let collapsed = GridSpec {
            minimum: [0.0000001, -0.1, -0.1],
            maximum: [0.0000002, 0.1, 0.1],
            ..small_grid()
        };
        assert_eq!(collapsed.validate(), Err(SdfError::InvalidGrid));
    }

    #[test]
    fn grid_rejects_simultaneous_sample_and_encoded_payload_over_budget() {
        let too_much_payload = GridSpec {
            dimensions: [8, 8, 8],
            max_samples: 1_000,
            max_canonical_bytes: 4_096,
            max_working_payload_bytes: 1_000,
            ..small_grid()
        };
        assert_eq!(
            too_much_payload.validate(),
            Err(SdfError::WorkingPayloadBudgetExceeded)
        );
    }

    #[test]
    fn grid_rejects_zero_dimensions_and_overflow_before_allocation() {
        let zero = GridSpec {
            dimensions: [8, 0, 8],
            ..small_grid()
        };
        assert_eq!(zero.validate(), Err(SdfError::InvalidGrid));
        let overflow = GridSpec {
            dimensions: [u32::MAX, u32::MAX, u32::MAX],
            max_samples: u64::MAX,
            max_canonical_bytes: u64::MAX,
            ..small_grid()
        };
        assert_eq!(overflow.validate(), Err(SdfError::SizeOverflow));
    }

    #[test]
    fn quantization_rejects_nan_infinity_and_out_of_range_values() {
        assert_eq!(quantize_distance(f64::NAN), Err(SdfError::NonFiniteResult));
        assert_eq!(quantize_distance(f64::INFINITY), Err(SdfError::NonFiniteResult));
        assert_eq!(
            quantize_distance(3_000.0),
            Err(SdfError::QuantizationOverflow)
        );
    }

    #[test]
    fn identical_grid_inputs_produce_identical_canonical_bytes() {
        let grid = small_grid();
        let field = FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
            width: 0.004,
        })
        .unwrap();
        let first = grid.sample(field).unwrap().canonical_bytes().unwrap();
        let second = grid.sample(field).unwrap().canonical_bytes().unwrap();
        assert_eq!(first, second);
        assert!(first.starts_with(&GRID_MAGIC));
        let sample = grid.sample(field).unwrap();
        assert_eq!(sample.canonical_len().unwrap(), first.len() as u64);
    }

    #[test]
    fn source_grounded_python_probe_values_match_fixed_point_contract() {
        let field = FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
            width: 0.004,
        })
        .unwrap();
        let probes: [([f64; 3], i32); 10] = [
            ([0.03, 0.0, 0.0], -60_000),
            ([0.16, 0.0, 0.005], -14_000),
            ([0.18, 0.0, 0.005], 0),
            ([0.16, 0.014, 0.005], 0),
            ([0.12, 0.0, 0.0], -30_000),
            ([0.24, 0.0, 0.0], 60_159),
            ([0.03, 0.0, 0.13], 20_000),
            ([-0.12, 0.0, 0.0], 30_000),
            ([0.06, -0.05, 0.02], -6_988),
            ([0.14, 0.0, -0.035], -3_526),
        ];
        for (point, expected_microunits) in probes {
            let actual = quantize_distance(field.evaluate(point).unwrap()).unwrap();
            assert_eq!(
                actual,
                expected_microunits,
                "reference probe mismatch at {point:?}"
            );
        }
    }

    #[test]
    fn canonical_and_diagnostic_headers_bind_axis_and_flattening_order() {
        let sampled = small_grid()
            .sample(FaceSkinField::source_fixture(UnionMode::Sharp).unwrap())
            .unwrap();
        let canonical = sampled.canonical_bytes().unwrap();
        let diagnostic = sampled.diagnostic_f32_bytes().unwrap();
        let fixture_end = 8 + 2 + 2 + FIXTURE_ID.len();
        let descriptor_start = fixture_end + 4;
        let dimensions_start = descriptor_start + FIELD_DESCRIPTOR_BYTES as usize;
        let axis_start = dimensions_start + 12;
        assert_eq!(&canonical[axis_start..axis_start + 4], &[1, 2, 3, 1]);
        assert_eq!(&diagnostic[axis_start..axis_start + 4], &[1, 2, 3, 1]);
    }

    #[test]
    fn diagnostic_f32_view_is_metadata_bound_and_dequantizes_canonical_samples() {
        let grid = small_grid();
        let field = FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
            width: 0.004,
        })
        .unwrap();
        let sampled = grid.sample(field).unwrap();
        let canonical = sampled.canonical_bytes().unwrap();
        let diagnostic = sampled.diagnostic_f32_bytes().unwrap();
        assert!(canonical.starts_with(b"OUROGRD1"));
        assert!(diagnostic.starts_with(b"OUROF32D"));
        assert_eq!(canonical.len(), diagnostic.len());

        let first_sample_offset = canonical_header_len() as usize;
        let first_i32 = i32::from_le_bytes(
            canonical[first_sample_offset..first_sample_offset + 4]
                .try_into()
                .unwrap(),
        );
        let first_f32 = f32::from_le_bytes(
            diagnostic[first_sample_offset..first_sample_offset + 4]
                .try_into()
                .unwrap(),
        );
        assert_eq!(
            first_f32,
            first_i32 as f32 / DISTANCE_QUANTIZATION as f32
        );
    }

    #[test]
    fn canonical_grid_metadata_binds_union_mode_and_bounds() {
        let grid = small_grid();
        let sharp = grid
            .sample(FaceSkinField::source_fixture(UnionMode::Sharp).unwrap())
            .unwrap()
            .canonical_bytes()
            .unwrap();
        let smooth = grid
            .sample(
                FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
                    width: 0.004,
                })
                .unwrap(),
            )
            .unwrap()
            .canonical_bytes()
            .unwrap();
        assert_ne!(sharp, smooth);

        let changed_grid = GridSpec {
            maximum: [0.26, 0.14, 0.16],
            ..grid
        };
        let changed = changed_grid
            .sample(FaceSkinField::source_fixture(UnionMode::Sharp).unwrap())
            .unwrap()
            .canonical_bytes()
            .unwrap();
        assert_ne!(sharp, changed);
    }
}
