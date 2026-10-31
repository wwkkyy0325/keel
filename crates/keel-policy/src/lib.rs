//! Profile resolution and the immutable runtime policy.
//!
//! This crate depends on `keel-core` only. Host capability validation stays in
//! `keel-runtime`, where the tool registry and context builder live.

mod config;
pub mod profile;

pub use config::{RuntimeConfig, TurnLimits};
pub use profile::{from_profile_str, load_profile};
