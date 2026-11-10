#![forbid(unsafe_code)]
//! Bounded deterministic topology/semantic audit for triangle meshes.
//!
//! This crate intentionally does not claim robust triangle-triangle
//! intersection detection. That independent geometric-embedding check is
//! reported as NotEvaluated and is planned against a pinned CGAL oracle.

use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    error::Error,
    fmt,
};

pub const AUDIT_SCHEMA_VERSION: u16 = 1;
pub const AUDIT_MAGIC: [u8; 8] = *b"OUROMAU1";
pub const MAX_VERTICES: usize = 100_000;
pub const MAX_FACES: usize = 200_000;
pub const MAX_INPUT_BYTES: u64 = 32 * 1024 * 1024;
pub const MAX_WORKING_BYTES: u64 = 96 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuditLimits {
    pub max_vertices: usize,
    pub max_faces: usize,
    pub max_input_bytes: u64,
    /// Estimated audit working payload; excludes allocator metadata and RSS.
    pub max_working_bytes: u64,
    /// Epsilon for the squared magnitude of the triangle cross product.
    pub near_degenerate_cross_squared_epsilon: f64,
}

impl Default for AuditLimits {
    fn default() -> Self {
        Self {
            max_vertices: MAX_VERTICES,
            max_faces: MAX_FACES,
            max_input_bytes: MAX_INPUT_BYTES,
            max_working_bytes: MAX_WORKING_BYTES,
            near_degenerate_cross_squared_epsilon: 1.0e-24,
        }
    }
}

