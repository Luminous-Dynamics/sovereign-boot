//! Bounded, renderer-independent 3D head and face geometry.
//!
//! The head shell and facial volumes are generated exclusively from the
//! versioned semantic landmark contract. This is a parametric identity mesh,
//! not a sculpt, shader, animation rig or GPU resource.

use std::{error::Error, fmt};

use crate::{
    contract::SCENE_CONTRACT_VERSION,
    ouroboros::{OuroborosLandmarkError, OuroborosLandmarks, OuroborosScene},
};

const FACE_MAGIC: [u8; 8] = *b"OUROFAC1";
const FACE_SCHEMA_VERSION: u16 = 1;
const FACE_REGION_COUNT: usize = 5;
const MAX_FACE_VERTICES: usize = 50_000;
const MAX_FACE_INDICES: usize = 300_000;
const MAX_FACE_BUFFER_BYTES: usize = 2_500_000;
const TAU: f32 = std::f32::consts::TAU;
const PI: f32 = std::f32::consts::PI;

pub const FACE_REGION_HEAD: u32 = 1;
pub const FACE_REGION_LEFT_EYE: u32 = 2;
pub const FACE_REGION_RIGHT_EYE: u32 = 3;
pub const FACE_REGION_NOSE: u32 = 4;
pub const FACE_REGION_MOUTH: u32 = 5;

/// Bounded UV-sphere tessellation shared by the five semantic face regions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosFaceMeshConfig {
    pub latitude_segments: u32,
    pub longitude_segments: u32,
}

impl Default for OuroborosFaceMeshConfig {
    fn default() -> Self {
        Self {
            latitude_segments: 24,
            longitude_segments: 48,
        }
    }
}

/// Host-supplied screen-size and resource budget for the semantic face mesh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosFaceMeshBudget {
    pub projected_face_radius_px: u32,
    pub max_vertices: u32,
    pub max_indices: u32,
    pub max_buffer_bytes: u32,
}

/// Face quality tier derived from the selected latitude/longitude tessellation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosFaceQualityTier {
    Economy,
    Balanced,
    Cinematic,
}

/// Reproducible face-mesh LOD decision, including all original host inputs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosFaceMeshPlan {
    pub budget: OuroborosFaceMeshBudget,
    pub requested: OuroborosFaceMeshConfig,
    pub config: OuroborosFaceMeshConfig,
    pub tier: OuroborosFaceQualityTier,
    pub vertex_count: u32,
    pub index_count: u32,
    pub estimated_buffer_bytes: u32,
}

impl OuroborosFaceMeshConfig {
    pub fn validate(self) -> Result<(), OuroborosFaceMeshError> {
        if !(8..=64).contains(&self.latitude_segments)
            || !(16..=128).contains(&self.longitude_segments)
        {
            return Err(OuroborosFaceMeshError::InvalidConfiguration);
        }
        let (vertices, indices, bytes) = face_resource_estimate(self)?;
        if vertices > MAX_FACE_VERTICES
            || indices > MAX_FACE_INDICES
            || bytes > MAX_FACE_BUFFER_BYTES
        {
            return Err(OuroborosFaceMeshError::SizeLimitExceeded);
        }
        Ok(())
    }
}

/// One renderer-neutral face vertex. region_id is stable across LODs.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosFaceVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub region_id: u32,
}

/// Receipt binding semantic landmarks, tessellation and every face geometry byte.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosFaceMeshReceipt {
    pub schema_version: u16,
    pub scene_contract_version: u16,
    pub landmark_digest: [u8; 32],
    pub vertex_count: u32,
    pub index_count: u32,
    pub digest: [u8; 32],
}

/// Errors from bounded face geometry construction and validation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosFaceMeshError {
    InvalidConfiguration,
    InvalidLandmarks(OuroborosLandmarkError),
    SizeOverflow,
    SizeLimitExceeded,
    BudgetTooSmall,
    AllocationFailed,
    InvalidGeneratedGeometry,
}

impl fmt::Display for OuroborosFaceMeshError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => write!(f, "invalid Ouroboros face mesh configuration"),
            Self::InvalidLandmarks(error) => write!(f, "invalid Ouroboros face landmarks: {error}"),
            Self::SizeOverflow => write!(f, "Ouroboros face mesh size arithmetic overflowed"),
            Self::SizeLimitExceeded => write!(f, "Ouroboros face mesh exceeds its resource ceiling"),
            Self::BudgetTooSmall => write!(f, "host budget cannot hold the minimum semantic face mesh"),
            Self::AllocationFailed => write!(f, "unable to reserve bounded Ouroboros face mesh storage"),
            Self::InvalidGeneratedGeometry => write!(f, "generated Ouroboros face mesh failed validation"),
        }
    }
}

impl Error for OuroborosFaceMeshError {}

