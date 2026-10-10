// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root
/// L-system mycelial growth renderer.
///
/// Generates procedural mycelial network growth seeded by the genesis phrase.
/// Renders to a raw pixel buffer using Bresenham's line algorithm — no GPU required.
use crate::{
    color::Rgba,
    settings::{
        MAX_TICKS_PER_BATCH, SceneSettings, SceneSettingsError, render_static_gradient_rgba,
    },
};
use rand_chacha::ChaCha12Rng;
use rand_core::{RngCore, SeedableRng};

/// Minimum branch length in pixels before a branch can spawn children.
const MIN_BRANCH_LEN: f32 = 4.0;

/// A single branch of the mycelial network.
#[derive(Debug, Clone)]
pub struct Branch {
    pub start: (f32, f32),
    pub end: (f32, f32),
    pub angle: f32,
    pub thickness: f32,
    pub opacity: f32,
    pub depth: u32,
    pub growing: bool,
    pub growth_progress: f32,
    /// Whether this branch has formed a node at its tip (intersection).
    pub has_node: bool,
    /// Node pulse brightness (0.0 = dormant, 1.0 = fully lit).
    pub node_brightness: f32,
}

impl Branch {
    fn new(start: (f32, f32), angle: f32, thickness: f32, depth: u32) -> Self {
        Self {
            start,
            end: start,
            angle,
            thickness,
            opacity: 1.0,
            depth,
            growing: true,
            growth_progress: 0.0,
            has_node: false,
            node_brightness: 0.0,
        }
    }

    fn length(&self) -> f32 {
        let dx = self.end.0 - self.start.0;
        let dy = self.end.1 - self.start.1;
        (dx * dx + dy * dy).sqrt()
    }

    /// Target length for this branch based on depth (deeper = shorter).
    fn target_length(&self) -> f32 {
        80.0 / (1.0 + self.depth as f32 * 0.5)
    }
}

/// The full mycelial network state.
pub struct MycelialNetwork {
    pub branches: Vec<Branch>,
    pub width: u32,
    pub height: u32,
    rng: ChaCha12Rng,
    center: (f32, f32),
    /// Elapsed time in seconds (fractional).
    pub elapsed: f32,
    /// All nodes pulsing simultaneously (completion events).
    pub global_pulse: f32,
    /// Contraction progress (0.0 = normal, 1.0 = fully contracted to center).
    pub contraction: f32,
    /// Effective validated settings for this scene. Kept private so callers
    /// cannot mutate a constructed scene past its validated resource limits.
    settings: SceneSettings,
    /// Number of canonical fixed-step ticks completed by advance_tick.
    simulation_ticks: u64,
}

impl MycelialNetwork {
    /// Legacy phrase-based constructor retained for compatibility.
    /// Numeric Scene Pack seeds must use with_settings; seed modes are distinct.
    pub fn new(width: u32, height: u32, genesis_phrase: &str) -> Self {
        let seed_bytes: [u8; 32] = *blake3::hash(genesis_phrase.as_bytes()).as_bytes();
        Self::from_seed_material(width, height, seed_bytes, SceneSettings::default())
    }

    /// Construct from a versioned numeric seed and bounded scene settings.
    pub fn with_settings(
        width: u32,
        height: u32,
        seed: u32,
        settings: SceneSettings,
    ) -> Result<Self, SceneSettingsError> {
        settings.validate_for_dimensions(width, height)?;
        let seed_bytes = SceneSettings::numeric_seed_material(seed);
        Ok(Self::from_seed_material(width, height, seed_bytes, settings))
    }

