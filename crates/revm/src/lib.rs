//! Revm is a Rust EVM implementation.
#![cfg_attr(not(feature = "std"), no_std)]

#[macro_use]
#[cfg(not(feature = "std"))]
extern crate alloc as std;

// Define modules.

mod builder;
mod context;

#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;

pub mod access_tracker;
mod dag;
pub mod db;
mod evm;
mod frame;
pub mod graph_wrapper;
pub mod handler;
mod inspector;
mod journaled_state;
#[cfg(feature = "serde-json")]
pub mod occda;
#[cfg(feature = "optimism")]
pub mod optimism;
pub mod ssa_access_tracker;
pub mod task;

// Export items.

pub use builder::EvmBuilder;
pub use context::{
    Context, ContextPrecompile, ContextPrecompiles, ContextStatefulPrecompile,
    ContextStatefulPrecompileArc, ContextStatefulPrecompileBox, ContextStatefulPrecompileMut,
    ContextWithHandlerCfg, EvmContext, InnerEvmContext,
};
pub use db::{
    CacheState, DBBox, State, StateBuilder, StateDBBox, TransitionAccount, TransitionState,
};
pub use db::{Database, DatabaseCommit, DatabaseRef, InMemoryDB};
pub use evm::{Evm, CALL_STACK_LIMIT};
pub use frame::{CallFrame, CreateFrame, Frame, FrameData, FrameOrResult, FrameResult};
pub use handler::Handler;
pub use inspector::{inspector_handle_register, inspectors, GetInspector, Inspector};
pub use journaled_state::{JournalCheckpoint, JournalEntry, JournaledState, ReadWriteSet};
// export Optimism types, helpers, and constants
pub use altius_benchtools::profiler;
#[cfg(feature = "optimism")]
pub use optimism::{L1BlockInfo, BASE_FEE_RECIPIENT, L1_BLOCK_CONTRACT, L1_FEE_RECIPIENT};

// Reexport libraries

#[doc(inline)]
pub use revm_interpreter as interpreter;
#[doc(inline)]
pub use revm_interpreter::primitives;
#[doc(inline)]
pub use revm_precompile as precompile;
