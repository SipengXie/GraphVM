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
pub mod instruction_table;
pub mod instructions;
pub mod tracer;
pub use context::*;
pub use executor::*;
pub use graph::*;
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

    /// Database error
    #[error("Database error: {0}")]
    Database(String),
}

impl ExecutionError {
    pub const EXPECTED_STACK_VALUE: &'static str = "Expected Stack output value";
    pub const EXPECTED_CALL_INPUT: &'static str = "Expected CallInput output value";
    pub const INPUT_MUST_BE_STACK_OR_CONST: &'static str = "Input must be Stack or Constant value";
    pub const EXPECTED_STORAGE_VALUE: &'static str = "Expected Storage output value";
    pub const INPUT_MUST_BE_STORAGE_VALUE: &'static str = "Input must be Storage value";
    pub const EXPECTED_CONTRACT_ENV_VALUE: &'static str = "Expected ContractEnv output value";
    pub const INPUT_MUST_BE_CONTRACT_ENV: &'static str = "Input must be ContractEnv value";
    pub const EXPECTED_MEMORY_VALUE: &'static str = "Expected Memory output value";
    pub const INPUT_MUST_BE_MEMORY_VALUE: &'static str = "Input must be Memory value";
    pub const EXPECTED_RETURN_DATA_BUFFER: &'static str = "Expected ReturnDataBuffer output value";
    pub const INPUT_MUST_BE_RETURN_DATA_BUFFER: &'static str =
        "Input must be ReturnDataBuffer value";
    pub const INPUT_MUST_BE_CALL_INPUT: &'static str = "Input must be CallInput value";
    pub const EXPECTED_INTERPRETER_RESULT: &'static str = "Expected InterpreterResult output value";
    pub const INPUT_MUST_BE_INTERPRETER_RESULT: &'static str =
        "Input must be InterpreterResult value";
    pub const INVALID_BOOLEAN_VALUE: &'static str = "Invalid boolean value";
    pub const INVALID_OPCODE_FOR_RESULT_CHANGE: &'static str =
        "Invalid opcode for instruction result change";
    pub const EXPECTED_GAS_COST: &'static str = "Expected GasCost output value";
    pub const EXPECTED_GAS_REFUND: &'static str = "Expected GasRefund output value";
    pub const EXPECTED_CONSTANT_I64: &'static str = "Expected ConstantI64 output value";
    pub const EXPECTED_TRANSIENT_VALUE: &'static str = "Expected Transient output value";
    pub const STORAGE_KEY_MISMATCH: &'static str = "Storage key mismatch with expected key";
    #[inline(always)]
    pub fn control_flow_not_deterministic(
        node: &impl std::fmt::Debug,
        old_jump: isize,
        new_jump: isize,
    ) -> String {
        format!(
            "Control flow is not deterministic. Node: {:?}, Old jump: {}, New jump: {}",
            node, old_jump, new_jump
        )
    }
}

/// Result type for operations that can fail with an ExecutionError
pub type Result<T> = std::result::Result<T, ExecutionError>;
