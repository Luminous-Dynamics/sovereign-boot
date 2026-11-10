#![forbid(unsafe_code)]
//! Bounded fixed-grid classical dual contouring over canonical OUROGRD1 samples.
//!
//! This is a deterministic CPU research baseline, not the SIGGRAPH 2026
//! discrete-SDF method, not adaptive/octree dual contouring, and not a robust
//! geometric self-intersection solver. The generated triangle view is passed
//! through the independent mesh-audit crate; self-intersections remain
//! NotEvaluated.

use std::{error::Error, fmt};

use ouroboros_mesh_audit::{
    AuditError, AuditLimits, CheckStatus, MeshAuditReport, SemanticTriangleMesh, audit_mesh,
};
use ouroboros_sdf_lab::{SampledGrid, SdfError};

pub const EXTRACTOR_SCHEMA_VERSION: u16 = 1;
pub const EXTRACTOR_MAGIC: [u8; 8] = *b"OURODCQ1";
pub const MAX_ACTIVE_CELLS: u64 = 2_097_152;
pub const MAX_CANONICAL_RECEIPT_BYTES: u64 = 24 * 1024 * 1024;
pub const DEFAULT_MAX_GEOMETRY_BYTES: u64 = 2_500_000;
pub const DEFAULT_MAX_WORKING_BYTES: u64 = 64 * 1024 * 1024;
pub const SKIN_FEATURE_MASK: u32 = (1 << 0) | (1 << 3);
pub const SKIN_FACE_REGION_ID: u32 = 1;

