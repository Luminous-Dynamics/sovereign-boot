// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root

//! Small, dependency-free visual theme contract for the early-boot renderer.
//!
//! Theme IDs are stable data, not executable scripts. Desktop renderers may map
//! the same IDs to richer shaders, but the boot path remains a bounded CPU/DRM
//! renderer and does not depend on Vulkan or a compositor.

use crate::color::Rgba;

/// Procedural scene families. Themes can share a renderer without sharing geometry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SceneKind {
    /// Branching network and connected nodes.
    MycelialNetwork,
    /// Warm radial sun, horizon rings, and solar rays.
    SolarCanopy,
    /// Sparse stars with low-contrast constellation paths.
    CosmicCartography,
    /// Layered flowing wave ribbons.
    OceanWaves,
    /// Epistemic lattice with edges and evidence nodes.
    EpistemicLattice,
    /// Interference-like abstract wave geometry; an artistic metaphor only.
    QuantumInterference,
    /// Soft, multi-layer aurora ribbons.
    AuroraRibbons,
    /// Minimal precision grid and drafting marks.
    GraphiteMinimal,
    /// Calm concentric mark for recovery and resilience.
    VerdantRecovery,
    /// Multiple connected communities without a central hub.
    CivicCommons,
}

/// User-selectable animation policy shared by boot and future desktop adapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionMode {
    /// Stable still scene; only a brief fade is allowed at handoff.
    Reduced,
    /// Lower update cadence and slower procedural evolution.
    LowPower,
    /// Moderate animation cadence for normal use.
    Balanced,
    /// Full boot animation, still bounded by the boot renderer's frame budget.
    Cinematic,
}

impl MotionMode {
    /// All motion modes in increasing motion-scale order from the visual manifest.
    pub const ALL: [Self; 4] = GENERATED_MOTION_MODE_ALL;

    /// Resolve an exact stable ID; unknown modes fail closed.
    pub fn from_id(id: &str) -> Option<Self> {
        GENERATED_MOTION_MODE_CONTRACTS
            .iter()
            .find(|contract| contract.id == id)
            .map(|contract| contract.mode)
    }

    fn contract(self) -> &'static MotionModeContract {
        GENERATED_MOTION_MODE_CONTRACTS
            .iter()
            .find(|contract| contract.mode == self)
            .expect("every MotionMode variant must have a generated Visual Genome contract")
    }

    pub fn id(self) -> &'static str {
        self.contract().id
    }

    /// Normalized motion amplitude, in [0, 1].
    pub fn scale(self) -> f32 {
        self.contract().motion_scale
    }

    /// Normalized particle budget, in [0, 1].
    pub fn particle_budget_scale(self) -> f32 {
        self.contract().particle_budget_scale
    }

    /// Minimum wall-clock interval between scene updates and progress polls.
    pub fn frame_interval_ms(self) -> u64 {
        self.contract().frame_interval_ms
    }

    /// (contraction seconds, convergence hold seconds, final fade seconds).
    pub fn transition_durations(self) -> (f32, f32, f32) {
        self.contract().transition_durations_seconds
    }

    /// Scene-entry crossfade budget in milliseconds.
    pub fn crossfade_ms(self) -> u64 {
        self.contract().crossfade_ms
    }
}

impl SceneKind {
    /// Stable, kebab-case scene-family identifier generated from the visual manifest.
    pub fn id(self) -> &'static str {
        GENERATED_THEME_CONTRACTS
            .iter()
            .find(|contract| contract.scene_kind == self)
            .map(|contract| contract.scene_family_id)
            .unwrap_or("unknown-scene-family")
    }
}

