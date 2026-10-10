//! Executes the production WIT component through Wasmtime without granting
//! it display, filesystem, network, or WASI host imports.
//
//! The linker intentionally has no imports registered. If the component
//! acquires a host dependency, instantiation fails until that capability is
//! reviewed and explicitly granted by the host policy.

use std::{
    env,
    error::Error,
    io,
    path::PathBuf,
};

use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, bindgen},
};

bindgen!({
    path: "../../crates/visual-component/wit",
    world: "visual-component",
});

fn fnv1a64(bytes: &[u8]) -> u64 {
    let mut hash = 0xcbf2_9ce4_8422_2325_u64;
    for byte in bytes {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

fn require_guest_ok(
    result: Result<(), String>,
    operation: &str,
) -> Result<(), Box<dyn Error>> {
    result.map_err(|message| io::Error::other(format!("{operation}: {message}")))?;
    Ok(())
}

fn main() -> Result<(), Box<dyn Error>> {
    let component_path = env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "usage: sovereign-visual-component-host-smoke <component.wasm>",
            )
        })?;

    let mut config = Config::new();
    config.consume_fuel(true);
    let engine = Engine::new(&config)?;
    let component = Component::from_file(&engine, &component_path)?;

    // No imports are provided. The scene component should be pure computation
    // and render output; the host remains in charge of presenting any frame.
    let linker = Linker::new(&engine);
    let mut store = Store::new(&engine, ());
    store.set_fuel(100_000_000)?;

    let bindings = VisualComponent::instantiate(&mut store, &component, &linker)?;
    let scene_api = bindings.luminous_sovereign_visual_scene();

    const WIDTH: u32 = 32;
    const HEIGHT: u32 = 24;
    const SEED: &str = "wasmtime-component-conformance-v1";
    const STEPS: usize = 20;
    const DT_SECONDS: f32 = 1.0 / 30.0;
    const ACTIVITY: f32 = 0.7;

    let first_scene = scene_api.call_constructor(&mut store, WIDTH, HEIGHT, SEED)?;
    let contract_version = scene_api.call_contract_version(&mut store, first_scene)?;
    if contract_version != 1 {
        return Err(io::Error::other(format!(
            "unexpected scene contract version: {contract_version}"
        ))
        .into());
    }

    for _ in 0..STEPS {
        require_guest_ok(
            scene_api.call_advance(&mut store, first_scene, DT_SECONDS, ACTIVITY)?,
            "advance first scene",
        )?;
    }

    // Invalid inputs must be rejected rather than silently changing semantics.
    if scene_api
        .call_advance(&mut store, first_scene, 1.0, ACTIVITY)?
        .is_ok()
    {
        return Err(io::Error::other("component accepted an out-of-range frame delta").into());
    }
    if scene_api.call_contract(&mut store, first_scene, f32::NAN)?.is_ok() {
        return Err(io::Error::other("component accepted non-finite contraction progress").into());
    }

    scene_api.call_pulse(&mut store, first_scene)?;
    require_guest_ok(
        scene_api.call_contract(&mut store, first_scene, 0.25)?,
        "contract first scene",
    )?;

    let first_frame = scene_api.call_render(&mut store, first_scene)?;
    let expected_len = (first_frame.width as usize)
        .checked_mul(first_frame.height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| io::Error::other("frame byte length overflow"))?;
    if first_frame.width != WIDTH || first_frame.height != HEIGHT {
        return Err(io::Error::other(format!(
            "unexpected frame dimensions: {}x{} (expected {WIDTH}x{HEIGHT})",
            first_frame.width, first_frame.height
        ))
        .into());
    }
    if first_frame.rgba.len() != expected_len {
        return Err(io::Error::other(format!(
            "invalid RGBA byte length: {} (expected {expected_len})",
            first_frame.rgba.len()
        ))
        .into());
    }

    let first_branch_count = scene_api.call_branch_count(&mut store, first_scene)?;

    // Replay a second scene using the same seed and inputs. Compare frames via
    // the actual component ABI, not by calling the Rust implementation directly.
    let replay_scene = scene_api.call_constructor(&mut store, WIDTH, HEIGHT, SEED)?;
    for _ in 0..STEPS {
        require_guest_ok(
            scene_api.call_advance(&mut store, replay_scene, DT_SECONDS, ACTIVITY)?,
            "advance replay scene",
        )?;
    }
    scene_api.call_pulse(&mut store, replay_scene)?;
    require_guest_ok(
        scene_api.call_contract(&mut store, replay_scene, 0.25)?,
        "contract replay scene",
    )?;
    let replay_frame = scene_api.call_render(&mut store, replay_scene)?;
    if first_frame.rgba != replay_frame.rgba {
        return Err(io::Error::other("identical replay inputs produced different RGBA frames").into());
    }

    // Verify the WIT constructor's fail-safe dimension normalization path.
    let normalized_scene = scene_api.call_constructor(&mut store, 0, 1080, "normalized")?;
    let normalized_width = scene_api.call_width(&mut store, normalized_scene)?;
    let normalized_height = scene_api.call_height(&mut store, normalized_scene)?;
    if (normalized_width, normalized_height) != (1, 1) {
        return Err(io::Error::other(format!(
            "unexpected normalized dimensions: {normalized_width}x{normalized_height}"
        ))
        .into());
    }

    // Explicitly release every guest-owned resource retained by this host.
    first_scene.resource_drop(&mut store)?;
    replay_scene.resource_drop(&mut store)?;
    normalized_scene.resource_drop(&mut store)?;

    println!(
        "component_runtime=wasmtime-49.0.2 scene_contract_version={contract_version} \
         dimensions={}x{} rgba_bytes={} frame_fnv1a64={:016x} replay=byte-identical \
         bounds=pass normalization=pass branches={} wasi_imports=none resources=dropped",
        first_frame.width,
        first_frame.height,
        first_frame.rgba.len(),
        fnv1a64(&first_frame.rgba),
        first_branch_count,
    );

    Ok(())
}