const NO_VERTEX: u32 = u32::MAX;
const POSITION_QUANTIZATION: f64 = 1_000_000_000.0;
const NORMAL_QUANTIZATION: f64 = 1_000_000_000.0;
const CELL_CORNER_OFFSETS: [[u32; 3]; 8] = [
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0],
    [1, 1, 0],
    [0, 0, 1],
    [1, 0, 1],
    [0, 1, 1],
    [1, 1, 1],
];
const CELL_EDGES: [[usize; 2]; 12] = [
    [0, 1],
    [2, 3],
    [4, 5],
    [6, 7],
    [0, 2],
    [1, 3],
    [4, 6],
    [5, 7],
    [0, 4],
    [1, 5],
    [2, 6],
    [3, 7],
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualContourConfig {
    pub max_vertices: u32,
    pub max_quads: u32,
    pub max_geometry_bytes: u64,
    /// Includes the cell-to-vertex map, canonical input grid bytes and output
    /// geometry payload. Excludes allocator metadata and process RSS.
    pub max_working_bytes: u64,
    /// QEF regularizer in billionths of normalized major-radius units.
    /// Zero is allowed for testing singular-system centroid fallback.
    pub qef_regularization_nanounits: u32,
    /// If the finite-difference gradient is shorter than this many billionths,
    /// the crossed grid edge's inside-to-outside direction is used as fallback.
    pub gradient_epsilon_nanounits: u32,
}

impl Default for DualContourConfig {
    fn default() -> Self {
        Self {
            max_vertices: 50_000,
            max_quads: 62_500,
            max_geometry_bytes: DEFAULT_MAX_GEOMETRY_BYTES,
            max_working_bytes: DEFAULT_MAX_WORKING_BYTES,
            qef_regularization_nanounits: 100,
            gradient_epsilon_nanounits: 1,
        }
    }
}

impl DualContourConfig {
    pub fn validate(self) -> Result<(), ExtractError> {
        if self.max_vertices == 0
            || self.max_vertices > 50_000
            || self.max_quads == 0
            || self.max_quads > 62_500
            || self.max_geometry_bytes == 0
            || self.max_geometry_bytes > DEFAULT_MAX_GEOMETRY_BYTES
            || self.max_working_bytes == 0
            || self.max_working_bytes > DEFAULT_MAX_WORKING_BYTES
        {
            return Err(ExtractError::InvalidConfig);
        }
        Ok(())
    }

    fn canonical_bytes(self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 * 2 + 8 * 2 + 4 * 2 + 4);
        put_u32(&mut out, self.max_vertices);
        put_u32(&mut out, self.max_quads);
        put_u64(&mut out, self.max_geometry_bytes);
        put_u64(&mut out, self.max_working_bytes);
        put_u32(&mut out, self.qef_regularization_nanounits);
        put_u32(&mut out, self.gradient_epsilon_nanounits);
        // Policy IDs: linear edge interpolation=1, central/one-sided finite
        // differences=1, centroid fallback=1, cell clamping=1, diagonal 0-2=1.
        out.extend_from_slice(&[1, 1, 1, 1, 1]);
        out
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualContourVertex {
    /// Normalized major-radius coordinates, quantized to 1e-9.
    pub position_nanounits: [i64; 3],
    /// Outward normal, quantized to 1e-9.
    pub normal_nanounits: [i32; 3],
    /// Coarse composite provenance: head shell (feature 1) + nose (feature 4).
    /// Local per-vertex attribution between those contributors is not yet
    /// reconstructed by this initial skin-union extractor.
    pub feature_mask: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DualContourMetrics {
    pub grid_dimensions: [u32; 3],
    pub active_cell_count: u32,
    pub sign_changing_edge_count: u32,
    pub quad_count: u32,
    pub qef_centroid_fallback_count: u32,
    pub qef_clamped_vertex_count: u32,
    pub qef_residual_p50_nanounits: u64,
    pub qef_residual_p95_nanounits: u64,
    pub qef_residual_max_nanounits: u64,
    pub geometry_bytes: u64,
    pub working_payload_estimate_bytes: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DualContourMesh {
    source_grid_canonical_bytes: Vec<u8>,
    config: DualContourConfig,
    vertices: Vec<DualContourVertex>,
    quads: Vec<[u32; 4]>,
    triangles: Vec<[u32; 3]>,
    metrics: DualContourMetrics,
    audit_report: MeshAuditReport,
}

impl DualContourMesh {
    pub fn vertices(&self) -> &[DualContourVertex] {
        &self.vertices
    }

    pub fn quads(&self) -> &[[u32; 4]] {
        &self.quads
    }

    pub fn triangles(&self) -> &[[u32; 3]] {
        &self.triangles
    }

    pub fn metrics(&self) -> DualContourMetrics {
        self.metrics
    }

    pub fn audit_report(&self) -> &MeshAuditReport {
        &self.audit_report
    }

    pub fn source_grid_canonical_bytes(&self) -> &[u8] {
        &self.source_grid_canonical_bytes
    }

    /// Canonical artifact binds the exact OUROGRD1 input bytes, extractor
    /// parameters, fixed-point geometry, quad/triangle topology, metrics and
    /// the independently recomputable mesh-audit receipt. Hash/sign externally.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, ExtractError> {
        let semantic_mesh = self.to_semantic_triangle_mesh()?;
        let recomputed = audit_mesh(&semantic_mesh, AuditLimits::default())?;
        if recomputed != self.audit_report {
            return Err(ExtractError::AuditReportMismatch);
        }
        let audit_bytes = self.audit_report.canonical_bytes(&semantic_mesh)?;
        let source_len = u64::try_from(self.source_grid_canonical_bytes.len())
            .map_err(|_| ExtractError::SizeOverflow)?;
        let vertex_bytes = (self.vertices.len() as u64)
            .checked_mul(40)
            .ok_or(ExtractError::SizeOverflow)?;
        let quad_bytes = (self.quads.len() as u64)
            .checked_mul(16)
            .ok_or(ExtractError::SizeOverflow)?;
        let triangle_bytes = (self.triangles.len() as u64)
            .checked_mul(12)
            .ok_or(ExtractError::SizeOverflow)?;
        let total = 8_u64
            .checked_add(2 + 4)
            .and_then(|v| v.checked_add(source_len))
            .and_then(|v| v.checked_add(self.config.canonical_bytes().len() as u64))
            .and_then(|v| v.checked_add(vertex_bytes))
            .and_then(|v| v.checked_add(quad_bytes))
            .and_then(|v| v.checked_add(triangle_bytes))
            .and_then(|v| v.checked_add(4 * 4 + 5 * 8 + 4))
            .and_then(|v| v.checked_add(audit_bytes.len() as u64))
            .ok_or(ExtractError::SizeOverflow)?;
        if total > MAX_CANONICAL_RECEIPT_BYTES {
            return Err(ExtractError::ReceiptBudgetExceeded);
        }
        let mut out = Vec::new();
        out.try_reserve_exact(total as usize)
            .map_err(|_| ExtractError::AllocationFailed)?;
        out.extend_from_slice(&EXTRACTOR_MAGIC);
        put_u16(&mut out, EXTRACTOR_SCHEMA_VERSION);
        put_u32(
            &mut out,
            u32::try_from(self.source_grid_canonical_bytes.len())
                .map_err(|_| ExtractError::SizeOverflow)?,
        );
        out.extend_from_slice(&self.source_grid_canonical_bytes);
        out.extend_from_slice(&self.config.canonical_bytes());
        put_u32(&mut out, self.vertices.len() as u32);
        for vertex in &self.vertices {
            for value in vertex.position_nanounits {
                put_i64(&mut out, value);
            }
            for value in vertex.normal_nanounits {
                put_i32(&mut out, value);
            }
            put_u32(&mut out, vertex.feature_mask);
        }
        put_u32(&mut out, self.quads.len() as u32);
        for quad in &self.quads {
            for index in quad {
                put_u32(&mut out, *index);
            }
        }
        put_u32(&mut out, self.triangles.len() as u32);
        for triangle in &self.triangles {
            for index in triangle {
                put_u32(&mut out, *index);
            }
        }
        put_u32(&mut out, self.metrics.active_cell_count);
        put_u32(&mut out, self.metrics.sign_changing_edge_count);
        put_u32(&mut out, self.metrics.qef_centroid_fallback_count);
        put_u32(&mut out, self.metrics.qef_clamped_vertex_count);
        put_u64(&mut out, self.metrics.qef_residual_p50_nanounits);
        put_u64(&mut out, self.metrics.qef_residual_p95_nanounits);
        put_u64(&mut out, self.metrics.qef_residual_max_nanounits);
        put_u64(&mut out, self.metrics.geometry_bytes);
        put_u64(&mut out, self.metrics.working_payload_estimate_bytes);
        put_u32(
            &mut out,
            u32::try_from(audit_bytes.len()).map_err(|_| ExtractError::SizeOverflow)?,
        );
        out.extend_from_slice(&audit_bytes);
        if out.len() as u64 != total {
            return Err(ExtractError::ReceiptLengthMismatch);
        }
        Ok(out)
    }

    pub fn to_semantic_triangle_mesh(&self) -> Result<SemanticTriangleMesh, ExtractError> {
        let mut vertices = Vec::new();
        vertices
            .try_reserve_exact(self.vertices.len())
            .map_err(|_| ExtractError::AllocationFailed)?;
        for vertex in &self.vertices {
            vertices.push(vertex.position_nanounits.map(|value| value as f64 / POSITION_QUANTIZATION));
        }
        let vertex_feature_masks = self.vertices.iter().map(|vertex| vertex.feature_mask).collect();
        semantic_triangle_mesh(&self.vertices, &self.triangles)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct HermiteSample {
    point: [f64; 3],
    normal: [f64; 3],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtractError {
    InvalidConfig,
    GridTooSmall,
    SizeOverflow,
    WorkingBudgetExceeded,
    GeometryBudgetExceeded,
    ReceiptBudgetExceeded,
    AllocationFailed,
    ExactZeroCrossing,
    SurfaceTouchesGridBoundary,
    MissingAdjacentCellVertex,
    DegenerateQuad,
    SingularQefUnresolved,
    NonFiniteFieldGradient,
    NoSurfaceCrossings,
    QuantizationOverflow,
    AuditReportMismatch,
    ReceiptLengthMismatch,
    Sdf(SdfError),
    Audit(AuditError),
}

impl fmt::Display for ExtractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for ExtractError {}

impl From<SdfError> for ExtractError {
    fn from(value: SdfError) -> Self {
        Self::Sdf(value)
    }
}

impl From<AuditError> for ExtractError {
    fn from(value: AuditError) -> Self {
        Self::Audit(value)
    }
}

pub fn extract(
    grid: &SampledGrid,
    config: DualContourConfig,
) -> Result<DualContourMesh, ExtractError> {
    config.validate()?;
    grid.validate()?;
    let spec = grid.spec;
    let grid_limits = spec.validate()?;
    let dims = spec.dimensions;
    if dims.iter().any(|dimension| *dimension < 3) {
        return Err(ExtractError::GridTooSmall);
    }
    let cell_dims = [dims[0] - 1, dims[1] - 1, dims[2] - 1];
    let cell_count = u64::from(cell_dims[0])
        .checked_mul(u64::from(cell_dims[1]))
        .and_then(|value| value.checked_mul(u64::from(cell_dims[2])))
        .ok_or(ExtractError::SizeOverflow)?;
    if cell_count > MAX_ACTIVE_CELLS {
        return Err(ExtractError::WorkingBudgetExceeded);
    }

    // Preflight a conservative payload estimate before allocating the cell map.
    let cell_map_bytes = cell_count
        .checked_mul(std::mem::size_of::<u32>() as u64)
        .ok_or(ExtractError::SizeOverflow)?;
    let max_vertex_bytes = u64::from(config.max_vertices)
        .checked_mul(40)
        .ok_or(ExtractError::SizeOverflow)?;
    let max_topology_bytes = u64::from(config.max_quads)
        .checked_mul(40)
        .ok_or(ExtractError::SizeOverflow)?;
    let residual_bytes = u64::from(config.max_vertices)
        .checked_mul(8)
        .ok_or(ExtractError::SizeOverflow)?;
    let audit_workspace = u64::from(config.max_quads)
        .checked_mul(2 * 3 * 144)
        .and_then(|value| {
            value.checked_add(u64::from(config.max_vertices).checked_mul(64)?)
        })
        .ok_or(ExtractError::SizeOverflow)?;
    let working_payload = grid_limits
        .sample_bytes
        .checked_add(grid_limits.canonical_bytes)
        .and_then(|value| value.checked_add(cell_map_bytes))
        .and_then(|value| value.checked_add(max_vertex_bytes))
        .and_then(|value| value.checked_add(max_topology_bytes))
        .and_then(|value| value.checked_add(residual_bytes))
        .and_then(|value| value.checked_add(audit_workspace))
        .ok_or(ExtractError::SizeOverflow)?;
    if working_payload > config.max_working_bytes {
        return Err(ExtractError::WorkingBudgetExceeded);
    }
    let source_grid_canonical_bytes = grid.canonical_bytes()?;

    let cell_count_usize = usize::try_from(cell_count).map_err(|_| ExtractError::SizeOverflow)?;
    let mut cell_vertex_ids = Vec::new();
    cell_vertex_ids
        .try_reserve_exact(cell_count_usize)
        .map_err(|_| ExtractError::AllocationFailed)?;
    cell_vertex_ids.resize(cell_count_usize, NO_VERTEX);

    let mut vertices: Vec<DualContourVertex> = Vec::new();
    vertices
        .try_reserve_exact(config.max_vertices as usize)
        .map_err(|_| ExtractError::AllocationFailed)?;
    let mut residuals = Vec::new();
    residuals
        .try_reserve_exact(config.max_vertices as usize)
        .map_err(|_| ExtractError::AllocationFailed)?;
    let mut qef_centroid_fallback_count = 0_u32;
    let mut qef_clamped_vertex_count = 0_u32;
    let mut active_cell_count = 0_u32;

    let min = spec.minimum.map(canonical_coordinate);
    let max = spec.maximum.map(canonical_coordinate);
    let spacing = [
        (max[0] - min[0]) / f64::from(dims[0] - 1),
        (max[1] - min[1]) / f64::from(dims[1] - 1),
        (max[2] - min[2]) / f64::from(dims[2] - 1),
    ];
    if spacing.iter().any(|value| !value.is_finite() || *value <= 0.0) {
        return Err(ExtractError::GridTooSmall);
    }

    for z in 0..cell_dims[2] {
        for y in 0..cell_dims[1] {
            for x in 0..cell_dims[0] {
                let mut corner_values = [0_i32; 8];
                let mut corner_points = [[0.0_f64; 3]; 8];
                let mut any_inside = false;
                let mut any_outside = false;
                for corner in 0..8 {
                    let offset = CELL_CORNER_OFFSETS[corner];
                    let gx = x + offset[0];
                    let gy = y + offset[1];
                    let gz = z + offset[2];
                    let sample_index = linear_index(dims, gx, gy, gz)?;
                    let value = *grid
                        .quantized_samples
                        .get(sample_index)
                        .ok_or(ExtractError::SizeOverflow)?;
                    corner_values[corner] = value;
                    any_inside |= value < 0;
                    any_outside |= value >= 0;
                    corner_points[corner] = [
                        grid_coordinate(min[0], max[0], gx, dims[0]),
                        grid_coordinate(min[1], max[1], gy, dims[1]),
                        grid_coordinate(min[2], max[2], gz, dims[2]),
                    ];
                }
                if !any_inside || !any_outside {
                    continue;
                }

                let mut hermite = [HermiteSample {
                    point: [0.0; 3],
                    normal: [0.0; 3],
                }; 12];
                let mut hermite_count = 0_usize;
                for edge in CELL_EDGES {
                    let a = edge[0];
                    let b = edge[1];
                    let va = corner_values[a];
                    let vb = corner_values[b];
                    if (va < 0) == (vb < 0) {
                        continue;
                    }
                    if va == 0 || vb == 0 {
                        return Err(ExtractError::ExactZeroCrossing);
                    }
                    let denominator = f64::from(va) - f64::from(vb);
                    if denominator == 0.0 || !denominator.is_finite() {
                        return Err(ExtractError::ExactZeroCrossing);
                    }
                    let t = (f64::from(va) / denominator).clamp(0.0, 1.0);
                    let point = lerp3(corner_points[a], corner_points[b], t);
                    let grid_a = [
                        x + CELL_CORNER_OFFSETS[a][0],
                        y + CELL_CORNER_OFFSETS[a][1],
                        z + CELL_CORNER_OFFSETS[a][2],
                    ];
                    let grid_b = [
                        x + CELL_CORNER_OFFSETS[b][0],
                        y + CELL_CORNER_OFFSETS[b][1],
                        z + CELL_CORNER_OFFSETS[b][2],
                    ];
                    let gradient_a = gradient_at_grid_vertex(
                        grid,
                        grid_a,
                        dims,
                        spacing,
                    )?;
                    let gradient_b = gradient_at_grid_vertex(
                        grid,
                        grid_b,
                        dims,
                        spacing,
                    )?;
                    let interpolated = lerp3(gradient_a, gradient_b, t);
                    let normal = if length3(interpolated)
                        <= config.gradient_epsilon_nanounits as f64 / NORMAL_QUANTIZATION
                    {
                        axis_fallback_normal(grid_a, grid_b, va < 0)
                    } else {
                        normalize3(interpolated).ok_or(ExtractError::NonFiniteFieldGradient)?
                    };
                    hermite[hermite_count] = HermiteSample { point, normal };
                    hermite_count += 1;
                }
                if hermite_count == 0 {
                    return Err(ExtractError::ExactZeroCrossing);
                }

                let centroid = hermite_centroid(&hermite[..hermite_count]);
                let cell_min = [
                    corner_points[0][0],
                    corner_points[0][1],
                    corner_points[0][2],
                ];
                let cell_max = [
                    corner_points[7][0],
                    corner_points[7][1],
                    corner_points[7][2],
                ];
                let (mut position, fallback) = solve_qef(
                    &hermite[..hermite_count],
                    centroid,
                    config.qef_regularization_nanounits as f64 / NORMAL_QUANTIZATION,
                );
                if fallback {
                    qef_centroid_fallback_count += 1;
                }
                let clamped = clamp3(position, cell_min, cell_max);
                if clamped != position {
                    qef_clamped_vertex_count += 1;
                    position = clamped;
                }
                let normal_sum = hermite[..hermite_count]
                    .iter()
                    .fold([0.0; 3], |acc, item| add3(acc, item.normal));
                let normal = normalize3(normal_sum).unwrap_or_else(|| hermite[0].normal);
                let residual = qef_rms_residual(&hermite[..hermite_count], position);
                residuals.push(residual);

                if vertices.len() >= config.max_vertices as usize {
                    return Err(ExtractError::GeometryBudgetExceeded);
                }
                let vertex_id =
                    u32::try_from(vertices.len()).map_err(|_| ExtractError::SizeOverflow)?;
                vertices.push(DualContourVertex {
                    position_nanounits: position.map(quantize_position)?,
                    normal_nanounits: normal.map(quantize_normal)?,
                    feature_mask: SKIN_FEATURE_MASK,
                });
                let cell_index = cell_linear_index(cell_dims, x, y, z)?;
                cell_vertex_ids[cell_index] = vertex_id;
                active_cell_count += 1;
            }
        }
    }

    if vertices.is_empty() {
        return Err(ExtractError::NoSurfaceCrossings);
    }

    let mut quads: Vec<[u32; 4]> = Vec::new();
    quads
        .try_reserve_exact(config.max_quads as usize)
        .map_err(|_| ExtractError::AllocationFailed)?;
    let mut sign_changing_edge_count = 0_u32;

    // X-oriented grid edges; the ring order has +X normal before sign reversal.
    for z in 0..dims[2] {
        for y in 0..dims[1] {
            for x in 0..(dims[0] - 1) {
                let a = sample_value(grid, dims, x, y, z)?;
                let b = sample_value(grid, dims, x + 1, y, z)?;
                if is_crossing(a, b)? {
                    if y == 0 || y + 1 == dims[1] || z == 0 || z + 1 == dims[2] {
                        return Err(ExtractError::SurfaceTouchesGridBoundary);
                    }
                    let mut quad = [
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y - 1, z - 1)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y, z - 1)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y, z)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y - 1, z)?,
                    ];
                    orient_quad(&mut quad, a < 0)?;
                    push_quad(&mut quads, quad, config.max_quads)?;
                    sign_changing_edge_count += 1;
                }
            }
        }
    }

    // Y-oriented grid edges; this ring order has +Y normal before reversal.
    for z in 0..dims[2] {
        for y in 0..(dims[1] - 1) {
            for x in 0..dims[0] {
                let a = sample_value(grid, dims, x, y, z)?;
                let b = sample_value(grid, dims, x, y + 1, z)?;
                if is_crossing(a, b)? {
                    if x == 0 || x + 1 == dims[0] || z == 0 || z + 1 == dims[2] {
                        return Err(ExtractError::SurfaceTouchesGridBoundary);
                    }
                    let mut quad = [
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x - 1, y, z - 1)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x - 1, y, z)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y, z)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y, z - 1)?,
                    ];
                    orient_quad(&mut quad, a < 0)?;
                    push_quad(&mut quads, quad, config.max_quads)?;
                    sign_changing_edge_count += 1;
                }
            }
        }
    }

    // Z-oriented grid edges; this ring order has +Z normal before reversal.
    for z in 0..(dims[2] - 1) {
        for y in 0..dims[1] {
            for x in 0..dims[0] {
                let a = sample_value(grid, dims, x, y, z)?;
                let b = sample_value(grid, dims, x, y, z + 1)?;
                if is_crossing(a, b)? {
                    if x == 0 || x + 1 == dims[0] || y == 0 || y + 1 == dims[1] {
                        return Err(ExtractError::SurfaceTouchesGridBoundary);
                    }
                    let mut quad = [
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x - 1, y - 1, z)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y - 1, z)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x, y, z)?,
                        required_cell_vertex(&cell_vertex_ids, cell_dims, x - 1, y, z)?,
                    ];
                    orient_quad(&mut quad, a < 0)?;
                    push_quad(&mut quads, quad, config.max_quads)?;
                    sign_changing_edge_count += 1;
                }
            }
        }
    }

    if quads.is_empty() {
        return Err(ExtractError::NoSurfaceCrossings);
    }
    let triangle_count = quads
        .len()
        .checked_mul(2)
        .ok_or(ExtractError::SizeOverflow)?;
    let geometry_bytes = (vertices.len() as u64)
        .checked_mul(40)
        .and_then(|value| value.checked_add((quads.len() as u64).checked_mul(40)?))
        .ok_or(ExtractError::SizeOverflow)?;
    if geometry_bytes > config.max_geometry_bytes {
        return Err(ExtractError::GeometryBudgetExceeded);
    }
    let mut triangles = Vec::new();
    triangles
        .try_reserve_exact(triangle_count)
        .map_err(|_| ExtractError::AllocationFailed)?;
    for quad in &quads {
        // Fixed diagonal (0,2), preserving the quad's winding.
        triangles.push([quad[0], quad[1], quad[2]]);
        triangles.push([quad[0], quad[2], quad[3]]);
    }

    residuals.sort_by(f64::total_cmp);
    let metrics = DualContourMetrics {
        grid_dimensions: dims,
        active_cell_count,
        sign_changing_edge_count,
        quad_count: u32::try_from(quads.len()).map_err(|_| ExtractError::SizeOverflow)?,
        qef_centroid_fallback_count,
        qef_clamped_vertex_count,
        qef_residual_p50_nanounits: residual_quantile(&residuals, 50)?,
        qef_residual_p95_nanounits: residual_quantile(&residuals, 95)?,
        qef_residual_max_nanounits: residual_to_nanounits(*residuals.last().ok_or(ExtractError::NoSurfaceCrossings)?)?,
        geometry_bytes,
        working_payload_estimate_bytes: working_payload,
    };
    let semantic_mesh = semantic_triangle_mesh(&vertices, &triangles)?;
    let audit_report = audit_mesh(&semantic_mesh, AuditLimits::default())?;
    Ok(DualContourMesh {
        source_grid_canonical_bytes,
        config,
        vertices,
        quads,
        triangles,
        metrics,
        audit_report,
    })
}

