//! Identity-preserving Ouroboros scene state.
//!
//! This module defines a portable, authority-free scene model. It does not
//! open a display, read input devices, access a camera, perform network I/O,
//! or own a frame clock. A host supplies bounded time steps and normalized
//! attention targets; a renderer consumes the returned form weights and body
//! samples.
//!
//! The Ouroboros remains one continuous ring identity while its material
//! expression blends between Nature, Humanity, Digital, and Enlightenment.
//! The head anchor is invariant across forms; only surface/profile parameters
//! and the host-supplied gaze direction change.

use std::{error::Error, fmt};

use crate::contract::{self, StepError};

const TAU: f32 = std::f32::consts::TAU;
const MIN_BODY_SAMPLES: u32 = 8;
const MAX_BODY_SAMPLES: u32 = 4096;
const CHECKPOINT_MAGIC: [u8; 8] = *b"OUROCKP1";
const CHECKPOINT_VERSION: u16 = 1;
const MAX_CHECKPOINT_BYTES: usize = 256;
const MAX_FORM_VELOCITY: f32 = 1_000.0;

/// Stable semantic expressions of the same Ouroboros identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OuroborosForm {
    Nature,
    Humanity,
    Digital,
    Enlightenment,
}

impl OuroborosForm {
    pub const ALL: [Self; 4] = [
        Self::Nature,
        Self::Humanity,
        Self::Digital,
        Self::Enlightenment,
    ];

    pub const fn id(self) -> &'static str {
        match self {
            Self::Nature => "nature",
            Self::Humanity => "humanity",
            Self::Digital => "digital",
            Self::Enlightenment => "enlightenment",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "nature" => Some(Self::Nature),
            "humanity" => Some(Self::Humanity),
            "digital" => Some(Self::Digital),
            "enlightenment" => Some(Self::Enlightenment),
            _ => None,
        }
    }

    pub const fn weights(self) -> FormWeights {
        match self {
            Self::Nature => FormWeights {
                nature: 1.0,
                humanity: 0.0,
                digital: 0.0,
                enlightenment: 0.08,
            },
            Self::Humanity => FormWeights {
                nature: 0.28,
                humanity: 1.0,
                digital: 0.08,
                enlightenment: 0.16,
            },
            Self::Digital => FormWeights {
                nature: 0.04,
                humanity: 0.12,
                digital: 1.0,
                enlightenment: 0.48,
            },
            Self::Enlightenment => FormWeights {
                nature: 0.46,
                humanity: 0.42,
                digital: 0.62,
                enlightenment: 1.0,
            },
        }
    }
}

/// Independent material/expression weights for a renderer.
///
/// These are not probabilities and do not need to sum to one. Each channel
/// represents an independently blendable visual contribution.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FormWeights {
    pub nature: f32,
    pub humanity: f32,
    pub digital: f32,
    pub enlightenment: f32,
}

impl FormWeights {
    const ZERO: Self = Self {
        nature: 0.0,
        humanity: 0.0,
        digital: 0.0,
        enlightenment: 0.0,
    };

    fn hermite(
        self,
        target: Self,
        start_velocity: Self,
        durations: [f32; 4],
        elapsed: f32,
    ) -> Self {
        let starts = [self.nature, self.humanity, self.digital, self.enlightenment];
        let ends = [
            target.nature,
            target.humanity,
            target.digital,
            target.enlightenment,
        ];
        let velocities = [
            start_velocity.nature,
            start_velocity.humanity,
            start_velocity.digital,
            start_velocity.enlightenment,
        ];
        let mut output = [0.0; 4];
        for index in 0..4 {
            let duration = f64::from(durations[index]);
            let t = (f64::from(elapsed) / duration).clamp(0.0, 1.0);
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let value = h00 * f64::from(starts[index])
                + h10 * duration * f64::from(velocities[index])
                + h01 * f64::from(ends[index]);
            // Evaluate in f64 so an analytically in-bounds cubic cannot
            // acquire a tiny negative/greater-than-one weight from f32
            // cancellation near a boundary.
            output[index] = value.clamp(0.0, 1.0) as f32;
        }
        Self {
            nature: output[0],
            humanity: output[1],
            digital: output[2],
            enlightenment: output[3],
        }
    }

    fn hermite_velocity(
        self,
        target: Self,
        start_velocity: Self,
        durations: [f32; 4],
        elapsed: f32,
    ) -> Self {
        let starts = [self.nature, self.humanity, self.digital, self.enlightenment];
        let ends = [
            target.nature,
            target.humanity,
            target.digital,
            target.enlightenment,
        ];
        let velocities = [
            start_velocity.nature,
            start_velocity.humanity,
            start_velocity.digital,
            start_velocity.enlightenment,
        ];
        let mut output = [0.0; 4];
        for index in 0..4 {
            let duration = f64::from(durations[index]);
            let t = (f64::from(elapsed) / duration).clamp(0.0, 1.0);
            if t >= 1.0 {
                output[index] = 0.0;
                continue;
            }
            let t2 = t * t;
            let d00 = 6.0 * t2 - 6.0 * t;
            let d10 = 3.0 * t2 - 4.0 * t + 1.0;
            let d01 = -6.0 * t2 + 6.0 * t;
            let value = (d00 * f64::from(starts[index])
                + d10 * duration * f64::from(velocities[index])
                + d01 * f64::from(ends[index]))
                / duration;
            output[index] = value as f32;
        }
        Self {
            nature: output[0],
            humanity: output[1],
            digital: output[2],
            enlightenment: output[3],
        }
    }

    fn is_finite_and_bounded(self) -> bool {
        [self.nature, self.humanity, self.digital, self.enlightenment]
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
    }

    fn is_finite_and_limited(self, limit: f32) -> bool {
        [self.nature, self.humanity, self.digital, self.enlightenment]
            .into_iter()
            .all(|value| value.is_finite() && value.abs() <= limit)
    }
}

/// Content address for one portable Ouroboros scene checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosCheckpointReceipt {
    pub schema_version: u16,
    pub scene_contract_version: u16,
    pub byte_length: u16,
    pub digest: [u8; 32],
}

/// Errors from validating, encoding or restoring a portable scene checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosCheckpointError {
    TooLarge,
    InvalidMagic,
    UnsupportedVersion(u16),
    UnsupportedSceneContract(u16),
    Truncated,
    TrailingBytes,
    InvalidTag(u8),
    InvalidOptionFlag(u8),
    InvalidConfiguration,
    InvalidState,
}

impl fmt::Display for OuroborosCheckpointError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooLarge => write!(f, "Ouroboros checkpoint exceeds the byte limit"),
            Self::InvalidMagic => write!(f, "invalid Ouroboros checkpoint magic"),
            Self::UnsupportedVersion(version) => {
                write!(f, "unsupported Ouroboros checkpoint version {version}")
            }
            Self::UnsupportedSceneContract(version) => {
                write!(f, "unsupported Ouroboros scene contract {version}")
            }
            Self::Truncated => write!(f, "Ouroboros checkpoint is truncated"),
            Self::TrailingBytes => write!(f, "Ouroboros checkpoint contains trailing bytes"),
            Self::InvalidTag(value) => write!(f, "invalid Ouroboros checkpoint tag {value}"),
            Self::InvalidOptionFlag(value) => {
                write!(f, "invalid Ouroboros checkpoint option flag {value}")
            }
            Self::InvalidConfiguration => {
                write!(f, "Ouroboros checkpoint configuration is invalid")
            }
            Self::InvalidState => write!(f, "Ouroboros checkpoint state is invalid"),
        }
    }
}

impl Error for OuroborosCheckpointError {}

/// Host-selected animation policy. Reduced mode is a true frozen scene state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosMotionMode {
    Reduced,
    LowPower,
    Balanced,
    Cinematic,
}

impl OuroborosMotionMode {
    fn time_scale(self) -> f32 {
        match self {
            Self::Reduced => 0.0,
            Self::LowPower => 0.35,
            Self::Balanced => 0.70,
            Self::Cinematic => 1.0,
        }
    }
}

/// Bounded parameters for the shared ring and its face orientation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosConfig {
    /// Normalized major radius of the ring, in 0.1..=0.45.
    pub major_radius: f32,
    /// Vertical ellipse scale, in 0.5..=1.0.
    pub vertical_scale: f32,
    /// Maximum organic radial deformation, in 0.0..=0.12.
    pub organic_deformation: f32,
    /// Maximum digital radial deformation, in 0.0..=0.05.
    pub digital_deformation: f32,
    /// Duration of an animated form transition, in 0.1..=120 seconds.
    pub transition_seconds: f32,
    /// Head orientation response, in 0.1..=20 radians-per-second scale.
    pub face_response: f32,
    /// Maximum host-directed horizontal head rotation, in 0.0..=0.75 radians.
    pub max_yaw_radians: f32,
    /// Maximum host-directed vertical head rotation, in 0.0..=0.55 radians.
    pub max_pitch_radians: f32,
}

impl Default for OuroborosConfig {
    fn default() -> Self {
        Self {
            major_radius: 0.38,
            vertical_scale: 0.82,
            organic_deformation: 0.075,
            digital_deformation: 0.025,
            transition_seconds: 8.0,
            face_response: 5.0,
            max_yaw_radians: 0.55,
            max_pitch_radians: 0.35,
        }
    }
}

impl OuroborosConfig {
    pub fn validate(self) -> Result<(), OuroborosError> {
        let finite = [
            self.major_radius,
            self.vertical_scale,
            self.organic_deformation,
            self.digital_deformation,
            self.transition_seconds,
            self.face_response,
            self.max_yaw_radians,
            self.max_pitch_radians,
        ]
        .into_iter()
        .all(f32::is_finite);

        if !finite
            || !(0.1..=0.45).contains(&self.major_radius)
            || !(0.5..=1.0).contains(&self.vertical_scale)
            || !(0.0..=0.12).contains(&self.organic_deformation)
            || !(0.0..=0.05).contains(&self.digital_deformation)
            || !(0.1..=120.0).contains(&self.transition_seconds)
            || !(0.1..=20.0).contains(&self.face_response)
            || !(0.0..=0.75).contains(&self.max_yaw_radians)
            || !(0.0..=0.55).contains(&self.max_pitch_radians)
        {
            return Err(OuroborosError::InvalidConfiguration);
        }
        Ok(())
    }
}

