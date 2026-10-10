//! Platform-neutral visual simulation for Sovereign Boot.
//!
//! This crate owns deterministic scene state and CPU pixel rendering only.
//! It deliberately does not open display devices or depend on a window system.

pub mod color;
pub mod mycelium;