fn semantic_triangle_mesh(
    vertices: &[DualContourVertex],
    triangles: &[[u32; 3]],
) -> Result<SemanticTriangleMesh, ExtractError> {
    let mut positions = Vec::new();
    positions
        .try_reserve_exact(vertices.len())
        .map_err(|_| ExtractError::AllocationFailed)?;
    let mut feature_masks = Vec::new();
    feature_masks
        .try_reserve_exact(vertices.len())
        .map_err(|_| ExtractError::AllocationFailed)?;
    for vertex in vertices {
        positions.push(vertex.position_nanounits.map(|value| value as f64 / POSITION_QUANTIZATION));
        feature_masks.push(vertex.feature_mask);
    }
    let mut faces = Vec::new();
    faces
        .try_reserve_exact(triangles.len())
        .map_err(|_| ExtractError::AllocationFailed)?;
    faces.extend_from_slice(triangles);
    let mut face_regions = Vec::new();
    face_regions
        .try_reserve_exact(faces.len())
        .map_err(|_| ExtractError::AllocationFailed)?;
    face_regions.resize(faces.len(), SKIN_FACE_REGION_ID);
    Ok(SemanticTriangleMesh {
        vertices: positions,
        vertex_feature_masks: feature_masks,
        faces,
        face_region_ids: face_regions,
    })
}

