//! SSA (Static Single Assignment) analysis for REVM
//!
//! This crate provides SSA analysis functionality for the REVM project.

mod call_types;
pub mod logger;
pub mod shadow_stack;
pub mod types;
pub mod utils;

pub use call_types::*;
pub use logger::SSALogger;
pub use shadow_stack::{InstructionResult, ShadowStack};
pub use types::*;
pub use utils::*;
