use crate::context::{ExternalContext, FrameContext};
use crate::instructions::arithmetic::{AddNode, DivNode, MulNode, SubNode};
use crate::instructions::contract::*;
use crate::instructions::control::StopInvalidNode;
use crate::instructions::host_env::{ChainIdNode, CoinbaseNode, TimestampNode};
use crate::instructions::memory::{MloadNode, MstoreNode};
use crate::typed_graph::TypedGraph;
use core::panic;
use revm_interpreter::{InstructionResult, SharedMemory};
use revm_primitives::{AccountInfo, Bytes, Env, U256, U256_ONE};
use revm_ssa::logger::LsnType;
use revm_ssa::{FrameInput, SSAInput, SSALogEntry};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Constant pool for storing constant values used in TypedNodes
pub struct ConstantPool {
    /// Storage for U256 constants
    u256_constants: Vec<U256>,
}

impl ConstantPool {
    pub fn new() -> Self {
        Self {
            u256_constants: Vec::new(),
        }
    }

    /// Add a U256 constant and return its pointer
    pub fn add_u256(&mut self, value: U256) -> *const U256 {
        self.u256_constants.push(value);
        &self.u256_constants[self.u256_constants.len() - 1] as *const U256
    }
}

/// Converts SSA log entries to a TypedGraph
pub struct SsaConverter {
    /// The TypedGraph being built
    graph: TypedGraph,
    /// Mapping from LSN to node index in the TypedGraph (LSN is 1-based, index is 0-based)
    lsn_to_node: HashMap<LsnType, usize>,
    /// Constant pool for storing constant values
    constant_pool: ConstantPool,
    /// Execution context (shared between nodes)
    context: Rc<RefCell<ExternalContext>>,
    /// Shared memory (shared between nodes)
    shared_memory: Rc<RefCell<SharedMemory>>,
    /// Environment pointer
    env: *const Env,
    /// First frame input
    first_frame_input: *const FrameInput,
}

impl SsaConverter {
    /// Create a new SsaConverter with execution context
    pub fn new(
        context: Rc<RefCell<ExternalContext>>,
        shared_memory: Rc<RefCell<SharedMemory>>,
        env: *const Env,
        first_frame_input: *const FrameInput,
    ) -> Self {
        Self {
            graph: TypedGraph::new(),
            lsn_to_node: HashMap::new(),
            constant_pool: ConstantPool::new(),
            context,
            shared_memory,
            env,
            first_frame_input,
        }
    }

    /// Get U256 pointer from SSA input
    fn get_u256_ptr(&mut self, input: &SSAInput) -> *const U256 {
        match input {
            SSAInput::Constant(value) => {
                // For constants, store in constant pool and return pointer
                self.constant_pool.add_u256(value.clone())
            }
            SSAInput::Stack(lsn_with_index) => {
                // For stack values, get the node and its U256 output pointer
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_u256_output()
            }
            // Add other input types as needed
            _ => panic!("Unsupported SSA input type"),
        }
    }

    /// Convert SSA log entries to TypedGraph
    pub fn convert(&mut self, entries: Vec<SSALogEntry>) -> (TypedGraph, ConstantPool) {
        eprintln!("Logs to convert: {:#?}", entries);
        // First pass: Create nodes for each entry
        for entry in entries.iter() {
            let node_index = self.create_node_for_entry(entry);
            self.lsn_to_node.insert(entry.lsn, node_index);
        }

        // Take ownership of graph and constant pool
        (
            std::mem::replace(&mut self.graph, TypedGraph::new()),
            std::mem::replace(&mut self.constant_pool, ConstantPool::new()),
        )
    }

