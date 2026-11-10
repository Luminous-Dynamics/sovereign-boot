use std::{env, error::Error, fs};

use ouroboros_sdf_lab::{FaceSkinField, GridSpec, UnionMode, MAX_CANONICAL_BYTES, MAX_SAMPLES};

fn main() -> Result<(), Box<dyn Error>> {
    let output = env::args()
        .nth(1)
        .unwrap_or_else(|| "ouroboros-face-skin-128.ourogrid".to_owned());

    let grid = GridSpec {
        minimum: [-0.18, -0.14, -0.16],
        maximum: [0.25, 0.14, 0.16],
        dimensions: [128, 128, 128],
        max_samples: MAX_SAMPLES,
        max_canonical_bytes: MAX_CANONICAL_BYTES,
        max_working_payload_bytes: MAX_WORKING_PAYLOAD_BYTES,
    };
    let field = FaceSkinField::source_fixture(UnionMode::PolynomialSmooth {
        width: 0.004,
    })?;
    let sampled = grid.sample(field)?;
    let bytes = sampled.canonical_bytes()?;
    fs::write(&output, &bytes)?;

    // Keep stdout a valid, path-independent JSON record. The output path is
    // an invocation detail; the bytes themselves bind the complete fixture.
    println!(
        "{{\"schema\":\"OUROGRD1\",\"fixture\":\"ouroboros-face-skin-v1\",\"field\":\"iq-ellipsoid-approx-poly-smooth-min-v1\",\"union\":\"polynomial-smooth-min\",\"smooth_width\":0.004,\"dimensions\":[128,128,128],\"bounds\":[[-0.18,-0.14,-0.16],[0.25,0.14,0.16]],\"axis_order\":\"first-axis-fastest\",\"sample_count\":{},\"quantization\":1000000,\"canonical_bytes\":{}}}",
        sampled.quantized_samples.len(),
        bytes.len()
    );
    Ok(())
}
