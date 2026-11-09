//! Deterministic, bounded 3D tube mesh for the Ouroboros centerline.
//!
//! This is geometry, not a renderer. It turns the shared semantic centerline
//! into a closed indexed tube with explicit normals and UV seams. A native,
//! WGPU, Vulkan or other host can consume the same vertex/index contract.
//! The CPU reference renderer remains a separate low-cost fallback/test oracle.

use std::fmt;

use crate::{
    contract::SCENE_CONTRACT_VERSION,
    ouroboros::{FormWeights, OuroborosError, OuroborosScene},
};

const TAU: f32 = std::f32::consts::TAU;
const MAX_VERTICES: usize = 262_144;
const MAX_INDICES: usize = 1_572_864;

/// Bounded tessellation and cross-section settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosMeshConfig {
    /// Number of samples around the closed semantic centerline.
    pub ring_segments: u32,
    /// Number of facets around each tube cross-section.
    pub tube_segments: u32,
    /// Tube radius in the same normalized scene space as the centerline.
    pub tube_radius: f32,
}

impl Default for OuroborosMeshConfig {
    fn default() -> Self {
        Self {
            ring_segments: 256,
            tube_segments: 16,
            tube_radius: 0.025,
        }
    }
}

/// Host-supplied presentation/resource budget.
///
/// The scene core never queries a monitor or GPU. The renderer host computes
/// these values and asks for a deterministic geometry plan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosMeshBudget {
    /// Projected radius of the visible Ouroboros ring, in pixels.
    pub projected_major_radius_px: u32,
    /// Projected radius of the tube cross-section, in pixels.
    pub projected_tube_radius_px: u32,
    pub max_vertices: u32,
    pub max_indices: u32,
    /// Maximum canonical vertex/index storage estimate, in bytes.
    pub max_buffer_bytes: u32,
}

/// Coarse, reproducible quality tier derived from the selected tessellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosQualityTier {
    Economy,
    Balanced,
    Cinematic,
}

/// A deterministic mesh selection and its exact resource estimate.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosMeshPlan {
    pub tier: OuroborosQualityTier,
    /// Complete, host-provided input budget used to produce this plan.
    pub budget: OuroborosMeshBudget,
    /// Tessellation requested by projected size before resource constraints.
    pub requested_ring_segments: u32,
    pub requested_tube_segments: u32,
    /// Tessellation selected after applying all hard resource ceilings.
    pub config: OuroborosMeshConfig,
    pub vertex_count: u32,
    pub index_count: u32,
    pub estimated_buffer_bytes: u32,
}

impl OuroborosMeshConfig {
    pub fn validate(self, scene: &OuroborosScene) -> Result<(), OuroborosMeshError> {
        if !(32..=2048).contains(&self.ring_segments)
            || !(6..=64).contains(&self.tube_segments)
            || !self.tube_radius.is_finite()
            || !(0.001..=0.08).contains(&self.tube_radius)
            || self.tube_radius > scene.config().major_radius * 0.20
        {
            return Err(OuroborosMeshError::InvalidConfiguration);
        }

        let vertices = (self.ring_segments as usize + 1)
            .checked_mul(self.tube_segments as usize + 1)
            .ok_or(OuroborosMeshError::SizeOverflow)?;
        let indices = (self.ring_segments as usize)
            .checked_mul(self.tube_segments as usize)
            .and_then(|quads| quads.checked_mul(6))
            .ok_or(OuroborosMeshError::SizeOverflow)?;

        if vertices > MAX_VERTICES || indices > MAX_INDICES || vertices > u32::MAX as usize {
            return Err(OuroborosMeshError::SizeLimitExceeded);
        }
        Ok(())
    }
}