    /// Create a TypedNode based on the SSA log entry's opcode
    fn create_node_for_entry(&mut self, entry: &SSALogEntry) -> usize {
        match entry.opcode {
            0x00 => {
                // STOP
                let result = InstructionResult::Stop;
                let node = StopInvalidNode::new(result);
                self.graph.add_node(Box::new(node))
            }
            // Arithmetic operations
            0x01 => {
                // ADD
                let node = AddNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            }
            0x03 => {
                // SUB
                let node = SubNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            }
            0x02 => {
                // MUL
                let node = MulNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            }
            0x04 => {
                // DIV
                let node = DivNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            }

            // Memory operations
            0x51 => {
                // MLOAD
                let node = MloadNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.shared_memory.clone(),
                );
                self.graph.add_node(Box::new(node))
            }
            0x52 => {
                // MSTORE
                let node = MstoreNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                    self.shared_memory.clone(),
                );
                self.graph.add_node(Box::new(node))
            }

            // Host environment operations
            0x46 => {
                // CHAINID
                let node = ChainIdNode::new(self.env);
                self.graph.add_node(Box::new(node))
            }
            0x41 => {
                // COINBASE
                let node = CoinbaseNode::new(self.env);
                self.graph.add_node(Box::new(node))
            }
            0x42 => {
                // TIMESTAMP
                let node = TimestampNode::new(self.env);
                self.graph.add_node(Box::new(node))
            }

            // Create operations
            0xD4 => {
                // MAKE_CREATE_FRAME
                let frame_input = self.get_frame_input_ptr(&entry.inputs[0]);
                let account_info = self.get_account_info_ptr(&entry.inputs[1]);
                let node =
                    MakeCreateFrameNode::new(frame_input, account_info, Some(self.context.clone()));
                self.graph.add_node(Box::new(node))
            }
            0xD5 => {
                // CREATE_RETURN

                // In the SsaGraph implementation, result and return_data are placed together, so they are within the same node
                let result = self.get_instruction_result(&entry.inputs[0]);
                let return_data = self.get_bytes_ptr(&entry.inputs[0]);

                let frame_context = self.get_frame_context_ptr(&entry.inputs[1]);
                let account_info = self.get_account_info_ptr(&entry.inputs[2]);

                let analyze_code = self.get_bool(&entry.inputs[3]);

                let node = CreateReturnNode::new(
                    result,
                    return_data,
                    frame_context,
                    Some(self.context.clone()),
                    account_info,
                    Some(analyze_code),
                );
                self.graph.add_node(Box::new(node))
            }

            0xD7 => {
                // MAKE_CALL_FRAME
                // Based on revm-ssa-graph's execute_make_call_frame inputs:
                // inputs[0]: FrameInput
                // inputs[1]: Storage(caller_info)
                // inputs[2]: Storage(target_info)
                // inputs[3]: Storage(bytecode_info)

                let frame_input_ptr = self.get_frame_input_ptr(&entry.inputs[0]);
                let caller_info_ptr = self.get_account_info_ptr(&entry.inputs[1]);
                let target_info_ptr = self.get_account_info_ptr(&entry.inputs[2]);
                let bytecode_info_ptr = self.get_account_info_ptr(&entry.inputs[3]);

                let node = MakeCallFrameNode::new(
                    frame_input_ptr,
                    caller_info_ptr,
                    target_info_ptr,
                    bytecode_info_ptr,
                    self.context.clone(),
                );
                self.graph.add_node(Box::new(node))
            }

            0xD8 => {
                // CALL_RETURN
                // Based on revm-ssa-graph's execute_call_return inputs:
                // inputs[0]: InterpreterResult
                // inputs[1]: ContractEnv

                // Get InstructionResult value from the source node's output
                let result_value = self.get_instruction_result(&entry.inputs[0]);

                // Get Bytes pointer from the *same* source node's output
                let return_data_ptr = self
                    .get_bytes_ptr(&entry.inputs[0])
                    .expect("Failed to get bytes pointer for CALL_RETURN"); // Expect Some

                // Get FrameContext pointer from ContractEnv input
                let frame_context_ptr = self
                    .get_frame_context_ptr(&entry.inputs[1])
                    .expect("Failed to get frame context pointer for CALL_RETURN"); // Expect Some

                let node = CallReturnNode::new(
                    result_value, // Pass the actual value
                    return_data_ptr,
                    frame_context_ptr,
                );
                self.graph.add_node(Box::new(node))
            }

