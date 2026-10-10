//! Native reference frame for cross-target browser-WASM parity checks.
//! stdout is raw RGBA8 bytes; do not write diagnostics to stdout.

use std::io::{self, Write};

use sovereign_visual_core::mycelium::MycelialNetwork;

fn main() -> io::Result<()> {
    let mut scene = MycelialNetwork::new(32, 24, "portable-smoke-fixture");
    for _ in 0..20 {
        scene.grow(1.0 / 30.0, 0.7);
    }

    io::stdout().lock().write_all(&scene.render_rgba())
}