impl OuroborosMeshConfig {
    /// Select the highest deterministic quality tier allowed by host budgets.
    ///
    /// Projected size determines the desired tessellation. Resource ceilings
    /// then select the best feasible ring/tube pair. No monitor, GPU, refresh
    /// rate or frame clock is queried by this pure planning function.
    pub fn plan_for_budget(
        scene: &OuroborosScene,
        tube_radius: f32,
        budget: OuroborosMeshBudget,
    ) -> Result<OuroborosMeshPlan, OuroborosMeshError> {
        if !tube_radius.is_finite()
            || !(0.001..=0.08).contains(&tube_radius)
            || tube_radius > scene.config().major_radius * 0.20
            || budget.max_vertices == 0
            || budget.max_indices == 0
            || budget.max_buffer_bytes == 0
        {
            return Err(OuroborosMeshError::InvalidConfiguration);
        }

        let desired_ring = desired_ring_segments(budget.projected_major_radius_px);
        let desired_tube = desired_tube_segments(budget.projected_tube_radius_px);
        let mut ring = desired_ring;
        loop {
            let mut tube = desired_tube;
            loop {
                let config = OuroborosMeshConfig {
                    ring_segments: ring,
                    tube_segments: tube,
                    tube_radius,
                };
                let (vertices, indices, bytes) = mesh_resource_estimate(config)?;
                if vertices <= budget.max_vertices
                    && indices <= budget.max_indices
                    && bytes <= budget.max_buffer_bytes
                    && config.validate(scene).is_ok()
                {
                    let tier = if ring <= 128 && tube <= 8 {
                        OuroborosQualityTier::Economy
                    } else if ring <= 512 && tube <= 24 {
                        OuroborosQualityTier::Balanced
                    } else {
                        OuroborosQualityTier::Cinematic
                    };
                    return Ok(OuroborosMeshPlan {
                        tier,
                        budget,
                        requested_ring_segments: desired_ring,
                        requested_tube_segments: desired_tube,
                        config,
                        vertex_count: vertices,
                        index_count: indices,
                        estimated_buffer_bytes: bytes,
                    });
                }
                if tube == 6 {
                    break;
                }
                tube = previous_tube_tier(tube);
            }
            if ring == 32 {
                break;
            }
            ring /= 2;
        }

        Err(OuroborosMeshError::BudgetTooSmall)
    }
}

fn desired_ring_segments(projected_radius_px: u32) -> u32 {
    // Keep projected centerline spacing near or below 0.75 px.
    let desired = (std::f64::consts::TAU * f64::from(projected_radius_px) / 0.75)
        .ceil()
        .clamp(32.0, 2048.0) as u32;
    let mut selected = 32_u32;
    while selected.saturating_mul(2) <= desired {
        selected *= 2;
    }
    selected
}

fn desired_tube_segments(projected_radius_px: u32) -> u32 {
    match projected_radius_px {
        0..=1 => 6,
        2 => 8,
        3..=4 => 12,
        5..=8 => 16,
        9..=12 => 24,
        13..=20 => 32,
        21..=32 => 48,
        _ => 64,
    }
}

fn previous_tube_tier(value: u32) -> u32 {
    match value {
        64 => 48,
        48 => 32,
        32 => 24,
        24 => 16,
        16 => 12,
        12 => 8,
        _ => 6,
    }
}

fn mesh_resource_estimate(
    config: OuroborosMeshConfig,
) -> Result<(u32, u32, u32), OuroborosMeshError> {
    let ring = config.ring_segments as usize;
    let tube = config.tube_segments as usize;
    let vertices = (ring + 1)
        .checked_mul(tube + 1)
        .ok_or(OuroborosMeshError::SizeOverflow)?;
    let indices = ring
        .checked_mul(tube)
        .and_then(|quads| quads.checked_mul(6))
        .ok_or(OuroborosMeshError::SizeOverflow)?;
    // 12 f32 values per vertex (48 bytes) plus 32-bit indices.
    let vertex_bytes = vertices
        .checked_mul(48)
        .ok_or(OuroborosMeshError::SizeOverflow)?;
    let index_bytes = indices
        .checked_mul(4)
        .ok_or(OuroborosMeshError::SizeOverflow)?;
    let bytes = vertex_bytes
        .checked_add(index_bytes)
        .ok_or(OuroborosMeshError::SizeOverflow)?;
    Ok((
        u32::try_from(vertices).map_err(|_| OuroborosMeshError::SizeLimitExceeded)?,
        u32::try_from(indices).map_err(|_| OuroborosMeshError::SizeLimitExceeded)?,
        u32::try_from(bytes).map_err(|_| OuroborosMeshError::SizeLimitExceeded)?,
    ))
}

/// One vertex on the closed Ouroboros tube.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosMeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub form_weights: FormWeights,
}

/// Renderer-independent indexed geometry.
#[derive(Debug, Clone, PartialEq)]
pub struct OuroborosMesh {
    pub vertices: Vec<OuroborosMeshVertex>,
    pub indices: Vec<u32>,
    pub config: OuroborosMeshConfig,
    /// Major radius of the semantic source scene.
    pub source_major_radius: f32,
    pub scene_contract_version: u16,
}

/// Content address for one canonical mesh artifact.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosMeshReceipt {
    pub scene_contract_version: u16,
    pub vertex_count: u32,
    pub index_count: u32,
    pub digest: [u8; 32],
}