    fn from_seed_material(
        width: u32,
        height: u32,
        seed_bytes: [u8; 32],
        settings: SceneSettings,
    ) -> Self {
        let rng = ChaCha12Rng::from_seed(seed_bytes);
        let center = (width as f32 / 2.0, height as f32 / 2.0);
        let initial_count = (4 + (seed_bytes[0] % 5) as usize)
            .min(settings.branch_limit as usize);
        let mut net = Self {
            branches: Vec::with_capacity(initial_count),
            width,
            height,
            rng,
            center,
            elapsed: 0.0,
            global_pulse: 0.0,
            contraction: 0.0,
            settings,
            simulation_ticks: 0,
        };

        // The seed's initial fan obeys the effective branch budget.
        let angle_step = std::f32::consts::TAU / initial_count as f32;
        for i in 0..initial_count {
            let angle = angle_step * i as f32 + (seed_bytes[1] as f32 / 255.0) * 0.5;
            net.branches.push(Branch::new(center, angle, 2.5, 0));
        }
        net
    }

    /// Advance one canonical tick; periodic pulses use the integer tick count.
    pub fn advance_tick(&mut self, activity: f32) -> Result<(), SceneSettingsError> {
        if !activity.is_finite() || !(0.0..=1.0).contains(&activity) {
            return Err(SceneSettingsError::InvalidActivity);
        }
        let dt = 1.0 / self.settings.fixed_step_hz as f32;
        self.grow_internal(dt, activity, false);
        self.simulation_ticks = self.simulation_ticks.saturating_add(1);
        if self.simulation_ticks % self.settings.pulse_interval_ticks() == 0 {
            self.pulse();
        }
        Ok(())
    }

    /// Advance a caller-bounded batch of canonical fixed ticks.
    pub fn advance_ticks(
        &mut self,
        ticks: u32,
        activity: f32,
    ) -> Result<(), SceneSettingsError> {
        if !activity.is_finite() || !(0.0..=1.0).contains(&activity) {
            return Err(SceneSettingsError::InvalidActivity);
        }
        let max_batch = self.settings.fixed_step_hz.min(MAX_TICKS_PER_BATCH);
        if ticks > max_batch {
            return Err(SceneSettingsError::TickBatchOutOfRange);
        }
        for _ in 0..ticks {
            self.advance_tick(activity)?;
        }
        Ok(())
    }

    /// Coordinate-continuous, deterministic drift keeps connected branch
    /// endpoints aligned because the offset is derived from position.
    fn drifted_point(&self, point: (f32, f32)) -> (f32, f32) {
        let amplitude = self.settings.drift_amplitude;
        if amplitude == 0.0 {
            return point;
        }
        let scale = self.width.min(self.height) as f32 * 0.01 * amplitude;
        let phase = point.0 * 0.037 + point.1 * 0.053;
        let time = self.elapsed;
        let dx = scale * ((phase + time * 0.67).sin() - phase.sin());
        let dy = scale * ((phase * 1.37 + time * 0.49).cos() - (phase * 1.37).cos());
        (point.0 + dx, point.1 + dy)
    }

    /// Read the effective immutable settings for this scene.
    pub fn settings(&self) -> &SceneSettings {
        &self.settings
    }

    /// Render the non-animated static-gradient fallback at the host-selected
    /// bounded brightness. This does not mutate scene state.
    pub fn render_static_fallback(
        &self,
        brightness: f32,
    ) -> Result<Vec<u8>, SceneSettingsError> {
        render_static_gradient_rgba(self.width, self.height, self.settings.palette, brightness)
    }

    /// Advance the legacy variable-delta API.
    ///
    /// For backward compatibility, this path preserves its historical minimum
    /// crawl at zero activity. New settings-driven hosts should use
    /// advance_tick/advance_ticks, where zero activity truly pauses growth.
    pub fn grow(&mut self, dt: f32, io_rate: f32) {
        self.grow_internal(dt, io_rate, true);
    }

