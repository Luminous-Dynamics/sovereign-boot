//! Typed, bounded settings for reproducible portable scenes.
//!
//! Parsing Scene Pack documents belongs in a validating host/adapter. This
//! module accepts only typed values, validates limits, and defines the numeric
//! seed encoding used by the additive Scene Pack construction path.

use std::{error::Error, fmt};

use crate::{
    color::{LEAF_GREEN, LICHEN_GREY, MOSS_DEEP, MYCELIAL_WHITE, Rgba, SOLAR_GOLD},
    contract::{self, DimensionsError},
};

pub const MAX_SCENE_BRANCHES: u32 = 8192;
pub const MAX_SCENE_DEPTH: u32 = 24;
/// Maximum simulation catch-up batch; hosts must pause instead of unbounded catch-up.
pub const MAX_TICKS_PER_BATCH: u32 = 120;
pub const MIN_MEMORY_MIB: u32 = 16;
pub const MAX_MEMORY_MIB: u32 = 2048;
const BYTES_PER_MIB: u64 = 1024 * 1024;
// Stable, conservative accounting unit rather than target-dependent size_of.
const ESTIMATED_BRANCH_BYTES: u64 = 64;

/// Semantic palette slots consumed by the CPU reference renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScenePalette {
    /// First-frame/initial-background color.
    pub canvas: Rgba,
    /// Settled background color.
    pub substrate: Rgba,
    /// Primary growing branch color.
    pub filament: Rgba,
    /// Node color.
    pub node: Rgba,
    /// Deeper/secondary branch color.
    pub lichen: Rgba,
    /// Final convergence flash color.
    pub glow: Rgba,
}

impl Default for ScenePalette {
    fn default() -> Self {
        // Preserve the existing renderer's established palette and initial
        // fade while giving every configurable role an explicit mapping.
        Self {
            canvas: Rgba(0x00, 0x00, 0x00, 0xff),
            substrate: MOSS_DEEP,
            filament: LEAF_GREEN,
            node: SOLAR_GOLD,
            lichen: LICHEN_GREY,
            glow: MYCELIAL_WHITE,
        }
    }
}

/// Simulation/resource values the renderer can actually enforce.
///
/// Presentation selection, FPS throttling, safe regions, visibility and
/// suspend policy stay in the host adapter; these settings never grant host
/// capabilities.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneSettings {
    /// Effective branch ceiling; enforce before each child is allocated.
    pub branch_limit: u32,
    /// Effective branch-depth ceiling.
    pub max_depth: u32,
    /// Multiplier applied to the legacy base growth-speed equation.
    pub growth_rate: f32,
    /// Canonical simulation ticks per second.
    pub fixed_step_hz: u32,
    /// Pulse period, quantized to the nearest whole simulation tick.
    pub pulse_period_seconds: f32,
    /// Normalized deterministic visual drift amplitude in 0..=1.
    pub drift_amplitude: f32,
    /// Conservative memory ceiling for renderer-owned buffers and branch storage.
    pub max_memory_mib: u32,
    /// Role-based colors used by the reference renderer.
    pub palette: ScenePalette,
}

impl Default for SceneSettings {
    fn default() -> Self {
        Self {
            branch_limit: 8192,
            max_depth: 12,
            growth_rate: 1.0,
            fixed_step_hz: 30,
            pulse_period_seconds: 7.5,
            drift_amplitude: 0.0,
            max_memory_mib: 128,
            palette: ScenePalette::default(),
        }
    }
}

/// A typed settings validation failure. Adapters may translate this to their
/// own stable error ABI without parsing diagnostic strings.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneSettingsError {
    InvalidDimensions(DimensionsError),
    BranchLimitOutOfRange,
    MaxDepthOutOfRange,
    GrowthRateOutOfRange,
    FixedStepFrequencyOutOfRange,
    PulsePeriodOutOfRange,
    DriftAmplitudeOutOfRange,
    MemoryBudgetOutOfRange,
    MemoryEstimateOverflow,
    ResourceBudgetExceeded,
    InvalidActivity,
    TickBatchOutOfRange,
}