/// Geometry construction and validation errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosMeshError {
    InvalidConfiguration,
    SizeOverflow,
    SizeLimitExceeded,
    BudgetTooSmall,
    AllocationFailed,
    Scene(OuroborosError),
    DegenerateCenterline,
    InvalidGeneratedGeometry,
}

impl fmt::Display for OuroborosMeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => write!(f, "invalid Ouroboros mesh configuration"),
            Self::SizeOverflow => write!(f, "Ouroboros mesh size arithmetic overflowed"),
            Self::SizeLimitExceeded => write!(f, "Ouroboros mesh exceeds the vertex/index budget"),
            Self::BudgetTooSmall => write!(
                f,
                "host mesh budget cannot hold the minimum Ouroboros geometry"
            ),
            Self::AllocationFailed => write!(f, "unable to reserve bounded Ouroboros mesh storage"),
            Self::Scene(error) => write!(f, "invalid Ouroboros scene: {error}"),
            Self::DegenerateCenterline => {
                write!(f, "Ouroboros centerline has a degenerate tangent")
            }
            Self::InvalidGeneratedGeometry => {
                write!(f, "generated Ouroboros mesh failed validation")
            }
        }
    }
}

impl std::error::Error for OuroborosMeshError {}

impl OuroborosMesh {
    /// Generate a closed tube mesh from the scene's invariant centerline.
    ///
    /// The duplicated perimeter rows/columns preserve independent UV seams
    /// while positions/normals are byte-identical across each seam.
    pub fn build(
        scene: &OuroborosScene,
        config: OuroborosMeshConfig,
    ) -> Result<(Self, OuroborosMeshReceipt), OuroborosMeshError> {
        config.validate(scene)?;

        let ring_count = config.ring_segments as usize;
        let tube_count = config.tube_segments as usize;
        let vertex_count = (ring_count + 1)
            .checked_mul(tube_count + 1)
            .ok_or(OuroborosMeshError::SizeOverflow)?;
        let index_count = ring_count
            .checked_mul(tube_count)
            .and_then(|quads| quads.checked_mul(6))
            .ok_or(OuroborosMeshError::SizeOverflow)?;

        let mut centers = Vec::new();
        centers
            .try_reserve_exact(ring_count)
            .map_err(|_| OuroborosMeshError::AllocationFailed)?;
        for index in 0..config.ring_segments {
            let sample = scene
                .body_sample(index, config.ring_segments)
                .map_err(OuroborosMeshError::Scene)?;
            centers.push(sample.centerline);
        }

        let mut tangents = Vec::new();
        tangents
            .try_reserve_exact(ring_count)
            .map_err(|_| OuroborosMeshError::AllocationFailed)?;
        for index in 0..ring_count {
            let previous = centers[(index + ring_count - 1) % ring_count];
            let next = centers[(index + 1) % ring_count];
            let tangent =
                normalize(sub(next, previous)).ok_or(OuroborosMeshError::DegenerateCenterline)?;
            tangents.push(tangent);
        }

        // The curve is bounded away from a vertical tangent by the scene
        // contract. Project the global Z axis into the local normal plane;
        // this gives a stable, low-twist frame around the closed ring.
        let up = [0.0, 0.0, 1.0];
        let mut radial_frames = Vec::new();
        let mut binormal_frames = Vec::new();
        radial_frames
            .try_reserve_exact(ring_count)
            .map_err(|_| OuroborosMeshError::AllocationFailed)?;
        binormal_frames
            .try_reserve_exact(ring_count)
            .map_err(|_| OuroborosMeshError::AllocationFailed)?;

        for tangent in &tangents {
            let radial =
                normalize(cross(*tangent, up)).ok_or(OuroborosMeshError::DegenerateCenterline)?;
            let binormal = normalize(cross(radial, *tangent))
                .ok_or(OuroborosMeshError::DegenerateCenterline)?;
            radial_frames.push(radial);
            binormal_frames.push(binormal);
        }

        let mut vertices = Vec::new();
        vertices
            .try_reserve_exact(vertex_count)
            .map_err(|_| OuroborosMeshError::AllocationFailed)?;
        let form_weights = scene.frame().form_weights;

        for ring_index in 0..=ring_count {
            let source_index = ring_index % ring_count;
            let center = centers[source_index];
            let radial = radial_frames[source_index];
            let binormal = binormal_frames[source_index];
            let u = ring_index as f32 / ring_count as f32;

            for tube_index in 0..=tube_count {
                // Use the exact first angle for the duplicate seam vertex;
                // its UV differs, but its geometry is bit-identical.
                let angle = if tube_index == tube_count {
                    0.0
                } else {
                    TAU * tube_index as f32 / tube_count as f32
                };
                let cosine = angle.cos();
                let sine = angle.sin();
                let normal = add(scale(radial, cosine), scale(binormal, sine));
                let position = add(center, scale(normal, config.tube_radius));

                vertices.push(OuroborosMeshVertex {
                    position,
                    normal,
                    uv: [u, tube_index as f32 / tube_count as f32],
                    form_weights,
                });
            }
        }

        let mut indices = Vec::new();
        indices
            .try_reserve_exact(index_count)
            .map_err(|_| OuroborosMeshError::AllocationFailed)?;
        let row = tube_count + 1;
        for ring_index in 0..ring_count {
            for tube_index in 0..tube_count {
                let a = ring_index * row + tube_index;
                let b = (ring_index + 1) * row + tube_index;
                let c = ring_index * row + tube_index + 1;
                let d = (ring_index + 1) * row + tube_index + 1;
                indices.extend_from_slice(&[
                    a as u32, b as u32, c as u32, b as u32, d as u32, c as u32,
                ]);
            }
        }

        let mesh = Self {
            vertices,
            indices,
            config,
            source_major_radius: scene.config().major_radius,
            scene_contract_version: crate::contract::SCENE_CONTRACT_VERSION,
        };
        mesh.validate()?;
        let receipt = mesh.receipt()?;
        Ok((mesh, receipt))
    }

