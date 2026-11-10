//! Executable state-transition oracle for the Waywallen renderer IPC v3 contract.
//!
//! This crate deliberately models messages and resource identities only. It
//! does not own sockets, file descriptors, Vulkan handles, DMA-BUFs or clocks.
//! The production adapter must map real transport events into these checked
//! transitions and separately qualify kernel/GPU synchronization.

use std::collections::BTreeSet;

pub mod render_target;

pub const IPC_PROTOCOL_VERSION: u32 = 3;
pub const RENDERER_SPAWN_VERSION: u32 = 10;
pub const MAX_FORMAT_COUNT: usize = 16;
pub const MAX_MODIFIER_COUNT: usize = 64;
pub const MAX_PLANE_COUNT: u32 = 4;
pub const MAX_SLOT_COUNT: u32 = 8;
pub const MAX_EXTENT: u32 = 16_384;

pub const MEMORY_HINT_DEVICE_LOCAL: u32 = 1 << 0;
pub const MEMORY_HINT_HOST_VISIBLE: u32 = 1 << 1;
pub const MEMORY_HINT_SCANOUT: u32 = 1 << 2;
pub const MEMORY_HINT_PROTECTED: u32 = 1 << 3;
pub const KNOWN_MEMORY_HINTS: u32 = MEMORY_HINT_DEVICE_LOCAL
    | MEMORY_HINT_HOST_VISIBLE
    | MEMORY_HINT_SCANOUT
    | MEMORY_HINT_PROTECTED;

pub const SYNC_DMABUF_IMPLICIT: u32 = 1 << 0;
pub const SYNC_SYNCOBJ_BINARY: u32 = 1 << 1;
pub const SYNC_SYNCOBJ_TIMELINE: u32 = 1 << 2;
pub const KNOWN_SYNC_CAPS: u32 =
    SYNC_DMABUF_IMPLICIT | SYNC_SYNCOBJ_BINARY | SYNC_SYNCOBJ_TIMELINE;

pub const COLOR_ENCODING_SRGB: u32 = 1 << 0;
// Non-sRGB C-bridge macros are excluded until the C/Rust bit mapping
// discrepancy is reconciled. On the daemon they currently decode as
// LINEAR (bit 1) and BT.601 (bit 2), not BT.709/BT.2020.
pub const UNSAFE_BRIDGE_COLOR_ENCODING_BT709: u32 = 1 << 1;
pub const UNSAFE_BRIDGE_COLOR_ENCODING_BT2020: u32 = 1 << 2;
pub const COLOR_RANGE_FULL: u32 = 1 << 5;
pub const COLOR_RANGE_LIMITED: u32 = 1 << 6;
pub const COLOR_ALPHA_PREMULTIPLIED: u32 = 1 << 7;
pub const COLOR_ALPHA_STRAIGHT: u32 = 1 << 8;
pub const COLOR_ENCODING_MASK: u32 = COLOR_ENCODING_SRGB;
pub const COLOR_RANGE_MASK: u32 = COLOR_RANGE_FULL | COLOR_RANGE_LIMITED;
pub const COLOR_ALPHA_MASK: u32 = COLOR_ALPHA_PREMULTIPLIED | COLOR_ALPHA_STRAIGHT;
pub const KNOWN_COLOR_CAPS: u32 = COLOR_ENCODING_MASK | COLOR_RANGE_MASK | COLOR_ALPHA_MASK;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrmNode {
    pub major: u32,
    pub minor: u32,
}

impl DrmNode {
    pub fn validate(self) -> Result<(), ContractError> {
        if (self.major == 0) != (self.minor == 0) {
            return Err(ContractError::InvalidDrmNode);
        }
        Ok(())
    }