    fn grow_internal(&mut self, dt: f32, io_rate: f32, minimum_crawl: bool) {
        self.elapsed += dt;

        // Decay global pulse
        if self.global_pulse > 0.0 {
            self.global_pulse = (self.global_pulse - dt * 2.0).max(0.0);
        }

        // Decay node brightness
        for branch in &mut self.branches {
            if branch.node_brightness > 0.0 {
                branch.node_brightness = (branch.node_brightness - dt * 1.5).max(0.0);
            }
        }

        let effective_activity = if minimum_crawl {
            io_rate.max(0.05)
        } else {
            io_rate.max(0.0)
        };
        let growth_speed = 30.0 * effective_activity * self.settings.growth_rate;

        // Grow existing branches
        let mut new_branches: Vec<Branch> = Vec::new();
        for branch in &mut self.branches {
            if !branch.growing {
                continue;
            }

            let target = branch.target_length();
            branch.growth_progress += growth_speed * dt / target;
            branch.growth_progress = branch.growth_progress.min(1.0);

            let len = target * branch.growth_progress;
            branch.end = (
                branch.start.0 + branch.angle.cos() * len,
                branch.start.1 + branch.angle.sin() * len,
            );

            // Branch complete — try spawning children
            if branch.growth_progress >= 1.0 {
                branch.growing = false;
                branch.has_node = true;
            }
        }

        // Spawn children from completed branches
        let total_branches = self.branches.len();
        for i in 0..total_branches {
            let branch = &self.branches[i];
            if branch.growing || !branch.has_node || branch.depth >= self.settings.max_depth {
                continue;
            }
            if branch.length() < MIN_BRANCH_LEN {
                continue;
            }
            if total_branches + new_branches.len() >= self.settings.branch_limit as usize {
                break;
            }

            // Already spawned children? Check if any branch starts at our end.
            let end = self.branches[i].end;
            let already_spawned = self.branches.iter().any(|b| {
                let dx = b.start.0 - end.0;
                let dy = b.start.1 - end.1;
                (dx * dx + dy * dy) < 1.0 && !std::ptr::eq(b, &self.branches[i])
            }) || new_branches.iter().any(|b: &Branch| {
                let dx = b.start.0 - end.0;
                let dy = b.start.1 - end.1;
                (dx * dx + dy * dy) < 1.0
            });

            if already_spawned {
                continue;
            }

            let depth = self.branches[i].depth;
            let angle = self.branches[i].angle;
            let thickness = self.branches[i].thickness;

            // 1-3 children per node
            let n_children = 1 + (self.rng.next_u32() % 3);
            for _ in 0..n_children {
                if total_branches + new_branches.len() >= self.settings.branch_limit as usize {
                    break;
                }
                let fork_angle = (15.0_f32 + (self.rng.next_u32() as f32 / u32::MAX as f32) * 30.0).to_radians();
                let sign = if self.rng.next_u32() & 1 == 1 { 1.0 } else { -1.0 };
                let child_angle = angle + fork_angle * sign;
                let child_thickness = (thickness * 0.75).max(0.5);
                new_branches.push(Branch::new(end, child_angle, child_thickness, depth + 1));
            }
        }

        self.branches.extend(new_branches);
    }

    /// Pulse all nodes simultaneously (call on derivation completion).
    pub fn pulse(&mut self) {
        self.global_pulse = 1.0;
        for branch in &mut self.branches {
            if branch.has_node {
                branch.node_brightness = 1.0;
            }
        }
    }

    /// Begin contraction toward center (for final kexec animation).
    /// Call each frame with increasing `progress` from 0.0 to 1.0.
    pub fn contract(&mut self, progress: f32) {
        self.contraction = progress.clamp(0.0, 1.0);
    }

