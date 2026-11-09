//! Executable oracle for Waywallen IPC v3 automatic render-target negotiation.
//!
//! This models protocol values and state transitions only. It does not
//! discover displays, query a GPU, allocate buffers or own file descriptors.
//! The host supplies hardware limits and the currently published pool identity.

use super::{
    ContractError, Extent, FormatCapability, ProducerCapabilities, MAX_EXTENT, MAX_SLOT_COUNT,
};

/// Wire values 1 and 2 from render_target_capabilities.mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTargetMode {
    NativeExtent,
    ScalableFixedAspect,
}

impl TryFrom<u32> for RenderTargetMode {
    type Error = ContractError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            1 => Ok(Self::NativeExtent),
            2 => Ok(Self::ScalableFixedAspect),
            _ => Err(ContractError::InvalidRenderTargetCapabilities),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTargetCapabilities {
    pub revision: u64,
    pub reference: Extent,
    pub mode: RenderTargetMode,
    pub min: Extent,
    pub max: Extent,
    pub alignment: Extent,
}

impl RenderTargetCapabilities {
    pub fn validate(self) -> Result<(), ContractError> {
        if self.revision == 0
            || !valid_extent(self.reference)
            || !valid_extent(self.min)
            || !valid_extent(self.max)
            || self.alignment.width == 0
            || self.alignment.height == 0
            || self.min.width > self.max.width
            || self.min.height > self.max.height
            || self.min.width > self.reference.width
            || self.min.height > self.reference.height
        {
            return Err(ContractError::InvalidRenderTargetCapabilities);
        }
        self.select(1.0, self.max).map(|_| ())
    }

    /// Mirror Waywallen's fixed-aspect target selector using a host-supplied
    /// hardware ceiling. The visual core itself never queries a GPU/display.
    pub fn select(self, demand: f64, hardware_max: Extent) -> Result<Extent, ContractError> {
        if !demand.is_finite()
            || demand <= 0.0
            || !valid_extent(hardware_max)
            || self.revision == 0
            || !valid_extent(self.reference)
            || !valid_extent(self.min)
            || !valid_extent(self.max)
            || self.alignment.width == 0
            || self.alignment.height == 0
        {
            return Err(ContractError::InvalidRenderTargetCapabilities);
        }

        let reference_limit = match self.mode {
            RenderTargetMode::NativeExtent => self.reference,
            RenderTargetMode::ScalableFixedAspect
                if self.reference.width >= self.reference.height =>
            {
                Extent { width: 7_680, height: 4_320 }
            }
            RenderTargetMode::ScalableFixedAspect => Extent { width: 4_320, height: 7_680 },
        };
        let limit = |reference: u32, maximum: u32, hardware: u32, alignment: u32| {
            reference.min(maximum).min(hardware) / alignment * alignment
        };
        let max_width = limit(
            reference_limit.width,
            self.max.width,
            hardware_max.width,
            self.alignment.width,
        );
        let max_height = limit(
            reference_limit.height,
            self.max.height,
            hardware_max.height,
            self.alignment.height,
        );
        if max_width < self.min.width || max_height < self.min.height {
            return Err(ContractError::InvalidRenderTargetCapabilities);
        }

        let lower = (f64::from(self.min.width) / f64::from(self.reference.width))
            .max(f64::from(self.min.height) / f64::from(self.reference.height));
        let upper = (f64::from(max_width) / f64::from(self.reference.width))
            .min(f64::from(max_height) / f64::from(self.reference.height));
        if lower > upper || upper <= 0.0 {
            return Err(ContractError::InvalidRenderTargetCapabilities);
        }

        let scale = demand.max(lower).min(upper);
        let align = |reference: u32, alignment: u32, maximum: u32| {
            let requested = (f64::from(reference) * scale).ceil() as u64;
            (requested.div_ceil(u64::from(alignment)) * u64::from(alignment))
                .min(u64::from(maximum)) as u32
        };
        let selected = Extent {
            width: align(self.reference.width, self.alignment.width, max_width),
            height: align(self.reference.height, self.alignment.height, max_height),
        };
        self.validate_extent(selected)?;
        Ok(selected)
    }

