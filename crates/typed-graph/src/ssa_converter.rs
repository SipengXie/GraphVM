use crate::context::{CallOutcome, CreateOutcome, ExternalContext, FrameContext};
use crate::instructions::arithmetic::{AddNode, DivNode, MulNode, SubNode, ModNode, AddModNode, MulModNode, ExpNode, SignExtendNode};
use crate::instructions::bitwise::{LtNode, GtNode, SltNode, SgtNode, EqNode, IsZeroNode, AndNode, OrNode, XorNode, NotNode, ByteNode, ShlNode, ShrNode, SarNode};
use crate::instructions::contract::{
    CallNode, CallcodeNode, DelegatecallNode, StaticcallNode, CreateNode, Create2Node, // Call/Create Initiation
    MakeCallFrameNode, CallReturnNode, InsertCallOutcomeNode, // Call Frame Management
    MakeCreateFrameNode, CreateReturnNode, InsertCreateOutcomeNode, // Create Frame Management
    DeductCallerNode, RefundGasNode // Gas/State Management Nodes
};
use crate::instructions::control::{JumpNode, JumpiNode, ReturnRevertNode, StopInvalidNode};
use crate::instructions::host_env::{
    ChainIdNode, CoinbaseNode, TimestampNode, NumberNode, DifficultyNode, GasLimitNode, 
    GasPriceNode, BaseFeeNode, OriginNode, BlobBaseFeeNode, BlobHashNode
};
use crate::instructions::memory::{MloadNode, MstoreNode, Mstore8Node, MsizeNode, McopyNode};
use crate::instructions::host::{ SloadNode, SstoreNode, BalanceNode, ExtcodesizeNode, ExtcodehashNode, BlockhashNode };
use crate::instructions::system::{ GasNode, AddressNode, CallerNode, CodesizeNode, CodecopyNode, CalldataloadNode, CalldatasizeNode, CallvalueNode, CalldatacopyNode, ReturndatasizeNode, ReturndatacopyNode, Keccak256Node };
use crate::typed_graph::TypedGraph;
use core::panic;
use revm_interpreter::{InstructionResult, SharedMemory};
use revm_primitives::{AccountInfo, Bytes, Env, U256, U256_ONE, HashMap};
use revm_ssa::logger::LsnType;
use revm_ssa::{FrameInput, SSAInput, SSALogEntry}; // Added SSAOutcome types
use std::cell::RefCell;
use std::rc::Rc;

/// Constant pool for storing constant values used in TypedNodes, ensuring uniqueness.
pub struct ConstantPool {
    /// Storage for unique U256 constants
    u256_constants: Vec<U256>,
    /// Map from U256 value to its index in u256_constants
    value_to_index: HashMap<U256, usize>,
}

impl ConstantPool {
    pub fn new() -> Self {
        Self {
            u256_constants: Vec::new(),
            value_to_index: HashMap::default(), // Initialize the HashMap
        }
    }

