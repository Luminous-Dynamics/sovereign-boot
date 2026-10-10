//! WIT-defined WebAssembly Component Model adapter for Sovereign Visual Core.
//!
//! The component exposes deterministic scene state and RGBA frames. The host
//! remains responsible for canvas/window creation, scheduling, display access,
//! interaction, lifecycle, security policy, and frame presentation.

use std::cell::RefCell;

use sovereign_visual_core::{
    contract::{self, StepError},
    mycelium::MycelialNetwork,
};

wit_bindgen::generate!({
    world: "visual-component",
});

use exports::luminous::sovereign_visual::scene::{Frame, Guest, GuestVisualScene};

fn step_error(error: StepError) -> String {
    match error {
        StepError::InvalidDelta => "dt-seconds must be finite and within 0..=0.25".to_owned(),
        StepError::InvalidActivity => "activity must be finite and within 0..=1".to_owned(),
    }
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
        self.network.borrow_mut().grow(dt_seconds, activity);
        Ok(())
    }

    fn pulse(&self) {
        self.network.borrow_mut().pulse();
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
            width: network.width,
            height: network.height,
            rgba: network.render_rgba(),
        }
    }

    fn width(&self) -> u32 {
        self.network.borrow().width
    }

    fn height(&self) -> u32 {
        self.network.borrow().height
    }

    fn branch_count(&self) -> u32 {
        self.network
            .borrow()
            .branches
            .len()
            .min(u32::MAX as usize) as u32
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

        for _ in 0..20 {
            native.grow(1.0 / 30.0, 0.7);
            guest.advance(1.0 / 30.0, 0.7).unwrap();
        }

        assert_eq!(guest.render().rgba, native.render_rgba());
    }

    #[test]
    fn guest_rejects_invalid_step_inputs_and_non_finite_progress() {
        let guest = <VisualScene as GuestVisualScene>::new(16, 16, "bounds".to_owned());
        assert!(guest.advance(1.0, 0.5).is_err());
        assert!(guest.advance(0.1, 1.5).is_err());
        assert!(guest.contract(f32::NAN).is_err());
    }
}