    /// Render the network to a pixel buffer (XRGB8888 format, row-major).
    /// The buffer must be `width * height` u32 values.
    pub fn render(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        assert!(buffer.len() >= w * h, "buffer too small");

        // Configurable canvas-to-substrate fade and convergence glow. The
        // legacy defaults preserve the established original palette.
        let bg = if self.elapsed < 1.0 {
            Rgba::lerp(
                self.settings.palette.canvas,
                self.settings.palette.substrate,
                self.elapsed,
            )
        } else {
            self.settings.palette.substrate
        };

        let bg = if self.contraction > 0.95 {
            let flash = ((self.contraction - 0.95) / 0.05).clamp(0.0, 1.0);
            Rgba::lerp(bg, self.settings.palette.glow, flash)
        } else {
            bg
        };

        // Fade to black after flash
        let bg_val = bg.to_xrgb8888();
        for pixel in buffer.iter_mut().take(w * h) {
            *pixel = bg_val;
        }

        // Don't draw during initial blackout or after contraction flash
        if self.elapsed < 1.0 || self.contraction > 0.98 {
            // During T=0-1s, draw single teal pixel at center after 1s
            if self.elapsed >= 0.9 && self.elapsed < 1.0 {
                let cx = self.center.0 as usize;
                let cy = self.center.1 as usize;
                if cx < w && cy < h {
                    buffer[cy * w + cx] = self.settings.palette.filament.to_xrgb8888();
                }
            }
            return;
        }

        // Draw all branches
        for branch in &self.branches {
            if branch.growth_progress < 0.01 {
                continue;
            }

            let drifted_start = self.drifted_point(branch.start);
            let drifted_end = self.drifted_point(branch.end);
            let (start, end) = if self.contraction > 0.0 {
                // Contract the drifted scene toward center.
                let c = self.contraction;
                let s = (
                    drifted_start.0 + (self.center.0 - drifted_start.0) * c,
                    drifted_start.1 + (self.center.1 - drifted_start.1) * c,
                );
                let e = (
                    drifted_end.0 + (self.center.0 - drifted_end.0) * c,
                    drifted_end.1 + (self.center.1 - drifted_end.1) * c,
                );
                (s, e)
            } else {
                (drifted_start, drifted_end)
            };

            // Role-based palette mapping is shared by configured scenes.
            let color = match branch.depth {
                0 => self.settings.palette.filament,
                1..=3 => Rgba::lerp(
                    self.settings.palette.filament,
                    self.settings.palette.lichen,
                    branch.depth as f32 * 0.15,
                ),
                _ => self.settings.palette.lichen,
            };

            // Brightness boost during global pulse
            let color = if self.global_pulse > 0.0 {
                color.brighten(1.0 + self.global_pulse * 0.8)
            } else {
                color
            };

            // Apply opacity based on depth
            let opacity = branch.opacity * (1.0 - branch.depth as f32 * 0.06).max(0.3);
            let color = color.with_opacity(opacity);

            // Draw the branch line
            draw_line_thick(
                buffer,
                w,
                h,
                start.0 as i32,
                start.1 as i32,
                end.0 as i32,
                end.1 as i32,
                branch.thickness.max(1.0) as u32,
                color,
            );

            // Draw node if present
            if branch.has_node && branch.length() > MIN_BRANCH_LEN {
                let node_color = if branch.node_brightness > 0.0 {
                    Rgba::lerp(
                        self.settings.palette.filament,
                        self.settings.palette.node,
                        branch.node_brightness,
                    )
                    .brighten(1.0 + branch.node_brightness * 0.5)
                } else {
                    self.settings.palette.filament.brighten(1.2)
                };
                let radius = (branch.thickness * 1.5 + 1.0) as i32;
                draw_filled_circle(buffer, w, h, end.0 as i32, end.1 as i32, radius, node_color);
            }
        }
    }
    /// Render a tightly packed RGBA8 frame in top-to-bottom row-major order.
    ///
    /// This is the portable pixel contract used by browser and Component Model
    /// hosts. The host owns presentation and scheduling.
    pub fn render_rgba(&self) -> Vec<u8> {
        let pixel_count = (self.width as usize) * (self.height as usize);
        let mut packed = vec![0_u32; pixel_count];
        self.render(&mut packed);

        let mut rgba = Vec::with_capacity(pixel_count * 4);
        for pixel in packed {
            rgba.push(((pixel >> 16) & 0xff) as u8);
            rgba.push(((pixel >> 8) & 0xff) as u8);
            rgba.push((pixel & 0xff) as u8);
            rgba.push(0xff);
        }
        rgba
    }

}

