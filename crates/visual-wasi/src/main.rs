//! Minimal host-portability smoke target for WASI.
//!
//! Renders a deterministic PPM frame to stdout. A WASI runtime/host decides
//! where stdout goes; this executable does not claim direct display access.

use sovereign_visual_core::{contract, mycelium::MycelialNetwork};
use std::io::{self, Write};
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
        eprintln!(
            "Usage: sovereign-visual-wasi [--width N] [--height N] [--seed TEXT] [--steps N] [--activity 0..1]\nWrites a binary PPM (P6) frame to stdout."
        );
        return ExitCode::SUCCESS;
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
