// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root
/// L-system mycelial growth renderer.
///
/// Generates deterministic procedural mycelial growth from a public visual seed.
/// Renders to a raw pixel buffer using Bresenham's line algorithm — no GPU required.
use crate::color::Rgba;
use crate::theme::{GrowthProfile, Palette, SceneKind, Theme, ThemeDensity, ThemeMotion};
use std::collections::HashMap;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

/// Minimum branch length in pixels before a branch can spawn children.
const MIN_BRANCH_LEN: f32 = 4.0;

/// Maximum number of branches to prevent runaway growth.
pub const MAX_BRANCHES: usize = 8192;

/// Maximum depth of branching recursion.
const MAX_DEPTH: u32 = 12;

/// Start time for the bounded scene-entry crossfade, in simulation seconds.
const SCENE_ENTRY_FADE_START: f32 = 1.0;
/// Default scene-entry crossfade used by the compatibility constructor, in seconds.
const DEFAULT_SCENE_ENTRY_FADE_DURATION: f32 = 0.4;
/// Minimum allowed configured scene-entry crossfade, in seconds.
const MIN_SCENE_ENTRY_FADE_DURATION: f32 = 0.05;
/// Maximum allowed configured scene-entry crossfade, in seconds.
const MAX_SCENE_ENTRY_FADE_DURATION: f32 = 2.0;

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
    /// Each completed node considers child creation once, avoiding repeated graph scans.
    pub children_spawned: bool,
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
            children_spawned: false,
        }
    }

    fn length(&self) -> f32 {
        let dx = self.end.0 - self.start.0;
        let dy = self.end.1 - self.start.1;
        (dx * dx + dy * dy).sqrt()
    }

    /// Target length based on depth and the theme's bounded scale multiplier.
    fn target_length(&self, scale: f32) -> f32 {
        (80.0 * scale) / (1.0 + self.depth as f32 * 0.5)
    }
}

#[derive(Debug, Clone, Copy)]
struct Star {
    position: (f32, f32),
    radius: i32,
    phase: f32,
}

/// The full procedural scene state used by the CPU reference renderer.
pub struct MycelialNetwork {
    pub branches: Vec<Branch>,
    pub width: u32,
    pub height: u32,
    rng: StdRng,
    center: (f32, f32),
    theme: Theme,
    stars: Vec<Star>,
    /// Motion-scaled simulation time in seconds (fractional).
    pub elapsed: f32,
    /// Unscaled presentation time; boot intro and scene-entry handoff must not
    /// stretch merely because the selected motion mode slows scene simulation.
    presentation_elapsed: f32,
    /// All nodes pulsing simultaneously (completion events).
    pub global_pulse: f32,
    /// Contraction progress (0.0 = normal, 1.0 = fully contracted to center).
    pub contraction: f32,
    /// Continuous fade from the current scene to black (0.0 = visible, 1.0 = black).
    pub exit_fade: f32,
    /// Per-mode scene-entry crossfade; configured from the visual manifest.
    scene_entry_fade_duration: f32,
    /// Theme palette used for every rendered pixel.
    pub palette: Palette,
    /// Theme-specific but bounded growth and pulse controls.
    pub growth_profile: GrowthProfile,
    /// Motion multiplier shared by growth, scene time and accessibility modes.
    pub motion_scale: f32,
    /// Animated star/constellation work budget in [0, 1].
    pub particle_budget_scale: f32,
    /// Compiled per-theme density caps.
    density: ThemeDensity,
    /// Per-theme growth and pulse intensity, compiled from the manifest.
    theme_motion: ThemeMotion,
}

impl MycelialNetwork {
    /// Create a network using the original Symthaea visual palette.
    pub fn new(width: u32, height: u32, visual_seed: &str) -> Self {
        Self::new_with_theme(width, height, visual_seed, Theme::MycelialGenesis)
    }

    /// Create a deterministic network with a selected visual theme.
    pub fn new_with_theme(width: u32, height: u32, visual_seed: &str, theme: Theme) -> Self {
        let hash = blake3::hash(visual_seed.as_bytes());
        let seed_bytes: [u8; 32] = *hash.as_bytes();
        let rng = StdRng::from_seed(seed_bytes);
        let center = (width as f32 / 2.0, height as f32 / 2.0);
        let palette = theme.palette();
        let growth_profile = theme.growth_profile();
        let density = theme.density();
        let theme_motion = theme.motion_tuning();

        // Stars use a separate deterministic stream so changing scene geometry
        // never perturbs the growth network's random sequence.
        let mut star_seed_input = visual_seed.as_bytes().to_vec();
        star_seed_input.extend_from_slice(b"/visual-genome/cosmic-cartography-v1");
        let star_seed = *blake3::hash(&star_seed_input).as_bytes();
        let mut star_rng = StdRng::from_seed(star_seed);
        let area = (width.max(1) as u64).saturating_mul(height.max(1) as u64);
        let star_count = if theme.scene_kind() == SceneKind::CosmicCartography {
            ((area / 25_000).clamp(32, 768) as usize).min(density.max_particles)
        } else {
            0
        };
        let safe_width = width.max(1) as f32;
        let safe_height = height.max(1) as f32;
        let stars = (0..star_count)
            .map(|_| Star {
                position: (
                    star_rng.gen_range(0.0..safe_width),
                    star_rng.gen_range(0.0..safe_height),
                ),
                radius: if star_rng.gen_bool(0.12) { 2 } else { 1 },
                phase: star_rng.gen_range(0.0..std::f32::consts::TAU),
            })
            .collect();

        let mut net = Self {
            branches: Vec::with_capacity(512),
            width,
            height,
            rng,
            center,
            theme,
            stars,
            elapsed: 0.0,
            presentation_elapsed: 0.0,
            global_pulse: 0.0,
            contraction: 0.0,
            exit_fade: 0.0,
            scene_entry_fade_duration: DEFAULT_SCENE_ENTRY_FADE_DURATION,
            palette,
            growth_profile,
            motion_scale: 1.0,
            particle_budget_scale: 1.0,
            density,
            theme_motion,
        };

        // Seed a bounded theme-specific number of branches around the center.
        let initial_count = growth_profile.initial_branch_min as usize
            + (seed_bytes[0] % growth_profile.initial_branch_variants) as usize;
        let angle_step = std::f32::consts::TAU / initial_count as f32;
        for i in 0..initial_count {
            let angle = angle_step * i as f32 + (seed_bytes[1] as f32 / 255.0) * 0.5;
            net.branches.push(Branch::new(center, angle, 2.5, 0));
        }

        net
    }

