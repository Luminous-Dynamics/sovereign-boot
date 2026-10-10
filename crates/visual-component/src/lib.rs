//! WIT-defined WebAssembly Component Model adapter for Sovereign Visual Core.
//!
//! The component exposes deterministic scene state and RGBA frames. The host
//! remains responsible for canvas/window creation, scheduling, display access,
//! interaction, lifecycle, security policy, and frame presentation.

use std::cell::RefCell;

use sovereign_visual_core::{
    color::Rgba,
    contract::{self, StepError},
    mycelium::MycelialNetwork,
    settings::{ScenePalette, SceneSettings, SceneSettingsError},
};

wit_bindgen::generate!({
    world: "visual-component",
});

use exports::luminous::sovereign_visual::scene::{
    Frame, Guest, GuestVisualScene, Rgb, ScenePalette as WitScenePalette,
    SceneSettings as WitSceneSettings,
};

fn step_error(error: StepError) -> String {
    match error {
        StepError::InvalidDelta => "dt-seconds must be finite and within 0..=0.25".to_owned(),
        StepError::InvalidActivity => "activity must be finite and within 0..=1".to_owned(),
    }
}

fn rgb_to_rgba(color: Rgb) -> Rgba {
    Rgba(color.r, color.g, color.b, 0xff)
}

fn settings_from_wit(settings: WitSceneSettings) -> (u32, SceneSettings) {
    let seed = settings.seed;
    let palette: WitScenePalette = settings.palette;
    (
        seed,
        SceneSettings {
            branch_limit: settings.branch_limit,
            max_depth: settings.max_depth,
            growth_rate: settings.growth_rate,
            fixed_step_hz: settings.fixed_step_hz,
            pulse_period_seconds: settings.pulse_period_seconds,
            drift_amplitude: settings.drift_amplitude,
            max_memory_mib: settings.max_memory_mib,
            palette: ScenePalette {
                canvas: rgb_to_rgba(palette.canvas),
                substrate: rgb_to_rgba(palette.substrate),
                filament: rgb_to_rgba(palette.filament),
                node: rgb_to_rgba(palette.node),
                lichen: rgb_to_rgba(palette.lichen),
                glow: rgb_to_rgba(palette.glow),
            },
        },
    )
}

struct Component;

struct VisualScene {
    network: RefCell<MycelialNetwork>,
}

impl GuestVisualScene for VisualScene {
    fn new(width: u32, height: u32, seed: String) -> Self {
        // WIT constructors cannot return Result, so use the shared normalizer.
        // The host must query width/height to learn the effective size.
        let (width, height) = contract::normalize_dimensions(width, height);
        Self {
            network: RefCell::new(MycelialNetwork::new(width, height, &seed)),
        }
    }

    fn contract_version(&self) -> u32 {
        u32::from(contract::SCENE_CONTRACT_VERSION)
    }

    fn advance(&self, dt_seconds: f32, activity: f32) -> Result<(), String> {
        contract::validate_step(dt_seconds, activity).map_err(step_error)?;
        self.network
            .borrow_mut()
            .advance_variable_delta(dt_seconds, activity)
            .map_err(|error| error.to_string())
    }

    fn pulse(&self) {
        self.network.borrow_mut().pulse();
    }

    fn configure(
        &self,
        width: u32,
        height: u32,
        settings: WitSceneSettings,
    ) -> Result<(), String> {
        let (seed, settings) = settings_from_wit(settings);
        let candidate = MycelialNetwork::with_settings(width, height, seed, settings)
            .map_err(|error: SceneSettingsError| error.to_string())?;
        // Construct and validate first, then replace atomically. Invalid
        // configuration never partially mutates the running scene.
        *self.network.borrow_mut() = candidate;
        Ok(())
    }

    fn advance_ticks(&self, ticks: u32, activity: f32) -> Result<(), String> {
        self.network
            .borrow_mut()
            .advance_ticks(ticks, activity)
            .map_err(|error| error.to_string())
    }

    fn contract(&self, progress: f32) -> Result<(), String> {
        if !contract::valid_progress(progress) {
            return Err("contraction progress must be finite".to_owned());
        }
        self.network.borrow_mut().contract(progress);
        Ok(())
    }

    fn render(&self) -> Frame {
        let network = self.network.borrow();
        Frame {
            width: network.width(),
            height: network.height(),
            rgba: network.render_rgba(),
        }
    }

    fn render_static_fallback(&self, brightness: f32) -> Result<Frame, String> {
        let network = self.network.borrow();
        let rgba = network
            .render_static_fallback(brightness)
            .map_err(|error| error.to_string())?;
        Ok(Frame {
            width: network.width(),
            height: network.height(),
            rgba,
        })
    }

    fn width(&self) -> u32 {
        self.network.borrow().width()
    }

    fn height(&self) -> u32 {
        self.network.borrow().height()
    }

    fn branch_count(&self) -> u32 {
        self.network.borrow().branch_count()
    }
}

