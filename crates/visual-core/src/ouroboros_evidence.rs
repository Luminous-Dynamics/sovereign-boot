//! Canonical evidence bundle joining one Ouroboros replay, checkpoint,
//! geometry plan, validated mesh and deterministic CPU reference capture.
//!
//! This is an evidence sealer, not a renderer host. It has no OS, GPU,
//! filesystem, display, camera or network authority. The evidence root makes
//! independent artifacts comparable without claiming that a GPU/desktop
//! adapter has been qualified.

use std::{error::Error, fmt};

use crate::{
    contract::SCENE_CONTRACT_VERSION,
    ouroboros::{OuroborosCheckpointError, OuroborosCheckpointReceipt},
    ouroboros_mesh::{
        OuroborosMesh, OuroborosMeshConfig, OuroborosMeshError, OuroborosMeshPlan,
        OuroborosMeshReceipt, OuroborosQualityTier,
    },
    ouroboros_render::{self, OuroborosRenderError, ReferenceImageReceipt},
    ouroboros_trace::{OuroborosTrace, ReplayReceipt, TraceError},
};

const EVIDENCE_MAGIC: [u8; 8] = *b"OUROEVB1";
const EVIDENCE_VERSION: u16 = 1;

/// Failure while assembling a canonical visual-evidence bundle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosEvidenceError {
    Trace(TraceError),
    Mesh(OuroborosMeshError),
    Render(OuroborosRenderError),
    Checkpoint(OuroborosCheckpointError),
    InvalidPlan,
    SizeOverflow,
}

impl fmt::Display for OuroborosEvidenceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Trace(error) => write!(f, "Ouroboros evidence trace failed: {error}"),
            Self::Mesh(error) => write!(f, "Ouroboros evidence mesh failed: {error}"),
            Self::Render(error) => write!(f, "Ouroboros evidence image failed: {error}"),
            Self::Checkpoint(error) => {
                write!(f, "Ouroboros evidence checkpoint failed: {error}")
            }
            Self::InvalidPlan => write!(f, "Ouroboros evidence mesh plan is inconsistent"),
            Self::SizeOverflow => write!(f, "Ouroboros evidence size arithmetic overflowed"),
        }
    }
}

impl Error for OuroborosEvidenceError {}

impl From<TraceError> for OuroborosEvidenceError {
    fn from(value: TraceError) -> Self {
        Self::Trace(value)
    }
}

impl From<OuroborosMeshError> for OuroborosEvidenceError {
    fn from(value: OuroborosMeshError) -> Self {
        Self::Mesh(value)
    }
}

impl From<OuroborosRenderError> for OuroborosEvidenceError {
    fn from(value: OuroborosRenderError) -> Self {
        Self::Render(value)
    }
}

impl From<OuroborosCheckpointError> for OuroborosEvidenceError {
    fn from(value: OuroborosCheckpointError) -> Self {
        Self::Checkpoint(value)
    }
}

/// One machine-readable root over all deterministic CPU evidence artifacts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosEvidenceReceipt {
    pub evidence_version: u16,
    pub scene_contract_version: u16,
    pub replay: ReplayReceipt,
    pub checkpoint: OuroborosCheckpointReceipt,
    pub mesh: OuroborosMeshReceipt,
    pub image: ReferenceImageReceipt,
    pub selected_tier: OuroborosQualityTier,
    pub requested_ring_segments: u32,
    pub requested_tube_segments: u32,
    pub selected_ring_segments: u32,
    pub selected_tube_segments: u32,
    pub estimated_buffer_bytes: u32,
    /// BLAKE3 digest of the canonical evidence bundle bytes.
    pub bundle_digest: [u8; 32],
}

/// All deterministic artifacts sealed by one evidence run.
///
/// pixels is the actual RGBA8 image. bundle_bytes contains the canonical
/// metadata and sub-receipts; receipt.bundle_digest hashes those bytes.
/// This bundle is content-addressed, not signed or authenticated.
#[derive(Debug, Clone, PartialEq)]
pub struct OuroborosEvidenceArtifact {
    pub pixels: Vec<u8>,
    pub bundle_bytes: Vec<u8>,
    pub receipt: OuroborosEvidenceReceipt,
}

