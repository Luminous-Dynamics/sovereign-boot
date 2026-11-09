//! Canonical, replayable event traces for Ouroboros.
//!
//! A trace makes the scene's inputs explicit: initial configuration, semantic
//! form, host attention targets, motion-policy changes and bounded time steps.
//! The binary encoding is versioned, little-endian and independent of serde.
//! Replaying the same trace with the same scene-contract version produces the
//! same receipt digests on a given target/build. This is not a claim of
//! cross-architecture floating-point or GPU bitwise parity.

use std::fmt;

use crate::{
    contract::{self, SCENE_CONTRACT_VERSION},
    ouroboros::{
        AttentionTarget, OuroborosConfig, OuroborosError, OuroborosForm,
        OuroborosMotionMode, OuroborosScene,
    },
};

const MAGIC: [u8; 8] = *b"OUROTRC1";
const TRACE_VERSION: u16 = 1;
const MAX_EVENTS: usize = 65_536;
const HEADER_SIZE: usize = 50;

/// One explicit host command in a deterministic Ouroboros replay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum OuroborosEvent {
    Advance { dt_seconds: f32 },
    TransitionTo { form: OuroborosForm },
    SetAttentionTarget { target: AttentionTarget },
    SetMotionMode { mode: OuroborosMotionMode },
}

/// A bounded, canonical event sequence from one initial Ouroboros state.
#[derive(Debug, Clone, PartialEq)]
pub struct OuroborosTrace {
    pub initial_config: OuroborosConfig,
    pub initial_form: OuroborosForm,
    pub events: Vec<OuroborosEvent>,
}

/// Content-addressed evidence produced by a deterministic trace replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReplayReceipt {
    pub trace_version: u16,
    pub scene_contract_version: u16,
    pub event_count: u32,
    /// BLAKE3 digest of the complete canonical trace bytes.
    pub trace_digest: [u8; 32],
    /// BLAKE3 digest of the final configuration and renderer-facing frame.
    pub final_state_digest: [u8; 32],
}

/// The final scene and its replay receipt.
#[derive(Debug, Clone)]
pub struct ReplayResult {
    pub scene: OuroborosScene,
    pub receipt: ReplayReceipt,
}

/// Errors from encoding, decoding, validating or replaying a trace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TraceError {
    InvalidMagic,
    UnsupportedTraceVersion(u16),
    UnsupportedSceneContract(u16),
    InvalidReservedBits,
    Truncated,
    TrailingBytes,
    TooManyEvents,
    InvalidForm(u8),
    InvalidMotionMode(u8),
    InvalidEventTag(u8),
    InvalidConfiguration(OuroborosError),
    InvalidAttentionTarget,
    InvalidStep(OuroborosError),
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidMagic => write!(f, "invalid Ouroboros trace magic"),
            Self::UnsupportedTraceVersion(version) => {
                write!(f, "unsupported Ouroboros trace version {version}")
            }
            Self::UnsupportedSceneContract(version) => {
                write!(f, "unsupported Ouroboros scene contract {version}")
            }
            Self::InvalidReservedBits => write!(f, "reserved trace header bits must be zero"),
            Self::Truncated => write!(f, "Ouroboros trace is truncated"),
            Self::TrailingBytes => write!(f, "Ouroboros trace contains trailing bytes"),
            Self::TooManyEvents => write!(f, "Ouroboros trace exceeds the event limit"),
            Self::InvalidForm(value) => write!(f, "invalid Ouroboros form tag {value}"),
            Self::InvalidMotionMode(value) => {
                write!(f, "invalid Ouroboros motion mode tag {value}")
            }
            Self::InvalidEventTag(value) => {
                write!(f, "invalid Ouroboros event tag {value}")
            }
            Self::InvalidConfiguration(error) => {
                write!(f, "invalid Ouroboros configuration: {error}")
            }
            Self::InvalidAttentionTarget => {
                write!(f, "attention target must be finite and normalized to -1..=1")
            }
            Self::InvalidStep(error) => write!(f, "invalid Ouroboros step: {error}"),
        }
    }
}