/// One source-feature bit per stable feature ID (bit 0 = feature 1).
/// More than one bit records a blended/provenance boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct SemanticTriangleMesh {
    pub vertices: Vec<[f64; 3]>,
    pub vertex_feature_masks: Vec<u32>,
    pub faces: Vec<[u32; 3]>,
    pub face_region_ids: Vec<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum CheckStatus {
    Pass = 1,
    Fail = 2,
    NotEvaluated = 3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditChecks {
    pub buffer_validity: CheckStatus,
    pub topology_manifold: CheckStatus,
    pub closed_surface: CheckStatus,
    pub orientation_consistency: CheckStatus,
    pub exact_degeneracy: CheckStatus,
    pub near_degeneracy: CheckStatus,
    pub semantic_provenance: CheckStatus,
    /// Not evaluated by this topology-only implementation.
    pub self_intersections: CheckStatus,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ComponentSummary {
    pub first_face: u32,
    pub vertices: u32,
    pub edges: u32,
    pub faces: u32,
    pub euler_characteristic: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshAuditReport {
    pub checks: AuditChecks,
    pub vertex_count: u32,
    pub face_count: u32,
    pub unique_edge_count: u32,
    pub boundary_edge_count: u32,
    pub manifold_edge_count: u32,
    pub non_manifold_edge_count: u32,
    pub inconsistent_oriented_edge_count: u32,
    pub non_manifold_vertex_count: u32,
    pub disconnected_vertex_link_count: u32,
    pub exact_degenerate_triangle_count: u32,
    pub near_degenerate_triangle_count: u32,
    pub coincident_position_group_count: u32,
    pub coincident_extra_vertex_count: u32,
    pub orphan_vertex_count: u32,
    pub unique_vertex_feature_mask_count: u32,
    pub unique_face_region_count: u32,
    pub components: Vec<ComponentSummary>,
    pub estimated_input_bytes: u64,
    pub estimated_working_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuditError {
    EmptyMesh,
    VertexLimitExceeded,
    FaceLimitExceeded,
    InputBudgetExceeded,
    WorkingBudgetExceeded,
    SizeOverflow,
    InvalidLimits,
    NonFiniteVertex,
    InvalidVertexFeatureMask,
    InvalidFaceRegion,
    FaceIndexOutOfBounds,
    RepeatedFaceIndex,
}

impl fmt::Display for AuditError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Error for AuditError {}

#[derive(Clone, Copy, Debug)]
struct EdgeUse {
    face: usize,
    forward: bool,
}

#[derive(Clone, Copy, Debug)]
struct EdgeStats {
    boundary: u32,
    manifold: u32,
    non_manifold: u32,
    inconsistent: u32,
}

pub fn audit_mesh(
    mesh: &SemanticTriangleMesh,
    limits: AuditLimits,
) -> Result<MeshAuditReport, AuditError> {
    let (estimated_input_bytes, estimated_working_bytes) =
        validate_input_before_workspace(mesh, limits)?;

    let vertex_count = mesh.vertices.len();
    let face_count = mesh.faces.len();
    let mut edges: BTreeMap<(u32, u32), Vec<EdgeUse>> = BTreeMap::new();
    let mut vertex_link_edges: Vec<Vec<(u32, u32)>> =
        (0..vertex_count).map(|_| Vec::new()).collect();
    let mut used_vertices = vec![false; vertex_count];
    let mut exact_degenerate = 0_u32;
    let mut near_degenerate = 0_u32;

    for (face_index, face) in mesh.faces.iter().enumerate() {
        let [a, b, c] = *face;
        let ia = a as usize;
        let ib = b as usize;
        let ic = c as usize;
        used_vertices[ia] = true;
        used_vertices[ib] = true;
        used_vertices[ic] = true;

        let pa = mesh.vertices[ia];
        let pb = mesh.vertices[ib];
        let pc = mesh.vertices[ic];
        let cross = cross(sub(pb, pa), sub(pc, pa));
        let cross_squared = dot(cross, cross);
        if cross_squared == 0.0 {
            exact_degenerate = exact_degenerate.saturating_add(1);
        } else if cross_squared <= limits.near_degenerate_cross_squared_epsilon {
            near_degenerate = near_degenerate.saturating_add(1);
        }

        add_edge(&mut edges, a, b, face_index);
        add_edge(&mut edges, b, c, face_index);
        add_edge(&mut edges, c, a, face_index);

        vertex_link_edges[ia].push((b, c));
        vertex_link_edges[ib].push((c, a));
        vertex_link_edges[ic].push((a, b));
    }

    let edge_stats = classify_edges(&edges, vertex_count);
    let boundary_degree = boundary_degrees(&edges, vertex_count);
    let (non_manifold_vertex_count, disconnected_vertex_link_count) =
        validate_vertex_links(&vertex_link_edges, &boundary_degree);

    let orphan_vertex_count = used_vertices.iter().filter(|used| !**used).count();
    let (coincident_position_group_count, coincident_extra_vertex_count) =
        count_coincident_positions(&mesh.vertices);
    let components = summarize_components(mesh, &edges);

    let unique_vertex_feature_masks: BTreeSet<u32> =
        mesh.vertex_feature_masks.iter().copied().collect();
    let unique_face_regions: BTreeSet<u32> = mesh.face_region_ids.iter().copied().collect();

    let topology_ok = edge_stats.non_manifold == 0 && non_manifold_vertex_count == 0;
    let orientation_ok = edge_stats.non_manifold == 0 && edge_stats.inconsistent == 0;
    let closed_ok = topology_ok && edge_stats.boundary == 0;
    let exact_ok = exact_degenerate == 0;
    let near_ok = near_degenerate == 0;
    let semantic_ok = mesh.vertex_feature_masks.iter().all(|mask| *mask != 0)
        && mesh.face_region_ids.iter().all(|region| *region != 0)
        && orphan_vertex_count == 0;

    Ok(MeshAuditReport {
        checks: AuditChecks {
            buffer_validity: CheckStatus::Pass,
            topology_manifold: status(topology_ok),
            closed_surface: status(closed_ok),
            orientation_consistency: status(orientation_ok),
            exact_degeneracy: status(exact_ok),
            near_degeneracy: status(near_ok),
            semantic_provenance: status(semantic_ok),
            self_intersections: CheckStatus::NotEvaluated,
        },
        vertex_count: to_u32(vertex_count)?,
        face_count: to_u32(face_count)?,
        unique_edge_count: to_u32(edges.len())?,
        boundary_edge_count: edge_stats.boundary,
        manifold_edge_count: edge_stats.manifold,
        non_manifold_edge_count: edge_stats.non_manifold,
        inconsistent_oriented_edge_count: edge_stats.inconsistent,
        non_manifold_vertex_count,
        disconnected_vertex_link_count,
        exact_degenerate_triangle_count: exact_degenerate,
        near_degenerate_triangle_count: near_degenerate,
        coincident_position_group_count,
        coincident_extra_vertex_count,
        orphan_vertex_count: to_u32(orphan_vertex_count)?,
        unique_vertex_feature_mask_count: to_u32(unique_vertex_feature_masks.len())?,
        unique_face_region_count: to_u32(unique_face_regions.len())?,
        components,
        estimated_input_bytes,
        estimated_working_bytes,
    })
}

fn validate_input_before_workspace(
    mesh: &SemanticTriangleMesh,
    limits: AuditLimits,
) -> Result<(u64, u64), AuditError> {
    if !limits.near_degenerate_cross_squared_epsilon.is_finite()
        || limits.near_degenerate_cross_squared_epsilon < 0.0
        || limits.max_vertices == 0
        || limits.max_faces == 0
        || limits.max_input_bytes == 0
        || limits.max_working_bytes == 0
    {
        return Err(AuditError::InvalidLimits);
    }
    if mesh.vertices.is_empty() || mesh.faces.is_empty() {
        return Err(AuditError::EmptyMesh);
    }
    if mesh.vertices.len() > MAX_VERTICES || mesh.vertices.len() > limits.max_vertices {
        return Err(AuditError::VertexLimitExceeded);
    }
    if mesh.faces.len() > MAX_FACES || mesh.faces.len() > limits.max_faces {
        return Err(AuditError::FaceLimitExceeded);
    }
    if mesh.vertex_feature_masks.len() != mesh.vertices.len()
        || mesh.face_region_ids.len() != mesh.faces.len()
    {
        return Err(AuditError::InvalidVertexFeatureMask);
    }

    let vertex_bytes = (mesh.vertices.len() as u64)
        .checked_mul(3 * 8 + 4)
        .ok_or(AuditError::SizeOverflow)?;
    let face_bytes = (mesh.faces.len() as u64)
        .checked_mul(3 * 4 + 4)
        .ok_or(AuditError::SizeOverflow)?;
    let input_bytes = vertex_bytes
        .checked_add(face_bytes)
        .ok_or(AuditError::SizeOverflow)?;
    if input_bytes > MAX_INPUT_BYTES || input_bytes > limits.max_input_bytes {
        return Err(AuditError::InputBudgetExceeded);
    }

    // Conservative payload estimate for edge records, three vertex-link edges
    // per face, vertex flags, DSU and sorted component sets. This is not RSS.
    let edge_records = (mesh.faces.len() as u64)
        .checked_mul(3)
        .and_then(|value| value.checked_mul(64))
        .ok_or(AuditError::SizeOverflow)?;
    let vertex_workspace = (mesh.vertices.len() as u64)
        .checked_mul(48)
        .ok_or(AuditError::SizeOverflow)?;
    let working_bytes = edge_records
        .checked_add(vertex_workspace)
        .ok_or(AuditError::SizeOverflow)?;
    if working_bytes > MAX_WORKING_BYTES || working_bytes > limits.max_working_bytes {
        return Err(AuditError::WorkingBudgetExceeded);
    }

    if mesh.vertices.iter().any(|point| !point.iter().all(|v| v.is_finite())) {
        return Err(AuditError::NonFiniteVertex);
    }
    if mesh.vertex_feature_masks.iter().any(|mask| *mask == 0) {
        return Err(AuditError::InvalidVertexFeatureMask);
    }
    if mesh.face_region_ids.iter().any(|region| *region == 0) {
        return Err(AuditError::InvalidFaceRegion);
    }

    for face in &mesh.faces {
        if face.iter().any(|index| *index as usize >= mesh.vertices.len()) {
            return Err(AuditError::FaceIndexOutOfBounds);
        }
        if face[0] == face[1] || face[1] == face[2] || face[2] == face[0] {
            return Err(AuditError::RepeatedFaceIndex);
        }
    }

    Ok((input_bytes, working_bytes))
}

fn add_edge(
    edges: &mut BTreeMap<(u32, u32), Vec<EdgeUse>>,
    from: u32,
    to: u32,
    face: usize,
) {
    let key = if from < to { (from, to) } else { (to, from) };
    let forward = from < to;
    edges.entry(key).or_default().push(EdgeUse { face, forward });
}

fn classify_edges(
    edges: &BTreeMap<(u32, u32), Vec<EdgeUse>>,
    vertex_count: usize,
) -> EdgeStats {
    let mut stats = EdgeStats {
        boundary: 0,
        manifold: 0,
        non_manifold: 0,
        inconsistent: 0,
    };
    let _ = vertex_count;
    for uses in edges.values() {
        match uses.len() {
            1 => stats.boundary += 1,
            2 => {
                stats.manifold += 1;
                if uses[0].forward == uses[1].forward {
                    stats.inconsistent += 1;
                }
            }
            _ => stats.non_manifold += 1,
        }
    }
    stats
}

fn boundary_degrees(
    edges: &BTreeMap<(u32, u32), Vec<EdgeUse>>,
    vertex_count: usize,
) -> Vec<u32> {
    let mut degree = vec![0_u32; vertex_count];
    for ((a, b), uses) in edges {
        if uses.len() == 1 {
            degree[*a as usize] += 1;
            degree[*b as usize] += 1;
        }
    }
    degree
}

fn validate_vertex_links(
    links: &[Vec<(u32, u32)>],
    boundary_degree: &[u32],
) -> (u32, u32) {
    let mut non_manifold = 0_u32;
    let mut disconnected = 0_u32;

    for (vertex_index, link_edges) in links.iter().enumerate() {
        if link_edges.is_empty() {
            continue;
        }
        let mut adjacency: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
        for (a, b) in link_edges {
            adjacency.entry(*a).or_default().push(*b);
            adjacency.entry(*b).or_default().push(*a);
        }

        let mut degree_one = 0_u32;
        let mut degree_two_only = true;
        for neighbours in adjacency.values() {
            match neighbours.len() {
                1 => degree_one += 1,
                2 => {}
                _ => degree_two_only = false,
            }
        }

        let boundary_edges = boundary_degree[vertex_index];
        let degree_shape_ok = match boundary_edges {
            0 => degree_two_only && degree_one == 0,
            2 => degree_two_only && degree_one == 2,
            _ => false,
        };
        if !degree_shape_ok {
            non_manifold += 1;
        }

        let start = *adjacency.keys().next().expect("non-empty link");
        let mut visited = BTreeSet::new();
        let mut queue = VecDeque::new();
        visited.insert(start);
        queue.push_back(start);
        while let Some(current) = queue.pop_front() {
            if let Some(neighbours) = adjacency.get(&current) {
                for neighbour in neighbours {
                    if visited.insert(*neighbour) {
                        queue.push_back(*neighbour);
                    }
                }
            }
        }
        if visited.len() != adjacency.len() {
            disconnected += 1;
            non_manifold += 1;
        }
    }

    (non_manifold, disconnected)
}

#[derive(Clone, Debug)]
struct DisjointSet {
    parent: Vec<usize>,
}

impl DisjointSet {
    fn new(count: usize) -> Self {
        Self {
            parent: (0..count).collect(),
        }
    }

    fn find(&mut self, value: usize) -> usize {
        if self.parent[value] != value {
            let root = self.find(self.parent[value]);
            self.parent[value] = root;
        }
        self.parent[value]
    }

    fn union(&mut self, a: usize, b: usize) {
        let root_a = self.find(a);
        let root_b = self.find(b);
        if root_a != root_b {
            let (low, high) = if root_a < root_b {
                (root_a, root_b)
            } else {
                (root_b, root_a)
            };
            self.parent[high] = low;
        }
    }
}

#[derive(Default)]
struct ComponentAccumulator {
    first_face: u32,
    vertices: BTreeSet<u32>,
    edges: BTreeSet<(u32, u32)>,
    faces: u32,
}

fn summarize_components(
    mesh: &SemanticTriangleMesh,
    edges: &BTreeMap<(u32, u32), Vec<EdgeUse>>,
) -> Vec<ComponentSummary> {
    let mut dsu = DisjointSet::new(mesh.faces.len());
    for uses in edges.values() {
        if let Some(first) = uses.first() {
            for other in uses.iter().skip(1) {
                dsu.union(first.face, other.face);
            }
        }
    }

    let mut accumulators: BTreeMap<usize, ComponentAccumulator> = BTreeMap::new();
    for (face_index, face) in mesh.faces.iter().enumerate() {
        let root = dsu.find(face_index);
        let entry = accumulators.entry(root).or_insert_with(|| ComponentAccumulator {
            first_face: face_index as u32,
            ..ComponentAccumulator::default()
        });
        entry.first_face = entry.first_face.min(face_index as u32);
        entry.faces += 1;
        let [a, b, c] = *face;
        entry.vertices.insert(a);
        entry.vertices.insert(b);
        entry.vertices.insert(c);
        entry.edges.insert(edge_key(a, b));
        entry.edges.insert(edge_key(b, c));
        entry.edges.insert(edge_key(c, a));
    }

    accumulators
        .into_values()
        .map(|component| ComponentSummary {
            first_face: component.first_face,
            vertices: component.vertices.len() as u32,
            edges: component.edges.len() as u32,
            faces: component.faces,
            euler_characteristic: component.vertices.len() as i64
                - component.edges.len() as i64
                + component.faces as i64,
        })
        .collect()
}

fn edge_key(a: u32, b: u32) -> (u32, u32) {
    if a < b { (a, b) } else { (b, a) }
}

fn count_coincident_positions(vertices: &[[f64; 3]]) -> (u32, u32) {
    let mut counts: BTreeMap<[u64; 3], u32> = BTreeMap::new();
    for point in vertices {
        let bits = point.map(|value| if value == 0.0 { 0 } else { value.to_bits() });
        *counts.entry(bits).or_default() += 1;
    }
    let groups = counts.values().filter(|count| **count > 1).count();
    let extras: u32 = counts
        .values()
        .filter(|count| **count > 1)
        .map(|count| count - 1)
        .sum();
    (groups as u32, extras)
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn status(condition: bool) -> CheckStatus {
    if condition { CheckStatus::Pass } else { CheckStatus::Fail }
}

fn to_u32(value: usize) -> Result<u32, AuditError> {
    u32::try_from(value).map_err(|_| AuditError::SizeOverflow)
}

impl MeshAuditReport {
    /// Canonical input-plus-result bytes. Hash externally; no digest algorithm
    /// is hidden inside this crate and no signature/authenticity is implied.
    pub fn canonical_bytes(
        &self,
        mesh: &SemanticTriangleMesh,
    ) -> Result<Vec<u8>, AuditError> {
        if mesh.vertices.len() != self.vertex_count as usize
            || mesh.faces.len() != self.face_count as usize
        {
            return Err(AuditError::SizeOverflow);
        }
        let input_bytes = self.estimated_input_bytes as usize;
        let mut out = Vec::new();
        out.try_reserve_exact(input_bytes.saturating_add(256))
            .map_err(|_| AuditError::WorkingBudgetExceeded)?;
        out.extend_from_slice(&AUDIT_MAGIC);
        put_u16(&mut out, AUDIT_SCHEMA_VERSION);
        put_u32(&mut out, self.vertex_count);
        put_u32(&mut out, self.face_count);
        for (vertex, mask) in mesh.vertices.iter().zip(&mesh.vertex_feature_masks) {
            for value in vertex {
                out.extend_from_slice(&value.to_bits().to_le_bytes());
            }
            put_u32(&mut out, *mask);
        }
        for (face, region) in mesh.faces.iter().zip(&mesh.face_region_ids) {
            for index in face {
                put_u32(&mut out, *index);
            }
            put_u32(&mut out, *region);
        }
        let statuses = [
            self.checks.buffer_validity,
            self.checks.topology_manifold,
            self.checks.closed_surface,
            self.checks.orientation_consistency,
            self.checks.exact_degeneracy,
            self.checks.near_degeneracy,
            self.checks.semantic_provenance,
            self.checks.self_intersections,
        ];
        for check in statuses {
            out.push(check as u8);
        }
        for value in [
            self.unique_edge_count,
            self.boundary_edge_count,
            self.manifold_edge_count,
            self.non_manifold_edge_count,
            self.inconsistent_oriented_edge_count,
            self.non_manifold_vertex_count,
            self.disconnected_vertex_link_count,
            self.exact_degenerate_triangle_count,
            self.near_degenerate_triangle_count,
            self.coincident_position_group_count,
            self.coincident_extra_vertex_count,
            self.orphan_vertex_count,
            self.unique_vertex_feature_mask_count,
            self.unique_face_region_count,
        ] {
            put_u32(&mut out, value);
        }
        put_u64(&mut out, self.estimated_input_bytes);
        put_u64(&mut out, self.estimated_working_bytes);
        put_u32(
            &mut out,
            u32::try_from(self.components.len()).map_err(|_| AuditError::SizeOverflow)?,
        );
        for component in &self.components {
            put_u32(&mut out, component.first_face);
            put_u32(&mut out, component.vertices);
            put_u32(&mut out, component.edges);
            put_u32(&mut out, component.faces);
            put_i64(&mut out, component.euler_characteristic);
        }
        Ok(out)
    }
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
fn put_i64(out: &mut Vec<u8>, value: i64) {
    out.extend_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tetrahedron() -> SemanticTriangleMesh {
        SemanticTriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            vertex_feature_masks: vec![1, 1, 1, 1],
            faces: vec![[0, 2, 1], [0, 1, 3], [0, 3, 2], [1, 2, 3]],
            face_region_ids: vec![1, 1, 1, 1],
        }
    }

    fn open_disk() -> SemanticTriangleMesh {
        SemanticTriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [1.0, 1.0, 0.0],
                [0.0, 1.0, 0.0],
            ],
            vertex_feature_masks: vec![1, 1, 1, 1],
            faces: vec![[0, 1, 2], [0, 2, 3]],
            face_region_ids: vec![1, 1],
        }
    }

    #[test]
    fn closed_tetrahedron_passes_topology_and_orientation_but_not_intersection_claim() {
        let mesh = tetrahedron();
        let report = audit_mesh(&mesh, AuditLimits::default()).unwrap();
        assert_eq!(report.checks.buffer_validity, CheckStatus::Pass);
        assert_eq!(report.checks.topology_manifold, CheckStatus::Pass);
        assert_eq!(report.checks.closed_surface, CheckStatus::Pass);
        assert_eq!(report.checks.orientation_consistency, CheckStatus::Pass);
        assert_eq!(report.checks.self_intersections, CheckStatus::NotEvaluated);
        assert_eq!(report.components.len(), 1);
        assert_eq!(report.components[0].euler_characteristic, 2);
    }

    #[test]
    fn open_disk_is_manifold_but_not_closed() {
        let report = audit_mesh(&open_disk(), AuditLimits::default()).unwrap();
        assert_eq!(report.checks.topology_manifold, CheckStatus::Pass);
        assert_eq!(report.checks.closed_surface, CheckStatus::Fail);
        assert_eq!(report.boundary_edge_count, 4);
    }

    #[test]
    fn disjoint_tetrahedra_are_reported_as_two_components() {
        let a = tetrahedron();
        let mut b = tetrahedron();
        let offset = a.vertices.len() as u32;
        for point in &mut b.vertices {
            point[0] += 4.0;
        }
        let mut mesh = a.clone();
        mesh.vertices.extend(b.vertices);
        mesh.vertex_feature_masks.extend(b.vertex_feature_masks);
        mesh.faces.extend(
            b.faces
                .into_iter()
                .map(|face| face.map(|index| index + offset)),
        );
        mesh.face_region_ids.extend(b.face_region_ids);
        let report = audit_mesh(&mesh, AuditLimits::default()).unwrap();
        assert_eq!(report.components.len(), 2);
        assert!(report.components.iter().all(|component| component.euler_characteristic == 2));
    }

    #[test]
    fn pinched_vertex_with_two_disconnected_link_cycles_fails_manifoldness() {
        let mut a = tetrahedron();
        let b = tetrahedron();
        a.vertices.extend(b.vertices.into_iter().skip(1).map(|mut p| {
            p[0] += 3.0;
            p
        }));
        a.vertex_feature_masks.extend(vec![1; 3]);
        let map = |index: u32| if index == 0 { 0 } else { index + 3 };
        a.faces.extend(b.faces.into_iter().map(|face| face.map(map)));
        a.face_region_ids.extend(vec![1; 4]);
        let report = audit_mesh(&a, AuditLimits::default()).unwrap();
        assert_eq!(report.checks.topology_manifold, CheckStatus::Fail);
        assert_eq!(report.disconnected_vertex_link_count, 1);
    }

    #[test]
    fn three_faces_incident_to_one_edge_are_non_manifold() {
        let mesh = SemanticTriangleMesh {
            vertices: vec![
                [0.0, 0.0, 0.0],
                [1.0, 0.0, 0.0],
                [0.0, 1.0, 0.0],
                [0.0, -1.0, 0.0],
                [0.0, 0.0, 1.0],
            ],
            vertex_feature_masks: vec![1; 5],
            faces: vec![[0, 1, 2], [1, 0, 3], [0, 1, 4]],
            face_region_ids: vec![1; 3],
        };
        let report = audit_mesh(&mesh, AuditLimits::default()).unwrap();
        assert_eq!(report.non_manifold_edge_count, 1);
        assert_eq!(report.checks.topology_manifold, CheckStatus::Fail);
    }

    #[test]
    fn inconsistent_shared_edge_direction_fails_orientation_check() {
        let mut mesh = open_disk();
        mesh.faces = vec![[0, 1, 2], [0, 3, 2]];
        let report = audit_mesh(&mesh, AuditLimits::default()).unwrap();
        assert_eq!(report.inconsistent_oriented_edge_count, 1);
        assert_eq!(report.checks.orientation_consistency, CheckStatus::Fail);
    }

    #[test]
    fn zero_area_and_near_degenerate_faces_are_separate_checks() {
        let mut mesh = open_disk();
        mesh.vertices[2] = [2.0, 0.0, 0.0];
        mesh.vertices[3] = [0.0, 1.0e-13, 0.0];
        let limits = AuditLimits {
            near_degenerate_cross_squared_epsilon: 1.0e-24,
            ..AuditLimits::default()
        };
        let report = audit_mesh(&mesh, limits).unwrap();
        assert!(report.exact_degenerate_triangle_count >= 1);
        assert!(report.near_degenerate_triangle_count >= 1);
        assert_eq!(report.checks.exact_degeneracy, CheckStatus::Fail);
        assert_eq!(report.checks.near_degeneracy, CheckStatus::Fail);
    }

    #[test]
    fn coincident_position_vertices_are_reported_even_when_indices_differ() {
        let mut mesh = open_disk();
        mesh.vertices.push(mesh.vertices[0]);
        mesh.vertex_feature_masks.push(1);
        let report = audit_mesh(&mesh, AuditLimits::default()).unwrap();
        assert_eq!(report.coincident_position_group_count, 1);
        assert_eq!(report.coincident_extra_vertex_count, 1);
        assert_eq!(report.orphan_vertex_count, 1);
        assert_eq!(report.checks.semantic_provenance, CheckStatus::Fail);
    }

    #[test]
    fn zero_feature_masks_and_zero_region_ids_fail_before_topology_workspace() {
        let mut mesh = open_disk();
        mesh.vertex_feature_masks[0] = 0;
        assert_eq!(
            audit_mesh(&mesh, AuditLimits::default()),
            Err(AuditError::InvalidVertexFeatureMask)
        );
        let mut mesh = open_disk();
        mesh.face_region_ids[0] = 0;
        assert_eq!(
            audit_mesh(&mesh, AuditLimits::default()),
            Err(AuditError::InvalidFaceRegion)
        );
    }

    #[test]
    fn invalid_indices_repeated_indices_and_non_finite_coordinates_reject() {
        let mut mesh = open_disk();
        mesh.faces[0] = [0, 1, 99];
        assert_eq!(
            audit_mesh(&mesh, AuditLimits::default()),
            Err(AuditError::FaceIndexOutOfBounds)
        );
        let mut mesh = open_disk();
        mesh.faces[0] = [0, 1, 1];
        assert_eq!(
            audit_mesh(&mesh, AuditLimits::default()),
            Err(AuditError::RepeatedFaceIndex)
        );
        let mut mesh = open_disk();
        mesh.vertices[0][0] = f64::NAN;
        assert_eq!(
            audit_mesh(&mesh, AuditLimits::default()),
            Err(AuditError::NonFiniteVertex)
        );
    }

    #[test]
    fn budgets_are_checked_before_topology_construction() {
        let mesh = open_disk();
        let limits = AuditLimits {
            max_working_bytes: 1,
            ..AuditLimits::default()
        };
        assert_eq!(
            audit_mesh(&mesh, limits),
            Err(AuditError::WorkingBudgetExceeded)
        );
        let limits = AuditLimits {
            max_input_bytes: 1,
            ..AuditLimits::default()
        };
        assert_eq!(
            audit_mesh(&mesh, limits),
            Err(AuditError::InputBudgetExceeded)
        );
    }

    #[test]
    fn audit_receipt_is_deterministic_and_binds_input_bytes() {
        let mesh = tetrahedron();
        let first = audit_mesh(&mesh, AuditLimits::default()).unwrap();
        let second = audit_mesh(&mesh, AuditLimits::default()).unwrap();
        assert_eq!(
            first.canonical_bytes(&mesh).unwrap(),
            second.canonical_bytes(&mesh).unwrap()
        );
        assert!(first.canonical_bytes(&mesh).unwrap().starts_with(&AUDIT_MAGIC));
        let mut changed = mesh.clone();
        changed.vertices[0][0] = 0.25;
        let changed_report = audit_mesh(&changed, AuditLimits::default()).unwrap();
        assert_ne!(
            first.canonical_bytes(&mesh).unwrap(),
            changed_report.canonical_bytes(&changed).unwrap()
        );
    }
}
