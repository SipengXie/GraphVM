//! SSA (Static Single Assignment) analysis for REVM
//! 
//! This crate provides SSA analysis functionality for the REVM project.

pub mod logger;
pub mod shadow_stack;
pub mod types;
pub mod utils;
mod call_types;

pub use logger::SSALogger;
pub use shadow_stack::{ShadowStack, InstructionResult};
pub use types::*;
pub use call_types::*;
pub use utils::*;
