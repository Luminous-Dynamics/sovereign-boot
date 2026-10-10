//! Minimal host-portability smoke target for WASI.
//!
//! Renders a deterministic PPM frame to stdout. A WASI runtime/host decides
//! where stdout goes; this executable does not claim direct display access.

use sovereign_visual_core::{contract, mycelium::MycelialNetwork};
use sovereign_visual_pack::{
    Composition, MAX_MANIFEST_BYTES, Motion, PresentationVariant, ValidatedScenePack,
    parse_scene_pack_v1,
};
use std::io::{self, Read, Write};
use std::process::ExitCode;

#[derive(Debug)]
struct Options {
    width: u32,
    height: u32,
    seed: String,
    steps: u32,
    activity: f32,
    help: bool,
    contract_version: bool,
    scene_pack_stdin: bool,
    presentation: Option<String>,
    seed_was_explicit: bool,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            width: 960,
            height: 540,
            seed: "sovereign-visual-default".to_owned(),
            steps: 90,
            activity: 1.0,
            help: false,
            contract_version: false,
            scene_pack_stdin: false,
            presentation: None,
            seed_was_explicit: false,
        }
    }
}

fn parse_options() -> Result<Options, String> {
    let mut out = Options::default();
    let mut args = std::env::args().skip(1);

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--help" | "-h" => out.help = true,
            "--contract-version" => out.contract_version = true,
            "--scene-pack-stdin" => out.scene_pack_stdin = true,
            "--presentation" => {
                out.presentation = Some(args.next().ok_or("--presentation requires a value")?);
            }
            "--width" => {
                out.width = args
                    .next()
                    .ok_or("--width requires a value")?
                    .parse()
                    .map_err(|_| "invalid --width")?;
            }
            "--height" => {
                out.height = args
                    .next()
                    .ok_or("--height requires a value")?
                    .parse()
                    .map_err(|_| "invalid --height")?;
            }
            "--seed" => {
                out.seed = args.next().ok_or("--seed requires a value")?;
                out.seed_was_explicit = true;
            }
            "--steps" => {
                out.steps = args
                    .next()
                    .ok_or("--steps requires a value")?
                    .parse()
                    .map_err(|_| "invalid --steps")?;
            }
            "--activity" => {
                out.activity = args
                    .next()
                    .ok_or("--activity requires a value")?
                    .parse()
                    .map_err(|_| "invalid --activity")?;
            }
            other => return Err(format!("unknown argument: {other}; try --help")),
        }
    }

    if !out.help {
        if out.presentation.is_some() && !out.scene_pack_stdin {
            return Err("--presentation requires --scene-pack-stdin".into());
        }
        if out.scene_pack_stdin && out.seed_was_explicit {
            return Err("--seed cannot override the numeric seed in a Scene Pack".into());
        }
        if out.contract_version && (out.scene_pack_stdin || out.presentation.is_some()) {
            return Err(
                "--contract-version cannot be combined with Scene Pack rendering options".into(),
            );
        }
    }
    if !out.help && !out.contract_version {
        validate_options(&out)?;
    }
    Ok(out)
}

fn validate_options(options: &Options) -> Result<(), String> {
    if contract::validate_dimensions(options.width, options.height).is_err() {
        return Err("dimensions must be nonzero, <=4096 per side, and <=8,294,400 pixels".into());
    }
    if options.steps > 10_000 {
        return Err("--steps must be <= 10000".into());
    }
    if contract::validate_step(0.0, options.activity).is_err() {
        return Err("--activity must be finite and in 0..=1".into());
    }
    Ok(())
}

fn parse_presentation(value: Option<&str>) -> Result<PresentationVariant, String> {
    match value.unwrap_or("boot") {
        "boot" => Ok(PresentationVariant::Boot),
        "desktop" => Ok(PresentationVariant::Desktop),
        "idle" => Ok(PresentationVariant::Idle),
        "lockedBackground" => Ok(PresentationVariant::LockedBackground),
        "staticFallback" => Ok(PresentationVariant::StaticFallback),
        other => Err(format!("unsupported presentation {other:?}")),
    }
}