/// Normalized, explicit host-provided attention target.
///
/// x and y are in -1..=1. This is a logical target supplied by the host,
/// not a request to enable a camera or inspect a pointer device.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AttentionTarget {
    pub x: f32,
    pub y: f32,
}

impl AttentionTarget {
    pub const CENTER: Self = Self { x: 0.0, y: 0.0 };

    pub fn new(x: f32, y: f32) -> Result<Self, OuroborosError> {
        if !x.is_finite()
            || !y.is_finite()
            || !(-1.0..=1.0).contains(&x)
            || !(-1.0..=1.0).contains(&y)
        {
            return Err(OuroborosError::InvalidAttentionTarget);
        }
        Ok(Self { x, y })
    }
}

/// Bounded orientation of the Ouroboros face relative to its neutral pose.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HeadPose {
    pub yaw_radians: f32,
    pub pitch_radians: f32,
}

impl Default for HeadPose {
    fn default() -> Self {
        Self {
            yaw_radians: 0.0,
            pitch_radians: 0.0,
        }
    }
}

/// Independent schema version for the semantic head/face attachment contract.
pub const OUROBOROS_LANDMARK_SCHEMA_VERSION: u16 = 1;
const OUROBOROS_LANDMARK_MAGIC: [u8; 8] = *b"OUROLND1";

/// Renderer-neutral semantic attachment points for one persistent Ouroboros head.
///
/// Points are expressed in the same normalized scene space as the body mesh.
/// The root anchor is invariant across the four semantic forms. The local
/// face basis follows only the bounded head pose; no renderer owns landmark
/// placement or may silently substitute a different rig.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosLandmarks {
    pub scene_contract_version: u16,
    pub schema_version: u16,
    pub head_anchor: [f32; 3],
    pub forward: [f32; 3],
    pub lateral: [f32; 3],
    pub up: [f32; 3],
    pub left_eye: [f32; 3],
    pub right_eye: [f32; 3],
    pub nose_tip: [f32; 3],
    pub mouth_center: [f32; 3],
    pub crown: [f32; 3],
    pub chin: [f32; 3],
}

/// Validation errors for public, renderer-consumed semantic landmarks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosLandmarkError {
    UnsupportedSchemaVersion,
    UnsupportedSceneContractVersion,
    NonFiniteCoordinate,
    HeadRootMismatch,
    InvalidBasis,
    PoseMismatch,
    LandmarkOutOfBounds,
    AsymmetricEyes,
}

impl fmt::Display for OuroborosLandmarkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedSchemaVersion => write!(f, "unsupported Ouroboros landmark schema"),
            Self::UnsupportedSceneContractVersion => {
                write!(f, "unsupported Ouroboros landmark scene contract")
            }
            Self::NonFiniteCoordinate => write!(f, "Ouroboros landmark contains a non-finite coordinate"),
            Self::HeadRootMismatch => write!(f, "Ouroboros landmark head root differs from the scene root"),
            Self::InvalidBasis => write!(f, "Ouroboros landmark basis is not orthonormal and right-handed"),
            Self::PoseMismatch => write!(f, "Ouroboros landmark basis does not match the scene head pose"),
            Self::LandmarkOutOfBounds => write!(f, "Ouroboros facial attachment lies outside the head envelope"),
            Self::AsymmetricEyes => write!(f, "Ouroboros eye attachments are not symmetric around the face basis"),
        }
    }
}

impl Error for OuroborosLandmarkError {}

/// Content address for one canonical semantic landmark set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OuroborosLandmarkReceipt {
    pub schema_version: u16,
    pub scene_contract_version: u16,
    pub byte_length: u16,
    pub digest: [u8; 32],
}

impl OuroborosLandmarks {
    /// Validate this public landmark value against the authoritative scene.
    ///
    /// This checks schema identity, finite coordinates, exact root identity,
    /// an orthonormal right-handed basis matching the scene's bounded pose,
    /// facial-envelope bounds and bilateral eye symmetry. Consumers should
    /// validate before using caller-supplied or deserialized landmark values.
    pub fn validate(self, scene: &OuroborosScene) -> Result<(), OuroborosLandmarkError> {
        if self.schema_version != OUROBOROS_LANDMARK_SCHEMA_VERSION {
            return Err(OuroborosLandmarkError::UnsupportedSchemaVersion);
        }
        if self.scene_contract_version != contract::SCENE_CONTRACT_VERSION {
            return Err(OuroborosLandmarkError::UnsupportedSceneContractVersion);
        }

        let points = [
            self.head_anchor,
            self.forward,
            self.lateral,
            self.up,
            self.left_eye,
            self.right_eye,
            self.nose_tip,
            self.mouth_center,
            self.crown,
            self.chin,
        ];
        if points.into_iter().flatten().any(|value| !value.is_finite()) {
            return Err(OuroborosLandmarkError::NonFiniteCoordinate);
        }

        let expected_root = scene.frame().head_anchor;
        if self.head_anchor != expected_root {
            return Err(OuroborosLandmarkError::HeadRootMismatch);
        }

        let tolerance = 2.0e-5;
        let dot = dot3;
        let unit = |axis: [f32; 3]| (dot(axis, axis) - 1.0).abs() <= tolerance;
        if !unit(self.forward)
            || !unit(self.lateral)
            || !unit(self.up)
            || dot(self.forward, self.lateral).abs() > tolerance
            || dot(self.forward, self.up).abs() > tolerance
            || dot(self.lateral, self.up).abs() > tolerance
            || distance3(cross3(self.forward, self.lateral), self.up) > tolerance
        {
            return Err(OuroborosLandmarkError::InvalidBasis);
        }

        let pose = scene.head_pose();
        let expected_forward = normalize3([
            pose.yaw_radians.cos() * pose.pitch_radians.cos(),
            pose.yaw_radians.sin() * pose.pitch_radians.cos(),
            pose.pitch_radians.sin(),
        ]);
        let expected_lateral = normalize3(cross3([0.0, 0.0, 1.0], expected_forward));
        let expected_up = normalize3(cross3(expected_forward, expected_lateral));
        if distance3(self.forward, expected_forward) > tolerance
            || distance3(self.lateral, expected_lateral) > tolerance
            || distance3(self.up, expected_up) > tolerance
        {
            return Err(OuroborosLandmarkError::PoseMismatch);
        }

        let radius = scene.config.major_radius;
        let envelope = radius * 0.2001;
        for point in points.into_iter().skip(4) {
            if distance3(point, self.head_anchor) > envelope {
                return Err(OuroborosLandmarkError::LandmarkOutOfBounds);
            }
        }

        let eye_delta = sub3(self.left_eye, self.right_eye);
        if dot(eye_delta, self.forward).abs() > radius * 1.0e-4
            || dot(eye_delta, self.up).abs() > radius * 1.0e-4
            || (dot(eye_delta, self.lateral) + radius * 0.10).abs() > radius * 1.0e-4
        {
            return Err(OuroborosLandmarkError::AsymmetricEyes);
        }

        Ok(())
    }

    /// Canonical little-endian representation; point order is part of schema v1.
    pub fn canonical_bytes(self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(136);
        bytes.extend_from_slice(&OUROBOROS_LANDMARK_MAGIC);
        bytes.extend_from_slice(&self.schema_version.to_le_bytes());
        bytes.extend_from_slice(&self.scene_contract_version.to_le_bytes());
        for point in [
            self.head_anchor,
            self.forward,
            self.lateral,
            self.up,
            self.left_eye,
            self.right_eye,
            self.nose_tip,
            self.mouth_center,
            self.crown,
            self.chin,
        ] {
            for value in point {
                bytes.extend_from_slice(&value.to_bits().to_le_bytes());
            }
        }
        bytes
    }

    pub fn receipt(self) -> OuroborosLandmarkReceipt {
        let bytes = self.canonical_bytes();
        OuroborosLandmarkReceipt {
            schema_version: self.schema_version,
            scene_contract_version: self.scene_contract_version,
            byte_length: bytes.len() as u16,
            digest: *blake3::hash(&bytes).as_bytes(),
        }
    }
}

/// Stable, renderer-facing snapshot of the current scene state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosFrame {
    pub target_form: OuroborosForm,
    pub form_weights: FormWeights,
    pub transition_progress: f32,
    pub head_pose: HeadPose,
    pub motion_mode: OuroborosMotionMode,
    /// The shared, invariant centerline anchor for the face/head.
    pub head_anchor: [f32; 3],
}

/// One sample on the continuous Ouroboros body centerline.
///
/// A renderer can expand this into a tube, ribbon, mesh, or shader-driven
/// surface. angle_radians is stable around the ring for temporal matching.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OuroborosBodySample {
    pub angle_radians: f32,
    pub centerline: [f32; 3],
    pub form_weights: FormWeights,
}

/// Typed errors shared by every adapter using this scene model.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OuroborosError {
    InvalidConfiguration,
    InvalidAttentionTarget,
    InvalidSampleCount,
    InvalidSampleIndex,
    InvalidStep(StepError),
}

impl fmt::Display for OuroborosError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration => write!(f, "invalid Ouroboros configuration"),
            Self::InvalidAttentionTarget => {
                write!(
                    f,
                    "attention target must be finite and normalized to -1..=1"
                )
            }
            Self::InvalidSampleCount => {
                write!(f, "body sample count must be in 8..=4096")
            }
            Self::InvalidSampleIndex => write!(f, "body sample index must be below sample count"),
            Self::InvalidStep(error) => write!(f, "invalid Ouroboros time step: {error:?}"),
        }
    }
}

impl Error for OuroborosError {}

/// Portable Ouroboros state machine.
///
/// Time is host-owned. Call advance with bounded steps (prefer a fixed 30 Hz
/// or 60 Hz schedule) and supply attention only through set_attention_target.
/// The core deliberately has no OS, camera, input-device, filesystem, or
/// network dependencies.
#[derive(Debug, Clone)]
pub struct OuroborosScene {
    config: OuroborosConfig,
    from_weights: FormWeights,
    target_form: OuroborosForm,
    transition_elapsed: f32,
    transition_durations: [f32; 4],
    transition_start_progress: f32,
    transition_start_velocity: FormWeights,
    attention_target: AttentionTarget,
    head_pose: HeadPose,
    motion_mode: OuroborosMotionMode,
    frozen_weights: Option<FormWeights>,
    frozen_progress: Option<f32>,
    frozen_velocity: Option<FormWeights>,
}

