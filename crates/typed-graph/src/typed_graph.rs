use revm_interpreter::InstructionResult;
use revm_primitives::{AccountInfo, Bytes, U256};

use crate::context::FrameInput;

/// Core trait that all typed nodes must implement
pub trait TypedNode {
    /// Execute the node's operation
    fn execute(&mut self) -> anyhow::Result<()>;
    
    /// Get U256 output at specified index if available
    fn get_u256_output(&self, _index: usize) -> Option<*const U256> {
        None
    }

    fn get_usize_output(&self) -> usize {
        0
    }

    fn get_instruction_result_output(&self) -> InstructionResult {
        InstructionResult::Continue
    }

    fn get_bytes_output(&self) -> Option<*const Bytes> {
        None
    }

    fn get_account_info_output(&self, _index: usize) -> Option<*const AccountInfo> {
        None
    }

    fn get_frame_input_output(&self) -> Option<*const FrameInput> {
        None // Default implementation returns None
    }
}

/// Trait for compile-time input type checking
pub trait HasInputType<T> {}

/// Trait for compile-time output type checking  
pub trait HasOutputType<T> {}

/// The main graph structure holding all nodes
pub struct TypedGraph {
    nodes: Vec<Box<dyn TypedNode>>,
    execution_order: Vec<usize>,
}

impl TypedGraph {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            execution_order: Vec::new(),
        }
    }

    /// Execute all nodes in topological order
    pub fn execute(&mut self) -> anyhow::Result<()> {
        for &idx in &self.execution_order {
            self.nodes[idx].execute()?;
        }
        Ok(())
    }

    /// Add a node to the graph
    pub fn add_node(&mut self, node: Box<dyn TypedNode>) -> usize {
        let idx = self.nodes.len();
        self.nodes.push(node);
        self.execution_order.push(idx);
        idx
    }
}