/// Five closed geometric regions bound to one canonical landmark set.
///
/// landmarks is retained as the authoritative attachment table. Rendering
/// backends bind materials and detail to the stable region IDs rather than
/// inventing backend-specific face coordinates.
#[derive(Debug, Clone, PartialEq)]
pub struct OuroborosFaceMesh {
    pub vertices: Vec<OuroborosFaceVertex>,
    pub indices: Vec<u32>,
    pub config: OuroborosFaceMeshConfig,
    pub landmarks: OuroborosLandmarks,
    pub source_major_radius: f32,
    pub scene_contract_version: u16,
}

impl OuroborosFaceMeshConfig {
    /// Choose the highest feasible deterministic face tessellation for the
    /// projected head radius and host resource ceilings.
    ///
    /// The host supplies projected size; the face model never queries a
    /// display or GPU. Requested segments round up to the face quality ladder.
    pub fn plan_for_budget(
        budget: OuroborosFaceMeshBudget,
    ) -> Result<OuroborosFaceMeshPlan, OuroborosFaceMeshError> {
        if budget.projected_face_radius_px == 0
            || budget.max_vertices == 0
            || budget.max_indices == 0
            || budget.max_buffer_bytes == 0
        {
            return Err(OuroborosFaceMeshError::InvalidConfiguration);
        }

        let requested = OuroborosFaceMeshConfig {
            latitude_segments: requested_face_latitude(budget.projected_face_radius_px),
            longitude_segments: requested_face_longitude(budget.projected_face_radius_px),
        };
        let latitude_ladder = [8_u32, 12, 16, 24, 32, 48, 64];
        let longitude_ladder = [16_u32, 24, 32, 48, 64, 96, 128];
        let mut best: Option<(u64, OuroborosFaceMeshPlan)> = None;

        for latitude in latitude_ladder {
            if latitude > requested.latitude_segments {
                continue;
            }
            for longitude in longitude_ladder {
                if longitude > requested.longitude_segments {
                    continue;
                }
                let config = OuroborosFaceMeshConfig {
                    latitude_segments: latitude,
                    longitude_segments: longitude,
                };
                let (vertices, indices, bytes) = face_resource_estimate(config)?;
                if vertices > budget.max_vertices as usize
                    || indices > budget.max_indices as usize
                    || bytes > budget.max_buffer_bytes as usize
                    || config.validate().is_err()
                {
                    continue;
                }

                let tier = if latitude <= 16 && longitude <= 32 {
                    OuroborosFaceQualityTier::Economy
                } else if latitude <= 32 && longitude <= 64 {
                    OuroborosFaceQualityTier::Balanced
                } else {
                    OuroborosFaceQualityTier::Cinematic
                };
                let plan = OuroborosFaceMeshPlan {
                    budget,
                    requested,
                    config,
                    tier,
                    vertex_count: u32::try_from(vertices)
                        .map_err(|_| OuroborosFaceMeshError::SizeLimitExceeded)?,
                    index_count: u32::try_from(indices)
                        .map_err(|_| OuroborosFaceMeshError::SizeLimitExceeded)?,
                    estimated_buffer_bytes: u32::try_from(bytes)
                        .map_err(|_| OuroborosFaceMeshError::SizeLimitExceeded)?,
                };
                let score = u64::from(latitude) * u64::from(longitude);
                if best
                    .as_ref()
                    .is_none_or(|(best_score, old)| {
                        score > *best_score
                            || (score == *best_score
                                && (latitude, longitude)
                                    > (old.config.latitude_segments, old.config.longitude_segments))
                    })
                {
                    best = Some((score, plan));
                }
            }
        }

        best.map(|(_, plan)| plan)
            .ok_or(OuroborosFaceMeshError::BudgetTooSmall)
    }

    /// Reject caller-mutated plans by recomputing the exact deterministic choice.
    pub fn validate_plan(
        plan: OuroborosFaceMeshPlan,
    ) -> Result<(), OuroborosFaceMeshError> {
        let canonical = Self::plan_for_budget(plan.budget)?;
        if canonical != plan {
            return Err(OuroborosFaceMeshError::InvalidConfiguration);
        }
        Ok(())
    }
}

fn requested_face_latitude(projected_radius_px: u32) -> u32 {
    let needed = (std::f64::consts::PI * f64::from(projected_radius_px) / 1.5)
        .ceil()
        .clamp(8.0, 64.0) as u32;
    [8_u32, 12, 16, 24, 32, 48, 64]
        .into_iter()
        .find(|segments| *segments >= needed)
        .unwrap_or(64)
}

fn requested_face_longitude(projected_radius_px: u32) -> u32 {
    let needed = (std::f64::consts::TAU * f64::from(projected_radius_px) / 1.5)
        .ceil()
        .clamp(16.0, 128.0) as u32;
    [16_u32, 24, 32, 48, 64, 96, 128]
        .into_iter()
        .find(|segments| *segments >= needed)
        .unwrap_or(128)
}