/// Bresenham's line algorithm.
fn draw_line(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    color: Rgba,
) {
    let mut x0 = x0;
    let mut y0 = y0;
    let dx = (x1 - x0).abs();
    let dy = -(y1 - y0).abs();
    let sx = if x0 < x1 { 1 } else { -1 };
    let sy = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;

    let packed = if color.3 == 0xff {
        color.to_xrgb8888()
    } else {
        // Will need compositing
        0
    };
    let opaque = color.3 == 0xff;

    loop {
        if x0 >= 0 && (x0 as usize) < buf_w && y0 >= 0 && (y0 as usize) < buf_h {
            let idx = y0 as usize * buf_w + x0 as usize;
            if opaque {
                buffer[idx] = packed;
            } else {
                // Alpha composite over existing pixel
                let dst_val = buffer[idx];
                let dst = Rgba(
                    ((dst_val >> 16) & 0xff) as u8,
                    ((dst_val >> 8) & 0xff) as u8,
                    (dst_val & 0xff) as u8,
                    0xff,
                );
                buffer[idx] = color.over(dst).to_xrgb8888();
            }
        }

        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            if x0 == x1 {
                break;
            }
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            if y0 == y1 {
                break;
            }
            err += dx;
            y0 += sy;
        }
    }
}

/// Draw a thick line by drawing multiple parallel Bresenham lines.
fn draw_line_thick(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    thickness: u32,
    color: Rgba,
) {
    if thickness <= 1 {
        draw_line(buffer, buf_w, buf_h, x0, y0, x1, y1, color);
        return;
    }

    let half = thickness as i32 / 2;
    let dx = (x1 - x0) as f32;
    let dy = (y1 - y0) as f32;
    let len = (dx * dx + dy * dy).sqrt();
    if len < 0.001 {
        return;
    }
    // Perpendicular direction
    let nx = -dy / len;
    let ny = dx / len;

    for i in -half..=half {
        let ox = (nx * i as f32).round() as i32;
        let oy = (ny * i as f32).round() as i32;
        draw_line(
            buffer,
            buf_w,
            buf_h,
            x0 + ox,
            y0 + oy,
            x1 + ox,
            y1 + oy,
            color,
        );
    }
}

