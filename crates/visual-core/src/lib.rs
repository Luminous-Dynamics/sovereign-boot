//! Platform-neutral visual simulation for Sovereign Boot.
//!
//! This crate owns deterministic scene state and CPU pixel rendering only.
//! It deliberately does not open display devices or depend on a window system.

pub mod color;
pub mod contract;
pub mod mycelium;
pub mod ouroboros;
pub mod ouroboros_evidence;
pub mod ouroboros_face;
pub mod ouroboros_mesh;
pub mod ouroboros_render;
pub mod ouroboros_trace;

pub mod settings;
