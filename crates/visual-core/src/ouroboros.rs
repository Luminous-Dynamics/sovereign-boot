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
    fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        let channel = |a: f32, b: f32| a + (b - a) * t;
        Self {
            nature: channel(self.nature, other.nature),
            humanity: channel(self.humanity, other.humanity),
            digital: channel(self.digital, other.digital),
            enlightenment: channel(self.enlightenment, other.enlightenment),
        }
    }

    fn is_finite_and_bounded(self) -> bool {
        [self.nature, self.humanity, self.digital, self.enlightenment]
            .into_iter()
            .all(|value| value.is_finite() && (0.0..=1.0).contains(&value))
    }
}

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
    transition_duration: f32,
    transition_start_progress: f32,
    attention_target: AttentionTarget,
    head_pose: HeadPose,
    motion_mode: OuroborosMotionMode,
    frozen_weights: Option<FormWeights>,
    frozen_progress: Option<f32>,
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
            transition_duration: config.transition_seconds,
            transition_start_progress: 0.0,
            attention_target: AttentionTarget::CENTER,
            head_pose: HeadPose::default(),
            motion_mode: OuroborosMotionMode::Balanced,
            frozen_weights: None,
            frozen_progress: None,
        })
    }

    pub fn config(&self) -> OuroborosConfig {
        self.config
    }

    /// Canonical internal state bytes for replay evidence.
    ///
    /// This includes pending attention and transition state, not just the
    /// currently visible frame. It is crate-private because the internal
    /// checkpoint layout is versioned with the scene contract.
    pub(crate) fn canonical_state_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(160);
        bytes.extend_from_slice(b"OUROSTA1");
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
            self.transition_duration,
            self.transition_start_progress,
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
                // Freeze the currently visible composition, not a future frame.
                self.frozen_weights = Some(self.current_weights());
                self.frozen_progress = Some(self.transition_progress());
            }
            _ => {
                if let Some(frozen) = self.frozen_weights.take() {
                    // Continue the remaining fraction of the original
                    // transition without a visual snap or false progress reset.
                    let progress = self.frozen_progress.take().unwrap_or(0.0);
                    self.from_weights = frozen;
                    if progress >= 1.0 {
                        self.transition_start_progress = 0.0;
                        self.transition_duration = self.config.transition_seconds;
                        self.transition_elapsed = self.transition_duration;
                    } else {
                        self.transition_start_progress = progress;
                        self.transition_duration =
                            self.config.transition_seconds * (1.0 - progress);
                        self.transition_elapsed = 0.0;
                    }
                }
            }
        }
        self.motion_mode = mode;
    }

    /// Begin a transition to another semantic expression.
    ///
    /// Re-targeting mid-transition starts from the currently evaluated weights,
    /// so changing direction cannot snap the body to an earlier form.
    pub fn transition_to(&mut self, form: OuroborosForm) {
        let current = self.current_weights();
        self.target_form = form;
        self.from_weights = current;
        self.transition_elapsed = 0.0;
        self.transition_duration = self.config.transition_seconds;
        self.transition_start_progress = 0.0;

        if self.motion_mode == OuroborosMotionMode::Reduced {
            self.frozen_weights = Some(form.weights());
            self.frozen_progress = Some(1.0);
            self.from_weights = form.weights();
            self.transition_elapsed = self.transition_duration;
        } else {
            self.frozen_weights = None;
            self.frozen_progress = None;
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
        self.transition_elapsed = (self.transition_elapsed + dt).min(self.transition_duration);

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
        let local = (self.transition_elapsed / self.transition_duration).clamp(0.0, 1.0);
        self.transition_start_progress + (1.0 - self.transition_start_progress) * local
    }

    fn current_weights(&self) -> FormWeights {
        if let Some(weights) = self.frozen_weights {
            return weights;
        }
        // A resumed transition starts from its frozen weights and traverses
        // only the remaining local segment; reported progress retains the
        // original transition's global fraction.
        let local = (self.transition_elapsed / self.transition_duration).clamp(0.0, 1.0);
        let eased = local * local * (3.0 - 2.0 * local);
        self.from_weights.lerp(self.target_form.weights(), eased)
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

        for _ in 0..60 {
            scene.advance(0.1).unwrap();
        }
        assert_eq!(scene.frame(), frozen);
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

        scene.set_motion_mode(OuroborosMotionMode::Reduced);
        let frozen = scene.frame();
        assert_eq!(frozen.form_weights, before_reduce.form_weights);
        assert_eq!(
            frozen.transition_progress,
            before_reduce.transition_progress
        );

        scene.set_motion_mode(OuroborosMotionMode::Balanced);
        let resumed = scene.frame();
        assert_eq!(resumed.form_weights, frozen.form_weights);
        assert_eq!(resumed.transition_progress, frozen.transition_progress);

        advance_for(&mut scene, 12.0);
        assert_eq!(scene.frame().form_weights, OuroborosForm::Humanity.weights());
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