impl OuroborosScene {
    pub fn new(
        config: OuroborosConfig,
        initial_form: OuroborosForm,
    ) -> Result<Self, OuroborosError> {
        config.validate()?;
        let weights = initial_form.weights();
        Ok(Self {
            config,
            from_weights: weights,
            target_form: initial_form,
            transition_elapsed: config.transition_seconds,
            transition_durations: [config.transition_seconds; 4],
            transition_start_progress: 0.0,
            transition_start_velocity: FormWeights::ZERO,
            attention_target: AttentionTarget::CENTER,
            head_pose: HeadPose::default(),
            motion_mode: OuroborosMotionMode::Balanced,
            frozen_weights: None,
            frozen_progress: None,
            frozen_velocity: None,
        })
    }

    pub fn config(&self) -> OuroborosConfig {
        self.config
    }

    /// Serialize the complete portable scene state into a bounded checkpoint.
    ///
    /// Device handles, renderer resources and host capabilities are deliberately
    /// excluded. The snapshot is suitable for a host to persist or hand off.
    pub fn encode_checkpoint(
        &self,
    ) -> Result<(Vec<u8>, OuroborosCheckpointReceipt), OuroborosCheckpointError> {
        self.validate_checkpoint_state()?;

        let mut bytes = Vec::with_capacity(160);
        bytes.extend_from_slice(&CHECKPOINT_MAGIC);
        bytes.extend_from_slice(&CHECKPOINT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&contract::SCENE_CONTRACT_VERSION.to_le_bytes());
        encode_checkpoint_config(self.config, &mut bytes);
        encode_checkpoint_weights(self.from_weights, &mut bytes);
        bytes.push(checkpoint_form_tag(self.target_form));
        bytes.push(checkpoint_motion_tag(self.motion_mode));
        checkpoint_push_f32(&mut bytes, self.transition_elapsed);
        for duration in self.transition_durations {
            checkpoint_push_f32(&mut bytes, duration);
        }
        checkpoint_push_f32(&mut bytes, self.transition_start_progress);
        encode_checkpoint_weights(self.transition_start_velocity, &mut bytes);
        checkpoint_push_f32(&mut bytes, self.attention_target.x);
        checkpoint_push_f32(&mut bytes, self.attention_target.y);
        checkpoint_push_f32(&mut bytes, self.head_pose.yaw_radians);
        checkpoint_push_f32(&mut bytes, self.head_pose.pitch_radians);
        encode_checkpoint_optional_weights(self.frozen_weights, &mut bytes);
        encode_checkpoint_optional_f32(self.frozen_progress, &mut bytes);
        encode_checkpoint_optional_weights(self.frozen_velocity, &mut bytes);

        if bytes.len() > MAX_CHECKPOINT_BYTES || bytes.len() > u16::MAX as usize {
            return Err(OuroborosCheckpointError::TooLarge);
        }
        let receipt = checkpoint_receipt(&bytes);
        Ok((bytes, receipt))
    }

    /// Restore a scene only after the entire checkpoint and its kinematics validate.
    pub fn restore_checkpoint(
        bytes: &[u8],
    ) -> Result<(Self, OuroborosCheckpointReceipt), OuroborosCheckpointError> {
        if bytes.len() > MAX_CHECKPOINT_BYTES {
            return Err(OuroborosCheckpointError::TooLarge);
        }

        let mut cursor = CheckpointCursor::new(bytes);
        if cursor.take::<8>()? != CHECKPOINT_MAGIC {
            return Err(OuroborosCheckpointError::InvalidMagic);
        }
        let version = cursor.u16()?;
        if version != CHECKPOINT_VERSION {
            return Err(OuroborosCheckpointError::UnsupportedVersion(version));
        }
        let contract_version = cursor.u16()?;
        if contract_version != contract::SCENE_CONTRACT_VERSION {
            return Err(OuroborosCheckpointError::UnsupportedSceneContract(
                contract_version,
            ));
        }

        let config = decode_checkpoint_config(&mut cursor)?;
        let from_weights = decode_checkpoint_weights(&mut cursor)?;
        let target_form = checkpoint_decode_form(cursor.u8()?)?;
        let motion_mode = checkpoint_decode_motion(cursor.u8()?)?;
        let transition_elapsed = cursor.f32()?;
        let mut transition_durations = [0.0; 4];
        for duration in &mut transition_durations {
            *duration = cursor.f32()?;
        }
        let transition_start_progress = cursor.f32()?;
        let transition_start_velocity = decode_checkpoint_weights(&mut cursor)?;
        let attention_target = AttentionTarget {
            x: cursor.f32()?,
            y: cursor.f32()?,
        };
        let head_pose = HeadPose {
            yaw_radians: cursor.f32()?,
            pitch_radians: cursor.f32()?,
        };
        let frozen_weights = decode_checkpoint_optional_weights(&mut cursor)?;
        let frozen_progress = decode_checkpoint_optional_f32(&mut cursor)?;
        let frozen_velocity = decode_checkpoint_optional_weights(&mut cursor)?;

        if !cursor.is_empty() {
            return Err(OuroborosCheckpointError::TrailingBytes);
        }

        let mut scene = Self::new(config, target_form)
            .map_err(|_| OuroborosCheckpointError::InvalidConfiguration)?;
        scene.from_weights = from_weights;
        scene.transition_elapsed = transition_elapsed;
        scene.transition_durations = transition_durations;
        scene.transition_start_progress = transition_start_progress;
        scene.transition_start_velocity = transition_start_velocity;
        scene.attention_target = attention_target;
        scene.head_pose = head_pose;
        scene.motion_mode = motion_mode;
        scene.frozen_weights = frozen_weights;
        scene.frozen_progress = frozen_progress;
        scene.frozen_velocity = frozen_velocity;
        scene.validate_checkpoint_state()?;

        Ok((scene, checkpoint_receipt(bytes)))
    }

    fn validate_checkpoint_state(&self) -> Result<(), OuroborosCheckpointError> {
        if self.config.validate().is_err() {
            return Err(OuroborosCheckpointError::InvalidConfiguration);
        }
        if !self.from_weights.is_finite_and_bounded()
            || !self
                .transition_start_velocity
                .is_finite_and_limited(MAX_FORM_VELOCITY)
            || !self.transition_start_progress.is_finite()
            || !(0.0..=1.0).contains(&self.transition_start_progress)
            || !self.transition_elapsed.is_finite()
            || self.transition_elapsed < 0.0
            || !self.transition_durations.iter().all(|duration| {
                duration.is_finite()
                    && *duration > 0.0
                    && *duration <= self.config.transition_seconds
            })
            || self.transition_elapsed > max_transition_duration(self.transition_durations)
            || !hermite_segments_are_bounded(
                self.from_weights,
                self.target_form.weights(),
                self.transition_start_velocity,
                self.transition_durations,
            )
            || AttentionTarget::new(self.attention_target.x, self.attention_target.y).is_err()
            || !self.head_pose.yaw_radians.is_finite()
            || self.head_pose.yaw_radians.abs() > self.config.max_yaw_radians
            || !self.head_pose.pitch_radians.is_finite()
            || self.head_pose.pitch_radians.abs() > self.config.max_pitch_radians
        {
            return Err(OuroborosCheckpointError::InvalidState);
        }

        match (
            self.motion_mode,
            self.frozen_weights,
            self.frozen_progress,
            self.frozen_velocity,
        ) {
            (OuroborosMotionMode::Reduced, Some(weights), Some(progress), Some(velocity))
                if weights.is_finite_and_bounded()
                    && progress.is_finite()
                    && (0.0..=1.0).contains(&progress)
                    && velocity.is_finite_and_limited(MAX_FORM_VELOCITY) => {}
            (OuroborosMotionMode::Reduced, _, _, _) => {
                return Err(OuroborosCheckpointError::InvalidState);
            }
            (_, None, None, None) => {}
            _ => return Err(OuroborosCheckpointError::InvalidState),
        }

        if !self.current_weights().is_finite_and_bounded()
            || !self
                .current_velocity()
                .is_finite_and_limited(MAX_FORM_VELOCITY)
        {
            return Err(OuroborosCheckpointError::InvalidState);
        }

        Ok(())
    }

    /// Canonical internal state bytes for replay evidence.
    ///
    /// This includes pending attention and transition state, not just the
    /// currently visible frame. It is crate-private because the internal
    /// checkpoint layout is versioned with the scene contract.
    pub(crate) fn canonical_state_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(160);
        bytes.extend_from_slice(b"OUROSTA2");
        bytes.extend_from_slice(&contract::SCENE_CONTRACT_VERSION.to_le_bytes());

        for value in [
            self.config.major_radius,
            self.config.vertical_scale,
            self.config.organic_deformation,
            self.config.digital_deformation,
            self.config.transition_seconds,
            self.config.face_response,
            self.config.max_yaw_radians,
            self.config.max_pitch_radians,
            self.from_weights.nature,
            self.from_weights.humanity,
            self.from_weights.digital,
            self.from_weights.enlightenment,
            self.transition_elapsed,
            self.transition_durations[0],
            self.transition_durations[1],
            self.transition_durations[2],
            self.transition_durations[3],
            self.transition_start_progress,
            self.transition_start_velocity.nature,
            self.transition_start_velocity.humanity,
            self.transition_start_velocity.digital,
            self.transition_start_velocity.enlightenment,
            self.attention_target.x,
            self.attention_target.y,
            self.head_pose.yaw_radians,
            self.head_pose.pitch_radians,
        ] {
            append_state_f32(&mut bytes, value);
        }