/// Replay, checkpoint, mesh and capture one trace under a previously selected
/// host budget. The function validates that the plan fits the generated mesh
/// and binds all artifact identities into one deterministic evidence root.
pub fn capture_ouroboros_evidence(
    trace: &OuroborosTrace,
    plan: OuroborosMeshPlan,
    width: u32,
    height: u32,
) -> Result<OuroborosEvidenceArtifact, OuroborosEvidenceError> {
    validate_plan_shape(plan)?;

    let replay = trace.replay()?;
    if replay.receipt.scene_contract_version != SCENE_CONTRACT_VERSION {
        return Err(OuroborosEvidenceError::InvalidPlan);
    }

    let (checkpoint_bytes, checkpoint_receipt) = replay.scene.encode_checkpoint()?;
    if checkpoint_bytes.len() > u16::MAX as usize {
        return Err(OuroborosEvidenceError::SizeOverflow);
    }

    let (mesh, mesh_receipt) = OuroborosMesh::build(&replay.scene, plan.config)?;
    mesh.validate()?;
    let expected_bytes = mesh_buffer_bytes(mesh_receipt.vertex_count, mesh_receipt.index_count)?;
    if mesh.config != plan.config
        || mesh_receipt.vertex_count != plan.vertex_count
        || mesh_receipt.index_count != plan.index_count
        || expected_bytes != plan.estimated_buffer_bytes
        || mesh_receipt.scene_contract_version != replay.receipt.scene_contract_version
    {
        return Err(OuroborosEvidenceError::InvalidPlan);
    }

    let (pixels, image_receipt) =
        ouroboros_render::render_reference_artifact(&replay.scene, width, height)?;
    let expected_pixel_bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or(OuroborosEvidenceError::SizeOverflow)?;
    if pixels.len() != expected_pixel_bytes
        || image_receipt.scene_contract_version != replay.receipt.scene_contract_version
    {
        return Err(OuroborosEvidenceError::InvalidPlan);
    }

    let bundle_bytes = encode_bundle(
        replay.receipt,
        checkpoint_receipt,
        mesh_receipt,
        image_receipt,
        plan,
    );
    let receipt = OuroborosEvidenceReceipt {
        evidence_version: EVIDENCE_VERSION,
        scene_contract_version: SCENE_CONTRACT_VERSION,
        replay: replay.receipt,
        checkpoint: checkpoint_receipt,
        mesh: mesh_receipt,
        image: image_receipt,
        selected_tier: plan.tier,
        requested_ring_segments: plan.requested_ring_segments,
        requested_tube_segments: plan.requested_tube_segments,
        selected_ring_segments: plan.config.ring_segments,
        selected_tube_segments: plan.config.tube_segments,
        estimated_buffer_bytes: plan.estimated_buffer_bytes,
        bundle_digest: *blake3::hash(&bundle_bytes).as_bytes(),
    };

    Ok(OuroborosEvidenceArtifact {
        pixels,
        bundle_bytes,
        receipt,
    })
}

fn validate_plan_shape(plan: OuroborosMeshPlan) -> Result<(), OuroborosEvidenceError> {
    let config = plan.config;
    if !(32..=2048).contains(&config.ring_segments)
        || !(6..=64).contains(&config.tube_segments)
        || !config.tube_radius.is_finite()
        || !(0.001..=0.08).contains(&config.tube_radius)
        || plan.requested_ring_segments < config.ring_segments
        || plan.requested_tube_segments < config.tube_segments
        || plan.requested_ring_segments > 2048
        || plan.requested_tube_segments > 64
    {
        return Err(OuroborosEvidenceError::InvalidPlan);
    }

    let expected_tier = if config.ring_segments <= 128 && config.tube_segments <= 8 {
        OuroborosQualityTier::Economy
    } else if config.ring_segments <= 512 && config.tube_segments <= 24 {
        OuroborosQualityTier::Balanced
    } else {
        OuroborosQualityTier::Cinematic
    };
    if plan.tier != expected_tier {
        return Err(OuroborosEvidenceError::InvalidPlan);
    }
    let expected = mesh_buffer_bytes(plan.vertex_count, plan.index_count)?;
    if expected != plan.estimated_buffer_bytes {
        return Err(OuroborosEvidenceError::InvalidPlan);
    }
    Ok(())
}

