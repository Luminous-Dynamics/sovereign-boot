// SPDX-License-Identifier: AGPL-3.0-or-later
//! Parse and validate WGSL without creating a GPU device or display surface.

use std::{env, fs, process};

fn main() {
    let Some(path) = env::args_os().nth(1) else {
        eprintln!("usage: visual-wgsl-check <shader.wgsl>");
        process::exit(2);
    };

    let path = path.into_string().unwrap_or_else(|_| {
        eprintln!("visual-wgsl-check: shader path is not valid UTF-8");
        process::exit(2);
    });

    let source = fs::read_to_string(&path).unwrap_or_else(|error| {
        eprintln!("visual-wgsl-check: cannot read {path}: {error}");
        process::exit(2);
    });

    let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| {
        eprintln!("WGSL parse failed for {path}:\n{}", error.emit_to_string(&source));
        process::exit(1);
    });

    let flags = naga::valid::ValidationFlags::all();
    let capabilities = naga::valid::Capabilities::all();
    let mut validator = naga::valid::Validator::new(flags, capabilities);

    if let Err(error) = validator.validate(&module) {
        eprintln!("visual-wgsl-check: WGSL validation failed for {path}:\n{error}");
        process::exit(1);
    }

    let entry_points = module.entry_points.len();
    println!(
        "validated {path}: {} entry points, WGSL parser + semantic validator passed",
        entry_points
    );
}