        bytes.push(match self.target_form {
            OuroborosForm::Nature => 0,
            OuroborosForm::Humanity => 1,
            OuroborosForm::Digital => 2,
            OuroborosForm::Enlightenment => 3,
        });
        bytes.push(match self.motion_mode {
            OuroborosMotionMode::Reduced => 0,
            OuroborosMotionMode::LowPower => 1,
            OuroborosMotionMode::Balanced => 2,
            OuroborosMotionMode::Cinematic => 3,
        });
        match self.frozen_weights {
            Some(weights) => {
                bytes.push(1);
                for value in [
                    weights.nature,
                    weights.humanity,
                    weights.digital,
                    weights.enlightenment,
                ] {
                    append_state_f32(&mut bytes, value);
                }
            }
            None => bytes.push(0),
        }
        match self.frozen_velocity {
            Some(velocity) => {
                bytes.push(1);
                for value in [
                    velocity.nature,
                    velocity.humanity,
                    velocity.digital,
                    velocity.enlightenment,
                ] {
                    append_state_f32(&mut bytes, value);
                }
            }
            None => bytes.push(0),
        }
        match self.frozen_progress {
            Some(progress) => {
                bytes.push(1);
                append_state_f32(&mut bytes, progress);
            }
            None => bytes.push(0),
        }
        bytes
    }

    pub fn target_form(&self) -> OuroborosForm {
        self.target_form
    }

    pub fn motion_mode(&self) -> OuroborosMotionMode {
        self.motion_mode
    }

    pub fn head_pose(&self) -> HeadPose {
        self.head_pose
    }

    pub fn set_attention_target(&mut self, target: AttentionTarget) -> Result<(), OuroborosError> {
        // Revalidate even though AttentionTarget has a checked constructor:
        // its fields are public for straightforward host-language bindings.
        self.attention_target = AttentionTarget::new(target.x, target.y)?;
        Ok(())
    }

    pub fn set_motion_mode(&mut self, mode: OuroborosMotionMode) {
        if mode == self.motion_mode {
            return;
        }

        match mode {
            OuroborosMotionMode::Reduced => {
                // Freeze the complete kinematic state, not just the visible
                // form weights. Scene time stops while Reduced is active.
                self.frozen_weights = Some(self.current_weights());
                self.frozen_progress = Some(self.transition_progress());
                self.frozen_velocity = Some(self.current_velocity());
            }
            _ => {
                if let Some(frozen) = self.frozen_weights.take() {
                    let progress = self.frozen_progress.take().unwrap_or(0.0);
                    let velocity = project_outward_boundary_velocity(
                        frozen,
                        self.frozen_velocity.take().unwrap_or(FormWeights::ZERO),
                    );
                    self.from_weights = frozen;
                    self.transition_start_velocity = velocity;
                    if progress >= 1.0 {
                        self.transition_start_velocity = FormWeights::ZERO;
                        self.transition_start_progress = 0.0;
                        self.transition_durations = [self.config.transition_seconds; 4];
                        self.transition_elapsed =
                            max_transition_duration(self.transition_durations);
                    } else {
                        self.transition_start_progress = progress;
                        let preferred = self.config.transition_seconds * (1.0 - progress);
                        self.transition_durations = bounded_transition_durations(
                            self.from_weights,
                            self.target_form.weights(),
                            self.transition_start_velocity,
                            preferred,
                        );
                        self.transition_elapsed = 0.0;
                    }
                }
            }
        }
        self.motion_mode = mode;
    }

    /// Begin a C1-continuous transition to another semantic expression.
    ///
    /// The current value and derivative become the new Hermite segment's
    /// initial conditions. The terminal derivative is zero. Segment duration
    /// is shortened when necessary to keep every independent form channel in
    /// its validated [0, 1] range.
    pub fn transition_to(&mut self, form: OuroborosForm) {
        let current = self.current_weights();
        let velocity = project_outward_boundary_velocity(current, self.current_velocity());
        self.target_form = form;
        self.from_weights = current;
        self.transition_start_velocity = velocity;
        self.transition_elapsed = 0.0;
        self.transition_start_progress = 0.0;
        self.transition_durations = bounded_transition_durations(
            self.from_weights,
            self.target_form.weights(),
            self.transition_start_velocity,
            self.config.transition_seconds,
        );

        if self.motion_mode == OuroborosMotionMode::Reduced {
            // Reduced motion has static semantics: choosing a new form
            // changes the composition immediately and gives it zero velocity.
            let target = form.weights();
            self.from_weights = target;
            self.transition_start_velocity = FormWeights::ZERO;
            self.transition_elapsed = max_transition_duration(self.transition_durations);
            self.frozen_weights = Some(target);
            self.frozen_progress = Some(1.0);
            self.frozen_velocity = Some(FormWeights::ZERO);
        } else {
            self.frozen_weights = None;
            self.frozen_progress = None;
            self.frozen_velocity = None;
        }
    }

    /// Advance the scene by a bounded host-supplied time step.
    pub fn advance(&mut self, dt_seconds: f32) -> Result<(), OuroborosError> {
        contract::validate_step(dt_seconds, 0.0).map_err(OuroborosError::InvalidStep)?;

        let time_scale = self.motion_mode.time_scale();
        if time_scale == 0.0 || dt_seconds == 0.0 {
            return Ok(());
        }

        let dt = dt_seconds * time_scale;
        self.transition_elapsed =
            (self.transition_elapsed + dt).min(max_transition_duration(self.transition_durations));

        let target_yaw = self.attention_target.x * self.config.max_yaw_radians;
        let target_pitch = -self.attention_target.y * self.config.max_pitch_radians;
        let response = (self.config.face_response * dt).clamp(0.0, 1.0);

        self.head_pose.yaw_radians += (target_yaw - self.head_pose.yaw_radians) * response;
        self.head_pose.pitch_radians += (target_pitch - self.head_pose.pitch_radians) * response;

        // Clamp after interpolation as a defense against accumulated numerical
        // drift and future changes to the host input contract.
        self.head_pose.yaw_radians = self
            .head_pose
            .yaw_radians
            .clamp(-self.config.max_yaw_radians, self.config.max_yaw_radians);
        self.head_pose.pitch_radians = self.head_pose.pitch_radians.clamp(
            -self.config.max_pitch_radians,
            self.config.max_pitch_radians,
        );

        Ok(())
    }

    pub fn frame(&self) -> OuroborosFrame {
        OuroborosFrame {
            target_form: self.target_form,
            form_weights: self.current_weights(),
            transition_progress: self.transition_progress(),
            head_pose: self.head_pose,
            motion_mode: self.motion_mode,
            head_anchor: [self.config.major_radius, 0.0, 0.0],
        }
    }

    /// Return the stable semantic head/face attachment contract.
    ///
    /// Form weights do not participate in landmark placement. The root is
    /// exactly the historical head anchor; the orthonormal face basis follows
    /// the bounded yaw/pitch state. All offsets scale with the scene major
    /// radius so host-selected scene sizes preserve proportions.
    pub fn landmarks(&self) -> OuroborosLandmarks {
        let head_anchor = [self.config.major_radius, 0.0, 0.0];
        let yaw = self.head_pose.yaw_radians;
        let pitch = self.head_pose.pitch_radians;
        let forward = normalize3([
            yaw.cos() * pitch.cos(),
            yaw.sin() * pitch.cos(),
            pitch.sin(),
        ]);
        let lateral = normalize3(cross3([0.0, 0.0, 1.0], forward));
        let up = normalize3(cross3(forward, lateral));
        let radius = self.config.major_radius;
        let point = |forward_offset: f32, lateral_offset: f32, up_offset: f32| {
            add3(
                add3(
                    add3(head_anchor, scale3(forward, radius * forward_offset)),
                    scale3(lateral, radius * lateral_offset),
                ),
                scale3(up, radius * up_offset),
            )
        };

        OuroborosLandmarks {
            scene_contract_version: contract::SCENE_CONTRACT_VERSION,
            schema_version: OUROBOROS_LANDMARK_SCHEMA_VERSION,
            head_anchor,
            forward,
            lateral,
            up,
            left_eye: point(0.06, -0.05, 0.02),
            right_eye: point(0.06, 0.05, 0.02),
            nose_tip: point(0.18, 0.0, 0.005),
            mouth_center: point(0.14, 0.0, -0.035),
            crown: point(0.015, 0.0, 0.10),
            chin: point(0.08, 0.0, -0.10),
        }
    }

    /// Deterministic point on the ring's shared body centerline.
    ///
    /// The first sample is the head anchor in every form. The renderer may
    /// use the four weights to blend surface materials, detail, and geometry
    /// without changing the semantic identity or ring topology.
    pub fn body_sample(
        &self,
        sample_index: u32,
        sample_count: u32,
    ) -> Result<OuroborosBodySample, OuroborosError> {
        if !(MIN_BODY_SAMPLES..=MAX_BODY_SAMPLES).contains(&sample_count) {
            return Err(OuroborosError::InvalidSampleCount);
        }
        if sample_index >= sample_count {
            return Err(OuroborosError::InvalidSampleIndex);
        }

        let angle = TAU * sample_index as f32 / sample_count as f32;
        let weights = self.current_weights();
        let organic =
            (weights.nature * 0.70 + weights.humanity * 0.20 + weights.enlightenment * 0.10)
                .clamp(0.0, 1.0);
        let digital = (weights.digital * 0.80 + weights.enlightenment * 0.15).clamp(0.0, 1.0);

        // Harmonics are zero at angle 0, preserving the shared face anchor.
        let radial = 1.0
            + self.config.organic_deformation * organic * (3.0 * angle).sin()
            + self.config.digital_deformation * digital * (11.0 * angle).sin();
        let radius = self.config.major_radius * radial;
        let y_radius = radius * self.config.vertical_scale;
        let z = self.config.major_radius
            * (0.055 * organic * (5.0 * angle).sin() + 0.025 * digital * (13.0 * angle).sin());

        let sample = OuroborosBodySample {
            angle_radians: angle,
            centerline: [radius * angle.cos(), y_radius * angle.sin(), z],
            form_weights: weights,
        };

        if !sample.form_weights.is_finite_and_bounded()
            || !sample.centerline.into_iter().all(f32::is_finite)
        {
            return Err(OuroborosError::InvalidConfiguration);
        }
        Ok(sample)
    }

    fn transition_progress(&self) -> f32 {
        if let Some(progress) = self.frozen_progress {
            return progress;
        }
        let starts = [
            self.from_weights.nature,
            self.from_weights.humanity,
            self.from_weights.digital,
            self.from_weights.enlightenment,
        ];
        let targets = [
            self.target_form.weights().nature,
            self.target_form.weights().humanity,
            self.target_form.weights().digital,
            self.target_form.weights().enlightenment,
        ];
        let velocities = [
            self.transition_start_velocity.nature,
            self.transition_start_velocity.humanity,
            self.transition_start_velocity.digital,
            self.transition_start_velocity.enlightenment,
        ];
        let mut slowest_active_progress = 1.0_f32;
        let mut has_active_channel = false;
        for index in 0..4 {
            if starts[index] == targets[index] && velocities[index] == 0.0 {
                continue;
            }
            has_active_channel = true;
            let channel_progress =
                (self.transition_elapsed / self.transition_durations[index]).clamp(0.0, 1.0);
            slowest_active_progress = slowest_active_progress.min(channel_progress);
        }
        let local = if has_active_channel {
            slowest_active_progress
        } else {
            1.0
        };
        self.transition_start_progress + (1.0 - self.transition_start_progress) * local
    }

    fn current_weights(&self) -> FormWeights {
        if let Some(weights) = self.frozen_weights {
            return weights;
        }
        self.from_weights.hermite(
            self.target_form.weights(),
            self.transition_start_velocity,
            self.transition_durations,
            self.transition_elapsed,
        )
    }

    fn current_velocity(&self) -> FormWeights {
        if let Some(velocity) = self.frozen_velocity {
            return velocity;
        }
        self.from_weights.hermite_velocity(
            self.target_form.weights(),
            self.transition_start_velocity,
            self.transition_durations,
            self.transition_elapsed,
        )
    }
}