fn mesh_buffer_bytes(vertices: u32, indices: u32) -> Result<u32, OuroborosEvidenceError> {
    let vertex_bytes = vertices
        .checked_mul(48)
        .ok_or(OuroborosEvidenceError::SizeOverflow)?;
    let index_bytes = indices
        .checked_mul(4)
        .ok_or(OuroborosEvidenceError::SizeOverflow)?;
    vertex_bytes
        .checked_add(index_bytes)
        .ok_or(OuroborosEvidenceError::SizeOverflow)
}

fn encode_bundle(
    replay: ReplayReceipt,
    checkpoint: OuroborosCheckpointReceipt,
    mesh: OuroborosMeshReceipt,
    image: ReferenceImageReceipt,
    plan: OuroborosMeshPlan,
) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(256);
    bytes.extend_from_slice(&EVIDENCE_MAGIC);
    bytes.extend_from_slice(&EVIDENCE_VERSION.to_le_bytes());
    bytes.extend_from_slice(&SCENE_CONTRACT_VERSION.to_le_bytes());

    bytes.extend_from_slice(&replay.trace_version.to_le_bytes());
    bytes.extend_from_slice(&replay.scene_contract_version.to_le_bytes());
    bytes.extend_from_slice(&replay.event_count.to_le_bytes());
    bytes.extend_from_slice(&replay.trace_digest);
    bytes.extend_from_slice(&replay.final_state_digest);

    bytes.extend_from_slice(&checkpoint.schema_version.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.scene_contract_version.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.byte_length.to_le_bytes());
    bytes.extend_from_slice(&checkpoint.digest);

    bytes.extend_from_slice(&mesh.scene_contract_version.to_le_bytes());
    bytes.extend_from_slice(&mesh.vertex_count.to_le_bytes());
    bytes.extend_from_slice(&mesh.index_count.to_le_bytes());
    bytes.extend_from_slice(&mesh.digest);

    bytes.extend_from_slice(&image.scene_contract_version.to_le_bytes());
    bytes.extend_from_slice(&image.width.to_le_bytes());
    bytes.extend_from_slice(&image.height.to_le_bytes());
    bytes.extend_from_slice(&image.pixel_digest);

    bytes.push(match plan.tier {
        OuroborosQualityTier::Economy => 0,
        OuroborosQualityTier::Balanced => 1,
        OuroborosQualityTier::Cinematic => 2,
    });
    bytes.extend_from_slice(&plan.requested_ring_segments.to_le_bytes());
    bytes.extend_from_slice(&plan.requested_tube_segments.to_le_bytes());
    bytes.extend_from_slice(&plan.config.ring_segments.to_le_bytes());
    bytes.extend_from_slice(&plan.config.tube_segments.to_le_bytes());
    bytes.extend_from_slice(&plan.config.tube_radius.to_bits().to_le_bytes());
    bytes.extend_from_slice(&plan.estimated_buffer_bytes.to_le_bytes());
    bytes
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        ouroboros::{OuroborosConfig, OuroborosForm},
        ouroboros_mesh::{OuroborosMeshBudget, OuroborosQualityTier},
        ouroboros_trace::{OuroborosEvent, OuroborosMotionMode},
    };

    fn trace() -> OuroborosTrace {
        let mut trace = OuroborosTrace::new(OuroborosConfig::default(), OuroborosForm::Nature);
        trace
            .push(OuroborosEvent::TransitionTo {
                form: OuroborosForm::Digital,
            })
            .unwrap();
        for _ in 0..48 {
            trace
                .push(OuroborosEvent::Advance {
                    dt_seconds: 1.0 / 60.0,
                })
                .unwrap();
        }
        trace
            .push(OuroborosEvent::SetMotionMode {
                mode: OuroborosMotionMode::Balanced,
            })
            .unwrap();
        trace
    }

    fn plan() -> OuroborosMeshPlan {
        let scene = trace().replay().unwrap().scene;
        OuroborosMeshConfig::plan_for_budget(
            &scene,
            0.025,
            OuroborosMeshBudget {
                projected_major_radius_px: 160,
                projected_tube_radius_px: 8,
                max_vertices: 40_000,
                max_indices: 240_000,
                max_buffer_bytes: 2_000_000,
            },
        )
        .unwrap()
    }

    #[test]
    fn evidence_bundle_binds_replay_checkpoint_mesh_and_pixels() {
        let trace = trace();
        let plan = plan();
        let first = capture_ouroboros_evidence(&trace, plan, 320, 320).unwrap();
        let second = capture_ouroboros_evidence(&trace, plan, 320, 320).unwrap();

        assert_eq!(first, second);
        assert_eq!(
            first.receipt.bundle_digest,
            *blake3::hash(&first.bundle_bytes).as_bytes()
        );
        assert_eq!(
            first.receipt.checkpoint.digest,
            {
                let replay = trace.replay().unwrap();
                replay.scene.encode_checkpoint().unwrap().1.digest
            }
        );
        assert_eq!(first.receipt.mesh.vertex_count, plan.vertex_count);
        assert_eq!(first.receipt.mesh.index_count, plan.index_count);
        assert_eq!(first.receipt.image.width, 320);
        assert_eq!(first.receipt.image.height, 320);
        assert_eq!(
            first.receipt.image.pixel_digest,
            *blake3::hash(&first.pixels).as_bytes()
        );
        assert_eq!(first.bundle_bytes.len(), 225);
    }

    #[test]
    fn evidence_bundle_changes_when_the_canonical_trace_changes() {
        let source = trace();
        let first = capture_ouroboros_evidence(&source, plan(), 128, 128).unwrap();

        let mut changed = source;
        changed
            .push(OuroborosEvent::Advance {
                dt_seconds: 1.0 / 60.0,
            })
            .unwrap();
        let second = capture_ouroboros_evidence(&changed, plan(), 128, 128).unwrap();
        assert_ne!(first.receipt.bundle_digest, second.receipt.bundle_digest);
        assert_ne!(
            first.receipt.replay.final_state_digest,
            second.receipt.replay.final_state_digest
        );
    }

    #[test]
    fn evidence_sealer_rejects_a_tampered_mesh_plan() {
        let mut invalid = plan();
        invalid.estimated_buffer_bytes -= 1;
        assert_eq!(
            capture_ouroboros_evidence(&trace(), invalid, 128, 128),
            Err(OuroborosEvidenceError::InvalidPlan)
        );

        let mut invalid_tier = plan();
        invalid_tier.tier = OuroborosQualityTier::Economy;
        assert_eq!(
            capture_ouroboros_evidence(&trace(), invalid_tier, 128, 128),
            Err(OuroborosEvidenceError::InvalidPlan)
        );

        let mut invalid_config = plan();
        invalid_config.config.tube_radius = 0.0;
        assert_eq!(
            capture_ouroboros_evidence(&trace(), invalid_config, 128, 128),
            Err(OuroborosEvidenceError::InvalidPlan)
        );
    }

    #[test]
    fn evidence_sealer_rejects_invalid_capture_dimensions() {
        assert!(matches!(
            capture_ouroboros_evidence(&trace(), plan(), 0, 128),
            Err(OuroborosEvidenceError::Render(_))
        ));
    }
}