    pub fn validate_extent(self, extent: Extent) -> Result<(), ContractError> {
        if !valid_extent(extent)
            || extent.width < self.min.width
            || extent.height < self.min.height
            || extent.width > self.max.width
            || extent.height > self.max.height
            || extent.width % self.alignment.width != 0
            || extent.height % self.alignment.height != 0
        {
            return Err(ContractError::InvalidExtent);
        }

        match self.mode {
            RenderTargetMode::NativeExtent
                if extent.width > self.reference.width || extent.height > self.reference.height =>
            {
                return Err(ContractError::InvalidExtent);
            }
            RenderTargetMode::ScalableFixedAspect => {
                let maximum = if self.reference.width >= self.reference.height {
                    Extent { width: 7_680, height: 4_320 }
                } else {
                    Extent { width: 4_320, height: 7_680 }
                };
                if extent.width > maximum.width || extent.height > maximum.height {
                    return Err(ContractError::InvalidExtent);
                }
            }
            _ => {}
        }

        // Mirror waywallen-bridge's alignment/aspect check with u64 products.
        let width_margin = extent.width - self.alignment.width;
        let height_margin = extent.height - self.alignment.height;
        if u64::from(width_margin) * u64::from(self.reference.height)
            >= u64::from(extent.height) * u64::from(self.reference.width)
            || u64::from(height_margin) * u64::from(self.reference.width)
                >= u64::from(extent.width) * u64::from(self.reference.height)
        {
            return Err(ContractError::RenderTargetAspectMismatch);
        }
        Ok(())
    }
}

/// Wire values from Waywallen's buffer_path enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTargetPath {
    OptimizedSameDevice,
    OptimizedSameVendor,
    CompatLinear,
    CompatCpuReadback,
}

impl TryFrom<u32> for RenderTargetPath {
    type Error = ContractError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::OptimizedSameDevice),
            1 => Ok(Self::OptimizedSameVendor),
            2 => Ok(Self::CompatLinear),
            3 => Ok(Self::CompatCpuReadback),
            _ => Err(ContractError::InvalidRenderTargetConfig),
        }
    }
}

/// Wire values from Waywallen's buffer_memory_source enum.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTargetMemorySource {
    GpuNative,
    GpuLinear,
    DmabufHeap,
}