    /// Validate closed seams, topology, index bounds, finite values and unit normals.
    pub fn validate(&self) -> Result<(), OuroborosMeshError> {
        // Validate scalar bounds before size arithmetic. The mesh fields are
        // public renderer data, so callers can construct hostile values.
        if self.scene_contract_version != SCENE_CONTRACT_VERSION
            || !(32..=2048).contains(&self.config.ring_segments)
            || !(6..=64).contains(&self.config.tube_segments)
            || !self.config.tube_radius.is_finite()
            || !(0.001..=0.08).contains(&self.config.tube_radius)
            || !self.source_major_radius.is_finite()
            || !(0.1..=0.45).contains(&self.source_major_radius)
            || self.config.tube_radius > self.source_major_radius * 0.20
        {
            return Err(OuroborosMeshError::InvalidGeneratedGeometry);
        }

        let ring_count = self.config.ring_segments as usize;
        let tube_count = self.config.tube_segments as usize;
        let expected_vertices = (ring_count + 1)
            .checked_mul(tube_count + 1)
            .ok_or(OuroborosMeshError::SizeOverflow)?;
        let expected_indices = ring_count
            .checked_mul(tube_count)
            .and_then(|quads| quads.checked_mul(6))
            .ok_or(OuroborosMeshError::SizeOverflow)?;
        if expected_vertices > MAX_VERTICES
            || expected_indices > MAX_INDICES
            || expected_vertices > u32::MAX as usize
            || self.vertices.len() != expected_vertices
            || self.indices.len() != expected_indices
        {
            return Err(OuroborosMeshError::InvalidGeneratedGeometry);
        }

        let Some(reference_weights) = self.vertices.first().map(|vertex| vertex.form_weights)
        else {
            return Err(OuroborosMeshError::InvalidGeneratedGeometry);
        };
        for vertex in &self.vertices {
            if !vertex.position.into_iter().all(f32::is_finite)
                || !vertex.normal.into_iter().all(f32::is_finite)
                || !vertex.uv.into_iter().all(f32::is_finite)
                || !weights_are_valid(vertex.form_weights)
                || vertex.form_weights != reference_weights
                || !vertex
                    .uv
                    .into_iter()
                    .all(|value| (0.0..=1.0).contains(&value))
            {
                return Err(OuroborosMeshError::InvalidGeneratedGeometry);
            }
            let length = dot(vertex.normal, vertex.normal).sqrt();
            if !length.is_finite() || (length - 1.0).abs() > 1.0e-4 {
                return Err(OuroborosMeshError::InvalidGeneratedGeometry);
            }
        }

        // Require the canonical two-triangle tessellation, not merely
        // in-range indices. This makes receipt validation meaningful even
        // when callers mutate the public vertex/index vectors.
        let row = tube_count + 1;
        let mut offset = 0;
        for ring_index in 0..ring_count {
            for tube_index in 0..tube_count {
                let a = (ring_index * row + tube_index) as u32;
                let b = ((ring_index + 1) * row + tube_index) as u32;
                let c = (ring_index * row + tube_index + 1) as u32;
                let d = ((ring_index + 1) * row + tube_index + 1) as u32;
                let expected = [a, b, c, b, d, c];
                if self.indices[offset..offset + 6] != expected {
                    return Err(OuroborosMeshError::InvalidGeneratedGeometry);
                }
                offset += 6;
            }
        }

        // Validate every triangle, not a sampling of triangles: no degenerate
        // faces and no inward winding relative to the interpolated vertex normals.
        for triangle in self.indices.chunks_exact(3) {
            let a = self.vertices[triangle[0] as usize];
            let b = self.vertices[triangle[1] as usize];
            let c = self.vertices[triangle[2] as usize];
            let face = cross(sub(b.position, a.position), sub(c.position, a.position));
            let area_squared = dot(face, face);
            let outward = dot(face, add(add(a.normal, b.normal), c.normal));
            if !area_squared.is_finite()
                || area_squared <= 1.0e-16
                || !outward.is_finite()
                || outward <= 1.0e-12
            {
                return Err(OuroborosMeshError::InvalidGeneratedGeometry);
            }
        }

        // Both periodic dimensions duplicate their seam vertex exactly;
        // UVs alone may differ at 0/1.
        for tube_index in 0..=tube_count {
            let first = self.vertices[tube_index];
            let last = self.vertices[ring_count * row + tube_index];
            if first.position != last.position || first.normal != last.normal {
                return Err(OuroborosMeshError::InvalidGeneratedGeometry);
            }
        }
        for ring_index in 0..=ring_count {
            let first = self.vertices[ring_index * row];
            let last = self.vertices[ring_index * row + tube_count];
            if first.position != last.position || first.normal != last.normal {
                return Err(OuroborosMeshError::InvalidGeneratedGeometry);
            }
        }

        Ok(())
    }

