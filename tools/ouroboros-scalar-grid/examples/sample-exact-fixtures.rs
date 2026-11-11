use std::{error::Error, fs, path::PathBuf};

use ouroboros_scalar_grid::{
    AnalyticField, FieldSemantics, GridSpec, ScalarGrid, MAX_CANONICAL_BYTES,
    MAX_DESCRIPTOR_BYTES, MAX_SAMPLES, MAX_WORKING_PAYLOAD_BYTES,
};

fn main() -> Result<(), Box<dyn Error>> {
    let directory = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("ouroboros-exact-sdf-fixtures"));
    fs::create_dir_all(&directory)?;

    let spec = |dimensions| GridSpec {
        minimum_microunits: [-3_000_000; 3],
        maximum_microunits: [3_000_000; 3],
        dimensions,
        max_samples: MAX_SAMPLES,
        max_descriptor_bytes: MAX_DESCRIPTOR_BYTES,
        max_canonical_bytes: MAX_CANONICAL_BYTES,
        max_working_payload_bytes: MAX_WORKING_PAYLOAD_BYTES,
    };

    let q = 759_250_125_i64;
    let fixtures = [
        (
            "sphere-64",
            AnalyticField::Sphere {
                center_microunits: [0; 3],
                radius_microunits: 1_500_000,
            },
        ),
        (
            "box-64",
            AnalyticField::Box {
                center_microunits: [0; 3],
                half_extents_microunits: [1_500_000, 1_000_000, 750_000],
            },
        ),
        (
            "rotated-box-64",
            AnalyticField::OrientedBox {
                center_microunits: [0; 3],
                half_extents_microunits: [1_500_000, 1_000_000, 750_000],
                rotation_q30: [0, 0, q, q],
            },
        ),
        (
            "capsule-64",
            AnalyticField::Capsule {
                endpoint_a_microunits: [0, 0, -1_000_000],
                endpoint_b_microunits: [0, 0, 1_000_000],
                radius_microunits: 750_000,
            },
        ),
        (
            "torus-64",
            AnalyticField::Torus {
                center_microunits: [0; 3],
                major_radius_microunits: 1_500_000,
                minor_radius_microunits: 500_000,
            },
        ),
        (
            "capped-cylinder-64",
            AnalyticField::CappedCylinder {
                center_microunits: [0; 3],
                radius_microunits: 1_000_000,
                half_length_microunits: 1_500_000,
            },
        ),
        (
            "plane-64",
            AnalyticField::Plane {
                normal_q30: [0, 0, 1_i64 << 30],
                offset_microunits: 0,
            },
        ),
    ];

    for (name, field) in fixtures {
        let grid = ScalarGrid::sample_exact(spec([64, 64, 64]), field, 0)?;
        let bytes = grid.canonical_bytes()?;
        let path = directory.join(format!("{name}.ourosgd"));
        fs::write(&path, &bytes)?;
        let limits = grid.validate()?;
        println!(
            "{{\"fixture\":\"{name}\",\"schema\":\"OUROSGD1\",\"semantics\":\"{:?}\",\"dimensions\":[64,64,64],\"samples\":{},\"bytes\":{},\"working_payload_bytes\":{},\"path\":\"{}\"}}",
            grid.semantics(),
            limits.sample_count,
            bytes.len(),
            limits.working_payload_bytes,
            path.display()
        );
    }

    Ok(())
}
