//! Browser-facing WASM adapter for Sovereign Visual Core.
//!
//! Returns portable RGBA bytes. The host owns canvas creation, scheduling,
//! visibility, power policy and presentation.

#[cfg(feature = "web")]
mod web {
    use sovereign_visual_core::{
        color::Rgba,
        contract::{self, DimensionsError, StepError},
        mycelium::MycelialNetwork,
        settings::{ScenePalette, SceneSettings, SceneSettingsError},
    };
    use sovereign_visual_pack::{
        Composition, Motion, PresentationVariant, parse_scene_pack_v1,
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

    fn settings_error(error: SceneSettingsError) -> JsError {
        JsError::new(&error.to_string())
    }

    fn pack_error(error: impl std::fmt::Display) -> JsError {
        JsError::new(&error.to_string())
    }

    fn apply_brightness(rgba: &mut [u8], brightness: f32) {
        const Q16_ONE: u64 = 65_535;
        let multiplier = (f64::from(brightness) * Q16_ONE as f64).round() as u64;
        for pixel in rgba.chunks_exact_mut(4) {
            for channel in &mut pixel[..3] {
                *channel = ((u64::from(*channel) * multiplier + Q16_ONE / 2) / Q16_ONE) as u8;
            }
        }
    }

    fn parse_presentation(value: &str) -> Result<PresentationVariant, JsError> {
        match value {
            "boot" => Ok(PresentationVariant::Boot),
            "desktop" => Ok(PresentationVariant::Desktop),
            "idle" => Ok(PresentationVariant::Idle),
            "lockedBackground" => Ok(PresentationVariant::LockedBackground),
            "staticFallback" => Ok(PresentationVariant::StaticFallback),
            _ => Err(JsError::new("unknown Scene Pack presentation variant")),
        }
    }

    /// Deterministic, host-driven scene. It never accesses DOM or display APIs.
    #[wasm_bindgen]
    pub struct VisualScene {
        network: MycelialNetwork,
        presentation: PresentationVariant,
        presentation_brightness: f32,
        presentation_max_fps: u32,
        scene_id: String,
        scene_version: String,
        scene_title: String,
    }

    #[wasm_bindgen]
    impl VisualScene {
        /// Construct a scene from dimensions and a deterministic seed phrase.
        #[wasm_bindgen(constructor)]
        pub fn new(width: u32, height: u32, seed: &str) -> Result<VisualScene, JsError> {
            contract::validate_dimensions(width, height).map_err(dimension_error)?;
            Ok(Self {
                network: MycelialNetwork::new(width, height, seed),
                presentation: PresentationVariant::Boot,
                presentation_brightness: 1.0,
                presentation_max_fps: 30,
                scene_id: "luminous.legacy".to_owned(),
                scene_version: "0.0.0".to_owned(),
                scene_title: "Legacy seed-phrase scene".to_owned(),
            })
        }

        /// Create a configured scene from a numeric seed and typed settings.
        ///
        /// palette_rgb must contain six RGB triplets in this order:
        /// canvas, substrate, filament, node, lichen, glow. Alpha is always
        /// opaque in Scene Pack v1. The legacy phrase constructor is unchanged.
        #[wasm_bindgen(js_name = createConfigured)]
        pub fn create_configured(
            width: u32,
            height: u32,
            seed: u32,
            branch_limit: u32,
            resource_max_branches: u32,
            max_depth: u32,
            growth_rate: f32,
            fixed_step_hz: u32,
            pulse_period_seconds: f32,
            drift_amplitude: f32,
            max_memory_mib: u32,
            palette_rgb: Vec<u8>,
        ) -> Result<VisualScene, JsError> {
            if palette_rgb.len() != 18 {
                return Err(JsError::new(
                    "palette_rgb must contain exactly 18 bytes for six RGB colors",
                ));
            }
            let color = |index: usize| {
                let offset = index * 3;
                Rgba(
                    palette_rgb[offset],
                    palette_rgb[offset + 1],
                    palette_rgb[offset + 2],
                    0xff,
                )
            };
            let settings = SceneSettings {
                branch_limit,
                resource_max_branches,
                max_depth,
                growth_rate,
                fixed_step_hz,
                pulse_period_seconds,
                drift_amplitude,
                max_memory_mib,
                palette: ScenePalette {
                    canvas: color(0),
                    substrate: color(1),
                    filament: color(2),
                    node: color(3),
                    lichen: color(4),
                    glow: color(5),
                },
            };
            let network = MycelialNetwork::with_settings(width, height, seed, settings)
                .map_err(settings_error)?;
            Ok(Self {
                network,
                presentation: PresentationVariant::Boot,
                presentation_brightness: 1.0,
                presentation_max_fps: 30,
                scene_id: "luminous.configured".to_owned(),
                scene_version: "0.0.0".to_owned(),
                scene_title: "Configured scene".to_owned(),
            })
        }

        /// Load the pinned Scene Pack through a host-supplied byte buffer. This
        /// guest parser never fetches files or exercises filesystem privileges.
        /// Only centered-network motion and the gradient-only static fallback
        /// are currently supported by this adapter; unsupported metadata fails closed.
        #[wasm_bindgen(js_name = createFromScenePack)]
        pub fn create_from_scene_pack(
            width: u32,
            height: u32,
            manifest_json: &[u8],
            presentation_name: &str,
        ) -> Result<VisualScene, JsError> {
            let pack = parse_scene_pack_v1(manifest_json).map_err(pack_error)?;
            if !pack.assets().is_empty() {
                return Err(JsError::new(
                    "browser adapter has no verified asset resolver; asset-bearing packs are unsupported",
                ));
            }
            if !pack.capabilities().required.is_empty() {
                return Err(JsError::new(
                    "browser adapter cannot satisfy required Scene Pack capabilities",
                ));
            }
            if !pack.inputs().is_empty() {
                return Err(JsError::new(
                    "browser adapter does not implement Scene Pack inputs",
                ));
            }

            let variant = parse_presentation(presentation_name)?;
            let selected = pack.presentations().get(variant);
            if !selected.safe_regions.is_empty() {
                return Err(JsError::new(
                    "browser adapter does not yet implement presentation safeRegions",
                ));
            }
            if selected.composition == Composition::GradientOnly {
                if variant != PresentationVariant::StaticFallback || selected.motion != Motion::None {
                    return Err(JsError::new(
                        "gradient-only is supported only for motion=none staticFallback",
                    ));
                }
            } else if selected.composition != Composition::CenteredNetwork
                || selected.motion == Motion::None
            {
                return Err(JsError::new(
                    "browser adapter currently supports centered-network motion or staticFallback only",
                ));
            }

            let scene_id = pack.scene_id().to_owned();
            let scene_version = pack.scene_version().to_owned();
            let scene_title = pack.title().to_owned();
            let presentation_brightness = selected.brightness;
            let presentation_max_fps = selected.max_fps;
            let network = pack.instantiate(width, height).map_err(pack_error)?;

            Ok(Self {
                network,
                presentation: variant,
                presentation_brightness,
                presentation_max_fps,
                scene_id,
                scene_version,
                scene_title,
            })
        }

        /// Advance canonical integer ticks. The core bounds catch-up batches;
        /// hosts should pause on hide/suspend rather than perform unbounded catch-up.
        pub fn advance_ticks(&mut self, ticks: u32, activity: f32) -> Result<(), JsError> {
            self.network
                .advance_ticks(ticks, activity)
                .map_err(settings_error)
        }

        /// Host-independent scene semantics version.
        pub fn contract_version(&self) -> u32 {
            u32::from(contract::SCENE_CONTRACT_VERSION)
        }

        /// Advance by the shared portable step contract and normalized activity.
        pub fn advance(&mut self, dt_seconds: f32, activity: f32) -> Result<(), JsError> {
            contract::validate_step(dt_seconds, activity).map_err(step_error)?;
            self.network
                .advance_variable_delta(dt_seconds, activity)
                .map_err(settings_error)
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

        /// Render the selected, validated presentation to opaque RGBA8 pixels.
        /// StaticFallback uses the no-motion gradient; centered-network variants
        /// use the reference core with fixed-point brightness scaling.
        #[wasm_bindgen(js_name = renderPresentedRgba)]
        pub fn render_presented_rgba(&self) -> Result<Vec<u8>, JsError> {
            if self.presentation == PresentationVariant::StaticFallback {
                return self
                    .network
                    .render_static_fallback(self.presentation_brightness)
                    .map_err(settings_error);
            }
            let mut rgba = self.network.render_rgba();
            apply_brightness(&mut rgba, self.presentation_brightness);
            Ok(rgba)
        }

        #[wasm_bindgen(js_name = fixedStepHz)]
        pub fn fixed_step_hz(&self) -> u32 {
            self.network.settings().fixed_step_hz
        }

        #[wasm_bindgen(js_name = presentationMaxFps)]
        pub fn presentation_max_fps(&self) -> u32 {
            self.presentation_max_fps
        }

        #[wasm_bindgen(js_name = isStaticFallback)]
        pub fn is_static_fallback(&self) -> bool {
            self.presentation == PresentationVariant::StaticFallback
        }

        #[wasm_bindgen(js_name = sceneId)]
        pub fn scene_id(&self) -> String {
            self.scene_id.clone()
        }

        #[wasm_bindgen(js_name = sceneVersion)]
        pub fn scene_version(&self) -> String {
            self.scene_version.clone()
        }

        #[wasm_bindgen(js_name = sceneTitle)]
        pub fn scene_title(&self) -> String {
            self.scene_title.clone()
        }

        /// Render the deterministic non-animated gradient fallback without
        /// mutating scene state. Intended for reduced motion or renderer failure.
        pub fn render_static_fallback(&self, brightness: f32) -> Result<Vec<u8>, JsError> {
            self.network
                .render_static_fallback(brightness)
                .map_err(settings_error)
        }

        pub fn width(&self) -> u32 {
            self.network.width()
        }

        pub fn height(&self) -> u32 {
            self.network.height()
        }

        pub fn branch_count(&self) -> u32 {
            self.network.branch_count()
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

        #[test]
        fn static_fallback_returns_configured_gradient_and_rejects_bad_brightness() {
            let palette = vec![
                10, 16, 14, 26, 46, 34, 126, 200, 160, 232, 197, 71, 90, 107, 94, 118, 217, 193,
            ];
            let scene = VisualScene::create_configured(
                2, 2, 20261010, 2048, 2048, 10, 0.28, 30, 7.5, 0.12, 128, palette,
            )
            .unwrap();
            let before = scene.render_rgba();
            let fallback = scene.render_static_fallback(1.0).unwrap();
            assert_eq!(&fallback[0..4], &[10, 16, 14, 255]);
            assert_eq!(&fallback[fallback.len() - 4..], &[26, 46, 34, 255]);
            assert_eq!(scene.render_rgba(), before);
            assert!(scene.render_static_fallback(f32::NAN).is_err());
        }

        #[test]
        fn configured_settings_are_validated_by_the_core() {
            let palette = vec![
                10, 16, 14, 26, 46, 34, 126, 200, 160, 232, 197, 71, 90, 107, 94, 118, 217,
                193,
            ];
            let scene = VisualScene::create_configured(
                32, 24, 20261010, 2048, 2048, 10, 0.28, 30, 7.5, 0.12, 128, palette,
            );
            assert!(scene.is_ok());
            assert!(VisualScene::create_configured(
                32, 24, 20261010, 8193, 8192, 10, 0.28, 30, 7.5, 0.12, 128,
                vec![0; 18],
            ).is_err());
            assert!(VisualScene::create_configured(
                32, 24, 20261010, 2048, 1024, 10, 0.28, 30, 7.5, 0.12, 128,
                vec![
                     10, 16, 14, 26, 46, 34, 126, 200, 160, 232, 197, 71, 90, 107, 94, 118,
                    217, 193,
                ],
            ).is_err(), "scene branch request cannot exceed host resource budget");
            assert!(VisualScene::create_configured(
                32, 24, 20261010, 2048, 2048, 10, 0.28, 30, 7.5, 0.12, 128,
                vec![0; 17],
            ).is_err());
        }
    }
}

#[cfg(feature = "web")]
pub use web::VisualScene;
