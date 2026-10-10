//! Native configured Scene Pack RGBA reference for cross-target conformance.
//!
//! stdout is raw RGBA8 bytes; diagnostics belong on stderr. This frame uses
//! the same pinned manifest, numeric seed, effective settings, and fixed-tick
//! sequence as the browser/WASM smoke test.
use std::io::{self, Write};

use sovereign_visual_pack::parse_scene_pack_v1;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 24;
    const ACTIVITY: f32 = 0.7;
    const SIMULATED_SECONDS: u32 = 4;

    let manifest_bytes = std::fs::read("tests/fixtures/first-germination.scene.json")?;
    let pack = parse_scene_pack_v1(&manifest_bytes)?;
    let mut scene = pack.instantiate(WIDTH, HEIGHT)?;
    let ticks = pack
        .settings()
        .fixed_step_hz
        .checked_mul(SIMULATED_SECONDS)
        .ok_or_else(|| io::Error::other("configured fixture tick count overflow"))?;

    // Advance one canonical tick at a time so the native reference follows
    // precisely the same operation sequence as renderConfiguredFixture().
    for _ in 0..ticks {
        scene.advance_ticks(1, ACTIVITY)?;
    }

    if scene.branch_count() > pack.resource_budget().max_branches {
        return Err(io::Error::other("native reference exceeded manifest branch budget").into());
    }
    let expected_len = (WIDTH as usize)
        .checked_mul(HEIGHT as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| io::Error::other("configured fixture frame length overflow"))?;
    let rgba = scene.render_rgba();
    if rgba.len() != expected_len {
        return Err(io::Error::other(format!(
            "native frame had {} bytes, expected {expected_len}",
            rgba.len()
        ))
        .into());
    }
    io::stdout().lock().write_all(&rgba)?;
    Ok(())
}