/// Project an impossible outward derivative back into the tangent cone
/// of a bounded [0, 1] channel. At an exact boundary, preserving an outward
/// derivative and remaining in the interval are mathematically incompatible.
/// Interior retargets preserve the derivative exactly.
fn project_outward_boundary_velocity(weights: FormWeights, velocity: FormWeights) -> FormWeights {
    let project = |weight: f32, speed: f32| {
        if (weight == 0.0 && speed < 0.0) || (weight == 1.0 && speed > 0.0) {
            0.0
        } else {
            speed
        }
    };
    FormWeights {
        nature: project(weights.nature, velocity.nature),
        humanity: project(weights.humanity, velocity.humanity),
        digital: project(weights.digital, velocity.digital),
        enlightenment: project(weights.enlightenment, velocity.enlightenment),
    }
}

/// Find a safe duration independently for each semantic weight channel.
///
/// A nearly-zero residual channel must not force the entire entity to snap.
/// Each channel preserves its exact initial derivative, uses a zero terminal
/// derivative, and gets its own analytically bounded cubic segment.
fn bounded_transition_durations(
    start: FormWeights,
    target: FormWeights,
    start_velocity: FormWeights,
    preferred: f32,
) -> [f32; 4] {
    let starts = [
        start.nature,
        start.humanity,
        start.digital,
        start.enlightenment,
    ];
    let targets = [
        target.nature,
        target.humanity,
        target.digital,
        target.enlightenment,
    ];
    let velocities = [
        start_velocity.nature,
        start_velocity.humanity,
        start_velocity.digital,
        start_velocity.enlightenment,
    ];
    let mut durations = [preferred.max(1.0e-6); 4];
    for index in 0..4 {
        for _ in 0..32 {
            if cubic_channel_is_bounded(
                starts[index],
                targets[index],
                velocities[index],
                durations[index],
            ) {
                break;
            }
            durations[index] *= 0.5;
        }
        debug_assert!(cubic_channel_is_bounded(
            starts[index],
            targets[index],
            velocities[index],
            durations[index]
        ));
    }
    durations
}

fn max_transition_duration(durations: [f32; 4]) -> f32 {
    durations.into_iter().fold(0.0_f32, f32::max)
}

fn hermite_segments_are_bounded(
    start: FormWeights,
    target: FormWeights,
    start_velocity: FormWeights,
    durations: [f32; 4],
) -> bool {
    [
        (
            start.nature,
            target.nature,
            start_velocity.nature,
            durations[0],
        ),
        (
            start.humanity,
            target.humanity,
            start_velocity.humanity,
            durations[1],
        ),
        (
            start.digital,
            target.digital,
            start_velocity.digital,
            durations[2],
        ),
        (
            start.enlightenment,
            target.enlightenment,
            start_velocity.enlightenment,
            durations[3],
        ),
    ]
    .into_iter()
    .all(|(p0, p1, velocity, duration)| {
        duration.is_finite()
            && duration > 0.0
            && cubic_channel_is_bounded(p0, p1, velocity, duration)
    })
}

fn cubic_channel_is_bounded(p0: f32, p1: f32, velocity: f32, duration: f32) -> bool {
    let p0 = f64::from(p0);
    let p1 = f64::from(p1);
    let tangent = f64::from(velocity) * f64::from(duration);
    let a = 2.0 * p0 + tangent - 2.0 * p1;
    let b = -3.0 * p0 - 2.0 * tangent + 3.0 * p1;
    let c = tangent;
    let evaluate = |t: f64| ((a * t + b) * t + c) * t + p0;
    let in_bounds = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);

    if !in_bounds(p0) || !in_bounds(p1) {
        return false;
    }

    // A cubic can only leave its endpoint range at an interior derivative
    // root. Solve that quadratic in f64 rather than sampling and aliasing it.
    let qa = 3.0 * a;
    let qb = 2.0 * b;
    let qc = c;
    if qa.abs() <= f64::EPSILON {
        if qb.abs() > f64::EPSILON {
            let root = -qc / qb;
            if (0.0..1.0).contains(&root) && !in_bounds(evaluate(root)) {
                return false;
            }
        }
        return true;
    }

    let discriminant = qb * qb - 4.0 * qa * qc;
    if discriminant < 0.0 {
        return true;
    }
    let sqrt_discriminant = discriminant.sqrt();
    let root_a = (-qb - sqrt_discriminant) / (2.0 * qa);
    let root_b = (-qb + sqrt_discriminant) / (2.0 * qa);
    [root_a, root_b]
        .into_iter()
        .filter(|root| (0.0..1.0).contains(root))
        .all(|root| in_bounds(evaluate(root)))
}

fn checkpoint_push_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn encode_checkpoint_config(config: OuroborosConfig, bytes: &mut Vec<u8>) {
    for value in [
        config.major_radius,
        config.vertical_scale,
        config.organic_deformation,
        config.digital_deformation,
        config.transition_seconds,
        config.face_response,
        config.max_yaw_radians,
        config.max_pitch_radians,
    ] {
        checkpoint_push_f32(bytes, value);
    }
}

fn decode_checkpoint_config(
    cursor: &mut CheckpointCursor<'_>,
) -> Result<OuroborosConfig, OuroborosCheckpointError> {
    let config = OuroborosConfig {
        major_radius: cursor.f32()?,
        vertical_scale: cursor.f32()?,
        organic_deformation: cursor.f32()?,
        digital_deformation: cursor.f32()?,
        transition_seconds: cursor.f32()?,
        face_response: cursor.f32()?,
        max_yaw_radians: cursor.f32()?,
        max_pitch_radians: cursor.f32()?,
    };
    config
        .validate()
        .map_err(|_| OuroborosCheckpointError::InvalidConfiguration)?;
    Ok(config)
}

fn encode_checkpoint_weights(weights: FormWeights, bytes: &mut Vec<u8>) {
    for value in [
        weights.nature,
        weights.humanity,
        weights.digital,
        weights.enlightenment,
    ] {
        checkpoint_push_f32(bytes, value);
    }
}

fn decode_checkpoint_weights(
    cursor: &mut CheckpointCursor<'_>,
) -> Result<FormWeights, OuroborosCheckpointError> {
    Ok(FormWeights {
        nature: cursor.f32()?,
        humanity: cursor.f32()?,
        digital: cursor.f32()?,
        enlightenment: cursor.f32()?,
    })
}

fn encode_checkpoint_optional_weights(value: Option<FormWeights>, bytes: &mut Vec<u8>) {
    match value {
        Some(weights) => {
            bytes.push(1);
            encode_checkpoint_weights(weights, bytes);
        }
        None => bytes.push(0),
    }
}

fn decode_checkpoint_optional_weights(
    cursor: &mut CheckpointCursor<'_>,
) -> Result<Option<FormWeights>, OuroborosCheckpointError> {
    match cursor.u8()? {
        0 => Ok(None),
        1 => Ok(Some(decode_checkpoint_weights(cursor)?)),
        other => Err(OuroborosCheckpointError::InvalidOptionFlag(other)),
    }
}

fn encode_checkpoint_optional_f32(value: Option<f32>, bytes: &mut Vec<u8>) {
    match value {
        Some(number) => {
            bytes.push(1);
            checkpoint_push_f32(bytes, number);
        }
        None => bytes.push(0),
    }
}

fn decode_checkpoint_optional_f32(
    cursor: &mut CheckpointCursor<'_>,
) -> Result<Option<f32>, OuroborosCheckpointError> {
    match cursor.u8()? {
        0 => Ok(None),
        1 => Ok(Some(cursor.f32()?)),
        other => Err(OuroborosCheckpointError::InvalidOptionFlag(other)),
    }
}

fn checkpoint_form_tag(form: OuroborosForm) -> u8 {
    match form {
        OuroborosForm::Nature => 0,
        OuroborosForm::Humanity => 1,
        OuroborosForm::Digital => 2,
        OuroborosForm::Enlightenment => 3,
    }
}

fn checkpoint_decode_form(value: u8) -> Result<OuroborosForm, OuroborosCheckpointError> {
    match value {
        0 => Ok(OuroborosForm::Nature),
        1 => Ok(OuroborosForm::Humanity),
        2 => Ok(OuroborosForm::Digital),
        3 => Ok(OuroborosForm::Enlightenment),
        other => Err(OuroborosCheckpointError::InvalidTag(other)),
    }
}

fn checkpoint_motion_tag(mode: OuroborosMotionMode) -> u8 {
    match mode {
        OuroborosMotionMode::Reduced => 0,
        OuroborosMotionMode::LowPower => 1,
        OuroborosMotionMode::Balanced => 2,
        OuroborosMotionMode::Cinematic => 3,
    }
}

fn checkpoint_decode_motion(value: u8) -> Result<OuroborosMotionMode, OuroborosCheckpointError> {
    match value {
        0 => Ok(OuroborosMotionMode::Reduced),
        1 => Ok(OuroborosMotionMode::LowPower),
        2 => Ok(OuroborosMotionMode::Balanced),
        3 => Ok(OuroborosMotionMode::Cinematic),
        other => Err(OuroborosCheckpointError::InvalidTag(other)),
    }
}

fn checkpoint_receipt(bytes: &[u8]) -> OuroborosCheckpointReceipt {
    OuroborosCheckpointReceipt {
        schema_version: CHECKPOINT_VERSION,
        scene_contract_version: contract::SCENE_CONTRACT_VERSION,
        byte_length: bytes.len() as u16,
        digest: *blake3::hash(bytes).as_bytes(),
    }
}