impl std::error::Error for TraceError {}

impl OuroborosTrace {
    pub fn new(initial_config: OuroborosConfig, initial_form: OuroborosForm) -> Self {
        Self {
            initial_config,
            initial_form,
            events: Vec::new(),
        }
    }

    pub fn push(&mut self, event: OuroborosEvent) -> Result<(), TraceError> {
        if self.events.len() >= MAX_EVENTS {
            return Err(TraceError::TooManyEvents);
        }
        validate_event(event)?;
        self.events.push(event);
        Ok(())
    }

    pub fn validate(&self) -> Result<(), TraceError> {
        self.initial_config
            .validate()
            .map_err(TraceError::InvalidConfiguration)?;
        if self.events.len() > MAX_EVENTS {
            return Err(TraceError::TooManyEvents);
        }
        for event in &self.events {
            validate_event(*event)?;
        }
        Ok(())
    }

    /// Encode a canonical, bounded little-endian trace.
    pub fn encode(&self) -> Result<Vec<u8>, TraceError> {
        self.validate()?;
        let mut bytes = Vec::with_capacity(HEADER_SIZE + self.events.len() * 9);
        bytes.extend_from_slice(&MAGIC);
        bytes.extend_from_slice(&TRACE_VERSION.to_le_bytes());
        bytes.extend_from_slice(&SCENE_CONTRACT_VERSION.to_le_bytes());
        bytes.push(form_tag(self.initial_form));
        bytes.push(0); // Reserved; must stay zero until a new trace version.
        encode_config(self.initial_config, &mut bytes);
        bytes.extend_from_slice(&(self.events.len() as u32).to_le_bytes());

        for event in &self.events {
            match event {
                OuroborosEvent::Advance { dt_seconds } => {
                    bytes.push(0);
                    push_f32(*dt_seconds, &mut bytes);
                }
                OuroborosEvent::TransitionTo { form } => {
                    bytes.push(1);
                    bytes.push(form_tag(*form));
                }
                OuroborosEvent::SetAttentionTarget { target } => {
                    bytes.push(2);
                    push_f32(target.x, &mut bytes);
                    push_f32(target.y, &mut bytes);
                }
                OuroborosEvent::SetMotionMode { mode } => {
                    bytes.push(3);
                    bytes.push(motion_mode_tag(*mode));
                }
            }
        }
        Ok(bytes)
    }

    /// Decode a trace only when its complete header and event stream validate.
    pub fn decode(bytes: &[u8]) -> Result<Self, TraceError> {
        let mut cursor = Cursor::new(bytes);
        if cursor.take::<8>()? != MAGIC {
            return Err(TraceError::InvalidMagic);
        }

        let version = cursor.u16()?;
        if version != TRACE_VERSION {
            return Err(TraceError::UnsupportedTraceVersion(version));
        }
        let scene_contract = cursor.u16()?;
        if scene_contract != SCENE_CONTRACT_VERSION {
            return Err(TraceError::UnsupportedSceneContract(scene_contract));
        }

        let initial_form = decode_form(cursor.u8()?)?;
        if cursor.u8()? != 0 {
            return Err(TraceError::InvalidReservedBits);
        }
        let initial_config = decode_config(&mut cursor)?;
        let event_count = cursor.u32()? as usize;
        if event_count > MAX_EVENTS {
            return Err(TraceError::TooManyEvents);
        }

        let mut events = Vec::with_capacity(event_count);
        for _ in 0..event_count {
            let tag = cursor.u8()?;
            let event = match tag {
                0 => OuroborosEvent::Advance {
                    dt_seconds: cursor.f32()?,
                },
                1 => OuroborosEvent::TransitionTo {
                    form: decode_form(cursor.u8()?)?,
                },
                2 => OuroborosEvent::SetAttentionTarget {
                    target: AttentionTarget {
                        x: cursor.f32()?,
                        y: cursor.f32()?,
                    },
                },
                3 => OuroborosEvent::SetMotionMode {
                    mode: decode_motion_mode(cursor.u8()?)?,
                },
                other => return Err(TraceError::InvalidEventTag(other)),
            };
            validate_event(event)?;
            events.push(event);
        }

        if !cursor.is_empty() {
            return Err(TraceError::TrailingBytes);
        }

        let trace = Self {
            initial_config,
            initial_form,
            events,
        };
        trace.validate()?;
        Ok(trace)
    }