/// Produce one deterministic RGBA8 image using the validated Scene Pack.
/// This CPU WASI renderer supports centered-network and gradient-only
/// compositions; it fails closed rather than silently ignoring metadata.
fn render_scene_pack_rgba(
    pack: &ValidatedScenePack,
    variant: PresentationVariant,
    width: u32,
    height: u32,
    steps: u32,
    activity: f32,
) -> Result<Vec<u8>, String> {
    if !pack.assets().is_empty() {
        return Err(
            "this WASI renderer has no safe asset resolver/hash provider; asset-bearing packs are unsupported"
                .into(),
        );
    }
    if !pack.capabilities().required.is_empty() {
        return Err(
            "this CPU WASI renderer cannot satisfy the Scene Pack's required capabilities".into(),
        );
    }
    if !pack.inputs().is_empty() {
        return Err(
            "this WASI renderer does not implement Scene Pack inputs; refusing to ignore them"
                .into(),
        );
    }

    let presentation = pack.presentations().get(variant);
    if !presentation.safe_regions.is_empty() {
        return Err(
            "this CPU WASI renderer does not yet apply presentation safeRegions".into(),
        );
    }

    let mut scene = pack
        .instantiate(width, height)
        .map_err(|error| error.to_string())?;

    if presentation.composition == Composition::GradientOnly {
        if variant != PresentationVariant::StaticFallback || presentation.motion != Motion::None {
            return Err(
                "gradient-only composition is implemented only for the staticFallback presentation"
                    .into(),
            );
        }
        return scene
            .render_static_fallback(presentation.brightness)
            .map_err(|error| error.to_string());
    }

    if presentation.motion == Motion::None {
        return Err(
            "a motion=none presentation must use the supported gradient-only static fallback"
                .into(),
        );
    }
    if presentation.composition != Composition::CenteredNetwork {
        return Err(format!(
            "presentation composition {:?} is not implemented by this CPU WASI renderer",
            presentation.composition
        ));
    }

    let ticks_per_second = scene.settings().fixed_step_hz.min(120);
    let mut remaining = steps;
    while remaining > 0 {
        let batch = remaining.min(ticks_per_second);
        scene
            .advance_ticks(batch, activity)
            .map_err(|error| error.to_string())?;
        remaining -= batch;
    }

    let mut rgba = scene.render_rgba();
    apply_brightness(&mut rgba, presentation.brightness);
    Ok(rgba)
}

/// Fixed-point brightness transfer shared by the one-frame PPM host.
fn apply_brightness(rgba: &mut [u8], brightness: f32) {
    const Q16_ONE: u64 = 65_535;
    let multiplier = (f64::from(brightness) * Q16_ONE as f64).round() as u64;
    for pixel in rgba.chunks_exact_mut(4) {
        for channel in &mut pixel[..3] {
            *channel = ((u64::from(*channel) * multiplier + Q16_ONE / 2) / Q16_ONE) as u8;
        }
    }
}

fn write_ppm_rgba(
    width: u32,
    height: u32,
    rgba: &[u8],
    stdout: &mut impl Write,
) -> io::Result<()> {
    let expected_bytes = (u64::from(width) * u64::from(height) * 4) as usize;
    if rgba.len() != expected_bytes {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!("invalid RGBA frame length: {} (expected {expected_bytes})", rgba.len()),
        ));
    }
    write!(stdout, "P6\n{} {}\n255\n", width, height)?;
    let mut row = Vec::with_capacity(width as usize * 3);
    for y in 0..height as usize {
        row.clear();
        let start = y * width as usize * 4;
        for pixel in rgba[start..start + width as usize * 4].chunks_exact(4) {
            row.extend_from_slice(&pixel[..3]);
        }
        stdout.write_all(&row)?;
    }
    Ok(())
}

/// Read a bounded manifest from stdin; WASI grants access only if the host
/// connects stdin. Asset-backed or capability-requiring packs are rejected.
fn render_scene_pack_ppm(options: &Options, stdout: &mut impl Write) -> Result<(), String> {
    let mut bytes = Vec::new();
    io::stdin()
        .lock()
        .take((MAX_MANIFEST_BYTES as u64) + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| format!("failed to read Scene Pack from stdin: {error}"))?;
    let pack = parse_scene_pack_v1(&bytes).map_err(|error| error.to_string())?;
    let variant = parse_presentation(options.presentation.as_deref())?;
    let rgba = render_scene_pack_rgba(
        &pack,
        variant,
        options.width,
        options.height,
        options.steps,
        options.activity,
    )?;
    write_ppm_rgba(options.width, options.height, &rgba, stdout)
        .map_err(|error| format!("PPM output failed: {error}"))
}

fn render_ppm(options: &Options, stdout: &mut impl Write) -> io::Result<()> {
    let mut network =
        MycelialNetwork::new(options.width, options.height, &options.seed);
    const DT: f32 = 1.0 / 30.0;

    for _ in 0..options.steps {
        network.grow(DT, options.activity);
    }

    let pixels = (u64::from(options.width) * u64::from(options.height)) as usize;
    let mut packed = vec![0_u32; pixels];
    network.render(&mut packed);
    write!(
        stdout,
        "P6\n{} {}\n255\n",
        options.width, options.height
    )?;

    let mut row = Vec::with_capacity(options.width as usize * 3);
    for y in 0..options.height as usize {
        row.clear();
        for pixel in &packed[y * options.width as usize..(y + 1) * options.width as usize] {
            row.push(((pixel >> 16) & 0xff) as u8);
            row.push(((pixel >> 8) & 0xff) as u8);
            row.push((pixel & 0xff) as u8);
        }
        stdout.write_all(&row)?;
    }
    Ok(())
}

