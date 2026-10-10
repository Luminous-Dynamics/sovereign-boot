//! Browser-facing WASM adapter for Sovereign Visual Core.
//!
//! Returns portable RGBA bytes. The host owns canvas creation, scheduling,
//! visibility, power policy and presentation.

#[cfg(feature = "web")]
mod web {
    use sovereign_visual_core::{
        contract::{self, DimensionsError, StepError},
        mycelium::MycelialNetwork,
    };
    use wasm_bindgen::prelude::*;

    fn dimension_error(error: DimensionsError) -> JsError {
        JsError::new(match error {
            DimensionsError::Zero => "frame dimensions must be nonzero",
            DimensionsError::DimensionTooLarge => "frame dimensions exceed per-side limit",
            DimensionsError::PixelBudgetExceeded => "frame dimensions exceed pixel budget",
        })
    }

    fn step_error(error: StepError) -> JsError {
        JsError::new(match error {
            StepError::InvalidDelta => "dt_seconds must be finite and in 0..=0.25",
            StepError::InvalidActivity => "activity must be finite and in 0..=1",
        })
    }

    /// Deterministic, host-driven scene. It never accesses DOM or display APIs.
    #[wasm_bindgen]
    pub struct VisualScene {
        network: MycelialNetwork,
    }

    #[wasm_bindgen]
    impl VisualScene {
        /// Construct a scene from dimensions and a deterministic seed phrase.
        #[wasm_bindgen(constructor)]
        pub fn new(width: u32, height: u32, seed: &str) -> Result<VisualScene, JsError> {
            contract::validate_dimensions(width, height).map_err(dimension_error)?;
            Ok(Self {
                network: MycelialNetwork::new(width, height, seed),
            })
        }

        /// Advance by the shared portable step contract and normalized activity.
        pub fn advance(&mut self, dt_seconds: f32, activity: f32) -> Result<(), JsError> {
            contract::validate_step(dt_seconds, activity).map_err(step_error)?;
            self.network.grow(dt_seconds, activity);
            Ok(())
        }

        /// Pulse formed nodes, e.g. after a host-side progress event.
        pub fn pulse(&mut self) {
            self.network.pulse();
        }

        /// Contract the scene to its center. Finite progress is clamped by core.
        pub fn contract(&mut self, progress: f32) -> Result<(), JsError> {
            if !contract::valid_progress(progress) {
                return Err(JsError::new("contraction progress must be finite"));
            }
            self.network.contract(progress);
            Ok(())
        }

        /// Render one tightly packed RGBA8 frame. The host owns presentation.
        pub fn render_rgba(&self) -> Vec<u8> {
            self.network.render_rgba()
        }

        pub fn width(&self) -> u32 {
            self.network.width
        }

        pub fn height(&self) -> u32 {
            self.network.height
        }

        pub fn branch_count(&self) -> u32 {
            self.network.branches.len().min(u32::MAX as usize) as u32
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn dimension_limits_are_shared_with_the_portable_core() {
            assert!(contract::validate_dimensions(0, 100).is_err());
            assert!(contract::validate_dimensions(1920, 1080).is_ok());
            assert!(contract::validate_dimensions(3840, 2160).is_ok());
            assert!(contract::validate_dimensions(4096, 2160).is_err());
        }

        #[test]
        fn progress_validation_matches_the_shared_contract() {
            assert!(contract::valid_progress(2.0));
            assert!(!contract::valid_progress(f32::NAN));
        }
    }
}

#[cfg(feature = "web")]
pub use web::VisualScene;