    /// Replay all events and compute content-addressed evidence.
    pub fn replay(&self) -> Result<ReplayResult, TraceError> {
        let encoded = self.encode()?;
        let mut scene = OuroborosScene::new(self.initial_config, self.initial_form)
            .map_err(TraceError::InvalidConfiguration)?;

        for event in &self.events {
            match *event {
                OuroborosEvent::Advance { dt_seconds } => scene
                    .advance(dt_seconds)
                    .map_err(TraceError::InvalidStep)?,
                OuroborosEvent::TransitionTo { form } => scene.transition_to(form),
                OuroborosEvent::SetAttentionTarget { target } => scene
                    .set_attention_target(target)
                    .map_err(|_| TraceError::InvalidAttentionTarget)?,
                OuroborosEvent::SetMotionMode { mode } => scene.set_motion_mode(mode),
            }
        }

        let final_state_bytes = scene.canonical_state_bytes();
        let receipt = ReplayReceipt {
            trace_version: TRACE_VERSION,
            scene_contract_version: SCENE_CONTRACT_VERSION,
            event_count: self.events.len() as u32,
            trace_digest: *blake3::hash(&encoded).as_bytes(),
            final_state_digest: *blake3::hash(&final_state_bytes).as_bytes(),
        };
        Ok(ReplayResult { scene, receipt })
    }
}

fn validate_event(event: OuroborosEvent) -> Result<(), TraceError> {
    match event {
        OuroborosEvent::Advance { dt_seconds } => contract::validate_step(dt_seconds, 0.0)
            .map_err(|error| TraceError::InvalidStep(OuroborosError::InvalidStep(error))),
        OuroborosEvent::TransitionTo { .. } => Ok(()),
        OuroborosEvent::SetAttentionTarget { target } => AttentionTarget::new(target.x, target.y)
            .map(|_| ())
            .map_err(|_| TraceError::InvalidAttentionTarget),
        OuroborosEvent::SetMotionMode { .. } => Ok(()),
    }
}

fn form_tag(form: OuroborosForm) -> u8 {
    match form {
        OuroborosForm::Nature => 0,
        OuroborosForm::Humanity => 1,
        OuroborosForm::Digital => 2,
        OuroborosForm::Enlightenment => 3,
    }
}

fn decode_form(value: u8) -> Result<OuroborosForm, TraceError> {
    match value {
        0 => Ok(OuroborosForm::Nature),
        1 => Ok(OuroborosForm::Humanity),
        2 => Ok(OuroborosForm::Digital),
        3 => Ok(OuroborosForm::Enlightenment),
        other => Err(TraceError::InvalidForm(other)),
    }
}

fn motion_mode_tag(mode: OuroborosMotionMode) -> u8 {
    match mode {
        OuroborosMotionMode::Reduced => 0,
        OuroborosMotionMode::LowPower => 1,
        OuroborosMotionMode::Balanced => 2,
        OuroborosMotionMode::Cinematic => 3,
    }
}

fn decode_motion_mode(value: u8) -> Result<OuroborosMotionMode, TraceError> {
    match value {
        0 => Ok(OuroborosMotionMode::Reduced),
        1 => Ok(OuroborosMotionMode::LowPower),
        2 => Ok(OuroborosMotionMode::Balanced),
        3 => Ok(OuroborosMotionMode::Cinematic),
        other => Err(TraceError::InvalidMotionMode(other)),
    }
}

fn push_f32(value: f32, bytes: &mut Vec<u8>) {
    bytes.extend_from_slice(&value.to_bits().to_le_bytes());
}