impl fmt::Display for SceneSettingsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidDimensions(e) => write!(f, "invalid scene dimensions: {e:?}"),
            Self::BranchLimitOutOfRange => write!(f, "branch_limit must be in 1..=8192"),
            Self::MaxDepthOutOfRange => write!(f, "max_depth must be in 1..=24"),
            Self::GrowthRateOutOfRange => write!(f, "growth_rate must be finite and in 0..=10"),
            Self::FixedStepFrequencyOutOfRange => write!(f, "fixed_step_hz must be in 1..=120"),
            Self::PulsePeriodOutOfRange => {
                write!(f, "pulse_period_seconds must be finite and in (0, 3600]")
            }
            Self::DriftAmplitudeOutOfRange => {
                write!(f, "drift_amplitude must be finite and in 0..=1")
            }
            Self::MemoryBudgetOutOfRange => write!(f, "max_memory_mib must be in 16..=2048"),
            Self::MemoryEstimateOverflow => write!(f, "scene memory estimate overflowed"),
            Self::ResourceBudgetExceeded => {
                write!(f, "scene exceeds its conservative renderer memory budget")
            }
            Self::InvalidActivity => write!(f, "activity must be finite and in 0..=1"),
            Self::TickBatchOutOfRange => write!(f, "tick batch must be in 0..=120"),
        }
    }
}

impl Error for SceneSettingsError {}

impl SceneSettings {
    /// Validate values and estimate peak renderer-owned memory for a frame.
    ///
    /// The estimate reserves 12 bytes per pixel for concurrent packed/output/
    /// transfer-sized frame buffers, plus up to 2x branch storage to account
    /// for Vec growth. Host runtime overhead, compositor/GPU allocations and
    /// unrelated process memory are not included in this estimate.
    pub fn validate_for_dimensions(
        &self,
        width: u32,
        height: u32,
    ) -> Result<(), SceneSettingsError> {
        contract::validate_dimensions(width, height)
            .map_err(SceneSettingsError::InvalidDimensions)?;

        if !(1..=MAX_SCENE_BRANCHES).contains(&self.branch_limit) {
            return Err(SceneSettingsError::BranchLimitOutOfRange);
        }
        if !(1..=MAX_SCENE_DEPTH).contains(&self.max_depth) {
            return Err(SceneSettingsError::MaxDepthOutOfRange);
        }
        if !self.growth_rate.is_finite() || !(0.0..=10.0).contains(&self.growth_rate) {
            return Err(SceneSettingsError::GrowthRateOutOfRange);
        }
        if !(1..=120).contains(&self.fixed_step_hz) {
            return Err(SceneSettingsError::FixedStepFrequencyOutOfRange);
        }
        if !self.pulse_period_seconds.is_finite()
            || !(0.0..=3600.0).contains(&self.pulse_period_seconds)
            || self.pulse_period_seconds == 0.0
        {
            return Err(SceneSettingsError::PulsePeriodOutOfRange);
        }
        if !self.drift_amplitude.is_finite()
            || !(0.0..=1.0).contains(&self.drift_amplitude)
        {
            return Err(SceneSettingsError::DriftAmplitudeOutOfRange);
        }
        if !(MIN_MEMORY_MIB..=MAX_MEMORY_MIB).contains(&self.max_memory_mib) {
            return Err(SceneSettingsError::MemoryBudgetOutOfRange);
        }

        let pixels = u64::from(width)
            .checked_mul(u64::from(height))
            .ok_or(SceneSettingsError::MemoryEstimateOverflow)?;
        let frame_bytes = pixels
            .checked_mul(12)
            .ok_or(SceneSettingsError::MemoryEstimateOverflow)?;
        let branch_bytes = u64::from(self.branch_limit)
            .checked_mul(ESTIMATED_BRANCH_BYTES)
            .and_then(|bytes| bytes.checked_mul(2))
            .ok_or(SceneSettingsError::MemoryEstimateOverflow)?;
        let estimate = frame_bytes
            .checked_add(branch_bytes)
            .ok_or(SceneSettingsError::MemoryEstimateOverflow)?;
        let budget = u64::from(self.max_memory_mib)
            .checked_mul(BYTES_PER_MIB)
            .ok_or(SceneSettingsError::MemoryEstimateOverflow)?;

        if estimate > budget {
            return Err(SceneSettingsError::ResourceBudgetExceeded);
        }

        Ok(())
    }