    /// Advance the simulation by `dt` seconds.
    /// `io_rate` controls growth speed (0.0 = dormant, 1.0 = maximum growth).
    /// Clamp the animation multiplier. Non-finite values fail to static mode.
    pub fn set_motion_scale(&mut self, scale: f32) {
        self.motion_scale = if scale.is_finite() {
            scale.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Scale cosmic star and constellation rendering work for animated modes.
    ///
    /// When motion is fully reduced, the renderer preserves the complete deterministic
    /// still composition; this budget governs animation workload, not theme identity.
    pub fn set_particle_budget_scale(&mut self, scale: f32) {
        self.particle_budget_scale = if scale.is_finite() {
            scale.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    fn rendered_star_count(&self) -> usize {
        let available = self.stars.len().min(self.density.max_particles);
        if self.motion_scale <= 0.0 {
            available
        } else {
            ((available as f32 * self.particle_budget_scale).ceil() as usize)
                .min(available)
        }
    }

    /// Configure the entry crossfade duration in seconds.
    ///
    /// Non-finite values restore the compatibility default; finite values are
    /// clamped to a deliberately bounded 50 ms–2 s interval.
    pub fn set_scene_entry_fade_duration(&mut self, duration_seconds: f32) {
        self.scene_entry_fade_duration = if duration_seconds.is_finite() {
            duration_seconds.clamp(
                MIN_SCENE_ENTRY_FADE_DURATION,
                MAX_SCENE_ENTRY_FADE_DURATION,
            )
        } else {
            DEFAULT_SCENE_ENTRY_FADE_DURATION
        };
    }

    /// Advance the bounded simulation. Reduced motion performs no state updates.
    pub fn grow(&mut self, dt: f32, io_rate: f32) {
        if !dt.is_finite() || dt <= 0.0 || !self.motion_scale.is_finite() || self.motion_scale <= 0.0 {
            return;
        }
        // Bound recovery from long scheduling stalls; animation must not jump by
        // several seconds after a pause or a delayed framebuffer operation.
        let presentation_dt = dt.clamp(0.0, 0.25);
        self.presentation_elapsed += presentation_dt;
        // The intro and scene handoff follow presentation time; scene evolution
        // and growth still respect the selected motion scale.
        let dt = presentation_dt * self.motion_scale.clamp(0.0, 1.0);
        if dt <= 0.0 {
            return;
        }
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

        let growth_speed = self.growth_profile.growth_speed
            * self.theme_motion.growth
            * io_rate.max(0.05);

        // Grow existing branches and detect the one-time child decisions in
        // the same pass. This avoids a second full-vector scan at every frame.
        let mut new_branches: Vec<Branch> = Vec::new();
        let mut has_pending_spawns = false;
        for branch in &mut self.branches {
            if !branch.growing {
                if branch.has_node && !branch.children_spawned {
                    has_pending_spawns = true;
                }
                continue;
            }

            let target = branch.target_length(self.growth_profile.branch_length_scale);
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
            if !branch.growing && branch.has_node && !branch.children_spawned {
                has_pending_spawns = true;
            }
        }

        // Index existing branch starts once per growth step. The original
        // renderer suppressed new forks when another branch already started at
        // an endpoint (within 1 px). Preserve that graph invariant without a
        // repeated full-list scan or one heap allocation per occupied cell.
        let total_branches = self.branches.len();
        if !has_pending_spawns {
            return;
        }

        let mut start_index = StartSpatialIndex::new(total_branches.saturating_add(64));
        for branch in &self.branches {
            start_index.insert(branch.start);
        }

        for i in 0..total_branches {
            if self.branches[i].growing
                || !self.branches[i].has_node
                || self.branches[i].children_spawned
            {
                continue;
            }

            if self.branches[i].depth >= MAX_DEPTH
                || self.branches[i].length() < MIN_BRANCH_LEN
            {
                self.branches[i].children_spawned = true;
                continue;
            }

            if total_branches + new_branches.len() >= self.density.max_branches.min(MAX_BRANCHES) {
                // No branch can ever be added after the cap is reached. Close all
                // pending spawn decisions so a saturated network stays allocation-free.
                for branch in &mut self.branches {
                    if !branch.growing && branch.has_node && !branch.children_spawned {
                        branch.children_spawned = true;
                    }
                }
                break;
            }

            let end = self.branches[i].end;
            if start_index.has_nearby(end) {
                // This node already intersects an existing branch start. Mark
                // the decision once so it is not reconsidered on every frame.
                self.branches[i].children_spawned = true;
                continue;
            }

            let depth = self.branches[i].depth;
            let angle = self.branches[i].angle;
            let thickness = self.branches[i].thickness;
            self.branches[i].children_spawned = true;

            // Theme-specific branching is bounded; the global cap is checked
            // before every append regardless of profile.
            let child_variants = (self.growth_profile.max_children
                - self.growth_profile.min_children
                + 1) as u32;
            let n_children = self.growth_profile.min_children as u32
                + self.rng.gen_range(0u32..child_variants);
            for _ in 0..n_children {
                if total_branches + new_branches.len() >= self.density.max_branches.min(MAX_BRANCHES) {
                    break;
                }
                let fork_angle = self
                    .rng
                    .gen_range(
                        self.growth_profile.min_fork_angle_degrees
                            ..self.growth_profile.max_fork_angle_degrees,
                    )
                    .to_radians();
                let sign = if self.rng.gen_bool(0.5) { 1.0 } else { -1.0 };
                let child_angle = angle + fork_angle * sign;
                let child_thickness = (thickness * 0.75).max(0.5);
                new_branches.push(Branch::new(end, child_angle, child_thickness, depth + 1));
                start_index.insert(end);
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

    /// Resolve the visual clock separately from the simulation clock.
    ///
    /// Reduced motion freezes simulation state, but must still produce the fully
    /// composed still image rather than freezing on the boot intro's black frame.
    /// Time-driven decorative effects use a fixed terminal phase in Reduced mode.
    fn visual_time(&self) -> f32 {
        if self.motion_scale <= 0.0 || !self.elapsed.is_finite() {
            SCENE_ENTRY_FADE_START + self.scene_entry_fade_duration
        } else {
            self.elapsed
        }
    }

    /// Resolve presentation time independently from motion-scaled simulation time.
    ///
    /// Low Power may slow scene evolution, but must not stretch the boot intro
    /// or delay scene-entry handoff. Preserve compatibility with offline fixtures
    /// that seed elapsed directly by taking the later finite clock.
    fn presentation_time(&self) -> f32 {
        if self.motion_scale <= 0.0
            || !self.motion_scale.is_finite()
            || !self.elapsed.is_finite()
            || !self.presentation_elapsed.is_finite()
        {
            SCENE_ENTRY_FADE_START + self.scene_entry_fade_duration
        } else {
            self.presentation_elapsed.max(self.elapsed)
        }
    }

    /// Render the network to a pixel buffer (XRGB8888 format, row-major).
    /// The buffer must be `width * height` u32 values.
    pub fn render(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        assert!(buffer.len() >= w * h, "buffer too small");

        // Motion-scaled simulation and presentation time are independent.
        // Reduced mode uses the terminal phase to show a complete still frame.
        let presentation_time = self.presentation_time();
        let bg = if presentation_time < 1.0 {
            Rgba::lerp(crate::color::BLACK, self.palette.background, presentation_time)
        } else {
            self.palette.background
        };

        for pixel in buffer.iter_mut().take(w * h) {
            *pixel = bg.to_xrgb8888();
        }

        // Keep the first presentation second calm for animated modes;
        // Reduced skips directly to the final still composition on its first frame.
        if presentation_time < 1.0 {
            if presentation_time >= 0.9 {
                let cx = self.center.0 as usize;
                let cy = self.center.1 as usize;
                if cx < w && cy < h {
                    buffer[cy * w + cx] = self.palette.primary.to_xrgb8888();
                }
            }
            self.apply_exit_fade(buffer);
            return;
        }

        // Distinct, bounded scene families share the same pixel format and
        // transition contract. Boot remains CPU-only and has no asset/network input.
        if w == 0 || h == 0 {
            return;
        }

        // Reduced mode renders a fixed composition. It does not merely label a
        // moving scene as reduced: all simulation clocks and pulsing stop.
        if self.motion_scale <= 0.0 {
            match self.theme.scene_kind() {
                SceneKind::MycelialNetwork => self.render_static_mycelial(buffer),
                _ => match self.theme.scene_kind() {
                    SceneKind::SolarCanopy => self.render_solar_canopy(buffer),
                    SceneKind::CosmicCartography => self.render_cosmic_cartography(buffer),
                    SceneKind::OceanWaves => self.render_ocean_waves(buffer),
                    SceneKind::EpistemicLattice => self.render_epistemic_lattice(buffer),
                    SceneKind::QuantumInterference => self.render_quantum_interference(buffer),
                    SceneKind::AuroraRibbons => self.render_aurora_ribbons(buffer),
                    SceneKind::GraphiteMinimal => self.render_graphite_minimal(buffer),
                    SceneKind::VerdantRecovery => self.render_verdant_recovery(buffer),
                    SceneKind::CivicCommons => self.render_civic_commons(buffer),
                    SceneKind::MycelialNetwork => self.render_static_mycelial(buffer),
                },
            }
            self.apply_scene_entry_fade(buffer);
            self.apply_exit_fade(buffer);
            return;
        }

        match self.theme.scene_kind() {
            SceneKind::MycelialNetwork => {}
            SceneKind::SolarCanopy => {
                self.render_solar_canopy(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::CosmicCartography => {
                self.render_cosmic_cartography(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::OceanWaves => {
                self.render_ocean_waves(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::EpistemicLattice => {
                self.render_epistemic_lattice(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::QuantumInterference => {
                self.render_quantum_interference(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::AuroraRibbons => {
                self.render_aurora_ribbons(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::GraphiteMinimal => {
                self.render_graphite_minimal(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::VerdantRecovery => {
                self.render_verdant_recovery(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
            SceneKind::CivicCommons => {
                self.render_civic_commons(buffer);
                self.apply_scene_entry_fade(buffer);
                self.apply_exit_fade(buffer);
                return;
            }
        }

        // Draw all branches
        for branch in &self.branches {
            if branch.growth_progress < 0.01 {
                continue;
            }

            let (start, end) = if self.contraction > 0.0 {
                // Contract toward center
                let c = self.contraction;
                let s = (
                    branch.start.0 + (self.center.0 - branch.start.0) * c,
                    branch.start.1 + (self.center.1 - branch.start.1) * c,
                );
                let e = (
                    branch.end.0 + (self.center.0 - branch.end.0) * c,
                    branch.end.1 + (self.center.1 - branch.end.1) * c,
                );
                (s, e)
            } else {
                (branch.start, branch.end)
            };

            // Depth gives the living network visual hierarchy. As the sequence
            // converges, it blends toward the selected theme's convergence color.
            let base_color = match branch.depth {
                0 => self.palette.primary,
                1..=3 => Rgba::lerp(
                    self.palette.primary,
                    self.palette.secondary,
                    branch.depth as f32 * 0.15,
                ),
                _ => self.palette.secondary,
            };
            let convergence = ((self.contraction - 0.72) / 0.28).clamp(0.0, 1.0);
            let color = Rgba::lerp(base_color, self.palette.convergence, convergence);

            // Brightness boost during global pulse
            let color = if self.global_pulse > 0.0 {
                color.brighten(1.0 + self.global_pulse * self.growth_profile.pulse_gain * self.theme_motion.pulse)
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
                    Rgba::lerp(self.palette.primary, self.palette.node, branch.node_brightness)
                        .brighten(1.0 + branch.node_brightness * 0.5)
                } else {
                    self.palette.primary.brighten(1.2)
                };
                let radius = (branch.thickness * 1.5 + 1.0) as i32;
                draw_filled_circle(buffer, w, h, end.0 as i32, end.1 as i32, radius, node_color);
            }
        }

        self.apply_scene_entry_fade(buffer);
        self.apply_exit_fade(buffer);
    }

    /// Render a deterministic still mycelial network for reduced-motion mode.
    fn render_static_mycelial(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        if w == 0 || h == 0 {
            return;
        }

        let radius = (w.min(h) as f32 * 0.36).max(6.0);
        for (index, root) in self.branches.iter().enumerate() {
            let angle = root.angle;
            let trunk_end = (
                self.center.0 + angle.cos() * radius,
                self.center.1 + angle.sin() * radius,
            );
            draw_line_thick(
                buffer, w, h,
                self.center.0.round() as i32, self.center.1.round() as i32,
                trunk_end.0.round() as i32, trunk_end.1.round() as i32,
                2,
                self.palette.primary.with_opacity(0.82),
            );
            draw_filled_circle(
                buffer, w, h,
                trunk_end.0.round() as i32, trunk_end.1.round() as i32,
                2, self.palette.node.with_opacity(0.85),
            );

            // Three pairs of small side branches give the still frame organic
            // structure without any frame-to-frame motion or random updates.
            for level in 0..3 {
                let along = 0.34 + level as f32 * 0.2;
                let origin = (
                    self.center.0 + angle.cos() * radius * along,
                    self.center.1 + angle.sin() * radius * along,
                );
                let length = radius * (0.22 - level as f32 * 0.045);
                for side in [-1.0_f32, 1.0_f32] {
                    let fork = angle + side * (0.58 + level as f32 * 0.12);
                    let end = (
                        origin.0 + fork.cos() * length,
                        origin.1 + fork.sin() * length,
                    );
                    let color = if (index + level) % 3 == 0 {
                        self.palette.secondary
                    } else {
                        self.palette.primary
                    };
                    draw_line_thick(
                        buffer, w, h,
                        origin.0.round() as i32, origin.1.round() as i32,
                        end.0.round() as i32, end.1.round() as i32,
                        1,
                        color.with_opacity(0.58),
                    );
                    draw_filled_circle(
                        buffer, w, h,
                        end.0.round() as i32, end.1.round() as i32,
                        1, self.palette.node.with_opacity(0.7),
                    );
                }
            }
        }
    }

    /// Render a solar horizon with warm rays and concentric canopy arcs.
    fn render_solar_canopy(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let width = w as f32;
        let height = h as f32;
        let sun = (width * 0.5, height * 0.72);
        let radius = (height * 0.11).max(4.0);
        let motion = (self.growth_profile.growth_speed / 30.0).clamp(0.0, 1.0);
        let pulse = 0.5 + 0.5 * (self.visual_time() * 0.08 * motion).sin();

        for ray in 0..33 {
            let angle = std::f32::consts::PI
                + std::f32::consts::PI * ray as f32 / 32.0;
            let reach = width.max(height) * 0.9;
            let end = (sun.0 + angle.cos() * reach, sun.1 + angle.sin() * reach);
            let color = Rgba::lerp(
                self.palette.secondary.with_opacity(0.08),
                self.palette.node.with_opacity(0.22),
                pulse,
            );
            draw_line(
                buffer, w, h,
                sun.0.round() as i32, sun.1.round() as i32,
                end.0.round() as i32, end.1.round() as i32,
                color,
            );
        }

        for ring in 0..4 {
            let r = radius * (1.55 + ring as f32 * 0.62);
            draw_circle_outline(
                buffer, w, h,
                sun.0.round() as i32, sun.1.round() as i32,
                r.round() as i32, 96,
                self.palette.primary.with_opacity(0.1 + (3 - ring) as f32 * 0.035),
            );
        }

        draw_filled_circle(
            buffer, w, h,
            sun.0.round() as i32, sun.1.round() as i32,
            radius.round() as i32,
            Rgba::lerp(self.palette.primary, self.palette.node, pulse * 0.55),
        );
    }

    /// Render an epistemic lattice: sparse relationships, provenance paths, and evidence nodes.
    fn render_epistemic_lattice(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let spacing = (w.min(h) / 7).clamp(18, 96) as i32;
        let rows = (h as i32 / spacing) + 2;
        let cols = (w as i32 / spacing) + 2;
        let contraction = ((self.contraction - 0.68) / 0.32).clamp(0.0, 1.0);
        // Let the evidence lattice breathe slowly in animated modes. Reduced
        // motion fixes the phase so time-only changes cannot alter still output.
        let temporal_breathe = if self.motion_scale <= 0.0 {
            0.0
        } else {
            (self.elapsed * self.theme_motion.pulse * 2.5).sin() * 0.07
        };

        let point = |row: i32, col: i32| {
            let stagger = if row.rem_euclid(2) == 0 { 0.0 } else { spacing as f32 * 0.5 };
            let x = col as f32 * spacing as f32 + stagger - spacing as f32 * 0.5;
            let y = row as f32 * spacing as f32 - spacing as f32 * 0.5;
            (
                x + (self.center.0 - x) * contraction,
                y + (self.center.1 - y) * contraction,
            )
        };

        for row in 0..rows {
            for col in 0..cols {
                let from = point(row, col);
                for (dr, dc) in [(0, 1), (1, 0), (1, -1)] {
                    let to_row = row + dr;
                    let to_col = col + dc;
                    if to_row >= rows || to_col < 0 || to_col >= cols {
                        continue;
                    }
                    let to = point(to_row, to_col);
                    let is_provenance_path = (row + col).rem_euclid(5) == 0;
                    let alpha = (if is_provenance_path { 0.25 } else { 0.09 })
                        + temporal_breathe;
                    let edge = Rgba::lerp(
                        self.palette.secondary.with_opacity(alpha),
                        self.palette.primary.with_opacity(alpha + 0.08),
                        (self.global_pulse * self.theme_motion.pulse).clamp(0.0, 1.0),
                    );
                    draw_line(
                        buffer, w, h,
                        from.0.round() as i32, from.1.round() as i32,
                        to.0.round() as i32, to.1.round() as i32,
                        edge,
                    );
                }

                if (row * 3 + col * 5).rem_euclid(4) == 0 {
                    draw_filled_circle(
                        buffer, w, h,
                        from.0.round() as i32, from.1.round() as i32,
                        if (row + col).rem_euclid(7) == 0 { 3 } else { 1 },
                        self.palette.node.with_opacity(0.72),
                    );
                }
            }
        }
    }

    /// Render crossing Lissajous bands as an abstract interference garden.
    /// This is a visual metaphor, not a simulation of quantum mechanics.
    fn render_quantum_interference(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let width = w as f32;
        let height = h as f32;
        let step = (w / 640).max(1) as i32;
        let motion = (self.growth_profile.growth_speed / 30.0).clamp(0.0, 1.0);
        let phase = self.visual_time() * 0.11 * motion;

        for family in 0..3 {
            let family_f = family as f32;
            let mut previous: Option<(f32, f32)> = None;
            let mut x = 0i32;
            while x <= w as i32 {
                let u = x as f32 / width;
                let carrier = (u * std::f32::consts::TAU * (1.2 + family_f * 0.33) + phase).sin();
                let envelope = (u * std::f32::consts::TAU * (0.32 + family_f * 0.11) - phase * 0.7).cos();
                let y = height * (0.5 + 0.27 * carrier * envelope);
                let x_warp = x as f32 + width * 0.06 * (u * std::f32::consts::TAU * 1.5 - phase).sin();
                let point = (
                    x_warp + (self.center.0 - x_warp) * self.contraction,
                    y + (self.center.1 - y) * self.contraction,
                );
                if let Some((px, py)) = previous {
                    let base = if family == 1 { self.palette.node } else { self.palette.primary };
                    draw_line_thick(
                        buffer, w, h,
                        px.round() as i32, py.round() as i32,
                        point.0.round() as i32, point.1.round() as i32,
                        if family == 1 { 2 } else { 1 },
                        base.with_opacity(if family == 1 { 0.23 } else { 0.17 }),
                    );
                }
                previous = Some(point);
                x += step;
            }
        }
    }

    /// Minimal drafting grid with a few precise marks; designed for focus and low motion.
    fn render_graphite_minimal(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let width = w as i32;
        let height = h as i32;
        let inset_x = (width / 8).max(1);
        let inset_y = (height / 8).max(1);
        let grid = self.palette.secondary.with_opacity(0.12);
        // Graphite's signature is a very slow breathing accent, not a moving
        // grid. Reduced mode fixes this opacity so identical inputs remain
        // pixel-identical even if an external caller advances elapsed time.
        let accent = if self.motion_scale <= 0.0 {
            self.palette.primary.with_opacity(0.35)
        } else {
            let pulse = (self.elapsed * self.theme_motion.pulse * 2.0).sin();
            self.palette.primary.with_opacity(0.42 + 0.20 * pulse)
        };

        for division in 1..8 {
            let x = inset_x + (width - 2 * inset_x) * division / 8;
            let y = inset_y + (height - 2 * inset_y) * division / 8;
            draw_line(buffer, w, h, x, inset_y, x, height - inset_y, grid);
            draw_line(buffer, w, h, inset_x, y, width - inset_x, y, grid);
        }

        let center = (self.center.0.round() as i32, self.center.1.round() as i32);
        let ring_radius = ((w.min(h) as f32) * 0.13).round() as i32;
        draw_circle_outline(buffer, w, h, center.0, center.1, ring_radius, 96, accent);
        draw_circle_outline(
            buffer, w, h, center.0, center.1,
            (ring_radius as f32 * 1.18).round() as i32, 96,
            self.palette.secondary.with_opacity(0.16),
        );
        let mark_x = inset_x + (width - 2 * inset_x) * 3 / 5;
        let mark_y = inset_y + (height - 2 * inset_y) * 2 / 5;
        draw_line(buffer, w, h, mark_x - 7, mark_y, mark_x + 7, mark_y, accent);
        draw_line(buffer, w, h, mark_x, mark_y - 7, mark_x, mark_y + 7, accent);
        draw_filled_circle(buffer, w, h, mark_x, mark_y, 2, self.palette.node);
    }

    /// Calm, static-first recovery mark with concentric rings and no strobing.
    fn render_verdant_recovery(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let center = (self.center.0.round() as i32, self.center.1.round() as i32);
        let unit = (w.min(h) as f32 * 0.07).max(3.0);
        // The recovery mark has a very gentle opacity breath in animated modes.
        // Reduced mode freezes the breath independently of externally supplied time.
        let temporal_breathe = if self.motion_scale <= 0.0 {
            0.0
        } else {
            (self.elapsed * 3.5).sin() * 0.07
        };
        let pulse = 0.5 + 0.5 * (self.visual_time() * 0.035).sin();

        for ring in 1..=4 {
            let radius = (unit * ring as f32 * (1.0 + pulse * 0.025)).round() as i32;
            draw_circle_outline(
                buffer, w, h,
                center.0, center.1, radius, 96,
                self.palette.primary
                    .with_opacity(0.14 + (4 - ring) as f32 * 0.05 + temporal_breathe),
            );
        }

        for spoke in 0..12 {
            let angle = std::f32::consts::TAU * spoke as f32 / 12.0;
            let start = (center.0 as f32 + angle.cos() * unit * 2.0, center.1 as f32 + angle.sin() * unit * 2.0);
            let end = (center.0 as f32 + angle.cos() * unit * 2.8, center.1 as f32 + angle.sin() * unit * 2.8);
            draw_line(
                buffer, w, h,
                start.0.round() as i32, start.1.round() as i32,
                end.0.round() as i32, end.1.round() as i32,
                self.palette.secondary.with_opacity(0.32 + temporal_breathe * 0.5),
            );
        }

        draw_filled_circle(
            buffer, w, h, center.0, center.1, (unit * 0.45).round() as i32,
            Rgba::lerp(
                self.palette.primary,
                self.palette.convergence,
                (0.25 + temporal_breathe).clamp(0.05, 0.45),
            ),
        );
    }

    /// Render several connected peer clusters; there is no central authority node.
    fn render_civic_commons(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let width = w as f32;
        let height = h as f32;
        let orbit = (w.min(h) as f32 * 0.25).max(8.0);
        let cluster_count = 5usize;
        let mut centers = Vec::with_capacity(cluster_count);

        for cluster in 0..cluster_count {
            let angle = std::f32::consts::TAU * cluster as f32 / cluster_count as f32
                - std::f32::consts::FRAC_PI_2;
            // Animated modes give each community a bounded, slow drift so Low
            // Power remains visibly alive at sampled frame rates without rapid flashing.
            // Reduced mode is an exact static layout with no time-dependent displacement.
            let phase = self.elapsed * 3.0 + cluster as f32 * 0.73;
            let (drift_x, drift_y) = if self.motion_scale > 0.0 && self.elapsed.is_finite() {
                (phase.sin() * 1.25, phase.cos() * 0.5)
            } else {
                (0.0, 0.0)
            };
            centers.push((
                width * 0.5 + angle.cos() * orbit + drift_x,
                height * 0.5 + angle.sin() * orbit * 0.72 + drift_y,
            ));
        }

        // Connect neighboring communities peer-to-peer, rather than through a hub.
        for cluster in 0..cluster_count {
            let next = (cluster + 1) % cluster_count;
            draw_line(
                buffer, w, h,
                centers[cluster].0.round() as i32, centers[cluster].1.round() as i32,
                centers[next].0.round() as i32, centers[next].1.round() as i32,
                self.palette.secondary.with_opacity(0.23),
            );
        }

        for cluster in 0..cluster_count {
            let center = centers[cluster];
            let count = 7usize;
            let mut nodes = Vec::with_capacity(count);
            for node in 0..count {
                let angle = std::f32::consts::TAU * node as f32 / count as f32
                    + cluster as f32 * 0.37;
                let radius = (w.min(h) as f32 * (0.026 + 0.006 * ((node + cluster) % 3) as f32)).max(3.0);
                nodes.push((
                    center.0 + angle.cos() * radius,
                    center.1 + angle.sin() * radius,
                ));
            }

            for node in 0..count {
                let next = (node + 1) % count;
                draw_line(
                    buffer, w, h,
                    nodes[node].0.round() as i32, nodes[node].1.round() as i32,
                    nodes[next].0.round() as i32, nodes[next].1.round() as i32,
                    self.palette.primary.with_opacity(0.38),
                );
                draw_filled_circle(
                    buffer, w, h,
                    nodes[node].0.round() as i32, nodes[node].1.round() as i32,
                    if node == 0 { 3 } else { 2 },
                    {
                        let base = if (node + cluster) % 4 == 0 {
                            self.palette.node
                        } else {
                            self.palette.primary
                        };
                        // Civic Commons is spatially stable, but its peer nodes gently
                        // breathe in animated modes. The elapsed clock already incorporates
                        // the selected motion scale, so Reduced remains a true static frame.
                        let phase = 0.5 + 0.5 * (self.visual_time() * 2.0
                            + node as f32 * 0.43
                            + cluster as f32 * 0.29).sin();
                        Rgba::lerp(
                            base,
                            self.palette.convergence,
                            phase * 0.28 * self.theme_motion.pulse,
                        )
                    },
                );
            }

            // One inter-community peer link per cluster; no special center or master node.
            let peer = centers[(cluster + 2) % cluster_count];
            draw_line(
                buffer, w, h,
                center.0.round() as i32, center.1.round() as i32,
                peer.0.round() as i32, peer.1.round() as i32,
                self.palette.secondary.with_opacity(0.12),
            );
        }
    }

    /// Render deterministic star points and sparse constellation paths.
    fn render_cosmic_cartography(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        let contraction = ((self.contraction - 0.65) / 0.35).clamp(0.0, 1.0);

        // Connect only nearby neighbors: a loose atlas, never a dense wireframe.
        let visible_star_count = self.rendered_star_count();
        let visible_stars = &self.stars[..visible_star_count];
        for (index, star) in visible_stars.iter().enumerate() {
            let candidates = [index + 1, index + 3];
            for next_index in candidates {
                let Some(next) = visible_stars.get(next_index) else {
                    continue;
                };
                let dx = star.position.0 - next.position.0;
                let dy = star.position.1 - next.position.1;
                let max_distance = (self.width.min(self.height) as f32) * 0.24;
                if dx * dx + dy * dy > max_distance * max_distance {
                    continue;
                }

                let start = (
                    star.position.0 + (self.center.0 - star.position.0) * contraction,
                    star.position.1 + (self.center.1 - star.position.1) * contraction,
                );
                let end = (
                    next.position.0 + (self.center.0 - next.position.0) * contraction,
                    next.position.1 + (self.center.1 - next.position.1) * contraction,
                );
                let edge = Rgba::lerp(
                    self.palette.secondary.with_opacity(0.18),
                    self.palette.convergence.with_opacity(0.24),
                    contraction,
                );
                draw_line(
                    buffer, w, h,
                    start.0 as i32, start.1 as i32,
                    end.0 as i32, end.1 as i32,
                    edge,
                );
            }
        }

        for star in visible_stars {
            let position = (
                star.position.0 + (self.center.0 - star.position.0) * contraction,
                star.position.1 + (self.center.1 - star.position.1) * contraction,
            );
            let twinkle = 0.58 + 0.32 * ((self.visual_time() * 0.7 + star.phase).sin() * 0.5 + 0.5);
            let color = Rgba::lerp(self.palette.primary, self.palette.convergence, contraction)
                .brighten(twinkle + self.global_pulse * 0.25 * self.theme_motion.pulse);
            draw_filled_circle(
                buffer, w, h,
                position.0.round() as i32,
                position.1.round() as i32,
                star.radius,
                color,
            );
        }
    }

    /// Render low-frequency, overlapping current lines as a calm fluid field.
    fn render_ocean_waves(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        if w == 0 || h == 0 {
            return;
        }
        let width = w as f32;
        let height = h as f32;
        let step = (w / 520).max(1) as i32;
        let motion = (self.growth_profile.growth_speed / 30.0).clamp(0.0, 1.0);
        let contraction = ((self.contraction - 0.68) / 0.32).clamp(0.0, 1.0);

        for layer in 0..5 {
            let layer_f = layer as f32;
            let frequency = 0.75 + layer_f * 0.19;
            let amplitude = height * (0.016 + layer_f * 0.006);
            let baseline = height * (0.28 + layer_f * 0.105);
            let phase = self.visual_time() * (0.12 + layer_f * 0.035) * motion;
            let base_color = match layer % 3 {
                0 => self.palette.secondary,
                1 => self.palette.primary,
                _ => self.palette.node,
            };

            let mut previous: Option<(f32, f32)> = None;
            let mut x = 0i32;
            while x <= w as i32 {
                let xf = x as f32 / width;
                let wave = (xf * std::f32::consts::TAU * frequency + phase).sin()
                    + 0.38 * (xf * std::f32::consts::TAU * (frequency * 0.37) - phase * 0.63).sin();
                let y = baseline + wave * amplitude;
                let point = (
                    x as f32 + (self.center.0 - x as f32) * contraction,
                    y + (self.center.1 - y) * contraction,
                );

                if let Some((px, py)) = previous {
                    let color = Rgba::lerp(
                        base_color.with_opacity(0.28),
                        self.palette.convergence.with_opacity(0.3),
                        contraction,
                    );
                    draw_line_thick(
                        buffer, w, h,
                        px.round() as i32, py.round() as i32,
                        point.0.round() as i32, point.1.round() as i32,
                        if layer == 2 { 2 } else { 1 },
                        color,
                    );
                }
                previous = Some(point);
                x += step;
            }
        }
    }

    /// Render layered aurora ribbons with a soft, low-luminance glow.
    fn render_aurora_ribbons(&self, buffer: &mut [u32]) {
        let w = self.width as usize;
        let h = self.height as usize;
        if w == 0 || h == 0 {
            return;
        }
        let width = w as f32;
        let height = h as f32;
        let step = (w / 720).max(1) as i32;
        let motion = (self.growth_profile.growth_speed / 30.0).clamp(0.0, 1.0);
        let contraction = ((self.contraction - 0.7) / 0.3).clamp(0.0, 1.0);

        for band in 0..4 {
            let band_f = band as f32;
            let frequency = 0.42 + band_f * 0.12;
            let amplitude = height * (0.035 + band_f * 0.006);
            let baseline = height * (0.2 + band_f * 0.17);
            let phase = self.visual_time() * 0.12 * motion + band_f * 1.13;
            let base_color = if band % 2 == 0 {
                self.palette.primary
            } else {
                self.palette.secondary
            };

            let mut previous: Option<(f32, f32)> = None;
            let mut x = 0i32;
            while x <= w as i32 {
                let xf = x as f32 / width;
                let wave = (xf * std::f32::consts::TAU * frequency + phase).sin()
                    + 0.45 * (xf * std::f32::consts::TAU * (frequency * 0.43) - phase * 0.7).sin();
                let y = baseline + wave * amplitude;
                let point = (
                    x as f32 + (self.center.0 - x as f32) * contraction,
                    y + (self.center.1 - y) * contraction,
                );

                if let Some((px, py)) = previous {
                    let glow = Rgba::lerp(base_color.with_opacity(0.035), self.palette.convergence.with_opacity(0.06), contraction);
                    let mid = Rgba::lerp(base_color.with_opacity(0.09), self.palette.convergence.with_opacity(0.12), contraction);
                    let core = Rgba::lerp(base_color.with_opacity(0.2), self.palette.convergence.with_opacity(0.26), contraction);
                    draw_line_thick(buffer, w, h, px.round() as i32, (py - 1.0).round() as i32, point.0.round() as i32, (point.1 - 1.0).round() as i32, 7, glow);
                    draw_line_thick(buffer, w, h, px.round() as i32, py.round() as i32, point.0.round() as i32, point.1.round() as i32, 3, mid);
                    draw_line(buffer, w, h, px.round() as i32, py.round() as i32, point.0.round() as i32, point.1.round() as i32, core);
                }
                previous = Some(point);
                x += step;
            }
        }
    }

    /// Set the bounded final fade. NaN values are treated as no fade.
    pub fn fade_out(&mut self, progress: f32) {
        self.exit_fade = if progress.is_finite() {
            progress.clamp(0.0, 1.0)
        } else {
            0.0
        };
    }

    /// Blend newly introduced scene detail over the already-visible theme background.
    ///
    /// The one-second intro deliberately draws only the background and seed. Once
    /// the full scene is eligible, reveal it over the configured duration instead of making a hard
    /// composition cut. This handoff follows presentation time, not slowed scene simulation.
    /// Reduced mode uses a terminal presentation phase and is not faded on every frame.
    fn apply_scene_entry_fade(&self, buffer: &mut [u32]) {
        let presentation_time = self.presentation_time();
        if !presentation_time.is_finite() {
            return;
        }
        let progress = ((presentation_time - SCENE_ENTRY_FADE_START) / self.scene_entry_fade_duration)
            .clamp(0.0, 1.0);
        // Smoothstep has zero slope at both endpoints, avoiding a velocity snap
        // when the composition first appears and when the crossfade completes.
        let factor = progress * progress * (3.0 - 2.0 * progress);
        if factor >= 0.999_999 {
            return;
        }

        let background = self.palette.background.to_xrgb8888();
        let inverse = 1.0 - factor;
        let pixels = (self.width as usize).saturating_mul(self.height as usize);
        for pixel in buffer.iter_mut().take(pixels) {
            let foreground = *pixel;
            let red = ((((background >> 16) & 0xff) as f32 * inverse
                + ((foreground >> 16) & 0xff) as f32 * factor)
                .round()) as u32;
            let green = ((((background >> 8) & 0xff) as f32 * inverse
                + ((foreground >> 8) & 0xff) as f32 * factor)
                .round()) as u32;
            let blue = (((background & 0xff) as f32 * inverse
                + (foreground & 0xff) as f32 * factor)
                .round()) as u32;
            *pixel = (red << 16) | (green << 8) | blue;
        }
    }

    fn apply_exit_fade(&self, buffer: &mut [u32]) {
        let factor = 1.0 - self.exit_fade.clamp(0.0, 1.0);
        if factor >= 0.999_999 {
            return;
        }

        let pixels = (self.width as usize).saturating_mul(self.height as usize);
        for pixel in buffer.iter_mut().take(pixels) {
            let red = (((*pixel >> 16) & 0xff) as f32 * factor).round() as u32;
            let green = (((*pixel >> 8) & 0xff) as f32 * factor).round() as u32;
            let blue = ((*pixel & 0xff) as f32 * factor).round() as u32;
            *pixel = (red << 16) | (green << 8) | blue;
        }
    }
}

/// Quantize a point to a one-pixel spatial-hash cell.
fn spatial_cell(point: (f32, f32)) -> (i32, i32) {
    (point.0.floor() as i32, point.1.floor() as i32)
}

#[derive(Debug, Clone, Copy)]
struct IndexedStart {
    position: (f32, f32),
    previous_in_cell: usize,
}

/// Spatial hash with flat point storage: no per-cell heap allocations.
struct StartSpatialIndex {
    heads: HashMap<(i32, i32), usize>,
    points: Vec<IndexedStart>,
}

impl StartSpatialIndex {
    fn new(capacity: usize) -> Self {
        Self {
            heads: HashMap::with_capacity(capacity),
            points: Vec::with_capacity(capacity),
        }
    }

    fn insert(&mut self, point: (f32, f32)) {
        let cell = spatial_cell(point);
        let previous = self.heads.get(&cell).copied().unwrap_or(usize::MAX);
        let index = self.points.len();
        self.points.push(IndexedStart {
            position: point,
            previous_in_cell: previous,
        });
        self.heads.insert(cell, index);
    }

    /// Match the old <1 px endpoint rule by checking the current cell and
    /// neighbors. Lookup work follows local occupancy, not the full branch list.
    fn has_nearby(&self, point: (f32, f32)) -> bool {
        let (cell_x, cell_y) = spatial_cell(point);
        for offset_x in -1..=1 {
            for offset_y in -1..=1 {
                let key = (cell_x + offset_x, cell_y + offset_y);
                let mut current = self.heads.get(&key).copied().unwrap_or(usize::MAX);
                while current != usize::MAX {
                    let candidate = self.points[current];
                    let dx = candidate.position.0 - point.0;
                    let dy = candidate.position.1 - point.1;
                    if dx * dx + dy * dy < 1.0 {
                        return true;
                    }
                    current = candidate.previous_in_cell;
                }
            }
        }
        false
    }
}

/// Draw a bounded polyline approximation of a circle outline.
fn draw_circle_outline(
    buffer: &mut [u32],
    buf_w: usize,
    buf_h: usize,
    cx: i32,
    cy: i32,
    radius: i32,
    segments: usize,
    color: Rgba,
) {
    if radius <= 0 || segments < 3 || buf_w == 0 || buf_h == 0 {
        return;
    }
    let segments = segments.min(256);
    let mut previous = (
        cx + radius,
        cy,
    );
    for step in 1..=segments {
        let angle = std::f32::consts::TAU * step as f32 / segments as f32;
        let point = (
            cx + (radius as f32 * angle.cos()).round() as i32,
            cy + (radius as f32 * angle.sin()).round() as i32,
        );
        draw_line(
            buffer, buf_w, buf_h,
            previous.0, previous.1,
            point.0, point.1,
            color,
        );
        previous = point;
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
        assert!(net.branches.len() <= MAX_BRANCHES);
        if net.branches.len() == MAX_BRANCHES {
            assert!(net
                .branches
                .iter()
                .filter(|branch| !branch.growing && branch.has_node)
                .all(|branch| branch.children_spawned));
        }
    }

    #[test]
    fn spatial_hash_matches_endpoint_deduplication_rule() {
        let mut index = StartSpatialIndex::new(4);
        index.insert((20.0, 30.0));
        assert!(index.has_nearby((20.5, 30.0)));
        assert!(!index.has_nearby((21.0, 30.0)));
        assert!(!index.has_nearby((24.0, 30.0)));
    }

    #[test]
    fn intersections_do_not_create_duplicate_forks() {
        let mut network = MycelialNetwork::new(100, 100, "intersection-test");
        let parent = Branch {
            start: (10.0, 10.0),
            end: (40.0, 10.0),
            angle: 0.0,
            thickness: 2.5,
            opacity: 1.0,
            depth: 0,
            growing: false,
            growth_progress: 1.0,
            has_node: true,
            node_brightness: 0.0,
            children_spawned: false,
        };
        let intersecting = Branch::new((40.5, 10.0), 1.0, 1.5, 1);
        network.branches = vec![parent, intersecting];

        network.grow(0.01, 1.0);

        assert_eq!(network.branches.len(), 2);
        assert!(network.branches[0].children_spawned);
    }

    #[test]
    fn completed_nodes_keep_their_one_time_spawn_decision() {
        let mut net = MycelialNetwork::new(200, 200, "spawn-state-test");
        for _ in 0..300 {
            net.grow(0.1, 1.0);
        }

        let previous: Vec<bool> = net.branches.iter().map(|branch| branch.children_spawned).collect();
        assert!(previous.iter().any(|spawned| *spawned));

        for _ in 0..30 {
            net.grow(0.1, 1.0);
        }

        for (index, was_spawned) in previous.iter().enumerate() {
            if *was_spawned {
                assert!(net.branches[index].children_spawned);
            }
        }
        assert!(net.branches.len() <= MAX_BRANCHES);
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
    fn low_power_slows_scene_time_without_stretching_the_boot_intro() {
        use crate::theme::MotionMode;

        let mut network = MycelialNetwork::new_with_theme(
            160, 90, "presentation-clock-regression", Theme::CivicDawn,
        );
        network.set_motion_scale(MotionMode::LowPower.scale());

        for _ in 0..45 {
            network.grow(1.0 / 30.0, 1.0);
        }

        assert!(
            (network.presentation_elapsed - 1.5).abs() < 0.001,
            "presentation time should advance at wall cadence"
        );
        assert!(
            (network.elapsed - 0.375).abs() < 0.001,
            "scene simulation time should remain scaled by Low Power"
        );
        assert!(
            network.presentation_time() >= 1.0 && network.visual_time() < 1.0,
            "presentation must leave the intro while the slower scene clock remains below one second"
        );

        let mut frame = vec![0u32; 160 * 90];
        network.render(&mut frame);
        assert!(
            frame.iter().any(|pixel| *pixel != network.palette.background.to_xrgb8888()),
            "the low-power scene should be visible after presentation handoff"
        );
    }

    #[test]
    fn reduced_motion_stops_simulation_time_and_branch_growth() {
        let mut network = MycelialNetwork::new(160, 90, "reduced-motion-test");
        network.set_motion_scale(0.0);
        let branches_before = network.branches.len();
        for _ in 0..200 {
            network.grow(0.1, 1.0);
        }
        assert_eq!(network.elapsed, 0.0);
        assert_eq!(network.branches.len(), branches_before);

        network.set_motion_scale(f32::NAN);
        assert_eq!(network.motion_scale, 0.0);
        network.grow(0.2, 1.0);
        assert_eq!(network.elapsed, 0.0);

        network.set_motion_scale(1.5);
        assert_eq!(network.motion_scale, 1.0);
        network.grow(9.0, 1.0);
        assert!(network.elapsed <= 0.25);
    }

    #[test]
    fn reduced_motion_frames_are_static_and_deterministic_for_all_themes() {
        for theme in Theme::ALL {
            let mut a = MycelialNetwork::new_with_theme(160, 90, "still-preview-v1", theme);
            let mut b = MycelialNetwork::new_with_theme(160, 90, "still-preview-v1", theme);
            a.elapsed = 8.0;
            b.elapsed = 8.25;
            a.set_motion_scale(0.0);
            b.set_motion_scale(0.0);
            let mut first = vec![0u32; 160 * 90];
            let mut second = vec![0u32; 160 * 90];
            a.render(&mut first);
            b.render(&mut second);
            assert_eq!(first, second, "static theme {} is nondeterministic", theme.id());
            assert!(
                first.iter().any(|pixel| *pixel != first[0]),
                "static theme {} rendered a flat frame", theme.id()
            );
        }
    }

    #[test]
    fn reduced_motion_skips_intro_and_renders_a_visible_still_at_startup() {
        for theme in Theme::ALL {
            let mut scene = MycelialNetwork::new_with_theme(
                160, 90, "reduced-startup-still-v1", theme,
            );
            scene.set_motion_scale(0.0);
            let mut frame = vec![0u32; 160 * 90];
            scene.render(&mut frame);
            assert_eq!(scene.elapsed, 0.0, "Reduced theme {} must not mutate simulation time", theme.id());
            assert!(frame.iter().any(|pixel| *pixel != frame[0]),
                "Reduced theme {} should be visible on the first frame", theme.id());
        }
    }

    #[test]
    fn all_ten_themes_render_deterministic_nonuniform_frames() {
        for theme in Theme::ALL {
            let mut a = MycelialNetwork::new_with_theme(160, 90, "scene-preview-v1", theme);
            let mut b = MycelialNetwork::new_with_theme(160, 90, "scene-preview-v1", theme);
            for _ in 0..10 {
                a.grow(0.2, 0.75);
                b.grow(0.2, 0.75);
            }
            a.elapsed = 8.0;
            b.elapsed = 8.0;
            let mut frame_a = vec![0u32; 160 * 90];
            let mut frame_b = vec![0u32; 160 * 90];
            a.render(&mut frame_a);
            b.render(&mut frame_b);
            assert_eq!(frame_a, frame_b, "theme {} is not deterministic", theme.id());
            assert!(
                frame_a.iter().any(|pixel| *pixel != frame_a[0]),
                "theme {} rendered a flat frame", theme.id()
            );
        }
    }

    #[test]
    fn verdant_recovery_has_motion_only_when_motion_is_enabled() {
        use crate::theme::MotionMode;

        for mode in [MotionMode::LowPower, MotionMode::Balanced, MotionMode::Cinematic] {
            let mut network = MycelialNetwork::new_with_theme(
                160, 90, "verdant-recovery-motion-regression", Theme::VerdantRecovery,
            );
            network.set_motion_scale(mode.scale());
            network.elapsed = 8.0;
            let mut earlier = vec![0u32; 160 * 90];
            network.render(&mut earlier);

            network.elapsed = 8.25;
            let mut later = vec![0u32; 160 * 90];
            network.render(&mut later);
            assert_ne!(
                earlier, later,
                "Verdant Recovery must breathe in {:?} mode",
                mode
            );
        }

        let mut reduced = MycelialNetwork::new_with_theme(
            160, 90, "verdant-recovery-motion-regression", Theme::VerdantRecovery,
        );
        reduced.set_motion_scale(MotionMode::Reduced.scale());
        reduced.elapsed = 8.0;
        let mut reduced_earlier = vec![0u32; 160 * 90];
        reduced.render(&mut reduced_earlier);
        reduced.elapsed = 8.25;
        let mut reduced_later = vec![0u32; 160 * 90];
        reduced.render(&mut reduced_later);
        assert_eq!(
            reduced_earlier, reduced_later,
            "Reduced Verdant Recovery must remain pixel-identical"
        );
    }

    #[test]
    fn epistemic_lattice_has_motion_only_when_motion_is_enabled() {
        use crate::theme::MotionMode;

        for mode in [MotionMode::LowPower, MotionMode::Balanced, MotionMode::Cinematic] {
            let mut network = MycelialNetwork::new_with_theme(
                160, 90, "epistemic-lattice-motion-regression", Theme::PrismOfEvidence,
            );
            network.set_motion_scale(mode.scale());
            network.elapsed = 8.0;
            let mut earlier = vec![0u32; 160 * 90];
            network.render(&mut earlier);

            network.elapsed = 8.25;
            let mut later = vec![0u32; 160 * 90];
            network.render(&mut later);
            assert_ne!(
                earlier, later,
                "Epistemic Lattice must respond to elapsed time in {:?} mode",
                mode
            );
        }

        let mut reduced = MycelialNetwork::new_with_theme(
            160, 90, "epistemic-lattice-motion-regression", Theme::PrismOfEvidence,
        );
        reduced.set_motion_scale(MotionMode::Reduced.scale());
        reduced.elapsed = 8.0;
        let mut reduced_earlier = vec![0u32; 160 * 90];
        reduced.render(&mut reduced_earlier);
        reduced.elapsed = 8.25;
        let mut reduced_later = vec![0u32; 160 * 90];
        reduced.render(&mut reduced_later);
        assert_eq!(
            reduced_earlier, reduced_later,
            "Reduced Epistemic Lattice must remain pixel-identical"
        );
    }

    #[test]
    fn graphite_focus_has_motion_only_when_motion_is_enabled() {
        use crate::theme::MotionMode;

        for mode in [MotionMode::LowPower, MotionMode::Balanced, MotionMode::Cinematic] {
            let mut network = MycelialNetwork::new_with_theme(
                160, 90, "graphite-motion-regression", Theme::GraphiteFocus,
            );
            network.set_motion_scale(mode.scale());
            network.elapsed = 8.0;
            let mut earlier = vec![0u32; 160 * 90];
            network.render(&mut earlier);

            network.elapsed = 8.25;
            let mut later = vec![0u32; 160 * 90];
            network.render(&mut later);
            assert_ne!(
                earlier, later,
                "Graphite Focus must respond to elapsed time in {:?} mode",
                mode
            );
        }

        let mut reduced = MycelialNetwork::new_with_theme(
            160, 90, "graphite-motion-regression", Theme::GraphiteFocus,
        );
        reduced.set_motion_scale(MotionMode::Reduced.scale());
        reduced.elapsed = 8.0;
        let mut reduced_earlier = vec![0u32; 160 * 90];
        reduced.render(&mut reduced_earlier);
        reduced.elapsed = 8.25;
        let mut reduced_later = vec![0u32; 160 * 90];
        reduced.render(&mut reduced_later);
        assert_eq!(
            reduced_earlier, reduced_later,
            "Reduced Graphite Focus must remain pixel-identical"
        );
    }

    #[test]
    fn civic_commons_has_motion_only_when_motion_is_enabled() {
        let mut network = MycelialNetwork::new_with_theme(
            160, 90, "civic-motion-regression", Theme::CivicDawn,
        );
        network.set_motion_scale(0.25);
        network.elapsed = 8.0;
        let mut earlier = vec![0u32; 160 * 90];
        network.render(&mut earlier);

        network.elapsed = 8.25;
        let mut later = vec![0u32; 160 * 90];
        network.render(&mut later);
        assert_ne!(earlier, later, "animated Civic Commons must not freeze in Low Power mode");

        network.set_motion_scale(0.0);
        network.elapsed = 8.0;
        let mut reduced_earlier = vec![0u32; 160 * 90];
        network.render(&mut reduced_earlier);
        network.elapsed = 8.25;
        let mut reduced_later = vec![0u32; 160 * 90];
        network.render(&mut reduced_later);
        assert_eq!(reduced_earlier, reduced_later, "Reduced Civic Commons must remain static");
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
    fn test_theme_changes_palette_and_growth_profile_deterministically() {
        let default = MycelialNetwork::new(128, 96, "public-test-seed");
        let themed_a = MycelialNetwork::new_with_theme(
            128,
            96,
            "public-test-seed",
            Theme::CosmicAtlas,
        );
        let themed_b = MycelialNetwork::new_with_theme(
            128,
            96,
            "public-test-seed",
            Theme::CosmicAtlas,
        );
        assert_eq!(themed_a.branches.len(), themed_b.branches.len());
        assert_eq!(themed_a.branches[0].angle, themed_b.branches[0].angle);
        assert_ne!(default.palette.background, themed_a.palette.background);
        assert_ne!(
            default.growth_profile.growth_speed,
            themed_a.growth_profile.growth_speed
        );
    }

    #[test]
    fn theme_density_cap_is_enforced_before_branch_creation() {
        let mut network = MycelialNetwork::new(640, 360, "theme-density-cap");
        let initial_count = network.branches.len();
        network.density.max_branches = initial_count;

        for branch in &mut network.branches {
            branch.growing = false;
            branch.has_node = true;
            branch.children_spawned = false;
            branch.growth_progress = 1.0;
            branch.end = (branch.start.0 + 10.0, branch.start.1);
        }

        network.grow(0.25, 1.0);
        assert_eq!(network.branches.len(), initial_count);
        assert!(network.branches.len() <= network.density.max_branches);
    }

    #[test]
    fn per_theme_growth_multiplier_changes_growth_without_changing_seed() {
        let mut slow = MycelialNetwork::new_with_theme(
            320, 180, "theme-motion-contract", Theme::CosmicAtlas
        );
        let mut fast = MycelialNetwork::new_with_theme(
            320, 180, "theme-motion-contract", Theme::CosmicAtlas
        );
        slow.theme_motion.growth = 0.2;
        fast.theme_motion.growth = 1.0;
        slow.grow(0.1, 1.0);
        fast.grow(0.1, 1.0);
        let slow_progress: f32 = slow.branches.iter().map(|branch| branch.growth_progress).sum();
        let fast_progress: f32 = fast.branches.iter().map(|branch| branch.growth_progress).sum();
        assert!(fast_progress > slow_progress);
        assert_eq!(Theme::CosmicAtlas.motion_tuning().pulse, 0.5);
    }

    #[test]
    fn particle_budget_bounds_animated_star_work_but_preserves_reduced_still_scene() {
        let mut network = MycelialNetwork::new_with_theme(
            320,
            180,
            "particle-budget",
            Theme::CosmicAtlas,
        );
        let total_stars = network.stars.len();
        assert!(total_stars >= 32);
        network.density.max_particles = 3;
        assert_eq!(network.rendered_star_count(), 3);
        network.density.max_particles = total_stars;

        network.set_motion_scale(1.0);
        network.set_particle_budget_scale(0.25);
        assert_eq!(
            network.rendered_star_count(),
            ((total_stars as f32) * 0.25).ceil() as usize
        );

        network.set_particle_budget_scale(f32::NAN);
        assert_eq!(network.particle_budget_scale, 0.0);
        assert_eq!(network.rendered_star_count(), 0);

        network.set_motion_scale(0.0);
        assert_eq!(
            network.rendered_star_count(),
            total_stars,
            "reduced motion should preserve the complete deterministic still composition"
        );

        let non_cosmic = MycelialNetwork::new_with_theme(
            320,
            180,
            "no-unneeded-stars",
            Theme::MycelialGenesis,
        );
        assert!(non_cosmic.stars.is_empty());
    }

    #[test]
    fn scene_entry_fade_duration_is_configurable_and_bounded() {
        let mut network = MycelialNetwork::new(4, 4, "parameterized-entry-fade");
        let background = network.palette.background.to_xrgb8888();
        let mut frame = vec![0x00ff_ffff; 16];

        network.set_scene_entry_fade_duration(0.75);
        network.elapsed = SCENE_ENTRY_FADE_START + 0.375;
        network.apply_scene_entry_fade(&mut frame);
        // Midpoint of the configured 750 ms duration is the smoothstep midpoint.
        assert_eq!(frame[0], {
            let red_bg = ((background >> 16) & 0xff) as f32;
            let green_bg = ((background >> 8) & 0xff) as f32;
            let blue_bg = (background & 0xff) as f32;
            let red = (red_bg * 0.5 + 255.0 * 0.5).round() as u32;
            let green = (green_bg * 0.5 + 255.0 * 0.5).round() as u32;
            let blue = (blue_bg * 0.5 + 255.0 * 0.5).round() as u32;
            (red << 16) | (green << 8) | blue
        });

        network.set_scene_entry_fade_duration(f32::NAN);
        assert_eq!(network.scene_entry_fade_duration, DEFAULT_SCENE_ENTRY_FADE_DURATION);
        network.set_scene_entry_fade_duration(0.001);
        assert_eq!(network.scene_entry_fade_duration, MIN_SCENE_ENTRY_FADE_DURATION);
        network.set_scene_entry_fade_duration(f32::INFINITY);
        assert_eq!(network.scene_entry_fade_duration, DEFAULT_SCENE_ENTRY_FADE_DURATION);
        network.set_scene_entry_fade_duration(3.0);
        assert_eq!(network.scene_entry_fade_duration, MAX_SCENE_ENTRY_FADE_DURATION);
    }

    #[test]
    fn test_scene_entry_fade_blends_detail_in_and_finishes_without_changing_static_output() {
        let mut net = MycelialNetwork::new(4, 4, "entry-fade-test");
        let background = net.palette.background.to_xrgb8888();
        let mut frame = vec![0x00ff_ffff; 16];

        // The first eligible scene frame blends to exactly the existing background.
        net.elapsed = SCENE_ENTRY_FADE_START;
        net.apply_scene_entry_fade(&mut frame);
        assert!(frame.iter().all(|pixel| *pixel == background));

        // The middle of the transition blends, without snapping to either endpoint.
        net.elapsed = SCENE_ENTRY_FADE_START + DEFAULT_SCENE_ENTRY_FADE_DURATION * 0.5;
        frame.fill(0x00ff_ffff);
        net.apply_scene_entry_fade(&mut frame);
        let midpoint = frame[0];
        assert_ne!(midpoint, background);
        assert_ne!(midpoint, 0x00ff_ffff);

        // Smoothstep keeps the early reveal closer to background than a linear 25% mix.
        net.elapsed = SCENE_ENTRY_FADE_START + DEFAULT_SCENE_ENTRY_FADE_DURATION * 0.25;
        frame.fill(0x00ff_ffff);
        net.apply_scene_entry_fade(&mut frame);
        let early = frame[0];
        let red_background = ((background >> 16) & 0xff) as f32;
        let red_early = (early >> 16) & 0xff;
        let expected_red = (red_background * (1.0 - 0.15625) + 255.0 * 0.15625).round() as u32;
        assert_eq!(red_early, expected_red);

        frame.fill(0x00ff_ffff);
        net.elapsed = 1.1;
        net.apply_scene_entry_fade(&mut frame);
        assert!(frame.iter().all(|pixel| *pixel != 0x00ff_ffff));
        assert!(frame.iter().all(|pixel| *pixel != background));

        net.elapsed = SCENE_ENTRY_FADE_START + DEFAULT_SCENE_ENTRY_FADE_DURATION;
        frame.fill(0x00ff_ffff);
        net.apply_scene_entry_fade(&mut frame);
        assert!(frame.iter().all(|pixel| *pixel == 0x00ff_ffff));

        // Reduced mode's fixed scene time is beyond the entry fade.
        net.elapsed = 8.0;
        frame.fill(0x0012_3456);
        net.apply_scene_entry_fade(&mut frame);
        assert!(frame.iter().all(|pixel| *pixel == 0x0012_3456));
    }

    #[test]
    fn test_exit_fade_is_bounded_and_reaches_black() {
        let mut net = MycelialNetwork::new(4, 4, "fade-test");
        let mut frame = vec![0x00ff_8040; 16];
        net.fade_out(0.5);
        net.apply_exit_fade(&mut frame);
        assert_eq!(frame[0], 0x0080_4020);
        net.fade_out(f32::NAN);
        assert_eq!(net.exit_fade, 0.0);
        net.fade_out(1.0);
        frame.fill(0x00ff_ffff);
        net.apply_exit_fade(&mut frame);
        assert!(frame.iter().all(|pixel| *pixel == 0));
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
}
