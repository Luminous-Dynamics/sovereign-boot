//! Browser-facing WASM adapter for Sovereign Visual Core.
//!
//! Returns portable RGBA bytes. The host owns canvas creation, scheduling,
//! visibility, power policy and presentation.

#[cfg(feature = "web")]
mod web {
    use sovereign_visual_core::mycelium::MycelialNetwork;
    use wasm_bindgen::prelude::*;

    const MAX_DIMENSION: u32 = 4096;
    const MAX_PIXELS: u64 = 8_294_400; // 3840x2160 ceiling; hosts may request less.

    fn validate_dimensions(width: u32, height: u32) -> Result<(), JsError> {
        let pixels = u64::from(width) * u64::from(height);
        if width == 0 || height == 0 || width > MAX_DIMENSION || height > MAX_DIMENSION || pixels > MAX_PIXELS {
            return Err(JsError::new("frame dimensions exceed Sovereign Visual Core limits"));
        }
        Ok(())
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
            validate_dimensions(width, height)?;
            Ok(Self { network: MycelialNetwork::new(width, height, seed) })
        }

        /// Advance by a bounded frame delta (0–250ms) and normalized activity.
        pub fn advance(&mut self, dt_seconds: f32, activity: f32) -> Result<(), JsError> {
            if !dt_seconds.is_finite() || !(0.0..=0.25).contains(&dt_seconds) {
                return Err(JsError::new("dt_seconds must be finite and in 0..=0.25"));
            }
            if !activity.is_finite() || !(0.0..=1.0).contains(&activity) {
                return Err(JsError::new("activity must be finite and in 0..=1"));
            }
            self.network.grow(dt_seconds, activity);
            Ok(())
        }

        /// Pulse formed nodes, e.g. after a host-side progress event.
        pub fn pulse(&mut self) {
            self.network.pulse();
        }

        /// Contract the scene to its center.
        pub fn contract(&mut self, progress: f32) -> Result<(), JsError> {
            if !progress.is_finite() {
                return Err(JsError::new("contraction progress must be finite"));
            }
            self.network.contract(progress);
            Ok(())
        }

        /// Render one tightly packed RGBA8 frame. The host owns presentation.
        pub fn render_rgba(&self) -> Vec<u8> {
            self.network.render_rgba()
        }

        pub fn width(&self) -> u32 { self.network.width }
        pub fn height(&self) -> u32 { self.network.height }
        pub fn branch_count(&self) -> u32 {
            self.network.branches.len().min(u32::MAX as usize) as u32
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn dimension_limits_are_explicit() {
            assert!(validate_dimensions(0, 100).is_err());
            assert!(validate_dimensions(1920, 1080).is_ok());
            assert!(validate_dimensions(3840, 2160).is_ok());
            assert!(validate_dimensions(4096, 2160).is_err());
        }
    }
}

#[cfg(feature = "web")]
pub use web::VisualScene;