    /// Versioned Scene Pack uint32 seed derivation.
    ///
    /// The domain separator and little-endian byte order are part of the v1
    /// engine contract. This intentionally differs from the legacy phrase
    /// constructor; neither seed representation is implicitly converted to
    /// the other.
    pub fn numeric_seed_material(seed: u32) -> [u8; 32] {
        const DOMAIN: &[u8] = b"luminous.scene-pack.seed.v1\0";
        let mut input = Vec::with_capacity(DOMAIN.len() + 4);
        input.extend_from_slice(DOMAIN);
        input.extend_from_slice(&seed.to_le_bytes());
        *blake3::hash(&input).as_bytes()
    }

    /// Convert a Scene Pack pulse period to a bounded integer tick interval.
    /// The validator's finite (0, 3600] bound makes the conversion safe.
    pub fn pulse_interval_ticks(&self) -> u64 {
        (self.pulse_period_seconds * self.fixed_step_hz as f32)
            .round()
            .max(1.0) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings_preserve_legacy_scene_limits_and_palette() {
        let settings = SceneSettings::default();
        assert_eq!(settings.branch_limit, 8192);
        assert_eq!(settings.max_depth, 12);
        assert_eq!(settings.growth_rate, 1.0);
        assert_eq!(settings.palette, ScenePalette::default());
        assert!(settings.validate_for_dimensions(1920, 1080).is_ok());
    }

    #[test]
    fn numeric_seed_is_stable_and_domain_separated() {
        assert_eq!(
            SceneSettings::numeric_seed_material(20261010),
            SceneSettings::numeric_seed_material(20261010)
        );
        assert_ne!(
            SceneSettings::numeric_seed_material(20261010),
            SceneSettings::numeric_seed_material(20261011)
        );
        assert_ne!(
            SceneSettings::numeric_seed_material(0),
            *blake3::hash(&0_u32.to_le_bytes()).as_bytes()
        );
    }

    #[test]
    fn invalid_settings_fail_closed() {
        let mut settings = SceneSettings::default();
        settings.branch_limit = 0;
        assert_eq!(
            settings.validate_for_dimensions(32, 24),
            Err(SceneSettingsError::BranchLimitOutOfRange)
        );

        settings = SceneSettings::default();
        settings.max_depth = 25;
        assert_eq!(
            settings.validate_for_dimensions(32, 24),
            Err(SceneSettingsError::MaxDepthOutOfRange)
        );

        settings = SceneSettings::default();
        settings.growth_rate = f32::NAN;
        assert_eq!(
            settings.validate_for_dimensions(32, 24),
            Err(SceneSettingsError::GrowthRateOutOfRange)
        );

        settings = SceneSettings::default();
        settings.fixed_step_hz = 0;
        assert_eq!(
            settings.validate_for_dimensions(32, 24),
            Err(SceneSettingsError::FixedStepFrequencyOutOfRange)
        );

        settings = SceneSettings::default();
        settings.pulse_period_seconds = f32::INFINITY;
        assert_eq!(
            settings.validate_for_dimensions(32, 24),
            Err(SceneSettingsError::PulsePeriodOutOfRange)
        );

        settings = SceneSettings::default();
        settings.drift_amplitude = f32::NAN;
        assert_eq!(
            settings.validate_for_dimensions(32, 24),
            Err(SceneSettingsError::DriftAmplitudeOutOfRange)
        );
    }

    #[test]
    fn dimensions_and_memory_budget_are_enforced() {
        let mut settings = SceneSettings::default();
        assert_eq!(
            settings.validate_for_dimensions(0, 1080),
            Err(SceneSettingsError::InvalidDimensions(DimensionsError::Zero))
        );

        settings.max_memory_mib = 16;
        assert_eq!(
            settings.validate_for_dimensions(3840, 2160),
            Err(SceneSettingsError::ResourceBudgetExceeded)
        );
    }

    #[test]
    fn pulse_period_is_quantized_to_ticks() {
        let mut settings = SceneSettings::default();
        settings.fixed_step_hz = 30;
        settings.pulse_period_seconds = 7.5;
        assert_eq!(settings.pulse_interval_ticks(), 225);
        settings.pulse_period_seconds = 0.001;
        assert_eq!(settings.pulse_interval_ticks(), 1);
    }
}