fn main() -> ExitCode {
    let options = match parse_options() {
        Ok(value) => value,
        Err(error) => {
            eprintln!("sovereign-visual-wasi: {error}");
            return ExitCode::from(2);
        }
    };

    if options.contract_version {
        println!("{}", contract::SCENE_CONTRACT_VERSION);
        return ExitCode::SUCCESS;
    }

    if options.help {
        eprintln!(concat!(
            "Usage: sovereign-visual-wasi [--width N] [--height N] [--seed TEXT] ",
            "[--steps N] [--activity 0..1] [--contract-version] ",
            "[--scene-pack-stdin] ",
            "[--presentation boot|desktop|idle|lockedBackground|staticFallback]\n",
            "Writes one binary PPM (P6) frame to stdout. Scene Pack JSON is read from stdin when ",
            "requested; this CPU renderer supports centered-network boot and gradient-only ",
            "staticFallback compositions only."
        ));
        return ExitCode::SUCCESS;
    }

    if options.scene_pack_stdin {
        return match render_scene_pack_ppm(&options, &mut io::stdout().lock()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("sovereign-visual-wasi: Scene Pack render failed: {error}");
                ExitCode::from(2)
            }
        };
    }

    match render_ppm(&options, &mut io::stdout().lock()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("sovereign-visual-wasi: output failed: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unbounded_work() {
        let mut options = Options::default();
        options.width = u32::MAX;
        assert!(validate_options(&options).is_err());

        options = Options::default();
        options.steps = 10_001;
        assert!(validate_options(&options).is_err());
    }

    const PACK_FIXTURE: &str =
        include_str!("../../../tests/fixtures/first-germination.scene.json");

    #[test]
    fn pinned_scene_pack_static_fallback_renders_deterministic_ppm_bytes() {
        let pack = parse_scene_pack_v1(PACK_FIXTURE.as_bytes()).unwrap();
        let rgba = render_scene_pack_rgba(
            &pack,
            PresentationVariant::StaticFallback,
            16,
            16,
            90,
            1.0,
        )
        .unwrap();

        assert_eq!(rgba.len(), 16 * 16 * 4);
        assert_eq!(&rgba[0..4], &[6, 9, 8, 255]);
        assert_eq!(&rgba[rgba.len() - 4..], &[15, 27, 20, 255]);

        let mut ppm = Vec::new();
        write_ppm_rgba(16, 16, &rgba, &mut ppm).unwrap();
        assert!(ppm.starts_with(b"P6\n16 16\n255\n"));
        assert_eq!(ppm.len(), b"P6\n16 16\n255\n".len() + 16 * 16 * 3);
    }

    #[test]
    fn pinned_scene_pack_boot_profile_replays_through_fixed_ticks() {
        let pack = parse_scene_pack_v1(PACK_FIXTURE.as_bytes()).unwrap();
        let a =
            render_scene_pack_rgba(&pack, PresentationVariant::Boot, 32, 24, 30, 0.7).unwrap();
        let b =
            render_scene_pack_rgba(&pack, PresentationVariant::Boot, 32, 24, 30, 0.7).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.len(), 32 * 24 * 4);
    }

    #[test]
    fn pack_renderer_fails_closed_on_unsupported_composition_and_assets() {
        let invalid_composition = PACK_FIXTURE.replace(
            r#""composition": "centered-network""#,
            r#""composition": "wide-network""#,
        );
        let pack = parse_scene_pack_v1(invalid_composition.as_bytes()).unwrap();
        assert!(
            render_scene_pack_rgba(&pack, PresentationVariant::Boot, 16, 16, 1, 1.0)
                .unwrap_err()
                .contains("composition")
        );

        let with_asset = PACK_FIXTURE.replace(
            r#""assets": []"#,
            r#""assets": [{
                "assetId": "preview-image",
                "path": "assets/preview.svg",
                "sha256": "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
                "mediaType": "image/svg+xml",
                "license": { "spdxId": "CC0-1.0" }
            }]"#,
        );
        let pack = parse_scene_pack_v1(with_asset.as_bytes()).unwrap();
        assert!(
            render_scene_pack_rgba(&pack, PresentationVariant::StaticFallback, 16, 16, 0, 1.0)
                .unwrap_err()
                .contains("asset resolver")
        );
    }

    #[test]
    fn renders_valid_ppm_header() {
        let options = Options {
            width: 16,
            height: 16,
            steps: 40,
            ..Options::default()
        };
        let mut bytes = Vec::new();
        render_ppm(&options, &mut bytes).unwrap();

        assert!(bytes.starts_with(b"P6\n16 16\n255\n"));
        assert_eq!(
            bytes.len(),
            b"P6\n16 16\n255\n".len() + 16 * 16 * 3
        );
    }
}