fn sample_value(grid: &SampledGrid, dims: [u32; 3], x: u32, y: u32, z: u32) -> Result<i32, ExtractError> {
    let index = linear_index(dims, x, y, z)?;
    grid.quantized_samples.get(index).copied().ok_or(ExtractError::SizeOverflow)
}

fn is_crossing(a: i32, b: i32) -> Result<bool, ExtractError> {
    if (a == 0 && b < 0) || (b == 0 && a < 0) {
        return Err(ExtractError::ExactZeroCrossing);
    }
    Ok((a < 0) != (b < 0))
}

fn required_cell_vertex(
    vertices: &[u32],
    dims: [u32; 3],
    x: u32,
    y: u32,
    z: u32,
) -> Result<u32, ExtractError> {
    let index = cell_linear_index(dims, x, y, z)?;
    let value = *vertices.get(index).ok_or(ExtractError::MissingAdjacentCellVertex)?;
    if value == NO_VERTEX {
        Err(ExtractError::MissingAdjacentCellVertex)
    } else {
        Ok(value)
    }
}

fn push_quad(quads: &mut Vec<[u32; 4]>, quad: [u32; 4], max_quads: u32) -> Result<(), ExtractError> {
    if quad[0] == quad[1]
        || quad[0] == quad[2]
        || quad[0] == quad[3]
        || quad[1] == quad[2]
        || quad[1] == quad[3]
        || quad[2] == quad[3]
    {
        return Err(ExtractError::DegenerateQuad);
    }
    if quads.len() >= max_quads as usize {
        return Err(ExtractError::GeometryBudgetExceeded);
    }
    quads.push(quad);
    Ok(())
}