            0xDA => {
                // DEDUCT_CALLER
                // Input 0: Caller Address (as U256) - Node needs to handle conversion
                let caller_u256_ptr = self.get_u256_ptr(&entry.inputs[0]);

                // Input 1: Is Create (bool from U256 Constant)
                let is_create = self.get_bool(&entry.inputs[1]);

                // Input 2: Cost (U256)
                let cost_ptr = self.get_u256_ptr(&entry.inputs[2]);

                let node = DeductCallerNode::new(
                    // Passing *const U256, DeductCallerNode must adapt.
                    // Alternatively, SsaConverter could manage an Address pool.
                    caller_u256_ptr,
                    is_create,
                    cost_ptr,
                    self.context.clone(),
                );
                self.graph.add_node(Box::new(node))
            }

            0xDB => {
                let node = RefundGasNode::new();
                self.graph.add_node(Box::new(node))
            }
            // TODO: Add more opcodes...
            _ => {
                panic!("Unimplemented opcode: 0x{:02x}", entry.opcode);
            }
        }
    }

    /// Gets a boolean value from a constant SSAInput
    fn get_bool(&self, input: &SSAInput) -> bool {
        match input {
            SSAInput::Constant(value) => *value == U256_ONE, // Assumes 1 is true
            _ => panic!("Expected Constant SSAInput for boolean, got {:?}", input),
        }
    }

    // Helper methods for getting typed pointers
    fn get_frame_input_ptr(&mut self, input: &SSAInput) -> *const FrameInput {
        match input {
            SSAInput::FrameInput(lsn_with_index) => {
                if lsn_with_index.0 == 0 {
                    self.first_frame_input
                } else {
                    let node_index = self.lsn_to_node[&lsn_with_index.0];
                    let node = self.graph.get_node(node_index);
                    node.get_frame_input_output().unwrap_or_else(|| {
                        panic!("Node {} does not provide FrameInput output", node_index)
                    })
                }
            }
            _ => panic!(
                "Expected FrameInput SSAInput for frame_input, got {:?}",
                input
            ),
        }
    }

    fn get_account_info_ptr(&mut self, input: &SSAInput) -> Option<*const AccountInfo> {
        match input {
            SSAInput::Storage(lsn_with_index) => {
                if lsn_with_index.0 == 0 {
                    None // let the node.execute() to get the account.
                } else {
                    let node_index = self.lsn_to_node[&lsn_with_index.0];
                    let node = self.graph.get_node(node_index);
                    node.get_account_info_output(lsn_with_index.1 as usize)
                }
            }
            _ => panic!("Expected Storage input for account_info, got {:?}", input),
        }
    }

    fn get_instruction_result(&mut self, input: &SSAInput) -> *const InstructionResult {
        match input {
            SSAInput::InterpreterResult(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_instruction_result_output()
            }
            _ => panic!(
                "Expected InterpreterResult input for instruction_result, got {:?}",
                input
            ),
        }
    }

    fn get_bytes_ptr(&mut self, input: &SSAInput) -> Option<*const Bytes> {
        match input {
            SSAInput::ReturnDataBuffer(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_bytes_output()
            }
            SSAInput::InterpreterResult(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_bytes_output()
            }
            _ => panic!("Expected ReturnDataBuffer input for bytes, got {:?}", input),
        }
    }

    fn get_frame_context_ptr(&mut self, input: &SSAInput) -> Option<*const FrameContext> {
        match input {
            SSAInput::ContractEnv(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_frame_context_output()
            }
            _ => panic!("Expected Stack input for frame_context, got {:?}", input),
        }
    }
}