impl OuroborosFaceMesh {
    /// Build one closed shell plus four closed feature volumes from semantic landmarks.
    pub fn build(
        scene: &OuroborosScene,
        landmarks: OuroborosLandmarks,
        config: OuroborosFaceMeshConfig,
    ) -> Result<(Self, OuroborosFaceMeshReceipt), OuroborosFaceMeshError> {
        config.validate()?;
        landmarks
            .validate(scene)
            .map_err(OuroborosFaceMeshError::InvalidLandmarks)?;

        let radius = scene.config().major_radius;
        let (vertices, indices) = generate_face_geometry(landmarks, config, radius)?;

        let mesh = Self {
            vertices,
            indices,
            config,
            landmarks,
            source_major_radius: radius,
            scene_contract_version: SCENE_CONTRACT_VERSION,
        };
        mesh.validate(scene)?;
        let receipt = mesh.receipt()?;
        Ok((mesh, receipt))
    }

    /// Build from a deterministic host-budgeted face LOD plan.
    pub fn build_with_plan(
        scene: &OuroborosScene,
        landmarks: OuroborosLandmarks,
        plan: OuroborosFaceMeshPlan,
    ) -> Result<(Self, OuroborosFaceMeshReceipt), OuroborosFaceMeshError> {
        OuroborosFaceMeshConfig::validate_plan(plan)?;
        let (mesh, receipt) = Self::build(scene, landmarks, plan.config)?;
        if receipt.vertex_count != plan.vertex_count
            || receipt.index_count != plan.index_count
            || face_resource_estimate(plan.config)?.2 != plan.estimated_buffer_bytes as usize
        {
            return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
        }
        Ok((mesh, receipt))
    }

    pub fn validate(&self, scene: &OuroborosScene) -> Result<(), OuroborosFaceMeshError> {
        self.config.validate()?;
        self.landmarks
            .validate(scene)
            .map_err(OuroborosFaceMeshError::InvalidLandmarks)?;
        if self.scene_contract_version != SCENE_CONTRACT_VERSION
            || !self.source_major_radius.is_finite()
            || self.source_major_radius != scene.config().major_radius
        {
            return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
        }

        let (expected_vertices, expected_indices, _) = face_resource_estimate(self.config)?;
        if expected_vertices > MAX_FACE_VERTICES
            || expected_indices > MAX_FACE_INDICES
            || self.vertices.len() != expected_vertices
            || self.indices.len() != expected_indices
        {
            return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
        }

        // Public mesh fields are mutable. Regenerate the canonical parameter
        // surface and compare every vertex/index before treating this artifact
        // as validated; counts and outward normals alone do not prove that the
        // geometry still follows its semantic landmark attachments.
        let (expected_vertices_data, expected_indices_data) = generate_face_geometry(
            self.landmarks,
            self.config,
            self.source_major_radius,
        )?;
        if self.vertices != expected_vertices_data || self.indices != expected_indices_data {
            return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
        }

        let per_region_vertices = sphere_vertex_count(self.config)?;
        let per_region_indices = sphere_index_count(self.config)?;
        let expected_regions = [
            FACE_REGION_HEAD,
            FACE_REGION_LEFT_EYE,
            FACE_REGION_RIGHT_EYE,
            FACE_REGION_NOSE,
            FACE_REGION_MOUTH,
        ];
        for (region_index, region_id) in expected_regions.into_iter().enumerate() {
            let vertex_start = region_index * per_region_vertices;
            let vertex_end = vertex_start + per_region_vertices;
            let region_vertices = &self.vertices[vertex_start..vertex_end];
            if region_vertices.iter().any(|vertex| {
                vertex.region_id != region_id
                    || !vertex.position.into_iter().all(f32::is_finite)
                    || !vertex.normal.into_iter().all(f32::is_finite)
                    || !vertex
                        .uv
                        .into_iter()
                        .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
            }) {
                return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
            }
            for vertex in region_vertices {
                let normal_length = dot(vertex.normal, vertex.normal).sqrt();
                if !normal_length.is_finite() || (normal_length - 1.0).abs() > 1.0e-4 {
                    return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
                }
            }

            let index_start = region_index * per_region_indices;
            let index_end = index_start + per_region_indices;
            let region_indices = &self.indices[index_start..index_end];
            validate_canonical_sphere_indices(region_indices, vertex_start as u32, self.config)?;
            for triangle in region_indices.chunks_exact(3) {
                let a = self.vertices[triangle[0] as usize];
                let b = self.vertices[triangle[1] as usize];
                let c = self.vertices[triangle[2] as usize];
                let face = cross(sub(b.position, a.position), sub(c.position, a.position));
                let area_squared = dot(face, face);
                let outward = dot(face, add(add(a.normal, b.normal), c.normal));
                if !area_squared.is_finite()
                    || area_squared <= 1.0e-18
                    || !outward.is_finite()
                    || outward <= 1.0e-14
                {
                    return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
                }
            }

            validate_region_seam(region_vertices, self.config)?;
        }
        Ok(())
    }

