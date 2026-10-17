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
    pub const ALL: [Self; 4] = [
        Self::Reduced,
        Self::LowPower,
        Self::Balanced,
        Self::Cinematic,
    ];

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "reduced" => Some(Self::Reduced),
            "low-power" => Some(Self::LowPower),
            "balanced" => Some(Self::Balanced),
            "cinematic" => Some(Self::Cinematic),
            _ => None,
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Reduced => "reduced",
            Self::LowPower => "low-power",
            Self::Balanced => "balanced",
            Self::Cinematic => "cinematic",
        }
    }

    pub const fn scale(self) -> f32 {
        match self {
            Self::Reduced => 0.0,
            Self::LowPower => 0.25,
            Self::Balanced => 0.65,
            Self::Cinematic => 1.0,
        }
    }

    /// Minimum wall-clock interval between scene updates and progress polls.
    pub const fn frame_interval_ms(self) -> u64 {
        match self {
            Self::Reduced => 250,
            Self::LowPower => 67,
            Self::Balanced | Self::Cinematic => 33,
        }
    }

    /// (contraction seconds, convergence hold seconds, final fade seconds).
    pub const fn transition_durations(self) -> (f32, f32, f32) {
        match self {
            Self::Reduced => (0.0, 0.0, 0.25),
            Self::LowPower => (2.0, 0.2, 0.75),
            Self::Balanced => (2.5, 0.35, 1.0),
            Self::Cinematic => (3.0, 0.5, 1.5),
        }
    }
}

impl SceneKind {
    /// Stable, kebab-case scene-family identifier used by the visual manifest.
    pub const fn id(self) -> &'static str {
        match self {
            Self::MycelialNetwork => "mycelial-network",
            Self::SolarCanopy => "solar-canopy",
            Self::CosmicCartography => "cosmic-cartography",
            Self::OceanWaves => "ocean-waves",
            Self::EpistemicLattice => "epistemic-lattice",
            Self::QuantumInterference => "quantum-interference",
            Self::AuroraRibbons => "aurora-ribbons",
            Self::GraphiteMinimal => "graphite-minimal",
            Self::VerdantRecovery => "verdant-recovery",
            Self::CivicCommons => "civic-commons",
        }
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

impl Theme {
    /// All supported themes in stable list order.
    pub const ALL: [Self; 10] = [
        Self::MycelialGenesis,
        Self::SolarCommons,
        Self::CosmicAtlas,
        Self::OceanicIntelligence,
        Self::PrismOfEvidence,
        Self::QuantumGarden,
        Self::AuroraSanctuary,
        Self::GraphiteFocus,
        Self::VerdantRecovery,
        Self::CivicDawn,
    ];