fn encode_config(config: OuroborosConfig, bytes: &mut Vec<u8>) {
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
        push_f32(value, bytes);
    }
}

fn decode_config(cursor: &mut Cursor<'_>) -> Result<OuroborosConfig, TraceError> {
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
        .map_err(TraceError::InvalidConfiguration)?;
    Ok(config)
}

struct Cursor<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    fn take<const N: usize>(&mut self) -> Result<[u8; N], TraceError> {
        let end = self
            .offset
            .checked_add(N)
            .ok_or(TraceError::Truncated)?;
        let slice = self
            .bytes
            .get(self.offset..end)
            .ok_or(TraceError::Truncated)?;
        let mut value = [0_u8; N];
        value.copy_from_slice(slice);
        self.offset = end;
        Ok(value)
    }

    fn u8(&mut self) -> Result<u8, TraceError> {
        Ok(self.take::<1>()?[0])
    }

    fn u16(&mut self) -> Result<u16, TraceError> {
        Ok(u16::from_le_bytes(self.take::<2>()?))
    }

    fn u32(&mut self) -> Result<u32, TraceError> {
        Ok(u32::from_le_bytes(self.take::<4>()?))
    }

    fn f32(&mut self) -> Result<f32, TraceError> {
        Ok(f32::from_bits(u32::from_le_bytes(self.take::<4>()?)))
    }

    fn is_empty(&self) -> bool {
        self.offset == self.bytes.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn representative_trace() -> OuroborosTrace {
        let mut trace = OuroborosTrace::new(
            OuroborosConfig::default(),
            OuroborosForm::Nature,
        );
        trace
            .push(OuroborosEvent::SetMotionMode {
                mode: OuroborosMotionMode::Cinematic,
            })
            .unwrap();
        trace
            .push(OuroborosEvent::TransitionTo {
                form: OuroborosForm::Digital,
            })
            .unwrap();
        trace
            .push(OuroborosEvent::SetAttentionTarget {
                target: AttentionTarget::new(0.45, -0.2).unwrap(),
            })
            .unwrap();
        for _ in 0..16 {
            trace
                .push(OuroborosEvent::Advance { dt_seconds: 0.1 })
                .unwrap();
        }
        trace
            .push(OuroborosEvent::SetMotionMode {
                mode: OuroborosMotionMode::Reduced,
            })
            .unwrap();
        trace
    }

    #[test]
    fn canonical_encoding_round_trips_exactly() {
        let trace = representative_trace();
        let encoded = trace.encode().unwrap();
        let decoded = OuroborosTrace::decode(&encoded).unwrap();
        assert_eq!(decoded, trace);
        assert_eq!(decoded.encode().unwrap(), encoded);
        assert_eq!(encoded.len(), HEADER_SIZE + 16 * 5 + 2 + 2 + 9 + 2);
    }

    #[test]
    fn identical_replays_produce_identical_receipts_and_frames() {
        let trace = representative_trace();
        let left = trace.replay().unwrap();
        let right = trace.replay().unwrap();

        assert_eq!(left.receipt, right.receipt);
        assert_eq!(left.scene.frame(), right.scene.frame());
        assert_eq!(left.receipt.trace_digest, *blake3::hash(&trace.encode().unwrap()).as_bytes());
    }

    #[test]
    fn any_input_change_changes_the_trace_digest() {
        let mut left = representative_trace();
        let mut right = representative_trace();
        right.events[2] = OuroborosEvent::SetAttentionTarget {
            target: AttentionTarget::new(-0.45, -0.2).unwrap(),
        };
        let left_receipt = left.replay().unwrap().receipt;
        let right_receipt = right.replay().unwrap().receipt;

        assert_ne!(left_receipt.trace_digest, right_receipt.trace_digest);
    }

    #[test]
    fn final_state_digest_changes_when_visible_state_changes() {
        let trace = representative_trace();
        let original = trace.replay().unwrap().receipt;

        let mut changed = trace;
        changed.events.push(OuroborosEvent::TransitionTo {
            form: OuroborosForm::Humanity,
        });
        let next = changed.replay().unwrap().receipt;
        assert_ne!(original.trace_digest, next.trace_digest);
        assert_ne!(original.final_state_digest, next.final_state_digest);
    }

    #[test]
    fn decoder_rejects_invalid_headers_tags_and_trailing_data() {
        let encoded = representative_trace().encode().unwrap();

        let mut bad_magic = encoded.clone();
        bad_magic[0] ^= 0xff;
        assert_eq!(
            OuroborosTrace::decode(&bad_magic),
            Err(TraceError::InvalidMagic)
        );

        let mut bad_tag = encoded.clone();
        bad_tag[HEADER_SIZE] = 0xff;
        assert_eq!(
            OuroborosTrace::decode(&bad_tag),
            Err(TraceError::InvalidEventTag(0xff))
        );

        let mut trailing = encoded;
        trailing.push(0);
        assert_eq!(
            OuroborosTrace::decode(&trailing),
            Err(TraceError::TrailingBytes)
        );
    }

    #[test]
    fn encoder_and_decoder_reject_non_finite_or_out_of_range_inputs() {
        let mut invalid_delta = OuroborosTrace::new(
            OuroborosConfig::default(),
            OuroborosForm::Nature,
        );
        invalid_delta.events.push(OuroborosEvent::Advance {
            dt_seconds: f32::NAN,
        });
        assert_eq!(
            invalid_delta.encode(),
            Err(TraceError::InvalidStep(OuroborosError::InvalidStep(
                contract::StepError::InvalidDelta
            )))
        );

        let mut invalid_target = OuroborosTrace::new(
            OuroborosConfig::default(),
            OuroborosForm::Nature,
        );
        invalid_target.events.push(OuroborosEvent::SetAttentionTarget {
            target: AttentionTarget { x: 2.0, y: 0.0 },
        });
        assert_eq!(
            invalid_target.encode(),
            Err(TraceError::InvalidAttentionTarget)
        );
    }

    #[test]
    fn reduced_mode_and_all_semantic_forms_are_replayable() {
        for form in OuroborosForm::ALL {
            for mode in [
                OuroborosMotionMode::Reduced,
                OuroborosMotionMode::LowPower,
                OuroborosMotionMode::Balanced,
                OuroborosMotionMode::Cinematic,
            ] {
                let mut trace = OuroborosTrace::new(OuroborosConfig::default(), form);
                trace
                    .push(OuroborosEvent::SetMotionMode { mode })
                    .unwrap();
                trace
                    .push(OuroborosEvent::TransitionTo {
                        form: OuroborosForm::ALL[(form_tag(form) as usize + 1) % 4],
                    })
                    .unwrap();
                for _ in 0..4 {
                    trace
                        .push(OuroborosEvent::Advance { dt_seconds: 0.1 })
                        .unwrap();
                }
                let replay = trace.replay().unwrap();
                assert!(replay.scene.frame().transition_progress.is_finite());
                assert!((0.0..=1.0).contains(&replay.scene.frame().transition_progress));
            }
        }
    }

    #[test]
    fn trace_event_count_is_bounded() {
        let mut trace = OuroborosTrace::new(
            OuroborosConfig::default(),
            OuroborosForm::Nature,
        );
        for _ in 0..MAX_EVENTS {
            trace.events.push(OuroborosEvent::Advance { dt_seconds: 0.0 });
        }
        assert_eq!(
            trace.push(OuroborosEvent::Advance { dt_seconds: 0.0 }),
            Err(TraceError::TooManyEvents)
        );
        assert_eq!(
            OuroborosTrace::decode(&{
                let mut bytes = trace.encode().unwrap();
                // Event count begins after the fixed header.
                let over_limit = (MAX_EVENTS as u32 + 1).to_le_bytes();
                bytes[46..50].copy_from_slice(&over_limit);
                bytes
            }),
            Err(TraceError::TooManyEvents)
        );
    }
}
