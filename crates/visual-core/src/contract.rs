//! Shared, versioned semantic limits for portable visual scenes.
//!
//! Every adapter translates these rules into its own error ABI. The rules
//! themselves live here so browser, WASI, Component Model and native hosts do
//! not accidentally accept different inputs for the same scene contract.

/// Version of the host-independent scene semantics (not the crate version).
pub const SCENE_CONTRACT_VERSION: u16 = 1;
/// Maximum supported width or height, before the total-pixel budget is applied.
pub const MAX_DIMENSION: u32 = 4096;
/// Upper bound chosen to cap memory use at roughly one 4K RGBA frame.
pub const MAX_PIXELS: u64 = 8_294_400;
/// Largest single host-driven simulation step; large gaps must be handled by
/// the host's pause/resume policy, not passed through as one simulation step.
pub const MAX_FRAME_DELTA_SECONDS: f32 = 0.25;
/// Activity is a normalized host signal, independent of any OS-specific metric.
pub const MAX_ACTIVITY: f32 = 1.0;

/// Why a frame size is outside the portable contract.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DimensionsError {
    Zero,
    DimensionTooLarge,
    PixelBudgetExceeded,
}

/// Validate a requested output size without modifying it.
pub fn validate_dimensions(width: u32, height: u32) -> Result<(), DimensionsError> {
    if width == 0 || height == 0 {
        return Err(DimensionsError::Zero);
    }
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(DimensionsError::DimensionTooLarge);
    }
    if u64::from(width) * u64::from(height) > MAX_PIXELS {
        return Err(DimensionsError::PixelBudgetExceeded);
    }
    Ok(())
}

/// Normalize dimensions for interfaces, such as a WIT constructor, that cannot
/// return a construction error. The caller must report the effective size to
/// the host. Other adapters may instead reject invalid requested dimensions.
pub fn normalize_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }

    let mut width = width.min(MAX_DIMENSION);
    let mut height = height.min(MAX_DIMENSION);
    let pixels = u64::from(width) * u64::from(height);

    if pixels > MAX_PIXELS {
        let scale = (MAX_PIXELS as f64 / pixels as f64).sqrt();
        width = ((f64::from(width) * scale).floor() as u32).max(1);
        height = ((f64::from(height) * scale).floor() as u32).max(1);
    }

    (width, height)
}

/// Stable semantic errors for host-driven simulation input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepError {
    InvalidDelta,
    InvalidActivity,
}

/// Validate a simulation step identically on every target.
pub fn validate_step(dt_seconds: f32, activity: f32) -> Result<(), StepError> {
    if !dt_seconds.is_finite() || !(0.0..=MAX_FRAME_DELTA_SECONDS).contains(&dt_seconds) {
        return Err(StepError::InvalidDelta);
    }
    if !activity.is_finite() || !(0.0..=MAX_ACTIVITY).contains(&activity) {
        return Err(StepError::InvalidActivity);
    }
    Ok(())
}

/// Contraction progress is clamped by the core renderer, but NaN and infinity
/// are not meaningful protocol values and must be rejected consistently.
pub fn valid_progress(progress: f32) -> bool {
    progress.is_finite()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dimensions_enforce_both_axis_and_pixel_limits() {
        assert_eq!(validate_dimensions(0, 1080), Err(DimensionsError::Zero));
        assert_eq!(
            validate_dimensions(MAX_DIMENSION + 1, 1),
            Err(DimensionsError::DimensionTooLarge)
        );
        assert_eq!(
            validate_dimensions(4096, 2160),
            Err(DimensionsError::PixelBudgetExceeded)
        );
        assert!(validate_dimensions(3840, 2160).is_ok());
    }

    #[test]
    fn normalization_always_produces_a_valid_nonzero_size() {
        for (requested_width, requested_height) in [
            (0, 1080),
            (1920, 1080),
            (4096, 4096),
            (u32::MAX, u32::MAX),
        ] {
            let (width, height) = normalize_dimensions(requested_width, requested_height);
            assert!(validate_dimensions(width, height).is_ok());
        }
    }

    #[test]
    fn simulation_input_boundaries_are_explicit() {
        assert!(validate_step(0.0, 0.0).is_ok());
        assert!(validate_step(MAX_FRAME_DELTA_SECONDS, MAX_ACTIVITY).is_ok());
        assert_eq!(validate_step(-0.01, 0.5), Err(StepError::InvalidDelta));
        assert_eq!(validate_step(0.26, 0.5), Err(StepError::InvalidDelta));
        assert_eq!(validate_step(f32::NAN, 0.5), Err(StepError::InvalidDelta));
        assert_eq!(validate_step(0.1, -0.1), Err(StepError::InvalidActivity));
        assert_eq!(validate_step(0.1, f32::INFINITY), Err(StepError::InvalidActivity));
    }

    #[test]
    fn progress_rejects_non_finite_values_but_leaves_clamping_to_core() {
        assert!(valid_progress(0.0));
        assert!(valid_progress(1.0));
        assert!(valid_progress(2.0));
        assert!(!valid_progress(f32::NAN));
        assert!(!valid_progress(f32::INFINITY));
    }
}