impl TryFrom<u32> for RenderTargetMemorySource {
    type Error = ContractError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::GpuNative),
            1 => Ok(Self::GpuLinear),
            2 => Ok(Self::DmabufHeap),
            _ => Err(ContractError::InvalidRenderTargetConfig),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTargetConfig {
    pub revision: u64,
    pub capabilities_revision: u64,
    pub extent: Extent,
    pub format: FormatCapability,
    pub sync_mode: u32,
    pub color: u32,
    pub memory_hint: u32,
    pub slot_count: u32,
    pub path: RenderTargetPath,
    pub memory_source: RenderTargetMemorySource,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTargetStatus {
    Applied,
    InvalidRequest,
    ResourceFailure,
    SchemeRejected,
}

impl TryFrom<u32> for RenderTargetStatus {
    type Error = ContractError;

    fn try_from(value: u32) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(Self::Applied),
            1 => Ok(Self::InvalidRequest),
            2 => Ok(Self::ResourceFailure),
            3 => Ok(Self::SchemeRejected),
            _ => Err(ContractError::InvalidRenderTargetOutcome),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RenderTargetOutcome {
    pub revision: u64,
    pub status: RenderTargetStatus,
    pub extent: Extent,
    pub pool_generation: u64,
}

/// Identity of the buffer pool published by the bridge/renderer host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PublishedTargetPool {
    pub generation: u64,
    pub extent: Extent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTargetCompletion {
    Applied { revision: u64, pool_generation: u64 },
    Failed { revision: u64, status: RenderTargetStatus },
    IgnoredStale,
}

/// Pure revision/pool-generation oracle for Waywallen Auto sizing.
#[derive(Debug, Clone, Default)]
pub struct RenderTargetSession {
    capabilities: Option<RenderTargetCapabilities>,
    last_config_revision: u64,
    pending: Option<RenderTargetConfig>,
    applied: Option<RenderTargetConfig>,
    applied_pool_generation: u64,
}

impl RenderTargetSession {
    /// Older revisions are ignored. Equal revisions are idempotent only when
    /// the complete body is identical; changed content requires a new revision.
    pub fn publish_capabilities(
        &mut self,
        capabilities: RenderTargetCapabilities,
    ) -> Result<bool, ContractError> {
        capabilities.validate()?;
        if let Some(current) = self.capabilities {
            if capabilities.revision < current.revision {
                return Ok(false);
            }
            if capabilities.revision == current.revision {
                if capabilities == current {
                    return Ok(false);
                }
                return Err(ContractError::RenderTargetCapabilityRevisionConflict);
            }
        }
        self.capabilities = Some(capabilities);
        Ok(true)
    }

    /// Accept one daemon config against the current capability revision and
    /// producer format inventory. At most one request may be in flight.
    pub fn receive_config(
        &mut self,
        config: RenderTargetConfig,
        producer: &ProducerCapabilities,
    ) -> Result<(), ContractError> {
        if self.pending.is_some() {
            return Err(ContractError::RenderTargetRequestInFlight);
        }
        if config.revision == 0 || config.revision <= self.last_config_revision {
            return Err(ContractError::NonMonotonicRenderTargetRevision);
        }
        let capabilities = self
            .capabilities
            .ok_or(ContractError::MissingRenderTargetCapabilities)?;
        if config.capabilities_revision != capabilities.revision {
            return Err(ContractError::StaleRenderTargetCapabilities);
        }
        capabilities.validate_extent(config.extent)?;
        if !producer.advertises(config.format) {
            return Err(ContractError::UnadvertisedFormat);
        }
        if config.extent.width > producer.max_extent.width
            || config.extent.height > producer.max_extent.height
        {
            return Err(ContractError::InvalidExtent);
        }
        if !(1..=MAX_SLOT_COUNT).contains(&config.slot_count) {
            return Err(ContractError::InvalidSlotCount);
        }

        self.last_config_revision = config.revision;
        self.pending = Some(config);
        Ok(())
    }

    /// A stale/unsolicited result is ignored. Status 0 only succeeds when
    /// outcome extent and the bridge-published pool exactly match the request.
    /// Failure statuses preserve the last-known-good pool.
    pub fn finish(
        &mut self,
        outcome: RenderTargetOutcome,
        published_pool: Option<PublishedTargetPool>,
    ) -> Result<RenderTargetCompletion, ContractError> {
        let Some(pending) = self.pending else {
            return Ok(RenderTargetCompletion::IgnoredStale);
        };
        if outcome.revision != pending.revision {
            return Ok(RenderTargetCompletion::IgnoredStale);
        }

        match outcome.status {
            RenderTargetStatus::Applied => {
                let Some(pool) = published_pool else {
                    return Err(ContractError::InvalidRenderTargetOutcome);
                };
                if outcome.extent != pending.extent
                    || pool.extent != outcome.extent
                    || pool.generation != outcome.pool_generation
                    || pool.generation == 0
                    || pool.generation <= self.applied_pool_generation
                {
                    self.pending = None;
                    return Err(ContractError::InvalidRenderTargetOutcome);
                }
                self.applied_pool_generation = pool.generation;
                self.applied = Some(pending);
                self.pending = None;
                Ok(RenderTargetCompletion::Applied {
                    revision: outcome.revision,
                    pool_generation: pool.generation,
                })
            }
            status => {
                self.pending = None;
                Ok(RenderTargetCompletion::Failed {
                    revision: outcome.revision,
                    status,
                })
            }
        }
    }

    pub fn capabilities(&self) -> Option<RenderTargetCapabilities> {
        self.capabilities
    }

    pub fn pending(&self) -> Option<RenderTargetConfig> {
        self.pending
    }

    pub fn applied(&self) -> Option<RenderTargetConfig> {
        self.applied
    }

    pub fn applied_pool_generation(&self) -> u64 {
        self.applied_pool_generation
    }
}

fn valid_extent(extent: Extent) -> bool {
    extent.width > 0
        && extent.height > 0
        && extent.width <= MAX_EXTENT
        && extent.height <= MAX_EXTENT
}

#[cfg(test)]
mod tests {
    use super::*;

    const AB24: u32 = 0x3432_4241;

    fn capabilities(mode: RenderTargetMode) -> RenderTargetCapabilities {
        RenderTargetCapabilities {
            revision: 1,
            reference: Extent { width: 1_920, height: 1_080 },
            mode,
            min: Extent { width: 320, height: 180 },
            max: Extent { width: 3_840, height: 2_160 },
            alignment: Extent { width: 8, height: 2 },
        }
    }

    fn producer_capabilities() -> ProducerCapabilities {
        ProducerCapabilities {
            fourccs: vec![AB24],
            mod_counts: vec![1],
            modifiers: vec![0],
            plane_counts: vec![1],
            max_extent: Extent { width: 3_840, height: 2_160 },
        }
    }

    fn config(revision: u64, capabilities_revision: u64, width: u32, height: u32) -> RenderTargetConfig {
        RenderTargetConfig {
            revision,
            capabilities_revision,
            extent: Extent { width, height },
            format: FormatCapability { fourcc: AB24, modifier: 0, plane_count: 1 },
            sync_mode: 1,
            color: 0,
            memory_hint: 0,
            slot_count: 3,
            path: RenderTargetPath::CompatLinear,
            memory_source: RenderTargetMemorySource::GpuLinear,
        }
    }

    #[test]
    fn selection_matches_fixed_aspect_and_hardware_ceiling() {
        let scalable = capabilities(RenderTargetMode::ScalableFixedAspect);
        scalable.validate().unwrap();
        assert_eq!(
            scalable.select(0.5, Extent { width: 3_840, height: 2_160 }).unwrap(),
            Extent { width: 960, height: 540 }
        );

        let native = capabilities(RenderTargetMode::NativeExtent);
        assert_eq!(
            native.select(2.0, Extent { width: 7_680, height: 4_320 }).unwrap(),
            Extent { width: 1_920, height: 1_080 }
        );
    }

    #[test]
    fn capability_revisions_are_monotonic_and_conflict_checked() {
        let mut session = RenderTargetSession::default();
        let first = capabilities(RenderTargetMode::NativeExtent);
        assert!(session.publish_capabilities(first).unwrap());
        assert!(!session.publish_capabilities(first).unwrap());

        let mut newer = first;
        newer.revision = 2;
        session.publish_capabilities(newer).unwrap();

        let mut older = first;
        assert!(!session.publish_capabilities(older).unwrap());

        let mut conflict = newer;
        conflict.max.width -= 8;
        assert_eq!(
            session.publish_capabilities(conflict),
            Err(ContractError::RenderTargetCapabilityRevisionConflict)
        );
    }

    #[test]
    fn invalid_capabilities_and_unknown_wire_enums_fail_closed() {
        assert_eq!(
            RenderTargetMode::try_from(0),
            Err(ContractError::InvalidRenderTargetCapabilities)
        );
        assert_eq!(
            RenderTargetStatus::try_from(4),
            Err(ContractError::InvalidRenderTargetOutcome)
        );

        let mut invalid = capabilities(RenderTargetMode::NativeExtent);
        invalid.alignment.width = 0;
        assert_eq!(invalid.validate(), Err(ContractError::InvalidRenderTargetCapabilities));

        let mut impossible = capabilities(RenderTargetMode::NativeExtent);
        impossible.min.width = 4_000;
        assert_eq!(impossible.validate(), Err(ContractError::InvalidRenderTargetCapabilities));
    }

    #[test]
    fn config_requires_latest_capabilities_valid_aspect_and_advertised_format() {
        let mut session = RenderTargetSession::default();
        session.publish_capabilities(capabilities(RenderTargetMode::NativeExtent)).unwrap();
        let producer = producer_capabilities();

        assert_eq!(
            session.receive_config(config(1, 9, 960, 540), &producer),
            Err(ContractError::StaleRenderTargetCapabilities)
        );
        assert_eq!(
            session.receive_config(config(1, 1, 960, 538), &producer),
            Err(ContractError::InvalidExtent)
        );

        let mut bad_format = config(1, 1, 960, 540);
        bad_format.format.modifier = 99;
        assert_eq!(
            session.receive_config(bad_format, &producer),
            Err(ContractError::UnadvertisedFormat)
        );

        let mut bad_aspect = capabilities(RenderTargetMode::NativeExtent);
        bad_aspect.alignment = Extent { width: 16, height: 1 };
        let mut aspect_session = RenderTargetSession::default();
        aspect_session.publish_capabilities(bad_aspect).unwrap();
        assert_eq!(
            aspect_session.receive_config(config(1, 1, 1_904, 1_080), &producer),
            Err(ContractError::RenderTargetAspectMismatch)
        );

        session.receive_config(config(1, 1, 960, 540), &producer).unwrap();
        assert_eq!(
            session.receive_config(config(2, 1, 1_280, 720), &producer),
            Err(ContractError::RenderTargetRequestInFlight)
        );
    }

    #[test]
    fn applied_result_requires_matching_published_pool_extent_and_generation() {
        let mut session = RenderTargetSession::default();
        session
            .publish_capabilities(capabilities(RenderTargetMode::ScalableFixedAspect))
            .unwrap();
        session.receive_config(config(1, 1, 960, 540), &producer_capabilities()).unwrap();

        let stale = RenderTargetOutcome {
            revision: 99,
            status: RenderTargetStatus::Applied,
            extent: Extent { width: 960, height: 540 },
            pool_generation: 1,
        };
        assert_eq!(
            session.finish(stale, None).unwrap(),
            RenderTargetCompletion::IgnoredStale
        );
        assert_eq!(session.pending().unwrap().revision, 1);

        let mismatched_pool = RenderTargetOutcome {
            revision: 1,
            status: RenderTargetStatus::Applied,
            extent: Extent { width: 960, height: 540 },
            pool_generation: 4,
        };
        assert_eq!(
            session.finish(
                mismatched_pool,
                Some(PublishedTargetPool {
                    generation: 3,
                    extent: mismatched_pool.extent,
                })
            ),
            Err(ContractError::InvalidRenderTargetOutcome)
        );
        assert!(session.pending().is_none());
        assert!(session.applied().is_none());
        assert_eq!(session.applied_pool_generation(), 0);

        // Invalid Applied evidence consumes revision 1. Recovery is a new
        // monotonically increasing request with a separately published pool.
        session.receive_config(config(2, 1, 960, 540), &producer_capabilities()).unwrap();
        let valid = RenderTargetOutcome {
            revision: 2,
            status: RenderTargetStatus::Applied,
            extent: Extent { width: 960, height: 540 },
            pool_generation: 4,
        };
        assert_eq!(
            session.finish(
                valid,
                Some(PublishedTargetPool {
                    generation: 4,
                    extent: valid.extent,
                })
            ).unwrap(),
            RenderTargetCompletion::Applied {
                revision: 2,
                pool_generation: 4,
            }
        );
        assert_eq!(session.applied().unwrap().extent.width, 960);
        assert_eq!(session.applied_pool_generation(), 4);
    }

    #[test]
    fn scheme_rejection_preserves_last_known_good_pool_and_allows_new_revision() {
        let mut session = RenderTargetSession::default();
        session.publish_capabilities(capabilities(RenderTargetMode::ScalableFixedAspect)).unwrap();
        let producer = producer_capabilities();

        session.receive_config(config(1, 1, 960, 540), &producer).unwrap();
        session.finish(
            RenderTargetOutcome {
                revision: 1,
                status: RenderTargetStatus::Applied,
                extent: Extent { width: 960, height: 540 },
                pool_generation: 2,
            },
            Some(PublishedTargetPool {
                generation: 2,
                extent: Extent { width: 960, height: 540 },
            }),
        ).unwrap();

        session.receive_config(config(2, 1, 1_280, 720), &producer).unwrap();
        assert_eq!(
            session.finish(
                RenderTargetOutcome {
                    revision: 2,
                    status: RenderTargetStatus::SchemeRejected,
                    extent: Extent { width: 0, height: 0 },
                    pool_generation: 2,
                },
                None,
            ).unwrap(),
            RenderTargetCompletion::Failed {
                revision: 2,
                status: RenderTargetStatus::SchemeRejected,
            }
        );
        assert_eq!(session.applied().unwrap().extent.width, 960);
        assert_eq!(session.applied_pool_generation(), 2);

        session.receive_config(config(3, 1, 1_280, 720), &producer).unwrap();
        assert_eq!(session.pending().unwrap().revision, 3);
    }
}