/// Stable theme IDs shared with themes/visual-genome-v1.json.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Theme {
    MycelialGenesis,
    SolarCommons,
    CosmicAtlas,
    OceanicIntelligence,
    PrismOfEvidence,
    QuantumGarden,
    AuroraSanctuary,
    GraphiteFocus,
    VerdantRecovery,
    CivicDawn,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ThemeDensity {
    pub(crate) max_branches: usize,
    pub(crate) max_particles: usize,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct ThemeMotion {
    pub(crate) growth: f32,
    pub(crate) pulse: f32,
    /// Retained for future input-aware desktop rendering. The CPU boot renderer
    /// has no pointer/camera displacement input and therefore applies no parallax.
    pub(crate) parallax: f32,
}

#[derive(Debug, Clone, Copy)]
struct ThemeContract {
    theme: Theme,
    id: &'static str,
    scene_kind: SceneKind,
    scene_family_id: &'static str,
    palette: Palette,
    growth_profile: GrowthProfile,
    density: ThemeDensity,
    motion: ThemeMotion,
}

#[derive(Debug, Clone, Copy)]
struct MotionModeContract {
    mode: MotionMode,
    id: &'static str,
    motion_scale: f32,
    particle_budget_scale: f32,
    frame_interval_ms: u64,
    transition_durations_seconds: (f32, f32, f32),
    crossfade_ms: u64,
}

include!("visual_genome_generated.rs");

impl Theme {
    /// All supported themes in the stable order defined by the visual manifest.
    pub const ALL: [Self; 10] = GENERATED_THEME_ALL;

    /// Resolve an exact stable ID; unknown IDs fail closed.
    pub fn from_id(id: &str) -> Option<Self> {
        GENERATED_THEME_CONTRACTS
            .iter()
            .find(|contract| contract.id == id)
            .map(|contract| contract.theme)
    }

    fn contract(self) -> &'static ThemeContract {
        GENERATED_THEME_CONTRACTS
            .iter()
            .find(|contract| contract.theme == self)
            .expect("every Theme variant must have a generated Visual Genome contract")
    }

    /// Canonical, locale-independent ID generated from the manifest.
    pub fn id(self) -> &'static str {
        self.contract().id
    }

    /// Select a bounded procedural geometry family generated from the manifest.
    pub fn scene_kind(self) -> SceneKind {
        self.contract().scene_kind
    }

    /// Palette used by the CPU reference renderer.
    pub fn palette(self) -> Palette {
        self.contract().palette
    }

    /// Bounded procedural-growth controls generated from the manifest.
    pub fn growth_profile(self) -> GrowthProfile {
        self.contract().growth_profile
    }

    /// Per-theme safety caps as stable primitive values for inventory tooling.
    pub fn density_caps(self) -> (usize, usize) {
        let density = self.density();
        (density.max_branches, density.max_particles)
    }

    /// Per-theme growth, pulse and parallax tuning as primitive values for inventory tooling.
    /// Internal contract structs remain crate-private.
    pub fn motion_tuning_values(self) -> (f32, f32, f32) {
        let motion = self.motion_tuning();
        (motion.growth, motion.pulse, motion.parallax)
    }

    /// Per-theme safety caps generated from the manifest.
    pub(crate) fn density(self) -> ThemeDensity {
        self.contract().density
    }

    /// Per-theme growth/pulse tuning generated from the manifest.
    pub(crate) fn motion_tuning(self) -> ThemeMotion {
        self.contract().motion
    }
}

/// Semantic colors shared by CPU output and future desktop shader uniforms.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub background: Rgba,
    pub primary: Rgba,
    pub secondary: Rgba,
    pub node: Rgba,
    pub convergence: Rgba,
}