fn orient_quad(quad: &mut [u32; 4], edge_starts_inside: bool) {
    if !edge_starts_inside {
        quad.swap(1, 3);
    }
}

fn linear_index(dims: [u32; 3], x: u32, y: u32, z: u32) -> Result<usize, ExtractError> {
    if x >= dims[0] || y >= dims[1] || z >= dims[2] {
        return Err(ExtractError::SizeOverflow);
    }
    let value = u64::from(x)
        .checked_add(
            u64::from(dims[0])
                .checked_mul(
                    u64::from(y)
                        .checked_add(u64::from(dims[1]).checked_mul(u64::from(z)).ok_or(ExtractError::SizeOverflow)?)
                        .ok_or(ExtractError::SizeOverflow)?,
                )
                .ok_or(ExtractError::SizeOverflow)?,
        )
        .ok_or(ExtractError::SizeOverflow)?;
    usize::try_from(value).map_err(|_| ExtractError::SizeOverflow)
}

fn cell_linear_index(dims: [u32; 3], x: u32, y: u32, z: u32) -> Result<usize, ExtractError> {
    if x >= dims[0] || y >= dims[1] || z >= dims[2] {
        return Err(ExtractError::SizeOverflow);
    }
    let value = u64::from(x)
        .checked_add(
            u64::from(dims[0])
                .checked_mul(
                    u64::from(y)
                        .checked_add(u64::from(dims[1]).checked_mul(u64::from(z)).ok_or(ExtractError::SizeOverflow)?)
                        .ok_or(ExtractError::SizeOverflow)?,
                )
                .ok_or(ExtractError::SizeOverflow)?,
        )
        .ok_or(ExtractError::SizeOverflow)?;
    usize::try_from(value).map_err(|_| ExtractError::SizeOverflow)
}