    /// Add a U256 constant if it doesn't exist, and return its pointer.
    /// Ensures that identical U256 values are stored only once.
    pub fn add_u256(&mut self, value: U256) -> *const U256 {
        // Check if the value already exists in the map
        if let Some(&index) = self.value_to_index.get(&value) {
            // If exists, return pointer to the existing value in the vector
            &self.u256_constants[index] as *const U256
        } else {
            // If not exists, add the value to the vector
            let index = self.u256_constants.len();
            self.u256_constants.push(value.clone()); // Clone the value for the vector
            // Add the value and its index to the map
            self.value_to_index.insert(value, index); // Take ownership of the original value for the map
            // Return pointer to the newly added value
            &self.u256_constants[index] as *const U256
        }
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
        // eprintln!("Logs to convert: {:#?}", entries);
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
            // Control Flow & Halting Operations
            0x00 => { // STOP
                let node = StopInvalidNode::new(InstructionResult::Stop);
                self.graph.add_node(Box::new(node))
            },
            0x56 => { // JUMP
                let target_pc_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let node = JumpNode::new(target_pc_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x57 => { // JUMPI
                let target_pc_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let condition_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let node = JumpiNode::new(target_pc_ptr, condition_ptr);
                self.graph.add_node(Box::new(node))
            },
            0xf3 => { // RETURN
                let offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[1]);
                // Input[2] in revm-ssa-graph is SSAInput::Memory, but ReturnRevertNode takes Rc<RefCell<SharedMemory>>
                let node = ReturnRevertNode::new(
                    offset_ptr,
                    len_ptr,
                    self.shared_memory.clone(),
                    InstructionResult::Return, // RETURN indicates success
                );
                self.graph.add_node(Box::new(node))
            },
            0xfd => { // REVERT
                let offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[1]);
                // Input[2] is memory state, node takes Rc<RefCell<SharedMemory>>
                let node = ReturnRevertNode::new(
                    offset_ptr,
                    len_ptr,
                    self.shared_memory.clone(),
                    InstructionResult::Revert, // REVERT indicates failure
                );
                self.graph.add_node(Box::new(node))
            },
            0xfe => { // INVALID
                let node = StopInvalidNode::new(InstructionResult::InvalidFEOpcode);
                self.graph.add_node(Box::new(node))
            },

            // Arithmetic operations
            0x01 => {
                // ADD
                let node = AddNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x03 => {
                // SUB
                let node = SubNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x02 => {
                // MUL
                let node = MulNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x04 => {
                // DIV
                let node = DivNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x06 => {
                // MOD
                let node = ModNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x08 => {
                // ADDMOD
                let node = AddModNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                    self.get_u256_ptr(&entry.inputs[2]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x09 => {
                // MULMOD
                let node = MulModNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                    self.get_u256_ptr(&entry.inputs[2]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x0a => {
                // EXP
                let node = ExpNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x0b => {
                // SIGNEXTEND
                let node = SignExtendNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },

            // Bitwise operations
            0x10 => { // LT
                let node = LtNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x11 => { // GT
                let node = GtNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x12 => { // SLT
                let node = SltNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x13 => { // SGT
                let node = SgtNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x14 => { // EQ
                let node = EqNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x15 => { // ISZERO
                let node = IsZeroNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x16 => { // AND
                let node = AndNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x17 => { // OR
                let node = OrNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x18 => { // XOR
                let node = XorNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                    self.get_u256_ptr(&entry.inputs[1]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x19 => { // NOT
                let node = NotNode::new(
                    self.get_u256_ptr(&entry.inputs[0]),
                );
                self.graph.add_node(Box::new(node))
            },
            0x1a => { // BYTE
                let node = ByteNode::new(
                    self.get_u256_ptr(&entry.inputs[0]), // index
                    self.get_u256_ptr(&entry.inputs[1]), // word
                );
                self.graph.add_node(Box::new(node))
            },
            0x1b => { // SHL
                let node = ShlNode::new(
                    self.get_u256_ptr(&entry.inputs[0]), // shift
                    self.get_u256_ptr(&entry.inputs[1]), // value
                );
                self.graph.add_node(Box::new(node))
            },
            0x1c => { // SHR
                let node = ShrNode::new(
                    self.get_u256_ptr(&entry.inputs[0]), // shift
                    self.get_u256_ptr(&entry.inputs[1]), // value
                );
                self.graph.add_node(Box::new(node))
            },
            0x1d => { // SAR
                let node = SarNode::new(
                    self.get_u256_ptr(&entry.inputs[0]), // shift
                    self.get_u256_ptr(&entry.inputs[1]), // value
                );
                self.graph.add_node(Box::new(node))
            },

            // Memory operations
            0x51 => {
                // MLOAD
                let offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let node = MloadNode::new(offset_ptr, self.shared_memory.clone());
                self.graph.add_node(Box::new(node))
            },
            0x52 => {
                // MSTORE
                let offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let value_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let node = MstoreNode::new(offset_ptr, value_ptr, self.shared_memory.clone());
                self.graph.add_node(Box::new(node))
            },
            0x53 => { // MSTORE8
                let offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let value_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let node = Mstore8Node::new(offset_ptr, value_ptr, self.shared_memory.clone());
                self.graph.add_node(Box::new(node))
            },
            0x59 => { // MSIZE
                // TypedNode directly reads from SharedMemory, SSA input[0] is not needed here.
                let node = MsizeNode::new(self.shared_memory.clone());
                self.graph.add_node(Box::new(node))
            },
            0x5f => { // MCOPY
                let dst_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let src_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[2]);
                let node = McopyNode::new(dst_ptr, src_ptr, len_ptr, self.shared_memory.clone());
                self.graph.add_node(Box::new(node))
            },

            // Host environment operations
            0x46 => {
                // CHAINID
                let node = ChainIdNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x43 => { // NUMBER
                let node = NumberNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x44 => { // DIFFICULTY / PREVRANDAO
                let node = DifficultyNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x45 => { // GASLIMIT
                let node = GasLimitNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x3a => { // GASPRICE
                let node = GasPriceNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x48 => { // BASEFEE
                let node = BaseFeeNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x32 => { // ORIGIN
                let node = OriginNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x4a => { // BLOBBASEFEE
                let node = BlobBaseFeeNode::new(self.env);
                self.graph.add_node(Box::new(node))
            },
            0x4f => { // BLOBHASH
                let index_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let node = BlobHashNode::new(index_ptr, self.env);
                self.graph.add_node(Box::new(node))
            },

            // Contract Operations
            0xF0 => { // CREATE
                let value_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let code_offset_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[2]);
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[3])
                                    .expect("CREATE needs FrameContext");
                let node = CreateNode::new(
                    value_ptr,
                    code_offset_ptr,
                    len_ptr,
                    self.shared_memory.clone(),
                    frame_ptr,
                );
                self.graph.add_node(Box::new(node))
            },
            0xF1 => { // CALL
                let gas_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let address_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let value_ptr = self.get_u256_ptr(&entry.inputs[2]);
                let args_offset_ptr = self.get_u256_ptr(&entry.inputs[3]);
                let args_size_ptr = self.get_u256_ptr(&entry.inputs[4]);
                let ret_offset_ptr = self.get_u256_ptr(&entry.inputs[5]);
                let ret_size_ptr = self.get_u256_ptr(&entry.inputs[6]);
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[7])
                                    .expect("CALL needs FrameContext");
                let node = CallNode::new((
                    gas_ptr,
                    address_ptr,
                    value_ptr,
                    args_offset_ptr,
                    args_size_ptr,
                    ret_offset_ptr,
                    ret_size_ptr,
                    self.shared_memory.clone(),
                    frame_ptr,
                ));
                self.graph.add_node(Box::new(node))
            },
            0xF2 => { // CALLCODE
                 let gas_ptr = self.get_u256_ptr(&entry.inputs[0]);
                 let address_ptr = self.get_u256_ptr(&entry.inputs[1]);
                 let value_ptr = self.get_u256_ptr(&entry.inputs[2]);
                 let args_offset_ptr = self.get_u256_ptr(&entry.inputs[3]);
                 let args_size_ptr = self.get_u256_ptr(&entry.inputs[4]);
                 let ret_offset_ptr = self.get_u256_ptr(&entry.inputs[5]);
                 let ret_size_ptr = self.get_u256_ptr(&entry.inputs[6]);
                 let frame_ptr = self.get_frame_context_ptr(&entry.inputs[7])
                                     .expect("CALLCODE needs FrameContext");
                 let node = CallcodeNode::new((
                     gas_ptr,
                     address_ptr,
                     value_ptr,
                     args_offset_ptr,
                     args_size_ptr,
                     ret_offset_ptr,
                     ret_size_ptr,
                     self.shared_memory.clone(),
                     frame_ptr,
                 ));
                 self.graph.add_node(Box::new(node))
            },
             0xF4 => { // DELEGATECALL
                 let gas_ptr = self.get_u256_ptr(&entry.inputs[0]);
                 let address_ptr = self.get_u256_ptr(&entry.inputs[1]);
                 // No value input for DELEGATECALL
                 let args_offset_ptr = self.get_u256_ptr(&entry.inputs[2]);
                 let args_size_ptr = self.get_u256_ptr(&entry.inputs[3]);
                 let ret_offset_ptr = self.get_u256_ptr(&entry.inputs[4]);
                 let ret_size_ptr = self.get_u256_ptr(&entry.inputs[5]);
                 let frame_ptr = self.get_frame_context_ptr(&entry.inputs[6]) // Index shift
                                     .expect("DELEGATECALL needs FrameContext");
                 let node = DelegatecallNode::new((
                     gas_ptr,
                     address_ptr,
                     args_offset_ptr,
                     args_size_ptr,
                     ret_offset_ptr,
                     ret_size_ptr,
                     self.shared_memory.clone(),
                     frame_ptr,
                 ));
                 self.graph.add_node(Box::new(node))
            },
             0xF5 => { // CREATE2
                 let value_ptr = self.get_u256_ptr(&entry.inputs[0]);
                 let code_offset_ptr = self.get_u256_ptr(&entry.inputs[1]);
                 let len_ptr = self.get_u256_ptr(&entry.inputs[2]);
                 let salt_ptr = self.get_u256_ptr(&entry.inputs[4]); // Salt is input[4] in revm-ssa-graph
                 let frame_ptr = self.get_frame_context_ptr(&entry.inputs[3]) // Frame is input[3]
                                     .expect("CREATE2 needs FrameContext");
                 let node = Create2Node::new(
                     value_ptr,
                     code_offset_ptr,
                     len_ptr,
                     salt_ptr,
                     self.shared_memory.clone(),
                     frame_ptr,
                 );
                 self.graph.add_node(Box::new(node))
            },
             0xFA => { // STATICCALL
                 let gas_ptr = self.get_u256_ptr(&entry.inputs[0]);
                 let address_ptr = self.get_u256_ptr(&entry.inputs[1]);
                  // No value input for STATICCALL
                 let args_offset_ptr = self.get_u256_ptr(&entry.inputs[2]);
                 let args_size_ptr = self.get_u256_ptr(&entry.inputs[3]);
                 let ret_offset_ptr = self.get_u256_ptr(&entry.inputs[4]);
                 let ret_size_ptr = self.get_u256_ptr(&entry.inputs[5]);
                 let frame_ptr = self.get_frame_context_ptr(&entry.inputs[6]) // Index shift
                                     .expect("STATICCALL needs FrameContext");
                 let node = StaticcallNode::new((
                     gas_ptr,
                     address_ptr,
                     args_offset_ptr,
                     args_size_ptr,
                     ret_offset_ptr,
                     ret_size_ptr,
                     self.shared_memory.clone(),
                     frame_ptr,
                 ));
                 self.graph.add_node(Box::new(node))
            },

             // --- Frame Management & Outcome Handling ---
            0xD0 => { // DEDUCT_CALLER (Placeholder opcode)
                 let caller_u256_ptr = self.get_u256_ptr(&entry.inputs[0]);
                 let is_create = self.get_bool(&entry.inputs[1]);
                 let cost_ptr = self.get_u256_ptr(&entry.inputs[2]);
                 let node = DeductCallerNode::new(
                     caller_u256_ptr,
                     is_create,
                     cost_ptr,
                     self.context.clone(),
                 );
                 self.graph.add_node(Box::new(node))
             },
            0xD4 => { // MAKE_CREATE_FRAME
                let frame_input_ptr = self.get_frame_input_ptr(&entry.inputs[0]);
                let caller_info_ptr = self.get_account_info_ptr(&entry.inputs[1]);
                let node = MakeCreateFrameNode::new(
                    frame_input_ptr,
                    caller_info_ptr,
                    Some(self.context.clone()),
                );
                self.graph.add_node(Box::new(node))
            },
            0xD5 => { // CREATE_RETURN
                let result_input = &entry.inputs[0];
                let result_ptr = self.get_instruction_result_ptr(result_input);
                let output_bytes_ptr = self.get_bytes_ptr(result_input);
                let frame_context_ptr = self.get_frame_context_ptr(&entry.inputs[1]);
                let target_info_ptr = self.get_account_info_ptr(&entry.inputs[2]);
                let analyze_code = self.get_bool(&entry.inputs[3]);

                let node = CreateReturnNode::new(
                    result_ptr,
                    output_bytes_ptr,
                    frame_context_ptr,
                    Some(self.context.clone()),
                    target_info_ptr,
                    Some(analyze_code),
                );
                self.graph.add_node(Box::new(node))
            },
            0xD6 => { // INSERT_CREATE_OUTCOME (Placeholder opcode)
                let outcome_ptr = self.get_create_outcome_ptr(&entry.inputs[0])
                                     .expect("INSERT_CREATE_OUTCOME needs CreateOutcome");
                let node = InsertCreateOutcomeNode::new(outcome_ptr);
                self.graph.add_node(Box::new(node))
            },
            0xD7 => { // MAKE_CALL_FRAME (Opcode used before, adjusted based on analysis)
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
             },
             0xD8 => { // CALL_RETURN (Opcode used before, adjusted based on analysis)
                 let result_input = &entry.inputs[0];
                 let result_value = self.get_instruction_result_ptr(result_input);
                 let return_data_ptr = self.get_bytes_ptr(result_input)
                     .expect("CALL_RETURN needs Bytes pointer");
                 let frame_context_ptr = self.get_frame_context_ptr(&entry.inputs[1])
                     .expect("CALL_RETURN needs FrameContext pointer");
                 let node = CallReturnNode::new(
                     result_value, // Pass the value
                     return_data_ptr,
                     frame_context_ptr,
                 );
                 self.graph.add_node(Box::new(node))
             },
            0xD9 => { // INSERT_CALL_OUTCOME (Placeholder opcode)
                let outcome_ptr = self.get_call_outcome_ptr(&entry.inputs[0])
                                      .expect("INSERT_CALL_OUTCOME needs CallOutcome");
                // Original frame context needed for ret_range
                let original_frame_ptr = self.get_frame_context_ptr(&entry.inputs[1])
                                            .expect("INSERT_CALL_OUTCOME needs original FrameContext");

                let node = InsertCallOutcomeNode::new(
                    outcome_ptr,
                    self.shared_memory.clone(),
                    original_frame_ptr
                );
                self.graph.add_node(Box::new(node))
            },
            0xDB => { // REFUND_GAS (Placeholder opcode)
                let node = RefundGasNode::new(); // Assuming RefundGasNode exists
                self.graph.add_node(Box::new(node))
            },

            // Host Operations (Storage, Account Info, Block Info)
            0x54 => { // SLOAD
                let address_u256_ptr = self.get_address_u256_from_contract_env(&entry.inputs[0]);
                let index_ptr = self.get_u256_ptr(&entry.inputs[1]);

                let (value_ptr, context_ref) = match &entry.inputs[2] {
                    SSAInput::Storage(lsn_with_index) if lsn_with_index.0 != 0 => {
                        let node_index = self.lsn_to_node[&lsn_with_index.0];
                        let node = self.graph.get_node(node_index);
                        // SLOAD's input value comes from the output of a previous SLOAD/SSTORE
                        let ptr = node.get_u256_output(); // Assuming previous node output is U256
                        (Some(ptr), None)
                    }
                    SSAInput::Storage(lsn_with_index) if lsn_with_index.0 == 0 => {
                        // Value comes from initial state (database)
                        (None, Some(self.context.clone()))
                    }
                    _ => panic!("SLOAD input[2] must be Storage, got {:?}", entry.inputs[2]),
                };
                // Ignore inputs[3] (AccountStatus) for now, as TypedNode doesn't use it.

                let node = SloadNode::new(address_u256_ptr, index_ptr, value_ptr, context_ref);
                self.graph.add_node(Box::new(node))
            },
            0x55 => { // SSTORE
                let address_u256_ptr = self.get_address_u256_from_contract_env(&entry.inputs[0]);
                let index_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let new_value_ptr = self.get_u256_ptr(&entry.inputs[2]);
                // Inputs 3, 4, 5 (origin_value, present_value, is_read) are ignored by the simplified SstoreNode

                let node = SstoreNode::new(
                    address_u256_ptr, 
                    index_ptr, 
                    new_value_ptr, 
                    Some(self.context.clone()) // Pass context for potential future state update logic
                );
                self.graph.add_node(Box::new(node))
            },
            0x31 => { // BALANCE
                let address_u256_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let (info_ptr, context_ref) = self.get_account_info_source(&entry.inputs[1]);
                let node = BalanceNode::new(address_u256_ptr, info_ptr, context_ref);
                self.graph.add_node(Box::new(node))
            },
             0x3b => { // EXTCODESIZE
                let address_u256_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let (info_ptr, context_ref) = self.get_account_info_source(&entry.inputs[1]);
                let node = ExtcodesizeNode::new(address_u256_ptr, info_ptr, context_ref);
                self.graph.add_node(Box::new(node))
            },
            0x3f => { // EXTCODEHASH
                let address_u256_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let (info_ptr, context_ref) = self.get_account_info_source(&entry.inputs[1]);
                let node = ExtcodehashNode::new(address_u256_ptr, info_ptr, context_ref);
                self.graph.add_node(Box::new(node))
            },
            0x40 => { // BLOCKHASH
                let number_ptr = self.get_u256_ptr(&entry.inputs[0]);
                // Get current block number from env and add to constant pool for stable pointer
                let current_block_number = unsafe { (*self.env).block.number };
                let current_block_number_ptr = self.constant_pool.add_u256(current_block_number);

                let node = BlockhashNode::new(
                    number_ptr, 
                    self.context.clone(), 
                    current_block_number_ptr
                );
                self.graph.add_node(Box::new(node))
            },

            // System Operations
            0x5a => { // GAS
                let gas_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let node = GasNode::new(gas_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x30 => { // ADDRESS
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[0])
                                    .expect("ADDRESS needs FrameContext");
                let node = AddressNode::new(frame_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x33 => { // CALLER
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[0])
                                    .expect("CALLER needs FrameContext");
                let node = CallerNode::new(frame_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x38 => { // CODESIZE
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[0])
                                    .expect("CODESIZE needs FrameContext");
                let node = CodesizeNode::new(frame_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x39 => { // CODECOPY
                let mem_offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let code_offset_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[2]);
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[3])
                                    .expect("CODECOPY needs FrameContext");
                let node = CodecopyNode::new(
                    mem_offset_ptr, 
                    code_offset_ptr, 
                    len_ptr, 
                    frame_ptr, 
                    self.shared_memory.clone()
                );
                self.graph.add_node(Box::new(node))
            },
            0x35 => { // CALLDATALOAD
                let offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[1])
                                    .expect("CALLDATALOAD needs FrameContext");
                let node = CalldataloadNode::new(offset_ptr, frame_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x36 => { // CALLDATASIZE
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[0])
                                    .expect("CALLDATASIZE needs FrameContext");
                let node = CalldatasizeNode::new(frame_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x34 => { // CALLVALUE
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[0])
                                    .expect("CALLVALUE needs FrameContext");
                let node = CallvalueNode::new(frame_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x37 => { // CALLDATACOPY
                let mem_offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let data_offset_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[2]);
                let frame_ptr = self.get_frame_context_ptr(&entry.inputs[3])
                                    .expect("CALLDATACOPY needs FrameContext");
                let node = CalldatacopyNode::new(
                    mem_offset_ptr, 
                    data_offset_ptr, 
                    len_ptr, 
                    frame_ptr, 
                    self.shared_memory.clone()
                );
                self.graph.add_node(Box::new(node))
            },
            0x3d => { // RETURNDATASIZE
                 let return_data_ptr = self.get_bytes_ptr(&entry.inputs[0])
                                           .expect("RETURNDATASIZE needs Bytes pointer");
                let node = ReturndatasizeNode::new(return_data_ptr);
                self.graph.add_node(Box::new(node))
            },
            0x3e => { // RETURNDATACOPY
                let mem_offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let data_offset_ptr = self.get_u256_ptr(&entry.inputs[1]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[2]);
                let return_data_ptr = self.get_bytes_ptr(&entry.inputs[3])
                                          .expect("RETURNDATACOPY needs Bytes pointer");
                let node = ReturndatacopyNode::new(
                    mem_offset_ptr, 
                    data_offset_ptr, 
                    len_ptr, 
                    return_data_ptr, 
                    self.shared_memory.clone()
                );
                self.graph.add_node(Box::new(node))
            },
            0x20 => { // KECCAK256
                let offset_ptr = self.get_u256_ptr(&entry.inputs[0]);
                let len_ptr = self.get_u256_ptr(&entry.inputs[1]);
                // Input[2] in revm-ssa-graph is SSAInput::Memory, but Keccak256Node takes Rc<RefCell<SharedMemory>>
                let node = Keccak256Node::new(offset_ptr, len_ptr, self.shared_memory.clone());
                self.graph.add_node(Box::new(node))
            },

            // ... other opcodes ...
            _ => {
                println!("Warning: Unimplemented opcode during conversion: 0x{:02x}", entry.opcode);
                // panic!("Unimplemented opcode: 0x{:02x}", entry.opcode);
                // Add a dummy node or skip? Adding a placeholder might be better.
                // For now, let's skip adding a node for unimplemented opcodes.
                // Returning usize::MAX might indicate skipping.
                 usize::MAX // Indicate that no node was added (or handle differently)
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

    fn get_instruction_result_ptr(&mut self, input: &SSAInput) -> *const InstructionResult {
        match input {
            SSAInput::InterpreterResult(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_instruction_result_output()
            }
            _ => panic!("Expected InterpreterResult input for instruction_result pointer, got {:?}", input),
        }
    }

    fn get_bytes_ptr(&mut self, input: &SSAInput) -> Option<*const Bytes> {
        match input {
            SSAInput::InterpreterResult(lsn_with_index) | SSAInput::ReturnDataBuffer(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_bytes_output()
            }
            // Allow fetching Bytes from CallOutcome as well, as CallReturnNode outputs it
            SSAInput::CallOutcome(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                node.get_bytes_output() // Assumes CallOutcome nodes also provide get_bytes_output
            }
            _ => panic!("Expected InterpreterResult, ReturnDataBuffer, or CallOutcome input for bytes pointer, got {:?}", input),
        }
    }

    fn get_frame_context_ptr(&mut self, input: &SSAInput) -> Option<*const FrameContext> {
        match input {
            SSAInput::ContractEnv(lsn_with_index) => {
                if lsn_with_index.0 == 0 {
                    panic!("Cannot get FrameContext pointer from initial state (LSN 0 ContractEnv)");
                } else {
                    let node_index = self.lsn_to_node[&lsn_with_index.0];
                    let node = self.graph.get_node(node_index);
                    node.get_frame_context_output()
                }
            }
            _ => panic!("Expected ContractEnv input for frame_context pointer, got {:?}", input),
        }
    }

    /// Gets a pointer to CallOutcome from a node outputting CallOutcome.
    fn get_call_outcome_ptr(&mut self, input: &SSAInput) -> Option<*const CallOutcome> {
        match input {
            SSAInput::CallOutcome(lsn_with_index) => {
                if lsn_with_index.0 == 0 {
                    panic!("Cannot get CallOutcome pointer from initial state (LSN 0)");
                } else {
                    let node_index = self.lsn_to_node[&lsn_with_index.0];
                    let node = self.graph.get_node(node_index);
                    // Assumes node implements get_call_outcome_output()
                    node.get_call_outcome_output()
                }
            }
            _ => panic!("Expected CallOutcome input for call_outcome pointer, got {:?}", input),
        }
    }

    /// Gets a pointer to CreateOutcome from a node outputting CreateOutcome.
    fn get_create_outcome_ptr(&mut self, input: &SSAInput) -> Option<*const CreateOutcome> {
        match input {
            SSAInput::CreateOutcome(lsn_with_index) => {
                if lsn_with_index.0 == 0 {
                    panic!("Cannot get CreateOutcome pointer from initial state (LSN 0)");
                } else {
                    let node_index = self.lsn_to_node[&lsn_with_index.0];
                    let node = self.graph.get_node(node_index);
                    // Assumes node implements get_create_outcome_output()
                    node.get_create_outcome_output()
                }
            }
            _ => panic!("Expected CreateOutcome input for create_outcome pointer, got {:?}", input),
        }
    }

    /// Helper to get the contract address (as U256) from a ContractEnv input.
    fn get_address_u256_from_contract_env(&mut self, input: &SSAInput) -> *const U256 {
        match input {
            SSAInput::ContractEnv(lsn_with_index) => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                if let Some(frame_context_ptr) = node.get_frame_context_output() {
                    let address = unsafe { (*frame_context_ptr).frame_input.target_address };
                    // Add address bytes to constant pool to get a stable U256 pointer
                    self.constant_pool.add_u256(U256::from_be_bytes(address.into_word().0))
                } else {
                    panic!("Node {} (LSN {}) does not provide FrameContext output at index {}", node_index, lsn_with_index.0, lsn_with_index.1)
                }
            }
            _ => panic!("Expected ContractEnv input for address, got {:?}", input),
        }
    }

    /// Helper to determine the source of AccountInfo (previous node or external context).
    fn get_account_info_source(&mut self, input: &SSAInput) -> (Option<*const AccountInfo>, Option<Rc<RefCell<ExternalContext>>>) {
        match input {
            SSAInput::Storage(lsn_with_index) if lsn_with_index.0 != 0 => {
                let node_index = self.lsn_to_node[&lsn_with_index.0];
                let node = self.graph.get_node(node_index);
                (node.get_account_info_output(lsn_with_index.1 as usize), None)
            }
            SSAInput::Storage(lsn_with_index) if lsn_with_index.0 == 0 => {
                (None, Some(self.context.clone()))
            }
             _ => panic!("Expected Storage input for account info source, got {:?}", input),
        }
    }
}