    pub const fn is_unknown(self) -> bool {
        self.major == 0 && self.minor == 0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Extent {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatCapability {
    pub fourcc: u32,
    pub modifier: u64,
    pub plane_count: u32,
}

/// Mirrors the flattened arrays used by Waywallen IPC v3 FormatCaps.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProducerCapabilities {
    pub fourccs: Vec<u32>,
    pub mod_counts: Vec<u32>,
    pub modifiers: Vec<u64>,
    pub plane_counts: Vec<u32>,
    pub max_extent: Extent,
    pub mem_hints: u32,
    pub sync_caps: u32,
    pub color_caps: u32,
}

impl ProducerCapabilities {
    pub fn validate(&self) -> Result<(), ContractError> {
        if self.fourccs.is_empty()
            || self.fourccs.len() > MAX_FORMAT_COUNT
            || self.mod_counts.len() != self.fourccs.len()
            || self.max_extent.width == 0
            || self.max_extent.height == 0
            || self.max_extent.width > MAX_EXTENT
            || self.max_extent.height > MAX_EXTENT
            || self.sync_caps == 0
            || self.sync_caps & !KNOWN_SYNC_CAPS != 0
            || self.color_caps & !KNOWN_COLOR_CAPS != 0
            || self.mem_hints & !KNOWN_MEMORY_HINTS != 0
        {
            return Err(ContractError::InvalidCapabilities);
        }

        let unique_fourccs: BTreeSet<u32> = self.fourccs.iter().copied().collect();
        if unique_fourccs.len() != self.fourccs.len() || unique_fourccs.contains(&0) {
            return Err(ContractError::InvalidCapabilities);
        }

        let total = self.mod_counts.iter().try_fold(0_usize, |sum, count| {
            if *count == 0 {
                return None;
            }
            sum.checked_add(*count as usize)
        });
        let Some(total) = total else {
            return Err(ContractError::InvalidCapabilities);
        };
        if total == 0
            || total > MAX_MODIFIER_COUNT
            || self.modifiers.len() != total
            || self.plane_counts.len() != total
        {
            return Err(ContractError::InvalidCapabilities);
        }

        if self
            .plane_counts
            .iter()
            .any(|count| !(1..=MAX_PLANE_COUNT).contains(count))
        {
            return Err(ContractError::InvalidCapabilities);
        }

        let mut offset = 0;
        let mut unique_entries = BTreeSet::new();
        for (fourcc, count) in self.fourccs.iter().zip(&self.mod_counts) {
            for index in offset..offset + *count as usize {
                let entry = (*fourcc, self.modifiers[index], self.plane_counts[index]);
                if !unique_entries.insert(entry) {
                    return Err(ContractError::InvalidCapabilities);
                }
            }
            offset += *count as usize;
        }

        Ok(())
    }

    pub fn advertises(&self, wanted: FormatCapability) -> bool {
        let mut offset = 0;
        for (fourcc, count) in self.fourccs.iter().zip(&self.mod_counts) {
            if *fourcc == wanted.fourcc {
                for index in offset..offset + *count as usize {
                    if self.modifiers[index] == wanted.modifier
                        && self.plane_counts[index] == wanted.plane_count
                    {
                        return true;
                    }
                }
                return false;
            }
            offset += *count as usize;
        }
        false
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BufferDirective {
    pub format: FormatCapability,
    pub extent: Extent,
    pub slot_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BindBuffers {
    pub generation: u64,
    pub slot_count: u32,
    /// Flattened SCM_RIGHTS descriptor count: slots * planes per slot.
    pub plane_fd_count: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FrameReady {
    pub image_index: u32,
    pub sequence: u64,
    pub produced_at_ns: u64,
    pub release_point: u64,
    pub acquire_sync_fd_present: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    AwaitInit,
    AwaitReady,
    AwaitReleaseSyncobj,
    AwaitFormatCaps,
    AwaitDirective,
    AwaitBind,
    Active,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContractError {
    InvalidPhase,
    ProtocolVersionMismatch { received: u32, expected: u32 },
    SpawnVersionMismatch { received: u32, expected: u32 },
    InvalidDrmNode,
    MissingReleaseSyncobj,
    InvalidCapabilities,
    UnadvertisedFormat,
    InvalidExtent,
    InvalidSlotCount,
    InvalidPoolGeneration,
    PlaneDescriptorCountMismatch { expected: u32, received: u32 },
    MissingAcquireSyncFd,
    InvalidImageIndex,
    NonMonotonicFrameSequence,
    NonMonotonicTimestamp,
    NonMonotonicReleasePoint,
    InvalidRenderTargetCapabilities,
    MissingRenderTargetCapabilities,
    RenderTargetCapabilityRevisionConflict,
    NonMonotonicRenderTargetRevision,
    StaleRenderTargetCapabilities,
    RenderTargetRequestInFlight,
    RenderTargetAspectMismatch,
    InvalidRenderTargetOutcome,
    InvalidRenderTargetConfig,
}

#[derive(Debug, Clone)]
pub struct RendererSession {
    phase: Phase,
    drm_node: Option<DrmNode>,
    capabilities: Option<ProducerCapabilities>,
    pending_directive: Option<BufferDirective>,
    pool_generation: u64,
    slot_count: u32,
    frame_sequence: u64,
    last_produced_at_ns: u64,
    release_point: u64,
}

impl Default for RendererSession {
    fn default() -> Self {
        Self {
            phase: Phase::AwaitInit,
            drm_node: None,
            capabilities: None,
            pending_directive: None,
            pool_generation: 0,
            slot_count: 0,
            frame_sequence: 0,
            last_produced_at_ns: 0,
            release_point: 0,
        }
    }
}

impl RendererSession {
    pub fn receive_init(
        &mut self,
        protocol_version: u32,
        spawn_version: u32,
    ) -> Result<(), ContractError> {
        if self.phase != Phase::AwaitInit {
            return Err(ContractError::InvalidPhase);
        }
        if protocol_version != IPC_PROTOCOL_VERSION {
            return Err(ContractError::ProtocolVersionMismatch {
                received: protocol_version,
                expected: IPC_PROTOCOL_VERSION,
            });
        }
        if spawn_version != RENDERER_SPAWN_VERSION {
            return Err(ContractError::SpawnVersionMismatch {
                received: spawn_version,
                expected: RENDERER_SPAWN_VERSION,
            });
        }
        self.phase = Phase::AwaitReady;
        Ok(())
    }

    /// Ready is the producer's first event and must carry a valid (or unknown)
    /// DRM render-node pair.
    pub fn send_ready(&mut self, node: DrmNode) -> Result<(), ContractError> {
        if self.phase != Phase::AwaitReady {
            return Err(ContractError::InvalidPhase);
        }
        node.validate()?;
        self.drm_node = Some(node);
        self.phase = Phase::AwaitReleaseSyncobj;
        Ok(())
    }

    pub fn send_release_syncobj(&mut self, descriptor_present: bool) -> Result<(), ContractError> {
        if self.phase != Phase::AwaitReleaseSyncobj {
            return Err(ContractError::InvalidPhase);
        }
        if !descriptor_present {
            return Err(ContractError::MissingReleaseSyncobj);
        }
        self.phase = Phase::AwaitFormatCaps;
        Ok(())
    }

    pub fn send_format_caps(
        &mut self,
        capabilities: ProducerCapabilities,
    ) -> Result<(), ContractError> {
        if self.phase != Phase::AwaitFormatCaps {
            return Err(ContractError::InvalidPhase);
        }
        capabilities.validate()?;
        self.capabilities = Some(capabilities);
        self.phase = Phase::AwaitDirective;
        Ok(())
    }

    pub fn receive_negotiate_buffers(
        &mut self,
        directive: BufferDirective,
    ) -> Result<(), ContractError> {
        if !matches!(self.phase, Phase::AwaitDirective | Phase::Active) {
            return Err(ContractError::InvalidPhase);
        }
        let capabilities = self
            .capabilities
            .as_ref()
            .ok_or(ContractError::InvalidPhase)?;
        if !capabilities.advertises(directive.format) {
            return Err(ContractError::UnadvertisedFormat);
        }
        if directive.extent.width == 0
            || directive.extent.height == 0
            || directive.extent.width > capabilities.max_extent.width
            || directive.extent.height > capabilities.max_extent.height
        {
            return Err(ContractError::InvalidExtent);
        }
        if !(1..=MAX_SLOT_COUNT).contains(&directive.slot_count) {
            return Err(ContractError::InvalidSlotCount);
        }
        self.pending_directive = Some(directive);
        self.phase = Phase::AwaitBind;
        Ok(())
    }

    pub fn send_bind_buffers(&mut self, bind: BindBuffers) -> Result<(), ContractError> {
        if self.phase != Phase::AwaitBind {
            return Err(ContractError::InvalidPhase);
        }
        let directive = self
            .pending_directive
            .ok_or(ContractError::InvalidPhase)?;
        if bind.generation == 0 || bind.generation <= self.pool_generation {
            return Err(ContractError::InvalidPoolGeneration);
        }
        if bind.slot_count != directive.slot_count {
            return Err(ContractError::InvalidSlotCount);
        }
        let expected_fds = bind
            .slot_count
            .checked_mul(directive.format.plane_count)
            .ok_or(ContractError::InvalidCapabilities)?;
        if bind.plane_fd_count != expected_fds {
            return Err(ContractError::PlaneDescriptorCountMismatch {
                expected: expected_fds,
                received: bind.plane_fd_count,
            });
        }

        self.pool_generation = bind.generation;
        self.slot_count = bind.slot_count;
        self.pending_directive = None;
        self.phase = Phase::Active;
        Ok(())
    }

    pub fn send_frame_ready(&mut self, frame: FrameReady) -> Result<(), ContractError> {
        if self.phase != Phase::Active {
            return Err(ContractError::InvalidPhase);
        }
        if !frame.acquire_sync_fd_present {
            return Err(ContractError::MissingAcquireSyncFd);
        }
        if frame.image_index >= self.slot_count {
            return Err(ContractError::InvalidImageIndex);
        }
        if frame.sequence == 0 || frame.sequence <= self.frame_sequence {
            return Err(ContractError::NonMonotonicFrameSequence);
        }
        if frame.produced_at_ns < self.last_produced_at_ns {
            return Err(ContractError::NonMonotonicTimestamp);
        }
        if frame.release_point == 0 || frame.release_point <= self.release_point {
            return Err(ContractError::NonMonotonicReleasePoint);
        }

        self.frame_sequence = frame.sequence;
        self.last_produced_at_ns = frame.produced_at_ns;
        self.release_point = frame.release_point;
        Ok(())
    }

    pub fn close(&mut self) {
        self.pending_directive = None;
        self.phase = Phase::Closed;
    }

    pub fn pool_generation(&self) -> u64 {
        self.pool_generation
    }

    pub fn frame_sequence(&self) -> u64 {
        self.frame_sequence
    }

    pub fn release_point(&self) -> u64 {
        self.release_point
    }

    pub fn drm_node(&self) -> Option<DrmNode> {
        self.drm_node
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const AB24: u32 = 0x3432_4241;

    fn capabilities() -> ProducerCapabilities {
        ProducerCapabilities {
            fourccs: vec![AB24],
            mod_counts: vec![1],
            modifiers: vec![0],
            plane_counts: vec![1],
            max_extent: Extent {
                width: 3840,
                height: 2160,
            },
            mem_hints: MEMORY_HINT_DEVICE_LOCAL | MEMORY_HINT_HOST_VISIBLE,
            sync_caps: KNOWN_SYNC_CAPS,
            color_caps: KNOWN_COLOR_CAPS,
        }
    }

    fn directive() -> BufferDirective {
        BufferDirective {
            format: FormatCapability {
                fourcc: AB24,
                modifier: 0,
                plane_count: 1,
            },
            extent: Extent {
                width: 1920,
                height: 1080,
            },
            slot_count: 3,
        }
    }

    fn initialized() -> RendererSession {
        let mut session = RendererSession::default();
        session.receive_init(IPC_PROTOCOL_VERSION, RENDERER_SPAWN_VERSION).unwrap();
        session
    }

    fn advertised() -> RendererSession {
        let mut session = initialized();
        session.send_ready(DrmNode { major: 226, minor: 128 }).unwrap();
        session.send_release_syncobj(true).unwrap();
        session.send_format_caps(capabilities()).unwrap();
        session
    }

    fn active() -> RendererSession {
        let mut session = advertised();
        session.receive_negotiate_buffers(directive()).unwrap();
        session.send_bind_buffers(BindBuffers {
            generation: 1,
            slot_count: 3,
            plane_fd_count: 3,
        }).unwrap();
        session
    }

    fn frame(sequence: u64, release_point: u64) -> FrameReady {
        FrameReady {
            image_index: 0,
            sequence,
            produced_at_ns: sequence * 16_666_667,
            release_point,
            acquire_sync_fd_present: true,
        }
    }

    #[test]
    fn valid_protocol_v3_spawn_v10_lifecycle_reaches_active_frames() {
        let mut session = active();
        assert_eq!(session.drm_node(), Some(DrmNode { major: 226, minor: 128 }));
        session.send_frame_ready(frame(1, 1)).unwrap();
        session.send_frame_ready(frame(2, 2)).unwrap();
        assert_eq!(session.pool_generation(), 1);
        assert_eq!(session.frame_sequence(), 2);
        assert_eq!(session.release_point(), 2);
    }

    #[test]
    fn init_versions_and_ready_order_fail_closed() {
        let mut session = RendererSession::default();
        assert_eq!(
            session.send_ready(DrmNode { major: 226, minor: 128 }),
            Err(ContractError::InvalidPhase)
        );
        assert_eq!(
            session.receive_init(2, RENDERER_SPAWN_VERSION),
            Err(ContractError::ProtocolVersionMismatch {
                received: 2,
                expected: IPC_PROTOCOL_VERSION
            })
        );
        assert_eq!(
            session.receive_init(IPC_PROTOCOL_VERSION, 9),
            Err(ContractError::SpawnVersionMismatch {
                received: 9,
                expected: RENDERER_SPAWN_VERSION
            })
        );
        session
            .receive_init(IPC_PROTOCOL_VERSION, RENDERER_SPAWN_VERSION)
            .unwrap();
        assert_eq!(
            session.receive_init(IPC_PROTOCOL_VERSION, RENDERER_SPAWN_VERSION),
            Err(ContractError::InvalidPhase)
        );
    }

    #[test]
    fn release_syncobj_is_mandatory_and_precedes_format_caps() {
        let mut session = initialized();
        session.send_ready(DrmNode { major: 0, minor: 0 }).unwrap();
        assert!(session.drm_node().unwrap().is_unknown());
        assert_eq!(
            session.send_format_caps(capabilities()),
            Err(ContractError::InvalidPhase)
        );
        assert_eq!(
            session.send_release_syncobj(false),
            Err(ContractError::MissingReleaseSyncobj)
        );
        session.send_release_syncobj(true).unwrap();
        session.send_format_caps(capabilities()).unwrap();
    }

    #[test]
    fn partial_unknown_drm_identity_is_rejected() {
        let mut session = initialized();
        assert_eq!(
            session.send_ready(DrmNode { major: 0, minor: 128 }),
            Err(ContractError::InvalidDrmNode)
        );
    }

    #[test]
    fn flattened_capability_array_invariants_are_enforced() {
        let mut malformed = capabilities();
        malformed.mod_counts.push(1);
        assert_eq!(
            malformed.validate(),
            Err(ContractError::InvalidCapabilities)
        );

        let mut malformed = capabilities();
        malformed.mod_counts[0] = 2;
        assert_eq!(
            malformed.validate(),
            Err(ContractError::InvalidCapabilities)
        );

        let mut malformed = capabilities();
        malformed.plane_counts[0] = 0;
        assert_eq!(
            malformed.validate(),
            Err(ContractError::InvalidCapabilities)
        );
    }

    #[test]
    fn directive_must_match_capabilities_extent_and_slot_bounds() {
        let mut session = advertised();
        let mut unsupported = directive();
        unsupported.format.modifier = 99;
        assert_eq!(
            session.receive_negotiate_buffers(unsupported),
            Err(ContractError::UnadvertisedFormat)
        );

        let mut oversized = directive();
        oversized.extent.width = 4096;
        assert_eq!(
            session.receive_negotiate_buffers(oversized),
            Err(ContractError::InvalidExtent)
        );

        let mut too_many = directive();
        too_many.slot_count = MAX_SLOT_COUNT + 1;
        assert_eq!(
            session.receive_negotiate_buffers(too_many),
            Err(ContractError::InvalidSlotCount)
        );
    }

    #[test]
    fn bind_requires_exact_pool_generation_slot_and_plane_descriptor_counts() {
        let mut session = advertised();
        session.receive_negotiate_buffers(directive()).unwrap();
        assert_eq!(
            session.send_bind_buffers(BindBuffers {
                generation: 0,
                slot_count: 3,
                plane_fd_count: 3,
            }),
            Err(ContractError::InvalidPoolGeneration)
        );
        assert_eq!(
            session.send_bind_buffers(BindBuffers {
                generation: 1,
                slot_count: 3,
                plane_fd_count: 2,
            }),
            Err(ContractError::PlaneDescriptorCountMismatch {
                expected: 3,
                received: 2,
            })
        );
        session.send_bind_buffers(BindBuffers {
            generation: 1,
            slot_count: 3,
            plane_fd_count: 3,
        }).unwrap();
        assert_eq!(
            session.send_bind_buffers(BindBuffers {
                generation: 1,
                slot_count: 3,
                plane_fd_count: 3,
            }),
            Err(ContractError::InvalidPhase)
        );
    }

    #[test]
    fn frames_require_valid_slot_acquire_fence_and_monotonic_counters() {
        let mut session = active();
        let mut missing_fence = frame(1, 1);
        missing_fence.acquire_sync_fd_present = false;
        assert_eq!(
            session.send_frame_ready(missing_fence),
            Err(ContractError::MissingAcquireSyncFd)
        );

        let mut invalid_slot = frame(1, 1);
        invalid_slot.image_index = 3;
        assert_eq!(
            session.send_frame_ready(invalid_slot),
            Err(ContractError::InvalidImageIndex)
        );

        session.send_frame_ready(frame(1, 1)).unwrap();
        assert_eq!(
            session.send_frame_ready(frame(1, 2)),
            Err(ContractError::NonMonotonicFrameSequence)
        );
        assert_eq!(
            session.send_frame_ready(frame(2, 1)),
            Err(ContractError::NonMonotonicReleasePoint)
        );

        let mut old_timestamp = frame(2, 2);
        old_timestamp.produced_at_ns = 1;
        assert_eq!(
            session.send_frame_ready(old_timestamp),
            Err(ContractError::NonMonotonicTimestamp)
        );

        session.send_frame_ready(frame(2, 2)).unwrap();
    }

    #[test]
    fn pool_rebind_requires_new_generation_and_keeps_connection_frame_sequence() {
        let mut session = active();
        session.send_frame_ready(frame(1, 1)).unwrap();
        session.receive_negotiate_buffers(directive()).unwrap();
        assert_eq!(
            session.send_frame_ready(frame(2, 2)),
            Err(ContractError::InvalidPhase)
        );
        assert_eq!(
            session.send_bind_buffers(BindBuffers {
                generation: 1,
                slot_count: 3,
                plane_fd_count: 3,
            }),
            Err(ContractError::InvalidPoolGeneration)
        );
        session.send_bind_buffers(BindBuffers {
            generation: 2,
            slot_count: 3,
            plane_fd_count: 3,
        }).unwrap();
        assert_eq!(session.pool_generation(), 2);
        session.send_frame_ready(frame(2, 2)).unwrap();
    }

    #[test]
    fn closed_renderer_session_rejects_all_further_messages() {
        let mut session = active();
        session.close();
        assert_eq!(
            session.send_frame_ready(frame(1, 1)),
            Err(ContractError::InvalidPhase)
        );
    }
}