struct CheckpointCursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> CheckpointCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], OuroborosCheckpointError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(OuroborosCheckpointError::Truncated)?;
        let source = self
            .bytes
            .get(self.offset..end)
            .ok_or(OuroborosCheckpointError::Truncated)?;
        let mut output = [0_u8; N];
        output.copy_from_slice(source);
        self.offset = end;
        Ok(output)
    }

    fn u8(&mut self) -> Result<u8, OuroborosCheckpointError> {
        Ok(self.take::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, OuroborosCheckpointError> {
        Ok(u16::from_le_bytes(self.take::<2>()?))
    }

    fn f32(&mut self) -> Result<f32, OuroborosCheckpointError> {
        Ok(f32::from_bits(u32::from_le_bytes(self.take::<4>()?)))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

fn dot3(left: [f32; 3], right: [f32; 3]) -> f32 {
    left[0] * right[0] + left[1] * right[1] + left[2] * right[2]
}

fn sub3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] - right[0], left[1] - right[1], left[2] - right[2]]
}

fn distance3(left: [f32; 3], right: [f32; 3]) -> f32 {
    let delta = sub3(left, right);
    dot3(delta, delta).sqrt()
}

fn add3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [left[0] + right[0], left[1] + right[1], left[2] + right[2]]
}

fn scale3(vector: [f32; 3], scale: f32) -> [f32; 3] {
    [vector[0] * scale, vector[1] * scale, vector[2] * scale]
}

fn cross3(left: [f32; 3], right: [f32; 3]) -> [f32; 3] {
    [
        left[1] * right[2] - left[2] * right[1],
        left[2] * right[0] - left[0] * right[2],
        left[0] * right[1] - left[1] * right[0],
    ]
}

fn normalize3(vector: [f32; 3]) -> [f32; 3] {
    let length = (vector[0] * vector[0] + vector[1] * vector[1] + vector[2] * vector[2]).sqrt();
    if !length.is_finite() || length <= f32::EPSILON {
        [1.0, 0.0, 0.0]
    } else {
        [vector[0] / length, vector[1] / length, vector[2] / length]
    }
}

