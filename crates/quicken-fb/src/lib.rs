// Copyright (C) 2024-2026 Tristan Stoltz / Luminous Dynamics
// SPDX-License-Identifier: AGPL-3.0-or-later
// Commercial licensing: see COMMERCIAL_LICENSE.md at repository root
//! Linux DRM/KMS host adapter for Sovereign Visual Core.

pub mod framebuffer;
pub mod progress;
pub mod vt;

// Preserve the existing renderer-facing paths while keeping the simulation
// independent of Linux display and VT APIs.
pub use sovereign_visual_core::{color, mycelium};
