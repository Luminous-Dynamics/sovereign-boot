// SPDX-License-Identifier: AGPL-3.0-or-later
// Luminous Visual Genome — desktop-only WGSL prototype.
//
// Status: shader prototype, exercised by a separate validation-only wgpu renderer.
// Uniform RGB components are linear-light values. Convert sRGB theme colors in
// the host adapter before upload. The stable style.x index comes from the theme
// manifest, so distinct scene families share the same host/shader ABI.

struct VisualParams {
    resolution: vec2<f32>,
    time: f32,
    motion_scale: f32,
    background: vec4<f32>,
    primary: vec4<f32>,
    secondary: vec4<f32>,
    node: vec4<f32>,
    convergence: vec4<f32>,
    style: vec4<f32>,
}

@group(0) @binding(0)
var<uniform> params: VisualParams;

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

fn hash21(p: vec2<f32>) -> f32 {
    let h = dot(p, vec2<f32>(127.1, 311.7));
    return fract(sin(h) * 43758.5453123);
}

fn value_noise(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (vec2<f32>(3.0, 3.0) - 2.0 * f);

    let a = hash21(i);
    let b = hash21(i + vec2<f32>(1.0, 0.0));
    let c = hash21(i + vec2<f32>(0.0, 1.0));
    let d = hash21(i + vec2<f32>(1.0, 1.0));

    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm(input: vec2<f32>) -> f32 {
    var p = input;
    var value = 0.0;
    var amplitude = 0.5;

    for (var octave = 0u; octave < 4u; octave = octave + 1u) {
        value += amplitude * value_noise(p);
        p = p * 2.03 + vec2<f32>(17.1, 9.2);
        amplitude *= 0.5;
    }
    return value;
}

// Only families that use warped noise pay for the field construction.
// Geometric families skip the generic multi-octave substrate entirely.
fn warped_field(p0: vec2<f32>, t: f32) -> vec2<f32> {
    let warp = vec2<f32>(
        fbm(p0 * 1.35 + vec2<f32>(0.0, t * 0.08)),
        fbm(p0 * 1.35 + vec2<f32>(8.7, -t * 0.06)),
    );
    return (p0 + (warp - vec2<f32>(0.5, 0.5)) * 0.9) * 3.2;
}

@fragment
fn fs_main(@builtin(position) pixel_position: vec4<f32>) -> @location(0) vec4<f32> {
    let resolution = max(params.resolution, vec2<f32>(1.0, 1.0));
    let uv = pixel_position.xy / resolution;
    let aspect = resolution.x / resolution.y;
    let p0 = (uv - vec2<f32>(0.5, 0.5)) * vec2<f32>(aspect, 1.0);
    let motion = clamp(params.motion_scale, 0.0, 1.0);
    let t = params.time * motion;

    // Keep scene-family cost proportional to the fields it consumes.
    var scene_glow = 0.0;
    var scene_nodes = 0.0;
    let family = u32(round(clamp(params.style.x, 0.0, 9.0)));
    switch family {
        case 0u: {
            // Mycelial network: organic filaments and soft junctions.
            let warped = warped_field(p0, t);
            let field_a = fbm(warped);
            let field_b = fbm(warped * 1.9 + vec2<f32>(13.0, 4.0));
            let filaments = exp(-abs(field_a - 0.5) * 38.0);
            let fine_filaments = exp(-abs(field_b - 0.52) * 52.0) * 0.22;
            let generic_glow = clamp(filaments + fine_filaments, 0.0, 1.0);
            let node_field = fbm(warped * 0.72 + vec2<f32>(t * 0.025, -t * 0.02));
            let generic_nodes = smoothstep(0.79, 0.88, node_field) * generic_glow;
            scene_glow = clamp(generic_glow + generic_nodes * 0.18, 0.0, 1.0);
            scene_nodes = generic_nodes;
        }
        case 1u: {
            // Solar canopy: radial corona, solar arcs, and ray structures.
            let angle = atan2(p0.y, p0.x);
            let radius = length(p0);
            let arc = radius - (0.20 + 0.035 * sin(angle * 7.0 + t * 0.015));
            let corona = exp(-abs(arc) * 34.0);
            let rays = pow(abs(cos(angle * 12.0)), 18.0);
            scene_glow = clamp(corona * (0.22 + rays * 0.78) + exp(-abs(radius - 0.38) * 23.0) * 0.24, 0.0, 1.0);
            scene_nodes = (1.0 - smoothstep(0.025, 0.13, radius)) * corona;
        }
        case 2u: {
            // Cosmic cartography: height contours plus sparse registered stars.
            let warped = warped_field(p0, t);
            let contours = exp(-abs(fbm(warped * 0.74) - 0.52) * 52.0);
            // A zero-motion still retains its full registered star field; in animated
            // modes the manifest budget controls the expected visible star density.
            var particle_budget = clamp(params.style.y, 0.0, 1.0);
            if motion <= 0.0 {
                particle_budget = 1.0;
            }
            let star_cell = floor(uv * vec2<f32>(160.0, 90.0));
            let star_hash = hash21(star_cell + vec2<f32>(23.0, 79.0));
            var stars = 0.0;
            if particle_budget >= 0.999 {
                stars = smoothstep(0.994, 0.999, star_hash);
            } else if particle_budget > 0.0 {
                let threshold_low = 1.0 - 0.006 * particle_budget;
                let threshold_high = 1.0 - 0.001 * particle_budget;
                stars = smoothstep(threshold_low, threshold_high, star_hash);
            }
            scene_glow = clamp(contours * 0.7 + stars * 0.9, 0.0, 1.0);
            scene_nodes = stars;
        }
        case 3u: {
            // Ocean waves: layered horizontal wave fronts with low-frequency drift.
            let wave_a = p0.y - 0.12 * sin(p0.x * 3.0 + t * 0.035) - 0.045 * sin(p0.x * 7.0 - t * 0.018);
            let wave_b = p0.y + 0.24 - 0.08 * sin(p0.x * 2.2 - t * 0.025);
            scene_glow = clamp(exp(-abs(wave_a) * 28.0) + exp(-abs(wave_b) * 23.0) * 0.72
                + exp(-abs(wave_a - 0.18) * 34.0) * 0.28, 0.0, 1.0);
            scene_nodes = smoothstep(0.92, 0.98, hash21(floor(vec2<f32>(p0.x * 15.0, p0.y * 8.0))));
        }
        case 4u: {
            // Epistemic lattice: connected grid lines and emphasized intersections.
            let warped = warped_field(p0, t);
            let cell = fract(warped * 0.78) - vec2<f32>(0.5, 0.5);
            let gap = min(abs(cell.x), abs(cell.y));
            let lines = 1.0 - smoothstep(0.018, 0.055, gap);
            let intersections = 1.0 - smoothstep(0.055, 0.16, length(cell));
            scene_glow = clamp(lines * 0.72 + intersections * 0.38, 0.0, 1.0);
            scene_nodes = intersections;
        }
        case 5u: {
            // Quantum interference: curved fringe bands with a moving phase.
            let warped = warped_field(p0, t);
            let phase = cos(length(p0 * vec2<f32>(1.0, 0.86)) * 33.0
                + 2.4 * sin(p0.x * 4.5 + t * 0.035));
            scene_glow = exp(-abs(phase) * 10.0);
            scene_nodes = smoothstep(0.96, 0.995, hash21(floor(warped * 2.0)));
        }
        case 6u: {
            // Aurora ribbons: translucent long ribbons with multiple slow arcs.
            let warped = warped_field(p0, t);
            let node_field = fbm(warped * 0.72 + vec2<f32>(t * 0.025, -t * 0.02));
            let ribbon_a = p0.y - 0.23 * sin(p0.x * 2.3 + t * 0.035) - 0.055 * sin(p0.x * 7.2);
            let ribbon_b = p0.y + 0.18 - 0.16 * sin(p0.x * 1.6 - t * 0.022);
            scene_glow = clamp(exp(-abs(ribbon_a) * 24.0) + exp(-abs(ribbon_b) * 19.0) * 0.76, 0.0, 1.0);
            scene_nodes = smoothstep(0.82, 0.93, node_field) * scene_glow;
        }
        case 7u: {
            // Graphite minimal: spare geometric strokes, no noisy full-field texture.
            let horizontal = abs(p0.y - 0.12 * sin(p0.x * 2.1));
            let vertical = abs(p0.x - 0.18 * sin(p0.y * 2.0));
            let strokes = max(1.0 - smoothstep(0.012, 0.03, horizontal),
                (1.0 - smoothstep(0.012, 0.03, vertical)) * 0.6);
            scene_glow = clamp(strokes * 0.38, 0.0, 1.0);
            scene_nodes = smoothstep(0.994, 0.999, hash21(floor(uv * vec2<f32>(8.0, 5.0))));
        }
        case 8u: {
            // Verdant recovery: central stems with alternating branchlets.
            let stem = abs(p0.x - 0.09 * sin(p0.y * 4.5));
            let branch_a = abs(p0.x - 0.18 * sin(p0.y * 4.5) - 0.055 * cos(p0.y * 8.0));
            let branch_b = abs(p0.x - 0.18 * sin(p0.y * 4.5) + 0.055 * cos(p0.y * 8.0));
            scene_glow = clamp(exp(-stem * 25.0) + exp(-branch_a * 31.0) * 0.7
                + exp(-branch_b * 31.0) * 0.7, 0.0, 1.0);
            scene_nodes = exp(-abs(p0.y - floor(p0.y * 8.0) / 8.0) * 55.0) * scene_glow;
        }
        case 9u: {
            // Civic commons: legible crossings and hubs in a shared street lattice.
            let cell = fract((p0 + vec2<f32>(0.5, 0.5)) * 4.0) - vec2<f32>(0.5, 0.5);
            let roads = 1.0 - smoothstep(0.03, 0.085, min(abs(cell.x), abs(cell.y)));
            let hubs = 1.0 - smoothstep(0.045, 0.12, length(cell));
            scene_glow = clamp(roads * 0.58 + hubs * 0.72, 0.0, 1.0);
            scene_nodes = hubs;
        }
        default: {}
    }

    var color = mix(params.background.rgb, params.secondary.rgb, scene_glow * 0.5);
    color = mix(color, params.primary.rgb, clamp(scene_glow * 1.15, 0.0, 1.0));
    color = mix(color, params.node.rgb, clamp(scene_nodes * 0.8, 0.0, 1.0));

    // Subtle vignette protects icon and panel legibility at screen edges.
    let radius = length((uv - vec2<f32>(0.5, 0.5)) * vec2<f32>(aspect, 1.0));
    let vignette = clamp(1.08 - radius * 0.48, 0.28, 1.0);
    color *= vignette;

    // Convergence uniform allows an authored handoff animation without a flash.
    color = mix(color, params.convergence.rgb, clamp(params.convergence.a, 0.0, 1.0));
    return vec4<f32>(clamp(color, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}