fn append_state_f32(bytes: &mut Vec<u8>, value: f32) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scene(form: OuroborosForm) -> OuroborosScene {
        OuroborosScene::new(OuroborosConfig::default(), form).expect("default config is valid")
    }

    fn advance_for(scene: &mut OuroborosScene, seconds: f32) {
        let mut remaining = seconds;
        while remaining > 0.0 {
            let dt = remaining.min(0.1);
            scene.advance(dt).expect("bounded step is valid");
            remaining -= dt;
        }
    }

    #[test]
    fn active_checkpoint_restores_the_exact_c1_trajectory() {
        let mut original = scene(OuroborosForm::Nature);
        original.transition_to(OuroborosForm::Digital);
        advance_for(&mut original, 2.35);
        original
            .set_attention_target(AttentionTarget::new(0.6, -0.35).unwrap())
            .unwrap();
        original.advance(0.12).unwrap();

        let (bytes, receipt) = original.encode_checkpoint().unwrap();
        assert!(bytes.len() <= MAX_CHECKPOINT_BYTES);
        assert_eq!(receipt.byte_length as usize, bytes.len());
        assert_eq!(receipt.digest, *blake3::hash(&bytes).as_bytes());

        let (mut restored, restored_receipt) = OuroborosScene::restore_checkpoint(&bytes).unwrap();
        assert_eq!(receipt, restored_receipt);
        assert_eq!(original.frame(), restored.frame());
        assert_eq!(original.current_velocity(), restored.current_velocity());
        assert_eq!(
            original.canonical_state_bytes(),
            restored.canonical_state_bytes()
        );

        for _ in 0..1_000 {
            original.advance(1.0 / 60.0).unwrap();
            restored.advance(1.0 / 60.0).unwrap();
            assert_eq!(original.frame(), restored.frame());
            assert_eq!(original.current_velocity(), restored.current_velocity());
            assert_eq!(
                original.canonical_state_bytes(),
                restored.canonical_state_bytes()
            );
        }
    }

    #[test]
    fn reduced_checkpoint_restores_frozen_kinematics_and_resume() {
        let mut original = scene(OuroborosForm::Humanity);
        original.transition_to(OuroborosForm::Enlightenment);
        advance_for(&mut original, 1.75);
        original.set_motion_mode(OuroborosMotionMode::Reduced);

        let (bytes, receipt) = original.encode_checkpoint().unwrap();
        let (mut restored, restored_receipt) = OuroborosScene::restore_checkpoint(&bytes).unwrap();
        assert_eq!(receipt, restored_receipt);
        assert_eq!(restored.motion_mode(), OuroborosMotionMode::Reduced);
        assert_eq!(original.frame(), restored.frame());
        assert_eq!(original.current_velocity(), restored.current_velocity());
        assert_eq!(
            original.canonical_state_bytes(),
            restored.canonical_state_bytes()
        );

        // Reduced scenes remain completely frozen until the host explicitly
        // resumes them. The restored kinematics must match the source exactly.
        for _ in 0..1_000 {
            original.advance(0.025).unwrap();
            restored.advance(0.025).unwrap();
        }
        assert_eq!(original.frame(), restored.frame());
        assert_eq!(original.current_velocity(), restored.current_velocity());

        original.set_motion_mode(OuroborosMotionMode::Cinematic);
        restored.set_motion_mode(OuroborosMotionMode::Cinematic);
        assert_eq!(original.frame(), restored.frame());
        assert_eq!(original.current_velocity(), restored.current_velocity());

        for _ in 0..1_000 {
            original.advance(0.025).unwrap();
            restored.advance(0.025).unwrap();
            assert_eq!(original.frame(), restored.frame());
            assert_eq!(original.current_velocity(), restored.current_velocity());
            assert_eq!(
                original.canonical_state_bytes(),
                restored.canonical_state_bytes()
            );
        }
    }

    #[test]
    fn checkpoints_round_trip_every_active_motion_policy() {
        for form in OuroborosForm::ALL {
            for mode in [
                OuroborosMotionMode::Reduced,
                OuroborosMotionMode::LowPower,
                OuroborosMotionMode::Balanced,
                OuroborosMotionMode::Cinematic,
            ] {
                let mut source = scene(form);
                let target = OuroborosForm::ALL[(checkpoint_form_tag(form) as usize + 1) % 4];
                source.transition_to(target);
                advance_for(&mut source, 1.25);
                source.set_motion_mode(mode);

                let (bytes, receipt) = source.encode_checkpoint().unwrap();
                let (restored, restored_receipt) =
                    OuroborosScene::restore_checkpoint(&bytes).unwrap();
                assert_eq!(receipt, restored_receipt);
                assert_eq!(source.frame(), restored.frame());
                assert_eq!(source.current_velocity(), restored.current_velocity());
                assert_eq!(
                    source.canonical_state_bytes(),
                    restored.canonical_state_bytes()
                );
            }
        }
    }

    #[test]
    fn checkpoint_decoder_rejects_malformed_and_unsupported_inputs() {
        let scene = scene(OuroborosForm::Nature);
        let encoded = scene.encode_checkpoint().unwrap().0;

        let mut bad_magic = encoded.clone();
        bad_magic[0] ^= 0xff;
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&bad_magic),
            Err(OuroborosCheckpointError::InvalidMagic)
        ));

        let mut bad_version = encoded.clone();
        bad_version[8..10].copy_from_slice(&99_u16.to_le_bytes());
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&bad_version),
            Err(OuroborosCheckpointError::UnsupportedVersion(99))
        ));

        let mut bad_contract = encoded.clone();
        bad_contract[10..12].copy_from_slice(&99_u16.to_le_bytes());
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&bad_contract),
            Err(OuroborosCheckpointError::UnsupportedSceneContract(99))
        ));

        let mut bad_form = encoded.clone();
        bad_form[60] = 0xff;
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&bad_form),
            Err(OuroborosCheckpointError::InvalidTag(0xff))
        ));

        let mut bad_motion = encoded.clone();
        bad_motion[61] = 0xff;
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&bad_motion),
            Err(OuroborosCheckpointError::InvalidTag(0xff))
        ));

        // An active scene has three absent optional fields at bytes 118..121.
        let mut bad_option = encoded.clone();
        bad_option[118] = 2;
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&bad_option),
            Err(OuroborosCheckpointError::InvalidOptionFlag(2))
        ));

        let mut non_finite = encoded.clone();
        non_finite[12..16].copy_from_slice(&f32::NAN.to_bits().to_le_bytes());
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&non_finite),
            Err(OuroborosCheckpointError::InvalidConfiguration)
        ));

        let mut trailing = encoded.clone();
        trailing.push(0);
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&trailing),
            Err(OuroborosCheckpointError::TrailingBytes)
        ));

        let oversized = vec![0_u8; MAX_CHECKPOINT_BYTES + 1];
        assert!(matches!(
            OuroborosScene::restore_checkpoint(&oversized),
            Err(OuroborosCheckpointError::TooLarge)
        ));

        for length in 0..encoded.len() {
            assert!(matches!(
                OuroborosScene::restore_checkpoint(&encoded[..length]),
                Err(OuroborosCheckpointError::Truncated)
            ));
        }
    }

    #[test]
    fn checkpoint_validation_rejects_out_of_range_kinematics() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.transition_to(OuroborosForm::Digital);
        advance_for(&mut scene, 1.0);

        let mut invalid_velocity = scene.clone();
        invalid_velocity.transition_start_velocity.digital = f32::INFINITY;
        assert_eq!(
            invalid_velocity.encode_checkpoint(),
            Err(OuroborosCheckpointError::InvalidState)
        );

        let mut invalid_timing = scene.clone();
        invalid_timing.transition_durations[2] = f32::NAN;
        assert_eq!(
            invalid_timing.encode_checkpoint(),
            Err(OuroborosCheckpointError::InvalidState)
        );

        let mut invalid_pose = scene;
        invalid_pose.head_pose.yaw_radians = f32::INFINITY;
        assert_eq!(
            invalid_pose.encode_checkpoint(),
            Err(OuroborosCheckpointError::InvalidState)
        );
    }

    #[test]
    fn checkpoint_schema_and_size_are_stable_for_active_and_reduced_states() {
        let mut active = scene(OuroborosForm::Nature);
        active.transition_to(OuroborosForm::Humanity);
        advance_for(&mut active, 1.0);
        let (active_bytes, active_receipt) = active.encode_checkpoint().unwrap();

        let mut reduced = active;
        reduced.set_motion_mode(OuroborosMotionMode::Reduced);
        let (reduced_bytes, reduced_receipt) = reduced.encode_checkpoint().unwrap();

        assert_eq!(active_receipt.schema_version, CHECKPOINT_VERSION);
        assert_eq!(
            active_receipt.scene_contract_version,
            contract::SCENE_CONTRACT_VERSION
        );
        assert_eq!(reduced_receipt.schema_version, CHECKPOINT_VERSION);
        assert_eq!(active_bytes.len(), 121);
        assert_eq!(reduced_bytes.len(), 157);
        assert!(active_bytes.len() <= 256);
        assert!(reduced_bytes.len() <= 256);
        assert!(reduced_bytes.len() > active_bytes.len());
        assert_ne!(active_receipt.digest, reduced_receipt.digest);
    }

    #[test]
    fn every_form_has_a_stable_round_trip_id() {
        for form in OuroborosForm::ALL {
            assert_eq!(OuroborosForm::from_id(form.id()), Some(form));
        }
        assert_eq!(OuroborosForm::from_id("NATURE"), None);
        assert_eq!(OuroborosForm::from_id("unknown"), None);
    }

    #[test]
    fn default_identity_and_head_anchor_are_stable() {
        for form in OuroborosForm::ALL {
            let scene = scene(form);
            assert_eq!(scene.frame().head_anchor, [0.38, 0.0, 0.0]);
            assert_eq!(
                scene.body_sample(0, 128).unwrap().centerline,
                [0.38, 0.0, 0.0]
            );
        }
    }

    #[test]
    fn semantic_forms_have_distinct_material_weights() {
        let values = OuroborosForm::ALL.map(OuroborosForm::weights);
        for (index, weights) in values.iter().enumerate() {
            assert!(weights.is_finite_and_bounded());
            assert!(!values[..index].contains(weights));
        }
    }

    #[test]
    fn transition_reaches_target_without_changing_identity_anchor() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.transition_to(OuroborosForm::Enlightenment);
        // Balanced mode runs at 0.70x; include that policy in the duration.
        advance_for(&mut scene, 12.0);

        let frame = scene.frame();
        assert_eq!(frame.form_weights, OuroborosForm::Enlightenment.weights());
        assert_eq!(frame.transition_progress, 1.0);
        assert_eq!(scene.current_velocity(), FormWeights::ZERO);
        assert_eq!(frame.head_anchor, [0.38, 0.0, 0.0]);
    }

    #[test]
    fn retargeting_mid_transition_is_position_continuous() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.transition_to(OuroborosForm::Humanity);
        advance_for(&mut scene, 2.0);
        let before = scene.frame().form_weights;

        scene.transition_to(OuroborosForm::Digital);
        let after = scene.frame().form_weights;
        assert_eq!(before, after);
    }

    #[test]
    fn outward_velocity_is_projected_only_at_exact_weight_boundaries() {
        let weights = FormWeights {
            nature: 0.0,
            humanity: 1.0,
            digital: 0.25,
            enlightenment: 0.75,
        };
        let velocity = FormWeights {
            nature: -0.4,
            humanity: 0.3,
            digital: -0.2,
            enlightenment: 0.1,
        };
        assert_eq!(
            project_outward_boundary_velocity(weights, velocity),
            FormWeights {
                nature: 0.0,
                humanity: 0.0,
                digital: -0.2,
                enlightenment: 0.1,
            }
        );

        let interior = FormWeights {
            nature: 1.0e-7,
            humanity: 1.0 - 1.0e-7,
            digital: 0.25,
            enlightenment: 0.75,
        };
        assert_eq!(
            project_outward_boundary_velocity(interior, velocity),
            velocity,
            "interior retargets preserve their derivative exactly"
        );
    }

    #[test]
    fn retargeting_preserves_each_form_channel_velocity() {
        for initial in OuroborosForm::ALL {
            for target in OuroborosForm::ALL {
                for next_target in OuroborosForm::ALL {
                    let mut scene = scene(initial);
                    scene.transition_to(target);
                    advance_for(&mut scene, 2.5);

                    let before_weights = scene.current_weights();
                    let before_velocity = scene.current_velocity();
                    scene.transition_to(next_target);

                    assert_eq!(scene.current_weights(), before_weights);
                    let after_velocity = scene.current_velocity();
                    let before = [
                        before_velocity.nature,
                        before_velocity.humanity,
                        before_velocity.digital,
                        before_velocity.enlightenment,
                    ];
                    let after = [
                        after_velocity.nature,
                        after_velocity.humanity,
                        after_velocity.digital,
                        after_velocity.enlightenment,
                    ];
                    for channel in 0..4 {
                        assert!(
                            (before[channel] - after[channel]).abs() <= 1.0e-6,
                            "velocity discontinuity: {initial:?}->{target:?}->{next_target:?}, channel {channel}"
                        );
                    }

                    // Confirm the analytic derivative also matches a short
                    // finite-difference observation immediately after retarget.
                    let dt = 0.001;
                    let scene_time_delta = dt * scene.motion_mode.time_scale();
                    scene.advance(dt).unwrap();
                    let next = scene.current_weights();
                    let current = [
                        before_weights.nature,
                        before_weights.humanity,
                        before_weights.digital,
                        before_weights.enlightenment,
                    ];
                    let following = [next.nature, next.humanity, next.digital, next.enlightenment];
                    for channel in 0..4 {
                        let observed = (following[channel] - current[channel]) / scene_time_delta;
                        assert!(
                            (observed - after[channel]).abs() <= 5.0e-3,
                            "finite-difference velocity mismatch: {initial:?}->{target:?}->{next_target:?}, channel {channel}, observed={observed}, analytic={}",
                            after[channel]
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn repeated_retargets_keep_all_form_weights_finite_and_bounded() {
        for initial in OuroborosForm::ALL {
            for first_target in OuroborosForm::ALL {
                for second_target in OuroborosForm::ALL {
                    let mut scene = scene(initial);
                    scene.transition_to(first_target);
                    advance_for(&mut scene, 2.0);
                    scene.transition_to(second_target);

                    for step in 0..480 {
                        scene.advance(0.025).unwrap();
                        let weights = scene.frame().form_weights;
                        assert!(
                            weights.is_finite_and_bounded(),
                            "unbounded weights at adversarial step {step}: {weights:?}"
                        );
                    }
                    assert_eq!(scene.frame().form_weights, second_target.weights());
                    assert_eq!(scene.current_velocity(), FormWeights::ZERO);
                }
            }
        }
    }

    #[test]
    fn deterministic_adversarial_retarget_chains_preserve_bounds_and_finite_velocity() {
        let mut scene = scene(OuroborosForm::Nature);
        let mut random = 0x6d2b_79f5_u32;
        let modes = [
            OuroborosMotionMode::LowPower,
            OuroborosMotionMode::Balanced,
            OuroborosMotionMode::Cinematic,
            OuroborosMotionMode::Reduced,
        ];

        for step in 0..512 {
            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let target = OuroborosForm::ALL[(random as usize) % OuroborosForm::ALL.len()];
            scene.transition_to(target);
            assert!(
                scene
                    .transition_durations
                    .iter()
                    .all(|duration| duration.is_finite() && *duration > 0.0)
            );
            assert!(hermite_segments_are_bounded(
                scene.from_weights,
                scene.target_form.weights(),
                scene.transition_start_velocity,
                scene.transition_durations
            ));

            random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            let frames = (random >> 24) as usize;
            for _ in 0..frames {
                scene.advance(0.025).unwrap();
                let weights = scene.current_weights();
                let velocity = scene.current_velocity();
                assert!(weights.is_finite_and_bounded());
                assert!(
                    [
                        velocity.nature,
                        velocity.humanity,
                        velocity.digital,
                        velocity.enlightenment,
                    ]
                    .into_iter()
                    .all(f32::is_finite)
                );
                if frames % 16 == 0 {
                    assert!(scene.encode_checkpoint().unwrap().0.len() <= MAX_CHECKPOINT_BYTES);
                }
            }

            // Force frequent boundary-adjacent freeze/resume cycles as well
            // as all four motion policies; Reduced retargets must be static.
            if step % 7 == 0 {
                let before_reduce = scene.current_weights();
                let velocity_before_reduce = scene.current_velocity();
                scene.set_motion_mode(OuroborosMotionMode::Reduced);
                assert_eq!(scene.current_weights(), before_reduce);
                assert_eq!(scene.current_velocity(), velocity_before_reduce);

                random = random.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
                let reduced_target =
                    OuroborosForm::ALL[(random as usize) % OuroborosForm::ALL.len()];
                scene.transition_to(reduced_target);
                assert_eq!(scene.current_weights(), reduced_target.weights());
                assert_eq!(scene.current_velocity(), FormWeights::ZERO);

                let resume_mode = modes[(step / 7) % 3];
                scene.set_motion_mode(resume_mode);
                assert_eq!(scene.current_weights(), reduced_target.weights());
                assert_eq!(scene.current_velocity(), FormWeights::ZERO);
                assert!(scene.encode_checkpoint().is_ok());
            }
        }
    }

    #[test]
    fn semantic_landmarks_preserve_root_and_proportions_across_all_forms() {
        let expected = scene(OuroborosForm::Nature).landmarks();
        for form in OuroborosForm::ALL {
            let current = scene(form).landmarks();
            assert_eq!(current.head_anchor, [0.38, 0.0, 0.0]);
            assert_eq!(current.head_anchor, expected.head_anchor);
            assert_eq!(current.left_eye, expected.left_eye);
            assert_eq!(current.right_eye, expected.right_eye);
            assert_eq!(current.nose_tip, expected.nose_tip);
            assert_eq!(current.mouth_center, expected.mouth_center);
            assert_eq!(current.crown, expected.crown);
            assert_eq!(current.chin, expected.chin);
            assert_eq!(current.receipt(), expected.receipt());
        }
    }

    #[test]
    fn semantic_landmark_basis_is_finite_orthonormal_and_bounded_at_pose_limits() {
        for target in [
            AttentionTarget::new(-1.0, -1.0).unwrap(),
            AttentionTarget::new(1.0, 1.0).unwrap(),
            AttentionTarget::new(-1.0, 1.0).unwrap(),
            AttentionTarget::new(1.0, -1.0).unwrap(),
        ] {
            let mut scene = scene(OuroborosForm::Humanity);
            scene.set_attention_target(target).unwrap();
            advance_for(&mut scene, 8.0);
            let landmarks = scene.landmarks();
            let dot = |a: [f32; 3], b: [f32; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
            let length = |v: [f32; 3]| dot(v, v).sqrt();

            for axis in [landmarks.forward, landmarks.lateral, landmarks.up] {
                assert!(axis.into_iter().all(f32::is_finite));
                assert!((length(axis) - 1.0).abs() < 1.0e-5);
            }
            assert!(dot(landmarks.forward, landmarks.lateral).abs() < 1.0e-5);
            assert!(dot(landmarks.forward, landmarks.up).abs() < 1.0e-5);
            assert!(dot(landmarks.lateral, landmarks.up).abs() < 1.0e-5);
            assert_eq!(landmarks.head_anchor, scene.frame().head_anchor);

            let radius = scene.config().major_radius;
            for point in [
                landmarks.left_eye,
                landmarks.right_eye,
                landmarks.nose_tip,
                landmarks.mouth_center,
                landmarks.crown,
                landmarks.chin,
            ] {
                assert!(point.into_iter().all(f32::is_finite));
                let offset = [
                    point[0] - landmarks.head_anchor[0],
                    point[1] - landmarks.head_anchor[1],
                    point[2] - landmarks.head_anchor[2],
                ];
                assert!(length(offset) <= radius * 0.20);
            }

            let eye_delta = [
                landmarks.left_eye[0] - landmarks.right_eye[0],
                landmarks.left_eye[1] - landmarks.right_eye[1],
                landmarks.left_eye[2] - landmarks.right_eye[2],
            ];
            assert!((dot(eye_delta, landmarks.forward)).abs() < 1.0e-5);
            assert!((dot(eye_delta, landmarks.up)).abs() < 1.0e-5);
            assert!((dot(eye_delta, landmarks.lateral) + radius * 0.10).abs() < 1.0e-5);
        }
    }

    #[test]
    fn semantic_landmark_validation_rejects_tampered_public_values() {
        let scene = scene(OuroborosForm::Humanity);
        let valid = scene.landmarks();
        assert_eq!(valid.validate(&scene), Ok(()));

        let mut invalid = valid;
        invalid.schema_version += 1;
        assert_eq!(
            invalid.validate(&scene),
            Err(OuroborosLandmarkError::UnsupportedSchemaVersion)
        );

        let mut invalid = valid;
        invalid.head_anchor[0] += 0.01;
        assert_eq!(
            invalid.validate(&scene),
            Err(OuroborosLandmarkError::HeadRootMismatch)
        );

        let mut invalid = valid;
        invalid.forward[2] = f32::NAN;
        assert_eq!(
            invalid.validate(&scene),
            Err(OuroborosLandmarkError::NonFiniteCoordinate)
        );

        let mut invalid = valid;
        invalid.lateral = invalid.forward;
        assert_eq!(
            invalid.validate(&scene),
            Err(OuroborosLandmarkError::InvalidBasis)
        );

        let mut invalid = valid;
        invalid.left_eye = add3(invalid.left_eye, scale3(invalid.lateral, scene.config().major_radius * 0.04));
        assert_eq!(
            invalid.validate(&scene),
            Err(OuroborosLandmarkError::AsymmetricEyes)
        );

        let mut invalid = valid;
        invalid.nose_tip[0] += 1.0;
        assert_eq!(
            invalid.validate(&scene),
            Err(OuroborosLandmarkError::LandmarkOutOfBounds)
        );

        let mut invalid = valid;
        invalid.forward = [0.0, 1.0, 0.0];
        invalid.lateral = [-1.0, 0.0, 0.0];
        invalid.up = [0.0, 0.0, 1.0];
        assert_eq!(
            invalid.validate(&scene),
            Err(OuroborosLandmarkError::PoseMismatch)
        );
    }

    #[test]
    fn landmark_serialization_is_canonical_and_pose_sensitive() {
        let mut scene = scene(OuroborosForm::Digital);
        let first = scene.landmarks();
        let second = scene.landmarks();
        assert_eq!(first.canonical_bytes(), second.canonical_bytes());
        assert_eq!(first.receipt(), second.receipt());
        assert_eq!(first.canonical_bytes().len(), 132);
        assert_eq!(first.receipt().byte_length, 132);
        assert_eq!(
            first.receipt().digest,
            *blake3::hash(&first.canonical_bytes()).as_bytes()
        );

        scene
            .set_attention_target(AttentionTarget::new(1.0, -1.0).unwrap())
            .unwrap();
        advance_for(&mut scene, 8.0);
        let turned = scene.landmarks();
        assert_eq!(turned.head_anchor, first.head_anchor);
        assert_ne!(turned.receipt().digest, first.receipt().digest);
    }

    #[test]
    fn head_tracks_only_a_bounded_normalized_host_target() {
        let mut scene = scene(OuroborosForm::Nature);
        scene
            .set_attention_target(AttentionTarget::new(1.0, -1.0).unwrap())
            .unwrap();
        advance_for(&mut scene, 2.0);

        let pose = scene.head_pose();
        assert!(pose.yaw_radians > 0.0);
        assert!(pose.pitch_radians > 0.0);
        assert!(pose.yaw_radians <= scene.config().max_yaw_radians);
        assert!(pose.pitch_radians <= scene.config().max_pitch_radians);
    }

    #[test]
    fn invalid_attention_and_time_inputs_are_rejected() {
        assert_eq!(
            AttentionTarget::new(f32::NAN, 0.0),
            Err(OuroborosError::InvalidAttentionTarget)
        );
        assert_eq!(
            AttentionTarget::new(1.1, 0.0),
            Err(OuroborosError::InvalidAttentionTarget)
        );

        let mut scene = scene(OuroborosForm::Nature);
        assert_eq!(
            scene.advance(0.26),
            Err(OuroborosError::InvalidStep(StepError::InvalidDelta))
        );
    }

    #[test]
    fn invalid_configuration_is_rejected_before_scene_creation() {
        let config = OuroborosConfig {
            face_response: f32::NAN,
            ..OuroborosConfig::default()
        };
        assert_eq!(
            OuroborosScene::new(config, OuroborosForm::Nature).err(),
            Some(OuroborosError::InvalidConfiguration)
        );
    }

    #[test]
    fn reduced_mode_freezes_a_mid_transition_at_its_actual_progress() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.transition_to(OuroborosForm::Humanity);
        advance_for(&mut scene, 2.0);
        let visible = scene.frame();

        scene.set_motion_mode(OuroborosMotionMode::Reduced);
        let frozen = scene.frame();
        assert_eq!(frozen.form_weights, visible.form_weights);
        assert_eq!(frozen.transition_progress, visible.transition_progress);
        assert!(frozen.transition_progress < 1.0);

        // An explicit form selection during Reduced mode changes the still
        // immediately and reports the new target as complete.
        scene.transition_to(OuroborosForm::Digital);
        let selected = scene.frame();
        assert_eq!(selected.form_weights, OuroborosForm::Digital.weights());
        assert_eq!(selected.transition_progress, 1.0);

        // Reduced mode remains static at the newly selected target; it does
        // not revert to the composition that was frozen before retargeting.
        for _ in 0..60 {
            scene.advance(0.1).unwrap();
        }
        assert_eq!(scene.frame(), selected);
    }

    #[test]
    fn reduced_motion_freezes_morph_and_attention() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.set_motion_mode(OuroborosMotionMode::Reduced);
        scene.transition_to(OuroborosForm::Digital);
        scene
            .set_attention_target(AttentionTarget::new(1.0, 1.0).unwrap())
            .unwrap();
        let first = scene.frame();

        for _ in 0..60 {
            scene.advance(0.1).unwrap();
        }
        assert_eq!(scene.frame(), first);
    }

    #[test]
    fn leaving_reduced_motion_resumes_without_a_form_snap() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.set_motion_mode(OuroborosMotionMode::Reduced);
        scene.transition_to(OuroborosForm::Humanity);
        let frozen = scene.frame();

        scene.set_motion_mode(OuroborosMotionMode::Balanced);
        assert_eq!(scene.frame().form_weights, frozen.form_weights);
        assert_eq!(scene.frame().transition_progress, 1.0);
        advance_for(&mut scene, 12.0);
        assert_eq!(
            scene.frame().form_weights,
            OuroborosForm::Humanity.weights()
        );
        assert_eq!(scene.frame().transition_progress, 1.0);
    }

    #[test]
    fn resuming_a_frozen_mid_transition_preserves_global_progress() {
        let mut scene = scene(OuroborosForm::Nature);
        scene.transition_to(OuroborosForm::Humanity);
        advance_for(&mut scene, 2.0);
        let before_reduce = scene.frame();

        let velocity_before_reduce = scene.current_velocity();
        scene.set_motion_mode(OuroborosMotionMode::Reduced);
        let frozen = scene.frame();
        assert_eq!(frozen.form_weights, before_reduce.form_weights);
        assert_eq!(
            frozen.transition_progress,
            before_reduce.transition_progress
        );
        assert_eq!(scene.current_velocity(), velocity_before_reduce);

        scene.set_motion_mode(OuroborosMotionMode::Balanced);
        let resumed = scene.frame();
        assert_eq!(resumed.form_weights, frozen.form_weights);
        assert_eq!(resumed.transition_progress, frozen.transition_progress);
        assert_eq!(scene.current_velocity(), velocity_before_reduce);

        advance_for(&mut scene, 12.0);
        assert_eq!(
            scene.frame().form_weights,
            OuroborosForm::Humanity.weights()
        );
        assert_eq!(scene.frame().transition_progress, 1.0);
    }

    #[test]
    fn body_sampling_is_bounded_and_deterministic() {
        let scene = scene(OuroborosForm::Digital);
        let a = scene.body_sample(37, 256).unwrap();
        let b = scene.body_sample(37, 256).unwrap();
        assert_eq!(a, b);
        assert!(scene.body_sample(256, 256).is_err());
        assert!(scene.body_sample(0, 7).is_err());
        assert!(scene.body_sample(0, 4097).is_err());
    }

    #[test]
    fn same_replay_inputs_produce_the_same_frame_and_samples() {
        let mut left = scene(OuroborosForm::Nature);
        let mut right = scene(OuroborosForm::Nature);
        for scene in [&mut left, &mut right] {
            scene.transition_to(OuroborosForm::Digital);
            scene
                .set_attention_target(AttentionTarget::new(-0.5, 0.25).unwrap())
                .unwrap();
            advance_for(scene, 3.0);
        }

        assert_eq!(left.frame(), right.frame());
        for index in 0..128 {
            assert_eq!(
                left.body_sample(index, 128).unwrap(),
                right.body_sample(index, 128).unwrap()
            );
        }
    }
}