/// Theme-specific procedural controls with explicit, bounded ranges.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct GrowthProfile {
    pub initial_branch_min: u8,
    /// Count variation; must remain greater than zero.
    pub initial_branch_variants: u8,
    pub branch_length_scale: f32,
    pub growth_speed: f32,
    pub min_fork_angle_degrees: f32,
    pub max_fork_angle_degrees: f32,
    pub min_children: u8,
    pub max_children: u8,
    pub pulse_gain: f32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_round_trips_through_its_stable_id() {
        for theme in Theme::ALL {
            assert_eq!(Theme::from_id(theme.id()), Some(theme));
        }
    }

    #[test]
    fn unknown_and_case_changed_ids_are_rejected() {
        assert_eq!(Theme::from_id("unknown"), None);
        assert_eq!(Theme::from_id("Mycelial-Genesis"), None);
        assert_eq!(Theme::from_id(""), None);
    }

    #[test]
    fn theme_ids_are_unique() {
        for (index, theme) in Theme::ALL.iter().enumerate() {
            assert!(!Theme::ALL[..index].contains(theme));
        }
    }

    #[test]
    fn each_theme_has_opaque_palette_colors() {
        for theme in Theme::ALL {
            let palette = theme.palette();
            for color in [
                palette.background,
                palette.primary,
                palette.secondary,
                palette.node,
                palette.convergence,
            ] {
                assert_eq!(color.3, 0xff, "theme {} has non-opaque color", theme.id());
            }
        }
    }

    #[test]
    fn recovery_and_focus_themes_use_dark_backgrounds() {
        assert!(Theme::VerdantRecovery.palette().background.0 < 32);
        assert!(Theme::GraphiteFocus.palette().background.0 < 32);
    }

    #[test]
    fn growth_profiles_are_bounded_and_non_degenerate() {
        for theme in Theme::ALL {
            let profile = theme.growth_profile();
            assert!(profile.initial_branch_min > 0);
            assert!(profile.initial_branch_variants > 0);
            assert!(profile.branch_length_scale > 0.0);
            assert!(profile.growth_speed > 0.0 && profile.growth_speed <= 30.0);
            assert!(profile.min_fork_angle_degrees >= 0.0);
            assert!(profile.max_fork_angle_degrees > profile.min_fork_angle_degrees);
            assert!(profile.max_fork_angle_degrees <= 90.0);
            assert!(profile.min_children > 0 && profile.max_children >= profile.min_children);
            assert!(profile.pulse_gain >= 0.0 && profile.pulse_gain <= 1.0);
            let initial_max = profile.initial_branch_min + profile.initial_branch_variants - 1;
            assert!(initial_max <= 9, "theme {} exceeds root branch cap", theme.id());
        }
    }

    #[test]
    fn themes_change_more_than_their_palette() {
        let genesis = Theme::MycelialGenesis.growth_profile();
        let graphite = Theme::GraphiteFocus.growth_profile();
        assert_ne!(genesis.growth_speed, graphite.growth_speed);
        assert_ne!(genesis.branch_length_scale, graphite.branch_length_scale);
        assert_ne!(genesis.max_children, graphite.max_children);
    }

    #[test]
    fn every_theme_has_its_own_geometry_family() {
        let kinds = Theme::ALL.map(Theme::scene_kind);
        for (index, kind) in kinds.iter().enumerate() {
            assert!(!kinds[..index].contains(kind), "scene family {kind:?} is reused");
        }

        assert_eq!(Theme::MycelialGenesis.scene_kind(), SceneKind::MycelialNetwork);
        assert_eq!(Theme::SolarCommons.scene_kind(), SceneKind::SolarCanopy);
        assert_eq!(Theme::CosmicAtlas.scene_kind(), SceneKind::CosmicCartography);
        assert_eq!(Theme::OceanicIntelligence.scene_kind(), SceneKind::OceanWaves);
        assert_eq!(Theme::PrismOfEvidence.scene_kind(), SceneKind::EpistemicLattice);
        assert_eq!(Theme::QuantumGarden.scene_kind(), SceneKind::QuantumInterference);
        assert_eq!(Theme::AuroraSanctuary.scene_kind(), SceneKind::AuroraRibbons);
        assert_eq!(Theme::GraphiteFocus.scene_kind(), SceneKind::GraphiteMinimal);
        assert_eq!(Theme::VerdantRecovery.scene_kind(), SceneKind::VerdantRecovery);
        assert_eq!(Theme::CivicDawn.scene_kind(), SceneKind::CivicCommons);
    }

    #[test]
    fn scene_family_ids_are_unique_and_stable() {
        let ids = Theme::ALL.map(|theme| theme.scene_kind().id());
        for (index, id) in ids.iter().enumerate() {
            assert!(!ids[..index].contains(id), "scene ID {id} is reused");
        }
        assert_eq!(Theme::PrismOfEvidence.scene_kind().id(), "epistemic-lattice");
        assert_eq!(Theme::CivicDawn.scene_kind().id(), "civic-commons");
    }

    #[test]
    fn motion_modes_have_stable_ids_and_monotonic_budgets() {
        let mut previous_scale = 0.0;
        for mode in MotionMode::ALL {
            assert_eq!(MotionMode::from_id(mode.id()), Some(mode));
            assert!(mode.scale() >= previous_scale);
            assert!(mode.scale() <= 1.0);
            assert!(mode.frame_interval_ms() >= 33);
            assert!(mode.transition_durations().2 > 0.0);
            previous_scale = mode.scale();
        }
        assert_eq!(MotionMode::from_id("unknown"), None);
        assert_eq!(MotionMode::Reduced.scale(), 0.0);
    }
}


#[cfg(test)]
mod public_contract_tests {
    use super::Theme;

    #[test]
    fn primitive_contract_projection_is_bounded_for_every_theme() {
        for theme in Theme::ALL {
            let (max_branches, max_particles) = theme.density_caps();
            assert!((1..=8192).contains(&max_branches));
            assert!(max_particles <= 768);
            let (growth, pulse, parallax) = theme.motion_tuning_values();
            assert!((0.0..=1.0).contains(&growth));
            assert!((0.0..=1.0).contains(&pulse));
            assert!((0.0..=1.0).contains(&parallax));
        }
    }
}