    /// Canonical OUROFAC1 bytes. The landmark digest binds the semantic
    /// attachment table; vertices and indices bind every generated geometry byte.
    pub fn canonical_bytes(&self) -> Result<Vec<u8>, OuroborosFaceMeshError> {
        let landmark_receipt = self.landmarks.receipt();
        let capacity = self
            .vertices
            .len()
            .checked_mul(36)
            .and_then(|value| value.checked_add(self.indices.len().checked_mul(4)?))
            .and_then(|value| value.checked_add(64))
            .ok_or(OuroborosFaceMeshError::SizeOverflow)?;
        let mut bytes = Vec::new();
        bytes
            .try_reserve_exact(capacity)
            .map_err(|_| OuroborosFaceMeshError::AllocationFailed)?;
        bytes.extend_from_slice(&FACE_MAGIC);
        bytes.extend_from_slice(&FACE_SCHEMA_VERSION.to_le_bytes());
        bytes.extend_from_slice(&self.scene_contract_version.to_le_bytes());
        bytes.extend_from_slice(&landmark_receipt.digest);
        bytes.extend_from_slice(&self.config.latitude_segments.to_le_bytes());
        bytes.extend_from_slice(&self.config.longitude_segments.to_le_bytes());
        bytes.extend_from_slice(&self.source_major_radius.to_bits().to_le_bytes());
        bytes.extend_from_slice(&(self.vertices.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&(self.indices.len() as u32).to_le_bytes());
        for vertex in &self.vertices {
            for value in vertex.position.into_iter().chain(vertex.normal).chain(vertex.uv) {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            bytes.extend_from_slice(&vertex.region_id.to_le_bytes());
        }
        for index in &self.indices {
            bytes.extend_from_slice(&index.to_le_bytes());
        }
        Ok(bytes)
    }

    pub fn receipt(&self) -> Result<OuroborosFaceMeshReceipt, OuroborosFaceMeshError> {
        let bytes = self.canonical_bytes()?;
        Ok(OuroborosFaceMeshReceipt {
            schema_version: FACE_SCHEMA_VERSION,
            scene_contract_version: self.scene_contract_version,
            landmark_digest: self.landmarks.receipt().digest,
            vertex_count: u32::try_from(self.vertices.len())
                .map_err(|_| OuroborosFaceMeshError::SizeLimitExceeded)?,
            index_count: u32::try_from(self.indices.len())
                .map_err(|_| OuroborosFaceMeshError::SizeLimitExceeded)?,
            digest: *blake3::hash(&bytes).as_bytes(),
        })
    }
}

fn generate_face_geometry(
    landmarks: OuroborosLandmarks,
    config: OuroborosFaceMeshConfig,
    radius: f32,
) -> Result<(Vec<OuroborosFaceVertex>, Vec<u32>), OuroborosFaceMeshError> {
    config.validate()?;
    if !radius.is_finite() || !(0.1..=0.45).contains(&radius) {
        return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
    }

    let (vertex_count, index_count, _) = face_resource_estimate(config)?;
    let (_, _, estimated_bytes) = face_resource_estimate(config)?;
    if vertex_count > MAX_FACE_VERTICES
        || index_count > MAX_FACE_INDICES
        || estimated_bytes > MAX_FACE_BUFFER_BYTES
    {
        return Err(OuroborosFaceMeshError::SizeLimitExceeded);
    }

    let mut vertices = Vec::new();
    vertices
        .try_reserve_exact(vertex_count)
        .map_err(|_| OuroborosFaceMeshError::AllocationFailed)?;
    let mut indices = Vec::new();
    indices
        .try_reserve_exact(index_count)
        .map_err(|_| OuroborosFaceMeshError::AllocationFailed)?;

    let forward = landmarks.forward;
    let lateral = landmarks.lateral;
    let up = landmarks.up;

    // The shell is intentionally restrained. Feature volumes remain
    // independently addressable and are attached through the landmarks.
    append_ellipsoid(
        &mut vertices,
        &mut indices,
        config,
        point(landmarks.head_anchor, forward, lateral, up, 0.030 * radius, 0.0, 0.0),
        [forward, lateral, up],
        [0.120 * radius, 0.060 * radius, 0.110 * radius],
        FACE_REGION_HEAD,
    )?;
    append_ellipsoid(
        &mut vertices,
        &mut indices,
        config,
        landmarks.left_eye,
        [forward, lateral, up],
        [0.016 * radius, 0.018 * radius, 0.014 * radius],
        FACE_REGION_LEFT_EYE,
    )?;
    append_ellipsoid(
        &mut vertices,
        &mut indices,
        config,
        landmarks.right_eye,
        [forward, lateral, up],
        [0.016 * radius, 0.018 * radius, 0.014 * radius],
        FACE_REGION_RIGHT_EYE,
    )?;
    let nose_center = sub(landmarks.nose_tip, scale(forward, 0.020 * radius));
    append_ellipsoid(
        &mut vertices,
        &mut indices,
        config,
        nose_center,
        [forward, lateral, up],
        [0.020 * radius, 0.014 * radius, 0.018 * radius],
        FACE_REGION_NOSE,
    )?;
    append_ellipsoid(
        &mut vertices,
        &mut indices,
        config,
        landmarks.mouth_center,
        [forward, lateral, up],
        [0.008 * radius, 0.035 * radius, 0.006 * radius],
        FACE_REGION_MOUTH,
    )?;

    Ok((vertices, indices))
}

fn sphere_vertex_count(config: OuroborosFaceMeshConfig) -> Result<usize, OuroborosFaceMeshError> {
    (config.latitude_segments as usize - 1)
        .checked_mul(config.longitude_segments as usize + 1)
        .and_then(|value| value.checked_add(2))
        .ok_or(OuroborosFaceMeshError::SizeOverflow)
}

fn sphere_index_count(config: OuroborosFaceMeshConfig) -> Result<usize, OuroborosFaceMeshError> {
    (config.longitude_segments as usize)
        .checked_mul(config.latitude_segments as usize - 1)
        .and_then(|value| value.checked_mul(6))
        .ok_or(OuroborosFaceMeshError::SizeOverflow)
}

fn face_resource_estimate(
    config: OuroborosFaceMeshConfig,
) -> Result<(usize, usize, usize), OuroborosFaceMeshError> {
    let vertices = sphere_vertex_count(config)?
        .checked_mul(FACE_REGION_COUNT)
        .ok_or(OuroborosFaceMeshError::SizeOverflow)?;
    let indices = sphere_index_count(config)?
        .checked_mul(FACE_REGION_COUNT)
        .ok_or(OuroborosFaceMeshError::SizeOverflow)?;
    let bytes = vertices
        .checked_mul(36)
        .and_then(|value| value.checked_add(indices.checked_mul(4)?))
        .ok_or(OuroborosFaceMeshError::SizeOverflow)?;
    Ok((vertices, indices, bytes))
}

fn append_ellipsoid(
    vertices: &mut Vec<OuroborosFaceVertex>,
    indices: &mut Vec<u32>,
    config: OuroborosFaceMeshConfig,
    center: [f32; 3],
    basis: [[f32; 3]; 3],
    radii: [f32; 3],
    region_id: u32,
) -> Result<(), OuroborosFaceMeshError> {
    if !center.into_iter().all(f32::is_finite)
        || !radii
            .into_iter()
            .all(|radius| radius.is_finite() && radius > 0.0)
    {
        return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
    }
    let [forward, lateral, up] = basis;
    let base = u32::try_from(vertices.len())
        .map_err(|_| OuroborosFaceMeshError::SizeLimitExceeded)?;
    let latitude = config.latitude_segments as usize;
    let longitude = config.longitude_segments as usize;
    let row = longitude + 1;

    vertices.push(OuroborosFaceVertex {
        position: add(center, scale(up, radii[2])),
        normal: up,
        uv: [0.5, 0.0],
        region_id,
    });

    for latitude_index in 1..latitude {
        let phi = PI * latitude_index as f32 / latitude as f32;
        let sine_phi = phi.sin();
        let cosine_phi = phi.cos();
        for longitude_index in 0..=longitude {
            let theta = if longitude_index == longitude {
                0.0
            } else {
                TAU * longitude_index as f32 / longitude as f32
            };
            let cosine = theta.cos();
            let sine = theta.sin();
            let local = [
                radii[0] * sine_phi * cosine,
                radii[1] * sine_phi * sine,
                radii[2] * cosine_phi,
            ];
            let position = add(
                center,
                add(
                    scale(forward, local[0]),
                    add(scale(lateral, local[1]), scale(up, local[2])),
                ),
            );
            let normal = normalize(add(
                scale(forward, sine_phi * cosine / radii[0]),
                add(
                    scale(lateral, sine_phi * sine / radii[1]),
                    scale(up, cosine_phi / radii[2]),
                ),
            ))
            .ok_or(OuroborosFaceMeshError::InvalidGeneratedGeometry)?;
            vertices.push(OuroborosFaceVertex {
                position,
                normal,
                uv: [
                    longitude_index as f32 / longitude as f32,
                    latitude_index as f32 / latitude as f32,
                ],
                region_id,
            });
        }
    }

    let bottom_index = u32::try_from(vertices.len())
        .map_err(|_| OuroborosFaceMeshError::SizeLimitExceeded)?;
    vertices.push(OuroborosFaceVertex {
        position: sub(center, scale(up, radii[2])),
        normal: scale(up, -1.0),
        uv: [0.5, 1.0],
        region_id,
    });

    let first_ring = base + 1;
    for longitude_index in 0..longitude as u32 {
        indices.extend_from_slice(&[
            base,
            first_ring + longitude_index,
            first_ring + longitude_index + 1,
        ]);
    }

    for latitude_index in 0..(latitude - 2) as u32 {
        for longitude_index in 0..longitude as u32 {
            let a = base + 1 + latitude_index * row as u32 + longitude_index;
            let b = a + row as u32;
            let c = a + 1;
            let d = b + 1;
            // Reverse latitude/longitude parameter order so normals
            // and triangle winding both point outward.
            indices.extend_from_slice(&[a, c, b, c, d, b]);
        }
    }

    let last_ring = base + 1 + (latitude - 2) as u32 * row as u32;
    for longitude_index in 0..longitude as u32 {
        indices.extend_from_slice(&[
            last_ring + longitude_index,
            bottom_index,
            last_ring + longitude_index + 1,
        ]);
    }
    Ok(())
}

fn validate_canonical_sphere_indices(
    indices: &[u32],
    base: u32,
    config: OuroborosFaceMeshConfig,
) -> Result<(), OuroborosFaceMeshError> {
    let latitude = config.latitude_segments as usize;
    let longitude = config.longitude_segments as usize;
    let row = longitude + 1;
    let mut offset = 0_usize;

    for longitude_index in 0..longitude {
        let expected = [
            base,
            base + 1 + longitude_index as u32,
            base + 2 + longitude_index as u32,
        ];
        if indices.get(offset..offset + 3) != Some(expected.as_slice()) {
            return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
        }
        offset += 3;
    }

    for latitude_index in 0..(latitude - 2) {
        for longitude_index in 0..longitude {
            let a = base + 1 + (latitude_index * row + longitude_index) as u32;
            let b = a + row as u32;
            let c = a + 1;
            let d = b + 1;
            // latitude then longitude is an inward parameter-space
            // ordering; reverse each quad triangle for outward face winding.
            let expected = [a, c, b, c, d, b];
            if indices.get(offset..offset + 6) != Some(expected.as_slice()) {
                return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
            }
            offset += 6;
        }
    }

    let bottom = base + sphere_vertex_count(config)? as u32 - 1;
    let last_ring = base + 1 + (latitude - 2) as u32 * row as u32;
    for longitude_index in 0..longitude {
        let expected = [
            last_ring + longitude_index as u32,
            bottom,
            last_ring + longitude_index as u32 + 1,
        ];
        if indices.get(offset..offset + 3) != Some(&expected) {
            return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
        }
        offset += 3;
    }
    if offset != indices.len() {
        return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
    }
    Ok(())
}

fn validate_region_seam(
    vertices: &[OuroborosFaceVertex],
    config: OuroborosFaceMeshConfig,
) -> Result<(), OuroborosFaceMeshError> {
    let row = config.longitude_segments as usize + 1;
    for latitude_index in 0..(config.latitude_segments as usize - 1) {
        let first = vertices[1 + latitude_index * row];
        let last = vertices[1 + latitude_index * row + config.longitude_segments as usize];
        if first.position != last.position || first.normal != last.normal {
            return Err(OuroborosFaceMeshError::InvalidGeneratedGeometry);
        }
    }
    Ok(())
}

fn point(
    origin: [f32; 3],
    forward: [f32; 3],
    lateral: [f32; 3],
    up: [f32; 3],
    forward_offset: f32,
    lateral_offset: f32,
    up_offset: f32,
) -> [f32; 3] {
    add(
        add(
            add(origin, scale(forward, forward_offset)),
            scale(lateral, lateral_offset),
        ),
        scale(up, up_offset),
    )
}

fn add(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(value: [f32; 3], amount: f32) -> [f32; 3] {
    [value[0] * amount, value[1] * amount, value[2] * amount]
}

fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn normalize(value: [f32; 3]) -> Option<[f32; 3]> {
    let length = dot(value, value).sqrt();
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    Some(scale(value, 1.0 / length))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ouroboros::{OuroborosConfig, OuroborosForm};

    fn scene(form: OuroborosForm) -> OuroborosScene {
        OuroborosScene::new(OuroborosConfig::default(), form).unwrap()
    }

    #[test]
    fn face_lod_planner_is_deterministic_monotonic_and_budget_bounded() {
        let budgets = [
            OuroborosFaceMeshBudget {
                projected_face_radius_px: 12,
                max_vertices: 1_000,
                max_indices: 6_000,
                max_buffer_bytes: 80_000,
            },
            OuroborosFaceMeshBudget {
                projected_face_radius_px: 32,
                max_vertices: 20_000,
                max_indices: 120_000,
                max_buffer_bytes: 1_000_000,
            },
            OuroborosFaceMeshBudget {
                projected_face_radius_px: 64,
                max_vertices: 50_000,
                max_indices: 300_000,
                max_buffer_bytes: 2_500_000,
            },
        ];
        let mut previous_product = 0;
        for budget in budgets {
            let first = OuroborosFaceMeshConfig::plan_for_budget(budget).unwrap();
            let second = OuroborosFaceMeshConfig::plan_for_budget(budget).unwrap();
            assert_eq!(first, second);
            assert!(first.config.latitude_segments <= first.requested.latitude_segments);
            assert!(first.config.longitude_segments <= first.requested.longitude_segments);
            assert!(first.vertex_count <= budget.max_vertices);
            assert!(first.index_count <= budget.max_indices);
            assert!(first.estimated_buffer_bytes <= budget.max_buffer_bytes);
            let product = first.config.latitude_segments * first.config.longitude_segments;
            assert!(product >= previous_product);
            previous_product = product;
            OuroborosFaceMeshConfig::validate_plan(first).unwrap();
        }
    }

    #[test]
    fn face_lod_planner_rejects_infeasible_minimum_budget_and_tampered_plan() {
        assert_eq!(
            OuroborosFaceMeshConfig::plan_for_budget(OuroborosFaceMeshBudget {
                projected_face_radius_px: 32,
                max_vertices: 10,
                max_indices: 10,
                max_buffer_bytes: 10,
            }),
            Err(OuroborosFaceMeshError::BudgetTooSmall)
        );

        let mut plan = OuroborosFaceMeshConfig::plan_for_budget(OuroborosFaceMeshBudget {
            projected_face_radius_px: 24,
            max_vertices: MAX_FACE_VERTICES as u32,
            max_indices: MAX_FACE_INDICES as u32,
            max_buffer_bytes: MAX_FACE_BUFFER_BYTES as u32,
        })
        .unwrap();
        plan.estimated_buffer_bytes -= 1;
        assert_eq!(
            OuroborosFaceMeshConfig::validate_plan(plan),
            Err(OuroborosFaceMeshError::InvalidConfiguration)
        );
    }

    #[test]
    fn face_mesh_build_uses_the_exact_budgeted_plan() {
        let scene = scene(OuroborosForm::Humanity);
        let budget = OuroborosFaceMeshBudget {
            projected_face_radius_px: 48,
            max_vertices: 30_000,
            max_indices: 180_000,
            max_buffer_bytes: 1_500_000,
        };
        let plan = OuroborosFaceMeshConfig::plan_for_budget(budget).unwrap();
        let (mesh, receipt) =
            OuroborosFaceMesh::build_with_plan(&scene, scene.landmarks(), plan).unwrap();
        assert_eq!(mesh.config, plan.config);
        assert_eq!(receipt.vertex_count, plan.vertex_count);
        assert_eq!(receipt.index_count, plan.index_count);
    }

    #[test]
    fn face_mesh_estimated_buffers_stay_under_explicit_resource_ceilings() {
        for config in [
            OuroborosFaceMeshConfig::default(),
            OuroborosFaceMeshConfig {
                latitude_segments: 64,
                longitude_segments: 128,
            },
        ] {
            let (vertices, indices, bytes) = face_resource_estimate(config).unwrap();
            assert!(vertices <= MAX_FACE_VERTICES);
            assert!(indices <= MAX_FACE_INDICES);
            assert!(bytes <= MAX_FACE_BUFFER_BYTES);
        }
    }

    #[test]
    fn face_mesh_is_deterministic_closed_and_receipt_bound() {
        let scene = scene(OuroborosForm::Nature);
        let landmarks = scene.landmarks();
        let (mesh, receipt) =
            OuroborosFaceMesh::build(&scene, landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();
        let (repeat, repeat_receipt) =
            OuroborosFaceMesh::build(&scene, landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();

        assert_eq!(mesh, repeat);
        assert_eq!(receipt, repeat_receipt);
        assert_eq!(receipt.digest, *blake3::hash(&mesh.canonical_bytes().unwrap()).as_bytes());
        assert_eq!(receipt.landmark_digest, landmarks.receipt().digest);
        assert_eq!(receipt.schema_version, FACE_SCHEMA_VERSION);
        mesh.validate(&scene).unwrap();
    }

    #[test]
    fn face_mesh_preserves_semantic_attachments_and_stable_regions_across_forms() {
        let mut expected_indices = None;
        let mut expected_regions = None;
        for form in OuroborosForm::ALL {
            let scene = scene(form);
            let landmarks = scene.landmarks();
            let (mesh, _) =
                OuroborosFaceMesh::build(&scene, landmarks, OuroborosFaceMeshConfig::default())
                    .unwrap();
            assert_eq!(mesh.landmarks, landmarks);
            let regions: Vec<u32> = mesh.vertices.iter().map(|vertex| vertex.region_id).collect();
            if let Some(indices) = &expected_indices {
                assert_eq!(&mesh.indices, indices);
                assert_eq!(Some(&regions), expected_regions.as_ref());
            } else {
                expected_indices = Some(mesh.indices.clone());
                expected_regions = Some(regions);
            }
        }
    }

    #[test]
    fn pose_rotates_face_geometry_without_moving_the_root() {
        let neutral = scene(OuroborosForm::Humanity);
        let mut turned = scene(OuroborosForm::Humanity);
        turned
            .set_attention_target(crate::ouroboros::AttentionTarget::new(1.0, -1.0).unwrap())
            .unwrap();
        turned.advance(0.25).unwrap();

        let neutral_landmarks = neutral.landmarks();
        let turned_landmarks = turned.landmarks();
        let (neutral_mesh, neutral_receipt) =
            OuroborosFaceMesh::build(&neutral, neutral_landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();
        let (turned_mesh, turned_receipt) =
            OuroborosFaceMesh::build(&turned, turned_landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();
        assert_eq!(neutral_landmarks.head_anchor, turned_landmarks.head_anchor);
        assert_ne!(neutral_receipt.digest, turned_receipt.digest);
        assert_ne!(neutral_mesh.vertices, turned_mesh.vertices);
        assert_eq!(
            neutral_mesh.landmarks.head_anchor,
            turned_mesh.landmarks.head_anchor
        );
    }

    #[test]
    fn mesh_regions_keep_exact_landmark_centers() {
        let scene = scene(OuroborosForm::Enlightenment);
        let landmarks = scene.landmarks();
        let (mesh, _) =
            OuroborosFaceMesh::build(&scene, landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();
        assert_eq!(mesh.landmarks.left_eye, landmarks.left_eye);
        assert_eq!(mesh.landmarks.right_eye, landmarks.right_eye);
        assert_eq!(mesh.landmarks.nose_tip, landmarks.nose_tip);
        assert_eq!(mesh.landmarks.mouth_center, landmarks.mouth_center);
        assert_eq!(mesh.landmarks.crown, landmarks.crown);
        assert_eq!(mesh.landmarks.chin, landmarks.chin);
    }

    #[test]
    fn lower_lod_preserves_attachment_contract_and_allows_bounded_geometry() {
        let scene = scene(OuroborosForm::Nature);
        let landmarks = scene.landmarks();
        let high = OuroborosFaceMesh::build(
            &scene,
            landmarks,
            OuroborosFaceMeshConfig {
                latitude_segments: 32,
                longitude_segments: 64,
            },
        )
        .unwrap()
        .0;
        let low = OuroborosFaceMesh::build(
            &scene,
            landmarks,
            OuroborosFaceMeshConfig {
                latitude_segments: 8,
                longitude_segments: 16,
            },
        )
        .unwrap()
        .0;
        assert!(low.vertices.len() < high.vertices.len());
        assert!(low.indices.len() < high.indices.len());
        assert_eq!(low.landmarks, high.landmarks);
        assert_eq!(
            low.vertices
                .iter()
                .filter(|vertex| vertex.region_id == FACE_REGION_HEAD)
                .map(|vertex| vertex.region_id)
                .collect::<std::collections::BTreeSet<_>>(),
            [FACE_REGION_HEAD].into_iter().collect()
        );
    }

    #[test]
    fn face_mesh_rejects_hostile_configs_and_mutated_public_geometry() {
        let scene = scene(OuroborosForm::Digital);
        let landmarks = scene.landmarks();
        assert_eq!(
            OuroborosFaceMesh::build(
                &scene,
                landmarks,
                OuroborosFaceMeshConfig {
                    latitude_segments: u32::MAX,
                    longitude_segments: 48,
                }
            )
            .unwrap_err(),
            OuroborosFaceMeshError::InvalidConfiguration
        );

        let (mut mesh, _) =
            OuroborosFaceMesh::build(&scene, landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();
        mesh.indices[0] = u32::MAX;
        assert_eq!(
            mesh.validate(&scene),
            Err(OuroborosFaceMeshError::InvalidGeneratedGeometry)
        );

        let (mut mesh, _) =
            OuroborosFaceMesh::build(&scene, landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();
        mesh.vertices[0].position[0] = f32::NAN;
        assert_eq!(
            mesh.validate(&scene),
            Err(OuroborosFaceMeshError::InvalidGeneratedGeometry)
        );

        let (mut mesh, _) =
            OuroborosFaceMesh::build(&scene, landmarks, OuroborosFaceMeshConfig::default())
                .unwrap();
        mesh.vertices[1].position[0] += 0.01;
        assert_eq!(
            mesh.validate(&scene),
            Err(OuroborosFaceMeshError::InvalidGeneratedGeometry)
        );
    }
}