    /// Hash a canonical little-endian serialization, not Rust struct memory.
    pub fn receipt(&self) -> Result<OuroborosMeshReceipt, OuroborosMeshError> {
        self.validate()?;
        let mut bytes = Vec::new();
        let vertex_bytes = self
            .vertices
            .len()
            .checked_mul(4 * 12)
            .ok_or(OuroborosMeshError::SizeOverflow)?;
        let index_bytes = self
            .indices
            .len()
            .checked_mul(4)
            .ok_or(OuroborosMeshError::SizeOverflow)?;
        let estimated = vertex_bytes
            .checked_add(index_bytes)
            .and_then(|n| n.checked_add(26))
            .ok_or(OuroborosMeshError::SizeOverflow)?;
        bytes
            .try_reserve_exact(estimated)
            .map_err(|_| OuroborosMeshError::AllocationFailed)?;

        bytes.extend_from_slice(b"OUROMSH2");
        bytes.extend_from_slice(&self.scene_contract_version.to_le_bytes());
        bytes.extend_from_slice(&self.config.ring_segments.to_le_bytes());
        bytes.extend_from_slice(&self.config.tube_segments.to_le_bytes());
        append_f32(&mut bytes, self.config.tube_radius);
        append_f32(&mut bytes, self.source_major_radius);

        for vertex in &self.vertices {
            for value in vertex.position {
                append_f32(&mut bytes, value);
            }
            for value in vertex.normal {
                append_f32(&mut bytes, value);
            }
            for value in vertex.uv {
                append_f32(&mut bytes, value);
            }
            for value in [
                vertex.form_weights.nature,
                vertex.form_weights.humanity,
                vertex.form_weights.digital,
                vertex.form_weights.enlightenment,
            ] {
                append_f32(&mut bytes, value);
            }
        }
        for index in &self.indices {
            bytes.extend_from_slice(&index.to_le_bytes());
        }

        Ok(OuroborosMeshReceipt {
            scene_contract_version: self.scene_contract_version,
            vertex_count: self.vertices.len() as u32,
            index_count: self.indices.len() as u32,
            digest: *blake3::hash(&bytes).as_bytes(),
        })
    }
}

fn weights_are_valid(weights: FormWeights) -> bool {
    [
        weights.nature,
        weights.humanity,
        weights.digital,
        weights.enlightenment,
    ]
    .into_iter()
    .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
}