impl Guest for Component {
    type VisualScene = VisualScene;
}

export!(Component);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_zero_and_over_limit_dimensions() {
        assert_eq!(contract::normalize_dimensions(0, 1080), (1, 1));
        assert_eq!(contract::normalize_dimensions(1920, 1080), (1920, 1080));

        let (width, height) = contract::normalize_dimensions(4096, 4096);
        assert!(contract::validate_dimensions(width, height).is_ok());
    }

    #[test]
    fn component_guest_matches_native_core_rgba_for_same_seed_and_steps() {
        let seed = "component-parity-fixture";
        let mut native = MycelialNetwork::new(32, 24, seed);
        let guest = <VisualScene as GuestVisualScene>::new(32, 24, seed.to_owned());
        assert_eq!(guest.contract_version(), u32::from(contract::SCENE_CONTRACT_VERSION));

        for _ in 0..20 {
            native.grow(1.0 / 30.0, 0.7);
            guest.advance(1.0 / 30.0, 0.7).unwrap();
        }

        assert_eq!(guest.render().rgba, native.render_rgba());
    }

    fn sample_wit_settings(branch_limit: u32) -> WitSceneSettings {
        WitSceneSettings {
            seed: 20261010,
            branch_limit,
            max_depth: 10,
            growth_rate: 0.28,
            fixed_step_hz: 30,
            pulse_period_seconds: 7.5,
            drift_amplitude: 0.12,
            max_memory_mib: 128,
            palette: WitScenePalette {
                canvas: Rgb { r: 10, g: 16, b: 14 },
                substrate: Rgb { r: 26, g: 46, b: 34 },
                filament: Rgb { r: 126, g: 200, b: 160 },
                node: Rgb { r: 232, g: 197, b: 71 },
                lichen: Rgb { r: 90, g: 107, b: 94 },
                glow: Rgb { r: 118, g: 217, b: 193 },
            },
        }
    }

    #[test]
    fn wit_settings_configure_the_same_core_and_rgba_output() {
        let guest = <VisualScene as GuestVisualScene>::new(16, 16, "legacy".to_owned());
        guest.configure(32, 24, sample_wit_settings(2048)).unwrap();
        assert_eq!(guest.contract_version(), u32::from(contract::SCENE_CONTRACT_VERSION));

        for _ in 0..4 {
            guest.advance_ticks(30, 0.7).unwrap();
        }
        let guest_frame = guest.render();
        assert_eq!(guest_frame.width, 32);
        assert_eq!(guest_frame.height, 24);
        assert_eq!(guest_frame.rgba.len(), 32 * 24 * 4);
        assert!(guest.branch_count() <= 2048);

        let fallback = guest.render_static_fallback(1.0).unwrap();
        assert_eq!((fallback.width, fallback.height), (32, 24));
        assert_eq!(&fallback.rgba[0..4], &[10, 16, 14, 255]);
        assert_eq!(&fallback.rgba[fallback.rgba.len() - 4..], &[26, 46, 34, 255]);
        assert!(guest.render_static_fallback(f32::NAN).is_err());
        assert_eq!(guest.render().rgba, guest_frame.rgba);

        let native_settings = settings_from_wit(sample_wit_settings(2048)).1;
        let mut native =
            MycelialNetwork::with_settings(32, 24, 20261010, native_settings).unwrap();
        for _ in 0..4 {
            native.advance_ticks(30, 0.7).unwrap();
        }
        assert_eq!(guest_frame.rgba, native.render_rgba());
    }

    #[test]
    fn wit_resource_rejects_mixed_variable_and_fixed_step_modes() {
        let variable = <VisualScene as GuestVisualScene>::new(16, 16, "variable".to_owned());
        variable.advance(1.0 / 30.0, 0.7).unwrap();
        assert!(variable.advance_ticks(1, 0.7).is_err());

        let fixed = <VisualScene as GuestVisualScene>::new(16, 16, "fixed".to_owned());
        fixed.configure(32, 24, sample_wit_settings(2048)).unwrap();
        fixed.advance_ticks(1, 0.7).unwrap();
        assert!(fixed.advance(1.0 / 30.0, 0.7).is_err());
    }

    #[test]
    fn failed_wit_reconfiguration_is_atomic_and_batches_are_bounded() {
        let guest = <VisualScene as GuestVisualScene>::new(32, 24, "legacy".to_owned());
        assert!(guest.configure(48, 24, sample_wit_settings(8193)).is_err());
        assert_eq!((guest.width(), guest.height()), (32, 24));
        assert!(guest.advance_ticks(121, 0.7).is_err());
    }

    #[test]
    fn guest_rejects_invalid_step_inputs_and_non_finite_progress() {
        let guest = <VisualScene as GuestVisualScene>::new(16, 16, "bounds".to_owned());
        assert!(guest.advance(1.0, 0.5).is_err());
        assert!(guest.advance(0.1, 1.5).is_err());
        assert!(guest.contract(f32::NAN).is_err());
    }
}
