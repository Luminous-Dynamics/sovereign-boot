#![forbid(unsafe_code)]
//! Versioned generic scalar-grid interchange and exact analytic SDF fixtures.
//!
//! OUROSGD1 is intentionally separate from the face-specialized OUROGRD1.
//! It binds field semantics, a canonical field descriptor, the isovalue,
//! coordinate/axis/layout conventions, fixed bounds, dimensions and the exact
//! quantized sample stream. Extractors consume samples, never this field API.

use std::{error::Error, fmt};

pub const GRID_MAGIC: [u8; 8] = *b"OUROSGD1";
pub const GRID_SCHEMA_VERSION: u16 = 1;
pub const FIELD_MAGIC: [u8; 8] = *b"OUROFLD1";
pub const FIELD_SCHEMA_VERSION: u16 = 1;
pub const MAX_SAMPLES: u64 = 2_097_152;
pub const MAX_DESCRIPTOR_BYTES: u64 = 1_048_576;
pub const MAX_CANONICAL_BYTES: u64 = 16 * 1024 * 1024;
pub const MAX_WORKING_PAYLOAD_BYTES: u64 = 32 * 1024 * 1024;
pub const DISTANCE_QUANTIZATION: f64 = 1_000_000.0;
pub const COORDINATE_QUANTIZATION: f64 = 1_000_000.0;
pub const Q30_SCALE: f64 = 1_073_741_824.0;
const CANONICAL_HEADER_BYTES: u64 = 91;
const MAX_ABSOLUTE_PARAMETER_MICROUNITS: u64 = 1_000_000_000_000_000;
const UNIT_VECTOR_SQUARED_TOLERANCE: i128 = 1_i128 << 34;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum FieldSemantics {
    ExactAnalyticSdf = 1,
    ApproximateAnalyticField = 2,
    MeshDerivedSdf = 3,
    Unknown = 4,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScalarGridError {
    InvalidField,
    InvalidRotation,
    InvalidGrid,
    SizeOverflow,
    SampleBudgetExceeded,
    DescriptorBudgetExceeded,
    CanonicalByteBudgetExceeded,
    WorkingPayloadBudgetExceeded,
    AllocationFailed,
    NonFiniteInput,
    NonFiniteResult,
    QuantizationOverflow,
    DescriptorLengthMismatch,
    SampleCountMismatch,
    GridIndexOutOfBounds,
    ReceiptLengthMismatch,
}

impl fmt::Display for ScalarGridError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for ScalarGridError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AnalyticField {
    /// Exact Euclidean signed distance to a sphere.
    Sphere {
        center_microunits: [i64; 3],
        radius_microunits: i64,
    },
    /// Exact Euclidean signed distance to an axis-aligned box.
    Box {
        center_microunits: [i64; 3],
        half_extents_microunits: [i64; 3],
    },
    /// Exact Euclidean signed distance to a box under a unit Q30 quaternion.
    /// Quaternion component order is x, y, z, w.
    OrientedBox {
        center_microunits: [i64; 3],
        half_extents_microunits: [i64; 3],
        rotation_q30: [i64; 4],
    },
    /// Exact Euclidean signed distance to a capsule (segment swept by a sphere).
    Capsule {
        endpoint_a_microunits: [i64; 3],
        endpoint_b_microunits: [i64; 3],
        radius_microunits: i64,
    },
    /// Exact Euclidean signed distance to a circular ring torus around the Z axis.
    Torus {
        center_microunits: [i64; 3],
        major_radius_microunits: i64,
        minor_radius_microunits: i64,
    },
    /// Exact Euclidean signed distance to a finite, capped cylinder around the Z axis.
    CappedCylinder {
        center_microunits: [i64; 3],
        radius_microunits: i64,
        half_length_microunits: i64,
    },
    /// Exact signed distance to an infinite plane with a unit Q30 normal:
    /// dot(normal, point) - offset = 0.
    Plane {
        normal_q30: [i64; 3],
        offset_microunits: i64,
    },
}

impl AnalyticField {
    pub fn validate(self) -> Result<(), ScalarGridError> {
        match self {
            Self::Sphere { center_microunits, radius_microunits } => {
                validate_coordinates(center_microunits)?;
                validate_positive_parameter(radius_microunits)?;
            }
            Self::Box { center_microunits, half_extents_microunits } => {
                validate_coordinates(center_microunits)?;
                validate_positive_extents(half_extents_microunits)?;
            }
            Self::OrientedBox {
                center_microunits,
                half_extents_microunits,
                rotation_q30,
            } => {
                validate_coordinates(center_microunits)?;
                validate_positive_extents(half_extents_microunits)?;
                validate_unit_vector4(rotation_q30)?;
            }
            Self::Capsule {
                endpoint_a_microunits,
                endpoint_b_microunits,
                radius_microunits,
            } => {
                validate_coordinates(endpoint_a_microunits)?;
                validate_coordinates(endpoint_b_microunits)?;
                validate_positive_parameter(radius_microunits)?;
            }
            Self::Torus {
                center_microunits,
                major_radius_microunits,
                minor_radius_microunits,
            } => {
                validate_coordinates(center_microunits)?;
                validate_positive_parameter(minor_radius_microunits)?;
                validate_positive_parameter(major_radius_microunits)?;
                if major_radius_microunits <= minor_radius_microunits {
                    return Err(ScalarGridError::InvalidField);
                }
            }
            Self::CappedCylinder {
                center_microunits,
                radius_microunits,
                half_length_microunits,
            } => {
                validate_coordinates(center_microunits)?;
                validate_positive_parameter(radius_microunits)?;
                validate_positive_parameter(half_length_microunits)?;
            }
            Self::Plane { normal_q30, offset_microunits } => {
                validate_unit_vector3(normal_q30)?;
                validate_parameter(offset_microunits)?;
            }
        }
        Ok(())
    }

    pub fn evaluate(self, point: [f64; 3]) -> Result<f64, ScalarGridError> {
        self.validate()?;
        evaluate_validated(self, point)
    }

    pub fn canonical_descriptor(self) -> Result<Vec<u8>, ScalarGridError> {
        self.validate()?;
        let descriptor_len = match self {
            Self::Sphere { .. } | Self::Plane { .. } => 44,
            Self::Box { .. } => 60,
            Self::Torus { .. } | Self::CappedCylinder { .. } => 52,
            Self::Capsule { .. } => 68,
            Self::OrientedBox { .. } => 92,
        };
        let mut out = Vec::new();
        out.try_reserve_exact(descriptor_len)
            .map_err(|_| ScalarGridError::AllocationFailed)?;
        out.extend_from_slice(&FIELD_MAGIC);
        put_u16(&mut out, FIELD_SCHEMA_VERSION);
        out.push(FieldSemantics::ExactAnalyticSdf as u8);
        match self {
            Self::Sphere { center_microunits, radius_microunits } => {
                out.push(1);
                put_i64_array3(&mut out, center_microunits);
                put_i64(&mut out, radius_microunits);
            }
            Self::Box { center_microunits, half_extents_microunits } => {
                out.push(2);
                put_i64_array3(&mut out, center_microunits);
                put_i64_array3(&mut out, half_extents_microunits);
            }
            Self::OrientedBox { center_microunits, half_extents_microunits, rotation_q30 } => {
                out.push(3);
                put_i64_array3(&mut out, center_microunits);
                put_i64_array3(&mut out, half_extents_microunits);
                for value in rotation_q30 {
                    put_i64(&mut out, value);
                }
            }
            Self::Capsule { endpoint_a_microunits, endpoint_b_microunits, radius_microunits } => {
                out.push(4);
                put_i64_array3(&mut out, endpoint_a_microunits);
                put_i64_array3(&mut out, endpoint_b_microunits);
                put_i64(&mut out, radius_microunits);
            }
            Self::Torus { center_microunits, major_radius_microunits, minor_radius_microunits } => {
                out.push(5);
                put_i64_array3(&mut out, center_microunits);
                put_i64(&mut out, major_radius_microunits);
                put_i64(&mut out, minor_radius_microunits);
            }
            Self::CappedCylinder { center_microunits, radius_microunits, half_length_microunits } => {
                out.push(6);
                put_i64_array3(&mut out, center_microunits);
                put_i64(&mut out, radius_microunits);
                put_i64(&mut out, half_length_microunits);
            }
            Self::Plane { normal_q30, offset_microunits } => {
                out.push(7);
                for value in normal_q30 {
                    put_i64(&mut out, value);
                }
                put_i64(&mut out, offset_microunits);
            }
        }
        if out.len() != descriptor_len {
            return Err(ScalarGridError::DescriptorLengthMismatch);
        }
        Ok(out)
    }
}

fn evaluate_validated(field: AnalyticField, point: [f64; 3]) -> Result<f64, ScalarGridError> {
    if !point.into_iter().all(f64::is_finite) {
        return Err(ScalarGridError::NonFiniteInput);
    }
    let value = match field {
        AnalyticField::Sphere { center_microunits, radius_microunits } => {
            let p = sub3(point, to_units(center_microunits));
            length3(p) - to_units1(radius_microunits)
        }
        AnalyticField::Box { center_microunits, half_extents_microunits } => {
            let p = sub3(point, to_units(center_microunits));
            box_distance(p, to_units(half_extents_microunits))
        }
        AnalyticField::OrientedBox { center_microunits, half_extents_microunits, rotation_q30 } => {
            let p = sub3(point, to_units(center_microunits));
            let local = rotate_by_inverse_quaternion(p, rotation_q30);
            box_distance(local, to_units(half_extents_microunits))
        }
        AnalyticField::Capsule { endpoint_a_microunits, endpoint_b_microunits, radius_microunits } => {
            let a = to_units(endpoint_a_microunits);
            let b = to_units(endpoint_b_microunits);
            let ab = sub3(b, a);
            let ap = sub3(point, a);
            let denominator = dot3(ab, ab);
            let t = if denominator <= f64::EPSILON {
                0.0
            } else {
                (dot3(ap, ab) / denominator).clamp(0.0, 1.0)
            };
            length3(sub3(point, add3(a, scale3(ab, t)))) - to_units1(radius_microunits)
        }
        AnalyticField::Torus { center_microunits, major_radius_microunits, minor_radius_microunits } => {
            let p = sub3(point, to_units(center_microunits));
            let radial = p[0].hypot(p[1]) - to_units1(major_radius_microunits);
            radial.hypot(p[2]) - to_units1(minor_radius_microunits)
        }
        AnalyticField::CappedCylinder { center_microunits, radius_microunits, half_length_microunits } => {
            let p = sub3(point, to_units(center_microunits));
            let radial = p[0].hypot(p[1]) - to_units1(radius_microunits);
            let axial = p[2].abs() - to_units1(half_length_microunits);
            radial.max(0.0).hypot(axial.max(0.0)) + radial.max(axial).min(0.0)
        }
        AnalyticField::Plane { normal_q30, offset_microunits } => {
            let normal = normalize3(normal_q30.map(|value| value as f64 / Q30_SCALE));
            dot3(normal, point) - to_units1(offset_microunits)
        }
    };
    if value.is_finite() {
        Ok(value)
    } else {
        Err(ScalarGridError::NonFiniteResult)
    }
}

fn box_distance(point: [f64; 3], half_extents: [f64; 3]) -> f64 {
    let q = [
        point[0].abs() - half_extents[0],
        point[1].abs() - half_extents[1],
        point[2].abs() - half_extents[2],
    ];
    let outside = [q[0].max(0.0), q[1].max(0.0), q[2].max(0.0)];
    length3(outside) + q[0].max(q[1]).max(q[2]).min(0.0)
}

fn rotate_by_inverse_quaternion(point: [f64; 3], quaternion_q30: [i64; 4]) -> [f64; 3] {
    let mut q = quaternion_q30.map(|value| value as f64 / Q30_SCALE);
    let norm = q.iter().map(|value| value * value).sum::<f64>().sqrt();
    for value in &mut q {
        *value /= norm;
    }
    let qv = [-q[0], -q[1], -q[2]];
    let t = scale3(cross3(qv, point), 2.0);
    add3(point, add3(scale3(t, q[3]), cross3(qv, t)))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSpec {
    pub minimum_microunits: [i64; 3],
    pub maximum_microunits: [i64; 3],
    pub dimensions: [u32; 3],
    pub max_samples: u64,
    pub max_descriptor_bytes: u64,
    pub max_canonical_bytes: u64,
    pub max_working_payload_bytes: u64,
}

impl GridSpec {
    pub fn validate(self, descriptor_bytes: usize) -> Result<GridLimits, ScalarGridError> {
        validate_coordinates(self.minimum_microunits)?;
        validate_coordinates(self.maximum_microunits)?;
        if (0..3).any(|axis| self.maximum_microunits[axis] <= self.minimum_microunits[axis])
            || self.dimensions.iter().any(|dimension| *dimension < 2)
            || descriptor_bytes == 0
        {
            return Err(ScalarGridError::InvalidGrid);
        }
        let descriptor_len = u64::try_from(descriptor_bytes).map_err(|_| ScalarGridError::SizeOverflow)?;
        if descriptor_len > MAX_DESCRIPTOR_BYTES || descriptor_len > self.max_descriptor_bytes {
            return Err(ScalarGridError::DescriptorBudgetExceeded);
        }
        let sample_count = u64::from(self.dimensions[0])
            .checked_mul(u64::from(self.dimensions[1]))
            .and_then(|value| value.checked_mul(u64::from(self.dimensions[2])))
            .ok_or(ScalarGridError::SizeOverflow)?;
        if sample_count > MAX_SAMPLES || sample_count > self.max_samples {
            return Err(ScalarGridError::SampleBudgetExceeded);
        }
        let sample_bytes = sample_count.checked_mul(4).ok_or(ScalarGridError::SizeOverflow)?;
        let canonical_bytes = CANONICAL_HEADER_BYTES
            .checked_add(descriptor_len)
            .and_then(|value| value.checked_add(sample_bytes))
            .ok_or(ScalarGridError::SizeOverflow)?;
        if canonical_bytes > MAX_CANONICAL_BYTES || canonical_bytes > self.max_canonical_bytes {
            return Err(ScalarGridError::CanonicalByteBudgetExceeded);
        }
        let working_payload_bytes = sample_bytes
            .checked_add(canonical_bytes)
            .ok_or(ScalarGridError::SizeOverflow)?;
        if working_payload_bytes > MAX_WORKING_PAYLOAD_BYTES
            || working_payload_bytes > self.max_working_payload_bytes
        {
            return Err(ScalarGridError::WorkingPayloadBudgetExceeded);
        }
        let count = usize::try_from(sample_count).map_err(|_| ScalarGridError::SizeOverflow)?;
        Ok(GridLimits { sample_count, sample_bytes, canonical_bytes, working_payload_bytes, count })
    }

    pub fn linear_index(self, i: u32, j: u32, k: u32) -> Result<usize, ScalarGridError> {
        if self.dimensions.iter().any(|dimension| *dimension < 2)
            || i >= self.dimensions[0] || j >= self.dimensions[1] || k >= self.dimensions[2]
        {
            return Err(ScalarGridError::GridIndexOutOfBounds);
        }
        let nx = u64::from(self.dimensions[0]);
        let ny = u64::from(self.dimensions[1]);
        let index = u64::from(i)
            .checked_add(
                nx.checked_mul(
                    u64::from(j)
                        .checked_add(ny.checked_mul(u64::from(k)).ok_or(ScalarGridError::SizeOverflow)?)
                        .ok_or(ScalarGridError::SizeOverflow)?,
                )
                .ok_or(ScalarGridError::SizeOverflow)?,
            )
            .ok_or(ScalarGridError::SizeOverflow)?;
        usize::try_from(index).map_err(|_| ScalarGridError::SizeOverflow)
    }

    pub fn point_at_linear_index(self, index: usize) -> Result<[f64; 3], ScalarGridError> {
        validate_coordinates(self.minimum_microunits)?;
        validate_coordinates(self.maximum_microunits)?;
        if self.dimensions.iter().any(|dimension| *dimension < 2)
            || (0..3).any(|axis| self.maximum_microunits[axis] <= self.minimum_microunits[axis])
        {
            return Err(ScalarGridError::InvalidGrid);
        }
        let count = u64::from(self.dimensions[0])
            .checked_mul(u64::from(self.dimensions[1]))
            .and_then(|value| value.checked_mul(u64::from(self.dimensions[2])))
            .ok_or(ScalarGridError::SizeOverflow)?;
        if u64::try_from(index).map_err(|_| ScalarGridError::SizeOverflow)? >= count {
            return Err(ScalarGridError::GridIndexOutOfBounds);
        }
        let nx = u64::from(self.dimensions[0]);
        let ny = u64::from(self.dimensions[1]);
        let linear = u64::try_from(index).map_err(|_| ScalarGridError::SizeOverflow)?;
        let i = usize::try_from(linear % nx).map_err(|_| ScalarGridError::SizeOverflow)?;
        let j = usize::try_from((linear / nx) % ny).map_err(|_| ScalarGridError::SizeOverflow)?;
        let k = usize::try_from(linear / (nx * ny)).map_err(|_| ScalarGridError::SizeOverflow)?;
        Ok([
            interpolate_coordinate(self.minimum_microunits[0], self.maximum_microunits[0], i, self.dimensions[0] as usize),
            interpolate_coordinate(self.minimum_microunits[1], self.maximum_microunits[1], j, self.dimensions[1] as usize),
            interpolate_coordinate(self.minimum_microunits[2], self.maximum_microunits[2], k, self.dimensions[2] as usize),
        ])
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

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScalarGrid {
    spec: GridSpec,
    semantics: FieldSemantics,
    descriptor_bytes: Vec<u8>,
    isovalue_microunits: i32,
    quantized_samples: Vec<i32>,
}

impl ScalarGrid {
    /// Sample an exact analytic field. Stored values are field minus isovalue.
    pub fn sample_exact(
        spec: GridSpec,
        field: AnalyticField,
        isovalue_microunits: i32,
    ) -> Result<Self, ScalarGridError> {
        field.validate()?;
        let descriptor_bytes = field.canonical_descriptor()?;
        let limits = spec.validate(descriptor_bytes.len())?;
        let mut samples = Vec::new();
        samples.try_reserve_exact(limits.count).map_err(|_| ScalarGridError::AllocationFailed)?;
        for index in 0..limits.count {
            let point = spec.point_at_linear_index(index)?;
            let distance = evaluate_validated(field, point)?
                - isovalue_microunits as f64 / DISTANCE_QUANTIZATION;
            samples.push(quantize_distance(distance)?);
        }
        Ok(Self {
            spec,
            semantics: FieldSemantics::ExactAnalyticSdf,
            descriptor_bytes,
            isovalue_microunits,
            quantized_samples: samples,
        })
    }

    /// Wrap externally produced field-minus-isovalue samples. Descriptor bytes
    /// must identify the source mesh/field, distance backend and preprocessing.
    pub fn from_samples(
        spec: GridSpec,
        semantics: FieldSemantics,
        descriptor_bytes: Vec<u8>,
        isovalue_microunits: i32,
        quantized_samples: Vec<i32>,
    ) -> Result<Self, ScalarGridError> {
        let limits = spec.validate(descriptor_bytes.len())?;
        if quantized_samples.len() != limits.count {
            return Err(ScalarGridError::SampleCountMismatch);
        }
        Ok(Self { spec, semantics, descriptor_bytes, isovalue_microunits, quantized_samples })
    }

    pub fn validate(&self) -> Result<GridLimits, ScalarGridError> {
        let limits = self.spec.validate(self.descriptor_bytes.len())?;
        if self.quantized_samples.len() != limits.count {
            return Err(ScalarGridError::SampleCountMismatch);
        }
        Ok(limits)
    }

    pub fn spec(&self) -> GridSpec { self.spec }
    pub fn semantics(&self) -> FieldSemantics { self.semantics }
    pub fn descriptor_bytes(&self) -> &[u8] { &self.descriptor_bytes }
    pub fn isovalue_microunits(&self) -> i32 { self.isovalue_microunits }
    pub fn quantized_samples(&self) -> &[i32] { &self.quantized_samples }

    pub fn value_at(&self, i: u32, j: u32, k: u32) -> Result<i32, ScalarGridError> {
        let index = self.spec.linear_index(i, j, k)?;
        self.quantized_samples.get(index).copied().ok_or(ScalarGridError::GridIndexOutOfBounds)
    }

    /// Canonical little-endian OUROSGD1 bytes. No hash/signature is implied.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ScalarGridError> {
        let limits = self.validate()?;
        let mut out = Vec::new();
        out.try_reserve_exact(limits.canonical_bytes as usize).map_err(|_| ScalarGridError::AllocationFailed)?;
        out.extend_from_slice(&GRID_MAGIC);
        put_u16(&mut out, GRID_SCHEMA_VERSION);
        out.push(self.semantics as u8);
        out.push(1); // negative-inside / positive-outside
        out.push(1); // forward / lateral / up axis order
        out.push(1); // microunit coordinate and distance units
        out.push(1); // first-axis-fastest sample layout
        put_i32(&mut out, self.isovalue_microunits);
        put_u32(&mut out, u32::try_from(self.descriptor_bytes.len()).map_err(|_| ScalarGridError::SizeOverflow)?);
        out.extend_from_slice(&self.descriptor_bytes);
        for dimension in self.spec.dimensions { put_u32(&mut out, dimension); }
        put_i64_array3(&mut out, self.spec.minimum_microunits);
        put_i64_array3(&mut out, self.spec.maximum_microunits);
        put_u64(&mut out, limits.sample_count);
        for sample in &self.quantized_samples { put_i32(&mut out, *sample); }
        if out.len() as u64 != limits.canonical_bytes {
            return Err(ScalarGridError::ReceiptLengthMismatch);
        }
        Ok(out)
    }
}

fn validate_coordinates(values: [i64; 3]) -> Result<(), ScalarGridError> {
    for value in values { validate_parameter(value)?; }
    Ok(())
}
fn validate_parameter(value: i64) -> Result<(), ScalarGridError> {
    if value.unsigned_abs() > MAX_ABSOLUTE_PARAMETER_MICROUNITS { return Err(ScalarGridError::InvalidField); }
    Ok(())
}
fn validate_positive_parameter(value: i64) -> Result<(), ScalarGridError> {
    validate_parameter(value)?;
    if value <= 0 { return Err(ScalarGridError::InvalidField); }
    Ok(())
}
fn validate_positive_extents(values: [i64; 3]) -> Result<(), ScalarGridError> {
    for value in values { validate_positive_parameter(value)?; }
    Ok(())
}
fn validate_unit_vector3(values: [i64; 3]) -> Result<(), ScalarGridError> {
    let norm_squared = values.into_iter().try_fold(0_i128, |sum, value| {
        let component = i128::from(value);
        let square = component.checked_mul(component).ok_or(ScalarGridError::InvalidRotation)?;
        sum.checked_add(square).ok_or(ScalarGridError::InvalidRotation)
    })?;
    validate_unit_norm(norm_squared)
}
fn validate_unit_vector4(values: [i64; 4]) -> Result<(), ScalarGridError> {
    let norm_squared = values.into_iter().try_fold(0_i128, |sum, value| {
        let component = i128::from(value);
        let square = component.checked_mul(component).ok_or(ScalarGridError::InvalidRotation)?;
        sum.checked_add(square).ok_or(ScalarGridError::InvalidRotation)
    })?;
    validate_unit_norm(norm_squared)
}
fn validate_unit_norm(norm_squared: i128) -> Result<(), ScalarGridError> {
    let target = 1_i128 << 60;
    if (norm_squared - target).abs() > UNIT_VECTOR_SQUARED_TOLERANCE {
        return Err(ScalarGridError::InvalidRotation);
    }
    Ok(())
}
fn interpolate_coordinate(minimum: i64, maximum: i64, index: usize, count: usize) -> f64 {
    let min = minimum as f64 / COORDINATE_QUANTIZATION;
    let max = maximum as f64 / COORDINATE_QUANTIZATION;
    min + (max - min) * index as f64 / (count - 1) as f64
}
fn quantize_distance(value: f64) -> Result<i32, ScalarGridError> {
    if !value.is_finite() { return Err(ScalarGridError::NonFiniteResult); }
    let scaled = value * DISTANCE_QUANTIZATION;
    if !scaled.is_finite() || scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return Err(ScalarGridError::QuantizationOverflow);
    }
    Ok(scaled.round() as i32)
}
fn to_units(values: [i64; 3]) -> [f64; 3] {
    values.map(|value| value as f64 / COORDINATE_QUANTIZATION)
}
fn to_units1(value: i64) -> f64 { value as f64 / COORDINATE_QUANTIZATION }
fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0] + b[0], a[1] + b[1], a[2] + b[2]] }
fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] { [a[0] - b[0], a[1] - b[1], a[2] - b[2]] }
fn scale3(a: [f64; 3], scale: f64) -> [f64; 3] { [a[0] * scale, a[1] * scale, a[2] * scale] }
fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 { a[0] * b[0] + a[1] * b[1] + a[2] * b[2] }
fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn length3(a: [f64; 3]) -> f64 { dot3(a, a).sqrt() }
fn normalize3(a: [f64; 3]) -> [f64; 3] {
    let length = length3(a);
    [a[0] / length, a[1] / length, a[2] / length]
}
fn put_u16(out: &mut Vec<u8>, value: u16) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u32(out: &mut Vec<u8>, value: u32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_u64(out: &mut Vec<u8>, value: u64) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_i32(out: &mut Vec<u8>, value: i32) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_i64(out: &mut Vec<u8>, value: i64) { out.extend_from_slice(&value.to_le_bytes()); }
fn put_i64_array3(out: &mut Vec<u8>, values: [i64; 3]) { for value in values { put_i64(out, value); } }

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(dimensions: [u32; 3]) -> GridSpec {
        GridSpec {
            minimum_microunits: [-2_000_000; 3],
            maximum_microunits: [2_000_000; 3],
            dimensions,
            max_samples: MAX_SAMPLES,
            max_descriptor_bytes: MAX_DESCRIPTOR_BYTES,
            max_canonical_bytes: MAX_CANONICAL_BYTES,
            max_working_payload_bytes: MAX_WORKING_PAYLOAD_BYTES,
        }
    }

    #[test]
    fn sphere_distance_is_exact_at_surface_inside_and_outside() {
        let sphere = AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 1_000_000 };
        assert_eq!(sphere.evaluate([2.0, 0.0, 0.0]).unwrap(), 1.0);
        assert_eq!(sphere.evaluate([1.0, 0.0, 0.0]).unwrap(), 0.0);
        assert_eq!(sphere.evaluate([0.0, 0.0, 0.0]).unwrap(), -1.0);
    }

    #[test]
    fn box_distance_has_exact_face_edge_and_interior_values() {
        let field = AnalyticField::Box { center_microunits: [0; 3], half_extents_microunits: [1_000_000; 3] };
        assert_eq!(field.evaluate([2.0, 0.0, 0.0]).unwrap(), 1.0);
        assert!((field.evaluate([2.0, 2.0, 0.0]).unwrap() - 2.0_f64.sqrt()).abs() < 1e-12);
        assert_eq!(field.evaluate([0.0, 0.0, 0.0]).unwrap(), -1.0);
    }

    #[test]
    fn oriented_box_uses_canonical_unit_quaternion() {
        let half_turn = 759_250_125_i64;
        let field = AnalyticField::OrientedBox {
            center_microunits: [0; 3],
            half_extents_microunits: [1_000_000, 2_000_000, 1_000_000],
            rotation_q30: [0, 0, half_turn, half_turn],
        };
        let value = field.evaluate([0.0, 1.5, 0.0]).unwrap();
        assert!((value - 0.5).abs() < 2e-6);
        let invalid = AnalyticField::OrientedBox {
            center_microunits: [0; 3],
            half_extents_microunits: [1_000_000; 3],
            rotation_q30: [0, 0, 100, 100],
        };
        assert_eq!(invalid.validate(), Err(ScalarGridError::InvalidRotation));
    }

    #[test]
    fn capsule_distance_uses_closest_point_on_segment() {
        let field = AnalyticField::Capsule {
            endpoint_a_microunits: [0, 0, -1_000_000],
            endpoint_b_microunits: [0, 0, 1_000_000],
            radius_microunits: 500_000,
        };
        assert_eq!(field.evaluate([1.0, 0.0, 0.0]).unwrap(), 0.5);
        assert_eq!(field.evaluate([0.0, 0.0, 2.0]).unwrap(), 0.5);
    }

    #[test]
    fn torus_distance_is_exact_for_ring_torus() {
        let field = AnalyticField::Torus {
            center_microunits: [0; 3],
            major_radius_microunits: 2_000_000,
            minor_radius_microunits: 500_000,
        };
        assert_eq!(field.evaluate([2.5, 0.0, 0.0]).unwrap(), 0.0);
        assert_eq!(field.evaluate([0.0, 0.0, 0.0]).unwrap(), 1.5);
    }

    #[test]
    fn capped_cylinder_distance_is_exact_for_side_and_cap() {
        let field = AnalyticField::CappedCylinder {
            center_microunits: [0; 3],
            radius_microunits: 1_000_000,
            half_length_microunits: 1_000_000,
        };
        assert_eq!(field.evaluate([2.0, 0.0, 0.0]).unwrap(), 1.0);
        assert_eq!(field.evaluate([0.0, 0.0, 2.0]).unwrap(), 1.0);
        assert_eq!(field.evaluate([0.0, 0.0, 0.0]).unwrap(), -1.0);
    }

    #[test]
    fn plane_distance_requires_unit_normal_and_is_oriented() {
        let field = AnalyticField::Plane {
            normal_q30: [0, 0, 1_i64 << 30],
            offset_microunits: 500_000,
        };
        assert_eq!(field.evaluate([0.0, 0.0, 2.0]).unwrap(), 1.5);
        let invalid = AnalyticField::Plane { normal_q30: [0, 0, 0], offset_microunits: 0 };
        assert_eq!(invalid.validate(), Err(ScalarGridError::InvalidRotation));
    }

    #[test]
    fn every_analytic_descriptor_has_its_declared_canonical_length() {
        let fields = [
            AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 1_000_000 },
            AnalyticField::Box { center_microunits: [0; 3], half_extents_microunits: [1_000_000; 3] },
            AnalyticField::OrientedBox { center_microunits: [0; 3], half_extents_microunits: [1_000_000; 3], rotation_q30: [0, 0, 0, 1_i64 << 30] },
            AnalyticField::Capsule { endpoint_a_microunits: [0; 3], endpoint_b_microunits: [0, 0, 1_000_000], radius_microunits: 500_000 },
            AnalyticField::Torus { center_microunits: [0; 3], major_radius_microunits: 2_000_000, minor_radius_microunits: 500_000 },
            AnalyticField::CappedCylinder { center_microunits: [0; 3], radius_microunits: 1_000_000, half_length_microunits: 1_000_000 },
            AnalyticField::Plane { normal_q30: [0, 0, 1_i64 << 30], offset_microunits: 0 },
        ];
        let expected = [44, 60, 92, 68, 52, 52, 44];
        for (field, expected_len) in fields.into_iter().zip(expected) {
            assert_eq!(field.canonical_descriptor().unwrap().len(), expected_len);
        }
    }

    #[test]
    fn unit_quaternion_and_plane_normal_reject_integer_overflow_without_panicking() {
        let invalid_rotation = AnalyticField::OrientedBox {
            center_microunits: [0; 3],
            half_extents_microunits: [1_000_000; 3],
            rotation_q30: [i64::MAX, i64::MAX, i64::MAX, i64::MAX],
        };
        assert_eq!(invalid_rotation.validate(), Err(ScalarGridError::InvalidRotation));
        let invalid_plane = AnalyticField::Plane {
            normal_q30: [i64::MAX, i64::MAX, i64::MAX],
            offset_microunits: 0,
        };
        assert_eq!(invalid_plane.validate(), Err(ScalarGridError::InvalidRotation));
    }

    #[test]
    fn nonzero_isovalue_is_subtracted_from_every_sample_and_bound() {
        let grid = ScalarGrid::sample_exact(
            spec([3, 3, 3]),
            AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 1_000_000 },
            250_000,
        ).unwrap();
        assert_eq!(grid.isovalue_microunits(), 250_000);
        assert_eq!(grid.value_at(1, 1, 1).unwrap(), -1_250_000);
        let bytes = grid.canonical_bytes().unwrap();
        assert!(bytes.windows(4).any(|window| window == 250_000_i32.to_le_bytes()));
    }

    #[test]
    fn sampled_sphere_grid_binds_exact_semantics_and_negative_inside_values() {
        let grid = ScalarGrid::sample_exact(
            spec([3, 3, 3]),
            AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 1_000_000 },
            0,
        ).unwrap();
        assert_eq!(grid.semantics(), FieldSemantics::ExactAnalyticSdf);
        assert_eq!(grid.value_at(1, 1, 1).unwrap(), -1_000_000);
        assert_eq!(grid.value_at(2, 1, 1).unwrap(), 1_000_000);
    }

    #[test]
    fn canonical_grid_is_deterministic_and_binds_descriptor_and_samples() {
        let field = AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 1_000_000 };
        let a = ScalarGrid::sample_exact(spec([5, 4, 3]), field, 0).unwrap();
        let b = ScalarGrid::sample_exact(spec([5, 4, 3]), field, 0).unwrap();
        assert_eq!(a.canonical_bytes().unwrap(), b.canonical_bytes().unwrap());
        assert!(a.canonical_bytes().unwrap().starts_with(&GRID_MAGIC));
        let c = ScalarGrid::sample_exact(
            spec([5, 4, 3]),
            AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 900_000 },
            0,
        ).unwrap();
        assert_ne!(a.canonical_bytes().unwrap(), c.canonical_bytes().unwrap());
    }

    #[test]
    fn first_axis_is_fastest_in_sample_order() {
        let field = AnalyticField::Plane {
            normal_q30: [1_i64 << 30, 0, 0],
            offset_microunits: 0,
        };
        let grid = ScalarGrid::sample_exact(spec([3, 2, 2]), field, 0).unwrap();
        assert_eq!(
            grid.quantized_samples(),
            &[-2_000_000, 0, 2_000_000, -2_000_000, 0, 2_000_000,
              -2_000_000, 0, 2_000_000, -2_000_000, 0, 2_000_000]
        );
    }

    #[test]
    fn external_mesh_derived_samples_preserve_semantics_and_descriptor() {
        let grid = ScalarGrid::from_samples(
            spec([2, 2, 2]),
            FieldSemantics::MeshDerivedSdf,
            b"mesh-sha256:0123456789abcdef;distance-backend:v1".to_vec(),
            0,
            vec![-1, 1, 1, 1, 1, 1, 1, 1],
        ).unwrap();
        assert_eq!(grid.semantics(), FieldSemantics::MeshDerivedSdf);
        assert!(grid.canonical_bytes().unwrap().windows(8).any(|window| window == b"mesh-sha"));
    }

    #[test]
    fn descriptor_and_sample_budgets_reject_before_grid_construction() {
        let mut small = spec([8, 8, 8]);
        small.max_samples = 16;
        assert_eq!(small.validate(16), Err(ScalarGridError::SampleBudgetExceeded));
        let mut small = spec([2, 2, 2]);
        small.max_descriptor_bytes = 2;
        assert_eq!(small.validate(3), Err(ScalarGridError::DescriptorBudgetExceeded));
    }

    #[test]
    fn out_of_range_distance_quantization_fails_closed() {
        let result = ScalarGrid::sample_exact(
            spec([2, 2, 2]),
            AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 3_000_000_000 },
            0,
        );
        assert_eq!(result, Err(ScalarGridError::QuantizationOverflow));
    }

    #[test]
    fn non_exact_semantic_labels_are_preserved() {
        let grid = ScalarGrid::from_samples(
            spec([2, 2, 2]),
            FieldSemantics::ApproximateAnalyticField,
            b"approx-field-descriptor-v1".to_vec(),
            0,
            vec![-1, 1, 1, 1, 1, 1, 1, 1],
        ).unwrap();
        assert_eq!(grid.semantics(), FieldSemantics::ApproximateAnalyticField);
    }

    #[test]
    fn generic_grid_receipt_has_exact_header_and_payload_length() {
        let grid = ScalarGrid::sample_exact(
            spec([3, 3, 3]),
            AnalyticField::Sphere { center_microunits: [0; 3], radius_microunits: 1_000_000 },
            0,
        ).unwrap();
        let bytes = grid.canonical_bytes().unwrap();
        let limits = grid.validate().unwrap();
        assert_eq!(bytes.len() as u64, limits.canonical_bytes);
        assert_eq!(bytes.len() as u64, CANONICAL_HEADER_BYTES + grid.descriptor_bytes().len() as u64 + 27 * 4);
    }
}