    /// Resolve an exact stable ID. Unknown IDs must fail closed.
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "mycelial-genesis" => Some(Self::MycelialGenesis),
            "solar-commons" => Some(Self::SolarCommons),
            "cosmic-atlas" => Some(Self::CosmicAtlas),
            "oceanic-intelligence" => Some(Self::OceanicIntelligence),
            "prism-of-evidence" => Some(Self::PrismOfEvidence),
            "quantum-garden" => Some(Self::QuantumGarden),
            "aurora-sanctuary" => Some(Self::AuroraSanctuary),
            "graphite-focus" => Some(Self::GraphiteFocus),
            "verdant-recovery" => Some(Self::VerdantRecovery),
            "civic-dawn" => Some(Self::CivicDawn),
            _ => None,
        }
    }

    /// Canonical, locale-independent ID.
    pub const fn id(self) -> &'static str {
        match self {
            Self::MycelialGenesis => "mycelial-genesis",
            Self::SolarCommons => "solar-commons",
            Self::CosmicAtlas => "cosmic-atlas",
            Self::OceanicIntelligence => "oceanic-intelligence",
            Self::PrismOfEvidence => "prism-of-evidence",
            Self::QuantumGarden => "quantum-garden",
            Self::AuroraSanctuary => "aurora-sanctuary",
            Self::GraphiteFocus => "graphite-focus",
            Self::VerdantRecovery => "verdant-recovery",
            Self::CivicDawn => "civic-dawn",
        }
    }

    /// Select a bounded procedural geometry family for this theme.
    pub const fn scene_kind(self) -> SceneKind {
        match self {
            Self::MycelialGenesis => SceneKind::MycelialNetwork,
            Self::SolarCommons => SceneKind::SolarCanopy,
            Self::CosmicAtlas => SceneKind::CosmicCartography,
            Self::OceanicIntelligence => SceneKind::OceanWaves,
            Self::PrismOfEvidence => SceneKind::EpistemicLattice,
            Self::QuantumGarden => SceneKind::QuantumInterference,
            Self::AuroraSanctuary => SceneKind::AuroraRibbons,
            Self::GraphiteFocus => SceneKind::GraphiteMinimal,
            Self::VerdantRecovery => SceneKind::VerdantRecovery,
            Self::CivicDawn => SceneKind::CivicCommons,
        }
    }

    /// Palette used by the CPU reference renderer.
    pub const fn palette(self) -> Palette {
        match self {
            Self::MycelialGenesis => Palette {
                background: Rgba(0x10, 0x26, 0x1e, 0xff),
                primary: Rgba(0x7e, 0xc8, 0xa0, 0xff),
                secondary: Rgba(0x5a, 0x6b, 0x5e, 0xff),
                node: Rgba(0xe8, 0xc5, 0x47, 0xff),
                convergence: Rgba(0xc8, 0xea, 0xd7, 0xff),
            },
            Self::SolarCommons => Palette {
                background: Rgba(0x20, 0x18, 0x0f, 0xff),
                primary: Rgba(0xf6, 0xbd, 0x60, 0xff),
                secondary: Rgba(0x84, 0xa9, 0x8c, 0xff),
                node: Rgba(0xf2, 0x84, 0x82, 0xff),
                convergence: Rgba(0xf7, 0xe9, 0xce, 0xff),
            },
            Self::CosmicAtlas => Palette {
                background: Rgba(0x07, 0x0b, 0x1e, 0xff),
                primary: Rgba(0x7a, 0xa2, 0xf7, 0xff),
                secondary: Rgba(0xa7, 0x8b, 0xfa, 0xff),
                node: Rgba(0xf0, 0xab, 0xfc, 0xff),
                convergence: Rgba(0xdc, 0xe7, 0xff, 0xff),
            },
            Self::OceanicIntelligence => Palette {
                background: Rgba(0x03, 0x1f, 0x2d, 0xff),
                primary: Rgba(0x4d, 0xd0, 0xe1, 0xff),
                secondary: Rgba(0x00, 0x8a, 0x98, 0xff),
                node: Rgba(0xb2, 0xf7, 0xef, 0xff),
                convergence: Rgba(0xd7, 0xfa, 0xf7, 0xff),
            },
            Self::PrismOfEvidence => Palette {
                background: Rgba(0x11, 0x18, 0x27, 0xff),
                primary: Rgba(0x7d, 0xd3, 0xfc, 0xff),
                secondary: Rgba(0xc4, 0xb5, 0xfd, 0xff),
                node: Rgba(0xfd, 0xe6, 0x8a, 0xff),
                convergence: Rgba(0xf1, 0xf5, 0xf9, 0xff),
            },
            Self::QuantumGarden => Palette {
                background: Rgba(0x11, 0x13, 0x1d, 0xff),
                primary: Rgba(0x80, 0xcb, 0xc4, 0xff),
                secondary: Rgba(0xb3, 0x9d, 0xdb, 0xff),
                node: Rgba(0xff, 0xcc, 0x80, 0xff),
                convergence: Rgba(0xe5, 0xe7, 0xeb, 0xff),
            },
            Self::AuroraSanctuary => Palette {
                background: Rgba(0x11, 0x1c, 0x2c, 0xff),
                primary: Rgba(0x9b, 0xd9, 0xce, 0xff),
                secondary: Rgba(0x8b, 0xa9, 0xe8, 0xff),
                node: Rgba(0xd9, 0xc2, 0xff, 0xff),
                convergence: Rgba(0xea, 0xf5, 0xf5, 0xff),
            },
            Self::GraphiteFocus => Palette {
                background: Rgba(0x15, 0x17, 0x1a, 0xff),
                primary: Rgba(0xb8, 0xc0, 0xcc, 0xff),
                secondary: Rgba(0x73, 0x7b, 0x87, 0xff),
                node: Rgba(0xe6, 0xe9, 0xef, 0xff),
                convergence: Rgba(0xf5, 0xf5, 0xf5, 0xff),
            },
            Self::VerdantRecovery => Palette {
                background: Rgba(0x0c, 0x21, 0x1e, 0xff),
                primary: Rgba(0x69, 0xb9, 0x9f, 0xff),
                secondary: Rgba(0x68, 0x8b, 0x87, 0xff),
                node: Rgba(0xd0, 0xe8, 0xc7, 0xff),
                convergence: Rgba(0xe7, 0xf2, 0xe9, 0xff),
            },
            Self::CivicDawn => Palette {
                background: Rgba(0x17, 0x25, 0x36, 0xff),
                primary: Rgba(0x6f, 0xc7, 0xb6, 0xff),
                secondary: Rgba(0x84, 0xa1, 0xcd, 0xff),
                node: Rgba(0xf6, 0xc1, 0x77, 0xff),
                convergence: Rgba(0xf4, 0xf4, 0xe9, 0xff),
            },
        }
    }

    /// Bounded procedural-growth controls. These parameters change the actual
    /// branching rhythm, scale, and motion—not just the theme's colors.
    pub const fn growth_profile(self) -> GrowthProfile {
        match self {
            Self::MycelialGenesis => GrowthProfile {
                initial_branch_min: 4,
                initial_branch_variants: 5,
                branch_length_scale: 1.0,
                growth_speed: 30.0,
                min_fork_angle_degrees: 15.0,
                max_fork_angle_degrees: 45.0,
                min_children: 1,
                max_children: 3,
                pulse_gain: 0.8,
            },
            Self::SolarCommons => GrowthProfile {
                initial_branch_min: 6,
                initial_branch_variants: 3,
                branch_length_scale: 1.2,
                growth_speed: 26.0,
                min_fork_angle_degrees: 20.0,
                max_fork_angle_degrees: 55.0,
                min_children: 1,
                max_children: 2,
                pulse_gain: 0.7,
            },
            Self::CosmicAtlas => GrowthProfile {
                initial_branch_min: 4,
                initial_branch_variants: 3,
                branch_length_scale: 1.0,
                growth_speed: 17.0,
                min_fork_angle_degrees: 26.0,
                max_fork_angle_degrees: 70.0,
                min_children: 1,
                max_children: 2,
                pulse_gain: 0.5,
            },
            Self::OceanicIntelligence => GrowthProfile {
                initial_branch_min: 5,
                initial_branch_variants: 3,
                branch_length_scale: 0.75,
                growth_speed: 14.0,
                min_fork_angle_degrees: 8.0,
                max_fork_angle_degrees: 28.0,
                min_children: 1,
                max_children: 2,
                pulse_gain: 0.65,
            },
            Self::PrismOfEvidence => GrowthProfile {
                initial_branch_min: 6,
                initial_branch_variants: 2,
                branch_length_scale: 0.9,
                growth_speed: 16.0,
                min_fork_angle_degrees: 28.0,
                max_fork_angle_degrees: 42.0,
                min_children: 1,
                max_children: 2,
                pulse_gain: 0.45,
            },
            Self::QuantumGarden => GrowthProfile {
                initial_branch_min: 5,
                initial_branch_variants: 4,
                branch_length_scale: 1.1,
                growth_speed: 22.0,
                min_fork_angle_degrees: 35.0,
                max_fork_angle_degrees: 72.0,
                min_children: 1,
                max_children: 3,
                pulse_gain: 0.95,
            },
            Self::AuroraSanctuary => GrowthProfile {
                initial_branch_min: 4,
                initial_branch_variants: 3,
                branch_length_scale: 0.85,
                growth_speed: 8.0,
                min_fork_angle_degrees: 8.0,
                max_fork_angle_degrees: 24.0,
                min_children: 1,
                max_children: 2,
                pulse_gain: 0.4,
            },
            Self::GraphiteFocus => GrowthProfile {
                initial_branch_min: 4,
                initial_branch_variants: 2,
                branch_length_scale: 0.65,
                growth_speed: 4.0,
                min_fork_angle_degrees: 18.0,
                max_fork_angle_degrees: 32.0,
                min_children: 1,
                max_children: 1,
                pulse_gain: 0.2,
            },
            Self::VerdantRecovery => GrowthProfile {
                initial_branch_min: 3,
                initial_branch_variants: 3,
                branch_length_scale: 0.7,
                growth_speed: 6.0,
                min_fork_angle_degrees: 12.0,
                max_fork_angle_degrees: 28.0,
                min_children: 1,
                max_children: 2,
                pulse_gain: 0.25,
            },
            Self::CivicDawn => GrowthProfile {
                initial_branch_min: 7,
                initial_branch_variants: 3,
                branch_length_scale: 1.0,
                growth_speed: 16.0,
                min_fork_angle_degrees: 18.0,
                max_fork_angle_degrees: 48.0,
                min_children: 1,
                max_children: 2,
                pulse_gain: 0.5,
            },
        }
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