/// Draw a filled circle using the midpoint circle algorithm.
fn draw_filled_circle(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    cx: i32,
    cy: i32,
    radius: i32,
    color: Rgba,
) {
    let packed = color.to_xrgb8888();
    for dy in -radius..=radius {
        let y = cy + dy;
        if y < 0 || y as usize >= buf_h {
            continue;
        }
        let half_w = ((radius * radius - dy * dy) as f32).sqrt() as i32;
        let x_start = (cx - half_w).max(0) as usize;
        let x_end = ((cx + half_w) as usize).min(buf_w - 1);
        let row = y as usize * buf_w;
        for x in x_start..=x_end {
            buffer[row + x] = packed;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::color::{LEAF_GREEN, SOLAR_GOLD};

    #[test]
    fn test_network_creation() {
        let net = MycelialNetwork::new(800, 600, "test genesis phrase");
        assert!(!net.branches.is_empty());
        assert!(net.branches.len() <= 9); // 4-8 initial branches
    }

    #[test]
    fn test_deterministic_seeding() {
        let a = MycelialNetwork::new(800, 600, "sovereign quickening");
        let b = MycelialNetwork::new(800, 600, "sovereign quickening");
        assert_eq!(a.branches.len(), b.branches.len());
        for (ba, bb) in a.branches.iter().zip(b.branches.iter()) {
            assert_eq!(ba.angle, bb.angle);
        }
    }

    #[test]
    fn test_different_phrases_differ() {
        let a = MycelialNetwork::new(800, 600, "phrase one");
        let b = MycelialNetwork::new(800, 600, "phrase two");
        // Angles should differ
        assert_ne!(a.branches[0].angle, b.branches[0].angle);
    }

    #[test]
    fn test_growth() {
        let mut net = MycelialNetwork::new(800, 600, "grow test");
        let initial = net.branches.len();
        for _ in 0..100 {
            net.grow(0.1, 1.0);
        }
        assert!(net.branches.len() > initial);
    }

    #[test]
    fn test_max_branches_cap() {
        let mut net = MycelialNetwork::new(200, 200, "cap test");
        for _ in 0..2000 {
            net.grow(0.05, 1.0);
        }
        assert!(net.branches.len() <= net.settings.branch_limit as usize);
    }

    #[test]
    fn test_render_no_panic() {
        let mut net = MycelialNetwork::new(100, 80, "render test");
        net.grow(0.5, 1.0);
        net.elapsed = 2.0;
        let mut buf = vec![0u32; 100 * 80];
        net.render(&mut buf);
    }

    #[test]
    fn test_pulse_sets_brightness() {
        let mut net = MycelialNetwork::new(100, 100, "pulse test");
        for _ in 0..50 {
            net.grow(0.1, 1.0);
        }
        net.pulse();
        assert_eq!(net.global_pulse, 1.0);
        let any_bright = net.branches.iter().any(|b| b.node_brightness > 0.0);
        // Only branches with nodes should be bright
        let any_node = net.branches.iter().any(|b| b.has_node);
        assert_eq!(any_bright, any_node);
    }

    #[test]
    fn test_contraction() {
        let mut net = MycelialNetwork::new(100, 100, "contract test");
        net.contract(0.5);
        assert!((net.contraction - 0.5).abs() < f32::EPSILON);
        net.contract(1.5); // clamp
        assert!((net.contraction - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn test_bresenham_horizontal() {
        let mut buf = vec![0u32; 10 * 10];
        draw_line(&mut buf, 10, 10, 2, 5, 7, 5, LEAF_GREEN);
        // Pixels from x=2 to x=7 on row 5 should be set
        for x in 2..=7 {
            assert_ne!(buf[5 * 10 + x], 0);
        }
    }

    #[test]
    fn test_bresenham_out_of_bounds() {
        let mut buf = vec![0u32; 10 * 10];
        // Should not panic even with coords outside the buffer
        draw_line(&mut buf, 10, 10, -5, -5, 15, 15, LEAF_GREEN);
    }

    #[test]
    fn test_filled_circle() {
        let mut buf = vec![0u32; 20 * 20];
        draw_filled_circle(&mut buf, 20, 20, 10, 10, 3, SOLAR_GOLD);
        // Center pixel should be set
        assert_ne!(buf[10 * 20 + 10], 0);
    }

    #[test]
    fn rgba_frame_is_tightly_packed_and_opaque() {
        let network = MycelialNetwork::new(8, 4, "rgba fixture");
        let frame = network.render_rgba();
        assert_eq!(frame.len(), 8 * 4 * 4);
        assert!(frame.chunks_exact(4).all(|pixel| pixel[3] == 0xff));
    }

    #[test]
    fn same_seed_produces_identical_frame_across_runs() {
        let mut a = MycelialNetwork::new(80, 60, "portable visual fixture");
        let mut b = MycelialNetwork::new(80, 60, "portable visual fixture");
        for _ in 0..60 {
            a.grow(1.0 / 30.0, 0.7);
            b.grow(1.0 / 30.0, 0.7);
        }
        let mut frame_a = vec![0_u32; 80 * 60];
        let mut frame_b = vec![0_u32; 80 * 60];
        a.render(&mut frame_a);
        b.render(&mut frame_b);
        assert_eq!(frame_a, frame_b);
    }

    #[test]
    fn numeric_seed_settings_replay_identically_and_obey_branch_budget() {
        let mut settings = SceneSettings::default();
        settings.branch_limit = 1;
        settings.max_depth = 1;
        settings.growth_rate = 0.28;
        settings.fixed_step_hz = 30;

        let mut a = MycelialNetwork::with_settings(80, 60, 20261010, settings).unwrap();
        let mut b = MycelialNetwork::with_settings(80, 60, 20261010, settings).unwrap();
        assert_eq!(a.branches.len(), 1);
        assert_eq!(b.branches.len(), 1);

        for _ in 0..10 {
            a.advance_ticks(30, 0.7).unwrap();
            b.advance_ticks(30, 0.7).unwrap();
        }
        assert_eq!(a.render_rgba(), b.render_rgba());
        assert!(a.branches.len() <= 1);
        assert!(a.branches.iter().all(|branch| branch.depth <= 1));
    }

    #[test]
    fn configured_palette_controls_initial_and_settled_background() {
        let mut settings = SceneSettings::default();
        settings.palette.canvas = Rgba(1, 2, 3, 255);
        settings.palette.substrate = Rgba(4, 5, 6, 255);
        let mut net = MycelialNetwork::with_settings(32, 24, 7, settings).unwrap();

        assert_eq!(&net.render_rgba()[0..4], &[1, 2, 3, 255]);
        net.elapsed = 1.0;
        assert_eq!(&net.render_rgba()[0..4], &[4, 5, 6, 255]);
    }

    #[test]
    fn fixed_tick_pulse_schedule_uses_integer_ticks() {
        let mut settings = SceneSettings::default();
        settings.fixed_step_hz = 10;
        settings.pulse_period_seconds = 0.2;
        let mut net = MycelialNetwork::with_settings(32, 24, 42, settings).unwrap();

        net.advance_tick(0.0).unwrap();
        assert_eq!(net.global_pulse, 0.0);
        net.advance_tick(0.0).unwrap();
        assert_eq!(net.global_pulse, 1.0);
    }

    #[test]
    fn drift_amplitude_changes_configured_render_only() {
        let mut still_settings = SceneSettings::default();
        still_settings.drift_amplitude = 0.0;
        let mut drift_settings = still_settings;
        drift_settings.drift_amplitude = 0.8;

        let mut still = MycelialNetwork::with_settings(80, 60, 99, still_settings).unwrap();
        let mut drift = MycelialNetwork::with_settings(80, 60, 99, drift_settings).unwrap();
        for _ in 0..5 {
            still.advance_ticks(30, 0.7).unwrap();
            drift.advance_ticks(30, 0.7).unwrap();
        }

        assert_ne!(still.render_rgba(), drift.render_rgba());
    }

    #[test]
    fn legacy_variable_delta_api_preserves_minimum_crawl() {
        let mut net = MycelialNetwork::new(32, 24, "legacy-minimum-crawl");
        let initial = net.branches[0].growth_progress;
        net.grow(1.0 / 30.0, 0.0);
        assert!(net.branches[0].growth_progress > initial);
    }

    #[test]
    fn configured_tick_rejects_invalid_activity_and_unbounded_batches() {
        let mut net = MycelialNetwork::with_settings(
            32,
            24,
            1,
            SceneSettings::default(),
        )
        .unwrap();
        assert_eq!(
            net.advance_tick(f32::NAN),
            Err(SceneSettingsError::InvalidActivity)
        );
        assert_eq!(
            net.advance_tick(1.1),
            Err(SceneSettingsError::InvalidActivity)
        );
        assert_eq!(
            net.advance_ticks(31, 0.7),
            Err(SceneSettingsError::TickBatchOutOfRange)
        );

        // The effective batch ceiling follows the configured tick rate, not
        // the absolute 120-tick safety ceiling.
        let mut low_rate = SceneSettings::default();
        low_rate.fixed_step_hz = 10;
        let mut low_rate_scene =
            MycelialNetwork::with_settings(32, 24, 2, low_rate).unwrap();
        assert_eq!(
            low_rate_scene.advance_ticks(11, 0.7),
            Err(SceneSettingsError::TickBatchOutOfRange)
        );
        low_rate_scene.advance_ticks(10, 0.7).unwrap();

        let initial_progress = net.branches[0].growth_progress;
        net.advance_ticks(30, 0.0).unwrap();
        assert_eq!(net.branches[0].growth_progress, initial_progress);
    }
}
