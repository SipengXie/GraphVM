//! # revm-ssa-graph
//!
//! A Static Single Assignment (SSA) based analysis tool for the Ethereum Virtual Machine (EVM).
//! This crate provides functionality for constructing and analyzing dependency graphs of EVM bytecode,
//! enabling optimizations such as partial re-execution.
//!
//! ## ⚠️ Development Status
//!
//! This crate is currently under active development and some features may be unstable:
//! * Test coverage is incomplete
//! * Some tests may fail
//! * APIs might change
//! * Performance optimizations are ongoing
//!
//! ## Key Components
//!
//! * [`DependencyGraph`]: Core graph structure for tracking instruction dependencies
//! * [`SSAExecutor`]: Executes EVM bytecode using SSA-based analysis
//! * [`ExecutionTracer`]: Traces execution for analysis and debugging
//!
//! ## Example
//!
//! ```rust,no_run
//! use revm_ssa_graph::{DependencyGraph, SSAExecutor, ExecutionTracer};
//!
//! // Create dependency graph
//! let mut graph = DependencyGraph::new();
//!
//! // Add nodes and edges
//! graph.add_node(entry).unwrap();
//! graph.add_edges(entry.lsn).unwrap();
//!
//! // Create executor
//! let mut executor = SSAExecutor::new(graph, db, env);
//!
//! // Execute
//! executor.execute().unwrap();
//! ```
#[macro_use]
pub mod macros;
pub mod context;
pub mod executor;
pub mod graph;
pub mod instructions;
pub mod tracer;
pub use context::*;
pub use executor::*;
pub use graph::*;
use revm_ssa::SSAOutput;
use auto_impl::auto_impl;
pub use tracer::*;


/// Errors that can occur during SSA graph execution
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    /// Error indicating an invalid dependency in the graph
    #[error("Invalid dependency: {0}")]
    InvalidDependency(String),
    
    /// Error in graph construction or manipulation
    #[error("Graph error: {0}")]
    GraphError(String),
    
    /// Error during execution
    #[error("Execution error: {0}")]
    ExecutionError(String),
}

/// Result type for operations that can fail with an ExecutionError
pub type Result<T> = std::result::Result<T, ExecutionError>; 

#[auto_impl(&mut, Box)]
pub trait SsaDatabaseCommit {
    /// Commit changes to the database.
    fn commit_ssa_storage(&mut self, changes: Vec<SSAOutput>);
}