fn canonical_coordinate(value: f64) -> f64 {
    (value * 1_000_000.0).round() / 1_000_000.0
}

fn grid_coordinate(min: f64, max: f64, index: u32, count: u32) -> f64 {
    min + (max - min) * f64::from(index) / f64::from(count - 1)
}

fn lerp3(a: [f64; 3], b: [f64; 3], t: f64) -> [f64; 3] {
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

fn add3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale3(a: [f64; 3], scale: f64) -> [f64; 3] {
    [a[0] * scale, a[1] * scale, a[2] * scale]
}

fn dot3(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn length3(a: [f64; 3]) -> f64 {
    dot3(a, a).sqrt()
}

fn normalize3(a: [f64; 3]) -> Option<[f64; 3]> {
    let length = length3(a);
    if !length.is_finite() || length <= 1.0e-15 {
        None
    } else {
        Some(scale3(a, 1.0 / length))
    }
}

fn gradient_at_grid_vertex(
    grid: &SampledGrid,
    point: [u32; 3],
    dims: [u32; 3],
    spacing: [f64; 3],
) -> Result<[f64; 3], ExtractError> {
    let mut gradient = [0.0; 3];
    for axis in 0..3 {
        let mut low = point;
        let mut high = point;
        let derivative = if point[axis] == 0 {
            high[axis] += 1;
            (f64::from(sample_value(grid, dims, high[0], high[1], high[2])?)
                - f64::from(sample_value(grid, dims, low[0], low[1], low[2])?))
                / (1_000_000.0 * spacing[axis])
        } else if point[axis] + 1 == dims[axis] {
            low[axis] -= 1;
            (f64::from(sample_value(grid, dims, high[0], high[1], high[2])?)
                - f64::from(sample_value(grid, dims, low[0], low[1], low[2])?))
                / (1_000_000.0 * spacing[axis])
        } else {
            low[axis] -= 1;
            high[axis] += 1;
            (f64::from(sample_value(grid, dims, high[0], high[1], high[2])?)
                - f64::from(sample_value(grid, dims, low[0], low[1], low[2])?))
                / (2.0 * 1_000_000.0 * spacing[axis])
        };
        if !derivative.is_finite() {
            return Err(ExtractError::NonFiniteFieldGradient);
        }
        gradient[axis] = derivative;
    }
    Ok(gradient)
}

fn axis_fallback_normal(a: [u32; 3], b: [u32; 3], a_inside: bool) -> [f64; 3] {
    let mut normal = [0.0; 3];
    for axis in 0..3 {
        if a[axis] != b[axis] {
            normal[axis] = if a_inside { 1.0 } else { -1.0 };
            break;
        }
    }
    normal
}

fn hermite_centroid(samples: &[HermiteSample]) -> [f64; 3] {
    let mut sum = [0.0; 3];
    for sample in samples {
        sum = add3(sum, sample.point);
    }
    scale3(sum, 1.0 / samples.len() as f64)
}

fn solve_qef(
    samples: &[HermiteSample],
    centroid: [f64; 3],
    regularization: f64,
) -> ([f64; 3], bool) {
    let mut matrix = [[0.0_f64; 4]; 3];
    for sample in samples {
        let n = sample.normal;
        let rhs = dot3(n, sample.point);
        for row in 0..3 {
            for col in 0..3 {
                matrix[row][col] += n[row] * n[col];
            }
            matrix[row][3] += n[row] * rhs;
        }
    }
    for axis in 0..3 {
        matrix[axis][axis] += regularization;
        matrix[axis][3] += regularization * centroid[axis];
    }

    for pivot in 0..3 {
        let mut best = pivot;
        for row in (pivot + 1)..3 {
            if matrix[row][pivot].abs() > matrix[best][pivot].abs() {
                best = row;
            }
        }
        if matrix[best][pivot].abs() <= 1.0e-14 {
            return (centroid, true);
        }
        if best != pivot {
            matrix.swap(best, pivot);
        }
        let divisor = matrix[pivot][pivot];
        for col in pivot..4 {
            matrix[pivot][col] /= divisor;
        }
        for row in 0..3 {
            if row == pivot {
                continue;
            }
            let factor = matrix[row][pivot];
            for col in pivot..4 {
                matrix[row][col] -= factor * matrix[pivot][col];
            }
        }
    }
    let result = [matrix[0][3], matrix[1][3], matrix[2][3]];
    if result.iter().all(|value| value.is_finite()) {
        (result, false)
    } else {
        (centroid, true)
    }
}

fn clamp3(point: [f64; 3], minimum: [f64; 3], maximum: [f64; 3]) -> [f64; 3] {
    [
        point[0].clamp(minimum[0], maximum[0]),
        point[1].clamp(minimum[1], maximum[1]),
        point[2].clamp(minimum[2], maximum[2]),
    ]
}

fn qef_rms_residual(samples: &[HermiteSample], position: [f64; 3]) -> f64 {
    let sum = samples
        .iter()
        .map(|sample| {
            let residual = dot3(sample.normal, sub3(position, sample.point));
            residual * residual
        })
        .sum::<f64>();
    (sum / samples.len() as f64).sqrt()
}

fn residual_to_nanounits(value: f64) -> Result<u64, ExtractError> {
    let scaled = value * POSITION_QUANTIZATION;
    if !scaled.is_finite() || scaled < 0.0 || scaled >= u64::MAX as f64 {
        return Err(ExtractError::QuantizationOverflow);
    }
    Ok(scaled.round() as u64)
}

fn residual_quantile(values: &[f64], percentile: usize) -> Result<u64, ExtractError> {
    if values.is_empty() {
        return Err(ExtractError::NoSurfaceCrossings);
    }
    let index = (values.len() - 1) * percentile / 100;
    residual_to_nanounits(values[index])
}

fn quantize_position(value: f64) -> Result<i64, ExtractError> {
    let scaled = value * POSITION_QUANTIZATION;
    if !scaled.is_finite()
        || scaled < i64::MIN as f64
        || scaled >= 9_223_372_036_854_775_808.0
    {
        return Err(ExtractError::QuantizationOverflow);
    }
    Ok(scaled.round() as i64)
}

fn quantize_normal(value: f64) -> Result<i32, ExtractError> {
    let scaled = value.clamp(-1.0, 1.0) * NORMAL_QUANTIZATION;
    if !scaled.is_finite() || scaled < i32::MIN as f64 || scaled > i32::MAX as f64 {
        return Err(ExtractError::QuantizationOverflow);
    }
    Ok(scaled.round() as i32)
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
    use ouroboros_sdf_lab::{FaceSkinField, GridSpec, UnionMode, MAX_CANONICAL_BYTES, MAX_SAMPLES};

    fn grid(dimensions: [u32; 3], bounds: [[f64; 3]; 2]) -> GridSpec {
        GridSpec {
            minimum: bounds[0],
            maximum: bounds[1],
            dimensions,
            max_samples: MAX_SAMPLES,
            max_canonical_bytes: MAX_CANONICAL_BYTES,
            max_working_payload_bytes: 32 * 1024 * 1024,
        }
    }

    fn source_grid(dimensions: [u32; 3]) -> SampledGrid {
        let spec = grid(
            dimensions,
            [[-0.18, -0.14, -0.16], [0.25, 0.14, 0.16]],
        );
        let field = FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
            width: 0.004,
        })
        .unwrap();
        spec.sample(field).unwrap()
    }

    #[test]
    fn source_fixture_extracts_a_nonempty_candidate_mesh() {
        let sampled = source_grid([32, 32, 32]);
        let mesh = extract(&sampled, DualContourConfig::default()).unwrap();
        assert!(!mesh.vertices().is_empty());
        assert!(!mesh.quads().is_empty());
        assert_eq!(mesh.triangles().len(), mesh.quads().len() * 2);
        assert!(mesh.metrics().sign_changing_edge_count as usize == mesh.quads().len());
        assert_eq!(mesh.vertices().iter().all(|v| v.feature_mask == SKIN_FEATURE_MASK), true);
    }

    #[test]
    fn repeated_extraction_is_byte_deterministic() {
        let sampled = source_grid([32, 32, 32]);
        let first = extract(&sampled, DualContourConfig::default()).unwrap();
        let second = extract(&sampled, DualContourConfig::default()).unwrap();
        assert_eq!(first.canonical_bytes().unwrap(), second.canonical_bytes().unwrap());
    }

    #[test]
    fn quad_triangle_view_uses_fixed_diagonal_and_same_winding() {
        let sampled = source_grid([32, 32, 32]);
        let mesh = extract(&sampled, DualContourConfig::default()).unwrap();
        assert_eq!(mesh.triangles().len(), mesh.quads().len() * 2);
        for (index, quad) in mesh.quads().iter().enumerate() {
            assert_eq!(mesh.triangles()[index * 2], [quad[0], quad[1], quad[2]]);
            assert_eq!(mesh.triangles()[index * 2 + 1], [quad[0], quad[2], quad[3]]);
        }
    }

    #[test]
    fn audit_is_separate_and_self_intersection_remains_not_evaluated() {
        let sampled = source_grid([32, 32, 32]);
        let mesh = extract(&sampled, DualContourConfig::default()).unwrap();
        assert_eq!(
            mesh.audit_report().checks.self_intersections,
            CheckStatus::NotEvaluated
        );
    }

    #[test]
    fn output_receipt_binds_exact_source_grid_bytes() {
        let sampled = source_grid([24, 24, 24]);
        let mesh = extract(&sampled, DualContourConfig::default()).unwrap();
        assert_eq!(
            mesh.source_grid_canonical_bytes(),
            sampled.canonical_bytes().unwrap().as_slice()
        );
        assert!(mesh.canonical_bytes().unwrap().starts_with(&EXTRACTOR_MAGIC));
    }

    #[test]
    fn output_geometry_budget_rejects_before_exceeding_vertices() {
        let sampled = source_grid([32, 32, 32]);
        let mut config = DualContourConfig::default();
        config.max_vertices = 1;
        assert_eq!(
            extract(&sampled, config),
            Err(ExtractError::GeometryBudgetExceeded)
        );
    }

    #[test]
    fn sampled_surface_cut_by_grid_boundary_is_rejected() {
        let spec = grid(
            [24, 24, 24],
            [[-0.035, -0.025, -0.03], [0.10, 0.025, 0.03]],
        );
        let field = FaceSkinField::source_fixture(UnionMode::Sharp).unwrap();
        let sampled = spec.sample(field).unwrap();
        assert_eq!(
            extract(&sampled, DualContourConfig::default()),
            Err(ExtractError::SurfaceTouchesGridBoundary)
        );
    }

    #[test]
    fn exact_zero_crossing_is_rejected_without_symbolic_perturbation() {
        let mut sampled = source_grid([12, 12, 12]);
        sampled.quantized_samples[0] = 0;
        // The exact zero is only an error when it participates in an inside/outside edge.
        for z in 0..sampled.spec.dimensions[2] {
            for y in 0..sampled.spec.dimensions[1] {
                for x in 0..sampled.spec.dimensions[0] {
                    let index = linear_index(sampled.spec.dimensions, x, y, z).unwrap();
                    sampled.quantized_samples[index] = 1;
                }
            }
        }
        let dims = sampled.spec.dimensions;
        let a = linear_index(dims, 1, 1, 1).unwrap();
        let b = linear_index(dims, 2, 1, 1).unwrap();
        sampled.quantized_samples[a] = -1;
        sampled.quantized_samples[b] = 0;
        assert_eq!(
            extract(&sampled, DualContourConfig::default()),
            Err(ExtractError::ExactZeroCrossing)
        );
    }

    #[test]
    fn zero_regularization_singular_qef_uses_centroid_fallback() {
        let samples = [
            HermiteSample {
                point: [0.0, 0.25, 0.5],
                normal: [1.0, 0.0, 0.0],
            },
            HermiteSample {
                point: [0.0, 0.75, 0.5],
                normal: [1.0, 0.0, 0.0],
            },
        ];
        let centroid = hermite_centroid(&samples);
        let (position, fallback) = solve_qef(&samples, centroid, 0.0);
        assert!(fallback);
        assert_eq!(position, centroid);
    }

    #[test]
    fn too_small_grids_reject_before_cell_allocation() {
        let sampled = source_grid([2, 16, 16]);
        assert_eq!(
            extract(&sampled, DualContourConfig::default()),
            Err(ExtractError::GridTooSmall)
        );
    }
}
