//! WIT-defined WebAssembly Component Model adapter for Sovereign Visual Core.
//!
//! The component exposes deterministic scene state and RGBA frames. The host
//! remains responsible for canvas/window creation, scheduling, display access,
//! interaction, lifecycle, security policy, and frame presentation.

use std::cell::RefCell;

use sovereign_visual_core::mycelium::MycelialNetwork;

wit_bindgen::generate!({
    world: "visual-component",
});

use exports::luminous::sovereign_visual::scene::{Frame, Guest, GuestVisualScene};

const MAX_DIMENSION: u32 = 4096;
const MAX_PIXELS: u64 = 8_294_400;

fn normalize_dimensions(width: u32, height: u32) -> (u32, u32) {
    if width == 0 || height == 0 {
        return (1, 1);
    }

    let mut width = width.min(MAX_DIMENSION);
    let mut height = height.min(MAX_DIMENSION);
    let pixels = u64::from(width) * u64::from(height);

    if pixels > MAX_PIXELS {
        // WIT constructors cannot return a Result. Normalize defensively to a
        // bounded size while preserving aspect ratio; hosts can query width
        // and height to learn the actual effective dimensions.
        let scale = (MAX_PIXELS as f64 / pixels as f64).sqrt();
        width = ((width as f64 * scale).floor() as u32).max(1);
        height = ((height as f64 * scale).floor() as u32).max(1);
    }

    (width, height)
}

struct Component;

struct VisualScene {
    network: RefCell<MycelialNetwork>,
}

impl GuestVisualScene for VisualScene {
    fn new(width: u32, height: u32, seed: String) -> Self {
        let (width, height) = normalize_dimensions(width, height);
        Self {
            network: RefCell::new(MycelialNetwork::new(width, height, &seed)),
        }
    }

    fn advance(&self, dt_seconds: f32, activity: f32) -> Result<(), String> {
        if !dt_seconds.is_finite() || !(0.0..=0.25).contains(&dt_seconds) {
            return Err("dt-seconds must be finite and within 0..=0.25".to_owned());
        }
        if !activity.is_finite() || !(0.0..=1.0).contains(&activity) {
            return Err("activity must be finite and within 0..=1".to_owned());
        }
        self.network.borrow_mut().grow(dt_seconds, activity);
        Ok(())
    }

    fn pulse(&self) {
        self.network.borrow_mut().pulse();
    }

    fn contract(&self, progress: f32) -> Result<(), String> {
        if !progress.is_finite() {
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
        assert_eq!(normalize_dimensions(0, 1080), (1, 1));
        assert_eq!(normalize_dimensions(1920, 1080), (1920, 1080));

        let (width, height) = normalize_dimensions(4096, 4096);
        assert!(width > 0 && height > 0);
        assert!(width <= MAX_DIMENSION && height <= MAX_DIMENSION);
        assert!(u64::from(width) * u64::from(height) <= MAX_PIXELS);
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
}