fn append_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f32; 3], scalar: f32) -> [f32; 3] {
    [a[0] * scalar, a[1] * scalar, a[2] * scalar]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn normalize(vector: [f32; 3]) -> Option<[f32; 3]> {
    let length_squared = dot(vector, vector);
    if !length_squared.is_finite() || length_squared <= f32::EPSILON {
        return None;
    }
    let inverse_length = length_squared.sqrt().recip();
    let result = scale(vector, inverse_length);
    result.into_iter().all(f32::is_finite).then_some(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ouroboros::{OuroborosConfig, OuroborosForm};

    fn scene(form: OuroborosForm) -> OuroborosScene {
        OuroborosScene::new(OuroborosConfig::default(), form)
            .expect("default Ouroboros configuration is valid")
    }

    #[test]
    fn mesh_has_bounded_closed_topology() {
        let scene = scene(OuroborosForm::Nature);
        let (mesh, receipt) = OuroborosMesh::build(&scene, OuroborosMeshConfig::default()).unwrap();
        let ring = 256_usize;
        let tube = 16_usize;

        assert_eq!(mesh.vertices.len(), (ring + 1) * (tube + 1));
        assert_eq!(mesh.indices.len(), ring * tube * 6);
        assert_eq!(receipt.vertex_count as usize, mesh.vertices.len());
        assert_eq!(receipt.index_count as usize, mesh.indices.len());
        assert_eq!(receipt.scene_contract_version, SCENE_CONTRACT_VERSION);
        assert!(mesh.validate().is_ok());
    }

    #[test]
    fn all_forms_share_exact_topology_and_vertex_identity_layout() {
        let mut expected_indices = None;
        let mut expected_uvs = None;
        for form in OuroborosForm::ALL {
            let (mesh, _) =
                OuroborosMesh::build(&scene(form), OuroborosMeshConfig::default()).unwrap();
            if let Some(indices) = &expected_indices {
                assert_eq!(&mesh.indices, indices);
            } else {
                expected_indices = Some(mesh.indices.clone());
            }

            let uvs: Vec<[f32; 2]> = mesh.vertices.iter().map(|vertex| vertex.uv).collect();
            if let Some(expected) = &expected_uvs {
                assert_eq!(&uvs, expected);
            } else {
                expected_uvs = Some(uvs);
            }
        }
    }

    #[test]
    fn mesh_normals_are_unit_and_triangle_winding_points_outward() {
        let (mesh, _) = OuroborosMesh::build(
            &scene(OuroborosForm::Digital),
            OuroborosMeshConfig::default(),
        )
        .unwrap();

        for triangle in mesh.indices.chunks_exact(3).step_by(17) {
            let a = mesh.vertices[triangle[0] as usize];
            let b = mesh.vertices[triangle[1] as usize];
            let c = mesh.vertices[triangle[2] as usize];
            let geometric = cross(sub(b.position, a.position), sub(c.position, a.position));
            let expected = add(add(a.normal, b.normal), c.normal);
            assert!(dot(geometric, expected) > 0.0);
        }
    }

    #[test]
    fn both_uv_seams_duplicate_geometry_exactly() {
        let (mesh, _) = OuroborosMesh::build(
            &scene(OuroborosForm::Humanity),
            OuroborosMeshConfig::default(),
        )
        .unwrap();
        let ring = mesh.config.ring_segments as usize;
        let tube = mesh.config.tube_segments as usize;
        let row = tube + 1;

        for j in 0..=tube {
            let first = mesh.vertices[j];
            let last = mesh.vertices[ring * row + j];
            assert_eq!(first.position, last.position);
            assert_eq!(first.normal, last.normal);
            assert_eq!(first.uv[0], 0.0);
            assert_eq!(last.uv[0], 1.0);
        }
        for i in 0..=ring {
            let first = mesh.vertices[i * row];
            let last = mesh.vertices[i * row + tube];
            assert_eq!(first.position, last.position);
            assert_eq!(first.normal, last.normal);
            assert_eq!(first.uv[1], 0.0);
            assert_eq!(last.uv[1], 1.0);
        }
    }

    #[test]
    fn mesh_receipts_are_deterministic_and_form_sensitive() {
        let a = OuroborosMesh::build(
            &scene(OuroborosForm::Nature),
            OuroborosMeshConfig::default(),
        )
        .unwrap()
        .1;
        let b = OuroborosMesh::build(
            &scene(OuroborosForm::Nature),
            OuroborosMeshConfig::default(),
        )
        .unwrap()
        .1;
        let c = OuroborosMesh::build(
            &scene(OuroborosForm::Digital),
            OuroborosMeshConfig::default(),
        )
        .unwrap()
        .1;

        assert_eq!(a, b);
        assert_ne!(a.digest, c.digest);
    }

    #[test]
    fn validator_rejects_hostile_dimensions_without_overflowing() {
        let (mut mesh, _) = OuroborosMesh::build(
            &scene(OuroborosForm::Nature),
            OuroborosMeshConfig::default(),
        )
        .unwrap();
        mesh.config.ring_segments = u32::MAX;
        assert_eq!(
            mesh.validate(),
            Err(OuroborosMeshError::InvalidGeneratedGeometry)
        );
    }

    #[test]
    fn validator_rejects_noncanonical_indices_and_inward_winding() {
        let (mut mesh, _) = OuroborosMesh::build(
            &scene(OuroborosForm::Nature),
            OuroborosMeshConfig::default(),
        )
        .unwrap();
        mesh.indices[0] = mesh.indices[0].wrapping_add(1);
        assert_eq!(
            mesh.validate(),
            Err(OuroborosMeshError::InvalidGeneratedGeometry)
        );

        let (mut mesh, _) = OuroborosMesh::build(
            &scene(OuroborosForm::Digital),
            OuroborosMeshConfig::default(),
        )
        .unwrap();
        mesh.indices.swap(0, 1);
        assert_eq!(
            mesh.validate(),
            Err(OuroborosMeshError::InvalidGeneratedGeometry)
        );
    }

    #[test]
    fn validator_rejects_inconsistent_semantic_weights_and_scene_scale() {
        let (mut mesh, _) = OuroborosMesh::build(
            &scene(OuroborosForm::Humanity),
            OuroborosMeshConfig::default(),
        )
        .unwrap();
        mesh.vertices[0].form_weights.nature = 0.25;
        assert_eq!(
            mesh.validate(),
            Err(OuroborosMeshError::InvalidGeneratedGeometry)
        );

        let (mut mesh, _) = OuroborosMesh::build(
            &scene(OuroborosForm::Nature),
            OuroborosMeshConfig::default(),
        )
        .unwrap();
        mesh.source_major_radius = 0.1;
        assert_eq!(
            mesh.validate(),
            Err(OuroborosMeshError::InvalidGeneratedGeometry)
        );
    }

    #[test]
    fn host_budget_selects_deterministic_screen_size_aware_mesh_plans() {
        let scene = scene(OuroborosForm::Nature);
        let budget = OuroborosMeshBudget {
            projected_major_radius_px: 160,
            projected_tube_radius_px: 4,
            max_vertices: 40_000,
            max_indices: 240_000,
            max_buffer_bytes: 2_000_000,
        };
        let first = OuroborosMeshConfig::plan_for_budget(&scene, 0.025, budget).unwrap();
        let second = OuroborosMeshConfig::plan_for_budget(&scene, 0.025, budget).unwrap();
        assert_eq!(first, second);
        assert!(first.vertex_count <= budget.max_vertices);
        assert!(first.index_count <= budget.max_indices);
        assert!(first.estimated_buffer_bytes <= budget.max_buffer_bytes);
        assert_eq!(
            mesh_resource_estimate(first.config).unwrap(),
            (
                first.vertex_count,
                first.index_count,
                first.estimated_buffer_bytes
            )
        );

        let small = OuroborosMeshBudget {
            projected_major_radius_px: 24,
            projected_tube_radius_px: 1,
            ..budget
        };
        let small_plan = OuroborosMeshConfig::plan_for_budget(&scene, 0.025, small).unwrap();
        assert!(small_plan.config.ring_segments <= first.config.ring_segments);
        assert!(small_plan.config.tube_segments <= first.config.tube_segments);

        let large = OuroborosMeshBudget {
            projected_major_radius_px: 800,
            projected_tube_radius_px: 24,
            ..budget
        };
        let large_plan = OuroborosMeshConfig::plan_for_budget(&scene, 0.025, large).unwrap();
        assert!(large_plan.config.ring_segments >= first.config.ring_segments);
        assert!(large_plan.config.tube_segments >= first.config.tube_segments);
    }

    #[test]
    fn lod_budget_sweep_never_exceeds_declared_resource_ceilings() {
        let scene = scene(OuroborosForm::Enlightenment);
        let projected_ring_radii = [0, 1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1024, 2048];
        let projected_tube_radii = [0, 1, 2, 4, 8, 16, 32, 64];
        let budgets = [
            (2_000_u32, 12_000_u32, 160_000_u32),
            (10_000, 60_000, 600_000),
            (40_000, 240_000, 2_000_000),
            (100_000, 600_000, 8_000_000),
            (u32::MAX, u32::MAX, u32::MAX),
        ];

        for ring_px in projected_ring_radii {
            for tube_px in projected_tube_radii {
                for (max_vertices, max_indices, max_buffer_bytes) in budgets {
                    let budget = OuroborosMeshBudget {
                        projected_major_radius_px: ring_px,
                        projected_tube_radius_px: tube_px,
                        max_vertices,
                        max_indices,
                        max_buffer_bytes,
                    };
                    match OuroborosMeshConfig::plan_for_budget(&scene, 0.025, budget) {
                        Ok(plan) => {
                            assert!(plan.vertex_count <= max_vertices);
                            assert!(plan.index_count <= max_indices);
                            assert!(plan.estimated_buffer_bytes <= max_buffer_bytes);
                            assert!(plan.config.validate(&scene).is_ok());
                            assert_eq!(
                                mesh_resource_estimate(plan.config).unwrap(),
                                (
                                    plan.vertex_count,
                                    plan.index_count,
                                    plan.estimated_buffer_bytes
                                )
                            );
                            assert!(plan.requested_ring_segments >= plan.config.ring_segments);
                            assert!(plan.requested_tube_segments >= plan.config.tube_segments);
                        }
                        Err(OuroborosMeshError::BudgetTooSmall) => {
                            let minimum = OuroborosMeshConfig {
                                ring_segments: 32,
                                tube_segments: 6,
                                tube_radius: 0.025,
                            };
                            let (vertices, indices, bytes) =
                                mesh_resource_estimate(minimum).unwrap();
                            assert!(
                                vertices > max_vertices
                                    || indices > max_indices
                                    || bytes > max_buffer_bytes
                            );
                        }
                        Err(error) => panic!("unexpected LOD planner error: {error}"),
                    }
                }
            }
        }
    }

    #[test]
    fn lod_selection_is_monotonic_when_other_demand_and_budgets_are_fixed() {
        let scene = scene(OuroborosForm::Digital);
        let budget = OuroborosMeshBudget {
            projected_major_radius_px: 64,
            projected_tube_radius_px: 8,
            max_vertices: 80_000,
            max_indices: 480_000,
            max_buffer_bytes: 4_000_000,
        };
        let mut previous_ring = 0;
        for projected_radius in [16, 32, 64, 128, 256, 512, 1024, 2048] {
            let plan = OuroborosMeshConfig::plan_for_budget(
                &scene,
                0.025,
                OuroborosMeshBudget {
                    projected_major_radius_px: projected_radius,
                    ..budget
                },
            )
            .unwrap();
            assert!(plan.config.ring_segments >= previous_ring);
            previous_ring = plan.config.ring_segments;
        }

        let mut previous_tube = 0;
        for projected_tube in [1, 2, 4, 8, 16, 24, 32, 64] {
            let plan = OuroborosMeshConfig::plan_for_budget(
                &scene,
                0.025,
                OuroborosMeshBudget {
                    projected_major_radius_px: 128,
                    projected_tube_radius_px: projected_tube,
                    ..budget
                },
            )
            .unwrap();
            assert!(plan.config.tube_segments >= previous_tube);
            previous_tube = plan.config.tube_segments;
        }
    }

    #[test]
    fn host_budget_rejects_limits_below_minimum_geometry() {
        let scene = scene(OuroborosForm::Humanity);
        assert_eq!(
            OuroborosMeshConfig::plan_for_budget(
                &scene,
                0.025,
                OuroborosMeshBudget {
                    projected_major_radius_px: 100,
                    projected_tube_radius_px: 8,
                    max_vertices: 200,
                    max_indices: 1_000,
                    max_buffer_bytes: 10_000,
                }
            ),
            Err(OuroborosMeshError::BudgetTooSmall)
        );
    }

    #[test]
    fn invalid_mesh_budgets_are_rejected_before_generation() {
        let scene = scene(OuroborosForm::Nature);
        assert_eq!(
            OuroborosMesh::build(
                &scene,
                OuroborosMeshConfig {
                    ring_segments: 16,
                    ..OuroborosMeshConfig::default()
                },
            ),
            Err(OuroborosMeshError::InvalidConfiguration)
        );
        assert_eq!(
            OuroborosMeshConfig {
                tube_radius: 0.08,
                ..OuroborosMeshConfig::default()
            }
            .validate(&scene),
            Err(OuroborosMeshError::InvalidConfiguration)
        );
    }
}
