//! Executes the production WIT component through Wasmtime without granting
//! it display, filesystem, network, or WASI host imports.
//
//! The linker intentionally has no imports registered. If the component
//! acquires a host dependency, instantiation fails until that capability is
//! reviewed and explicitly granted by the host policy.

use std::{env, error::Error, io, path::PathBuf};

use wasmtime::{
    Config, Engine, Store,
    component::{Component, Linker, bindgen},
};

bindgen!({
    path: "../../crates/visual-component/wit",
    world: "visual-component",
});

use exports::luminous::sovereign_visual::scene::{Rgb, ScenePalette, SceneSettings};
use sovereign_visual_pack::{parse_scene_pack_v1, PresentationVariant, ValidatedScenePack};

/// Convert the production parser's immutable core settings to the WIT wire
/// record. The parser owns validation; this helper only adapts data types.
fn settings_for_wit(
    pack: &ValidatedScenePack,
    branch_limit_override: Option<u32>,
) -> SceneSettings {
    let core = pack.settings();
    let palette = core.palette;
    SceneSettings {
        seed: pack.seed(),
        branch_limit: branch_limit_override.unwrap_or(core.branch_limit),
        resource_max_branches: core.resource_max_branches,
        max_depth: core.max_depth,
        growth_rate: core.growth_rate,
        fixed_step_hz: core.fixed_step_hz,
        pulse_period_seconds: core.pulse_period_seconds,
        drift_amplitude: core.drift_amplitude,
        max_memory_mib: core.max_memory_mib,
        palette: ScenePalette {
            canvas: Rgb {
                r: palette.canvas.0,
                g: palette.canvas.1,
                b: palette.canvas.2,
            },
            substrate: Rgb {
                r: palette.substrate.0,
                g: palette.substrate.1,
                b: palette.substrate.2,
            },
            filament: Rgb {
                r: palette.filament.0,
                g: palette.filament.1,
                b: palette.filament.2,
            },
            node: Rgb {
                r: palette.node.0,
                g: palette.node.1,
                b: palette.node.2,
            },
            lichen: Rgb {
                r: palette.lichen.0,
                g: palette.lichen.1,
                b: palette.lichen.2,
            },
            glow: Rgb {
                r: palette.glow.0,
                g: palette.glow.1,
                b: palette.glow.2,
            },
        },
    }
}
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

    // Exercise the production parser on the pinned upstream fixture before
    // any guest calls. The fixture has no assets, so this no-import host needs
    // no file resolver or SHA-256 capability.
    let fixture_bytes = std::fs::read("tests/fixtures/first-germination.scene.json")?;
    let manifest = parse_scene_pack_v1(&fixture_bytes)?;
    let static_fallback_brightness = manifest
        .presentations()
        .get(PresentationVariant::StaticFallback)
        .brightness;
    let configured_settings = settings_for_wit(&manifest, None);
    let aggregate_branch_limit = manifest.resource_budget().max_branches;
    if configured_settings.branch_limit > aggregate_branch_limit {
        return Err(io::Error::other(
            "validated scene branchLimit exceeds resourceBudget.maxBranches",
        )
        .into());
    }

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
    if scene_api
        .call_advance_ticks(&mut store, first_scene, 1, ACTIVITY)?
        .is_ok()
    {
        return Err(
            io::Error::other("component allowed variable-delta to fixed-tick mode switching").into(),
        );
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
        return Err(
            io::Error::other("identical replay inputs produced different RGBA frames").into(),
        );
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

    // Exercise the typed settings path through the real Component Model ABI.
    let configured_a = scene_api.call_constructor(&mut store, 16, 16, "legacy-a")?;
    require_guest_ok(
        scene_api.call_configure(&mut store, configured_a, 32, 24, configured_settings)?,
        "configure numeric-seed scene A",
    )?;
    let configured_b = scene_api.call_constructor(&mut store, 16, 16, "legacy-b")?;
    require_guest_ok(
        scene_api.call_configure(&mut store, configured_b, 32, 24, configured_settings)?,
        "configure numeric-seed scene B",
    )?;

    // Advance four reproducible seconds in one-second batches derived from
    // the fixture's declared frequency and the API's enforced batch ceiling.
    for _ in 0..4 {
        require_guest_ok(
            scene_api.call_advance_ticks(
                &mut store,
                configured_a,
                configured_settings.fixed_step_hz,
                0.7,
            )?,
            "advance configured scene A",
        )?;
        require_guest_ok(
            scene_api.call_advance_ticks(
                &mut store,
                configured_b,
                configured_settings.fixed_step_hz,
                0.7,
            )?,
            "advance configured scene B",
        )?;
    }
    if scene_api
        .call_advance(&mut store, configured_a, DT_SECONDS, ACTIVITY)?
        .is_ok()
    {
        return Err(
            io::Error::other("component allowed fixed-tick to variable-delta mode switching").into(),
        );
    }
    let configured_frame = scene_api.call_render(&mut store, configured_a)?;
    let configured_replay = scene_api.call_render(&mut store, configured_b)?;
    let configured_expected_len =
        (configured_frame.width as usize) * (configured_frame.height as usize) * 4;
    if (configured_frame.width, configured_frame.height) != (32, 24)
        || configured_frame.rgba.len() != configured_expected_len
    {
        return Err(io::Error::other("configured WIT frame dimensions/byte length invalid").into());
    }
    if configured_frame.rgba != configured_replay.rgba {
        return Err(
            io::Error::other("configured WIT scenes did not replay byte-identically").into(),
        );
    }

    let static_fallback = scene_api
        .call_render_static_fallback(&mut store, configured_a, static_fallback_brightness)?
        .map_err(|message| io::Error::other(format!("static fallback: {message}")))?;
    if (static_fallback.width, static_fallback.height) != (32, 24)
        || static_fallback.rgba.len() != configured_expected_len
        || &static_fallback.rgba[0..4] != &[6, 9, 8, 255]
        || &static_fallback.rgba[static_fallback.rgba.len() - 4..] != &[15, 27, 20, 255]
    {
        return Err(
            io::Error::other("WIT static fallback failed its endpoint/shape contract").into(),
        );
    }
    if scene_api
        .call_render_static_fallback(&mut store, configured_a, f32::NAN)?
        .is_ok()
    {
        return Err(
            io::Error::other("component accepted non-finite static fallback brightness").into(),
        );
    }
    if scene_api.call_render(&mut store, configured_a)?.rgba != configured_frame.rgba {
        return Err(io::Error::other("static fallback mutated scene state").into());
    }

    let configured_branches = scene_api.call_branch_count(&mut store, configured_a)?;
    if configured_branches > aggregate_branch_limit {
        return Err(io::Error::other("configured WIT scene exceeded aggregate branch limit").into());
    }

    // Invalid settings must not mutate the existing scene, and catch-up
    // requests must remain bounded even through a Component Model caller.
    let unchanged_scene = scene_api.call_constructor(&mut store, 16, 16, "unchanged")?;
    let mut under_budget = settings_for_wit(&manifest, None);
    under_budget.resource_max_branches = configured_settings.branch_limit - 1;
    if scene_api
        .call_configure(&mut store, unchanged_scene, 48, 24, under_budget)?
        .is_ok()
    {
        return Err(
            io::Error::other("component accepted a scene branch request above the resource ceiling")
                .into(),
        );
    }
    if (scene_api.call_width(&mut store, unchanged_scene)?,
        scene_api.call_height(&mut store, unchanged_scene)?) != (16, 16)
    {
        return Err(io::Error::other("under-budget rejection partially mutated the scene").into());
    }

    if scene_api
        .call_configure(
            &mut store,
            unchanged_scene,
            48,
            24,
            settings_for_wit(&manifest, Some(8193)),
        )?
        .is_ok()
    {
        return Err(io::Error::other("component accepted a branch limit above the contract").into());
    }
    if (scene_api.call_width(&mut store, unchanged_scene)?,
        scene_api.call_height(&mut store, unchanged_scene)?) != (16, 16)
    {
        return Err(io::Error::other("failed configure partially mutated the scene").into());
    }
    if scene_api
        .call_advance_ticks(&mut store, unchanged_scene, 121, 0.7)?
        .is_ok()
    {
        return Err(io::Error::other("component accepted an unbounded tick batch").into());
    }

    // Explicitly release every guest-owned resource retained by this host.
    first_scene.resource_drop(&mut store)?;
    replay_scene.resource_drop(&mut store)?;
    normalized_scene.resource_drop(&mut store)?;
    configured_a.resource_drop(&mut store)?;
    configured_b.resource_drop(&mut store)?;
    unchanged_scene.resource_drop(&mut store)?;

    println!(
        "component_runtime=wasmtime-49.0.2 scene_contract_version={contract_version} \
         legacy_dimensions={}x{} legacy_rgba_bytes={} legacy_frame_fnv1a64={:016x} \
         configured_dimensions={}x{} configured_rgba_bytes={} configured_fnv1a64={:016x} \
         configured_replay=byte-identical branch_budget=pass invalid_config_atomic=pass \
         tick_batch_bound=pass wasi_imports=none resources=dropped",
        first_frame.width,
        first_frame.height,
        first_frame.rgba.len(),
        fnv1a64(&first_frame.rgba),
        configured_frame.width,
        configured_frame.height,
        configured_frame.rgba.len(),
        fnv1a64(&configured_frame.rgba),
    );

    Ok(())
}
