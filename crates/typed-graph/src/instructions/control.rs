use crate::instructions::memory::calc_memory_size;
use crate::typed_graph::{HasInputType, HasOutputType, TypedNode};
use revm_interpreter::{as_usize_saturated, InstructionResult, SharedMemory}; // Use InstructionResult
use revm_primitives::{Bytes, U256};
use std::cell::RefCell;
use std::rc::Rc; // Assuming path

// --- JUMP Node (0x56) ---

/// Node for JUMP operation. Determines the next instruction index.
/// Note: In TypedGraph, JUMP typically marks the end of a basic block/subgraph.
/// The actual jump logic (changing execution flow) is handled by the graph executor
/// based on the output of this node.
pub struct JumpNode {
    /// Input: *const U256 - The target program counter (PC).
    inputs: (*const U256,),
    /// Output: usize - The target PC as a usize index.
    outputs: (usize,),
}

impl HasInputType<(*const U256,)> for JumpNode {}
impl HasOutputType<(usize,)> for JumpNode {}

impl JumpNode {
    pub fn new(target_pc_ptr: *const U256) -> Self {
        Self {
            inputs: (target_pc_ptr,),
            outputs: (0,),
        } // Initialize with 0
    }
}

impl TypedNode for JumpNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            // Directly convert the target PC to usize.
            // Saturation ensures safety if the U256 value is too large.
            let target_pc = as_usize_saturated!(*self.inputs.0);
            self.outputs.0 = target_pc;
        }
        Ok(())
    }

    // Custom output getter for usize
    fn get_usize_output(&self) -> *const usize {
        &self.outputs.0 as *const usize
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "JumpNode: Jump to PC {}",
                *self.inputs.0
            )
        }
    }
}

// --- JUMPI Node (0x57) ---

/// Node for JUMPI operation. Conditionally determines the next instruction index.
/// Note: In TypedGraph, JUMPI typically marks the end of a basic block/subgraph.
/// The graph executor uses the condition and target outputs to branch execution.
pub struct JumpiNode {
    /// Inputs:
    /// 0: *const U256 - The target program counter (PC) if condition is non-zero.
    /// 1: *const U256 - The condition value.
    inputs: (*const U256, *const U256),
    /// Outputs:
    /// 0: usize - The target PC if jumping.
    outputs: (usize,),
}

impl HasInputType<(*const U256, *const U256)> for JumpiNode {}
impl HasOutputType<(usize,)> for JumpiNode {}

impl JumpiNode {
    pub fn new(target_pc_ptr: *const U256, condition_ptr: *const U256) -> Self {
        Self {
            inputs: (target_pc_ptr, condition_ptr),
            outputs: (0,),
        } // Initialize
    }
}

impl TypedNode for JumpiNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let target_pc = as_usize_saturated!(*self.inputs.0);
            let condition = *self.inputs.1;

            let should_jump = !condition.is_zero();
            self.outputs.0 = if should_jump { target_pc } else { 0 }; // Store target only if jumping
        }
        Ok(())
    }

    fn get_usize_output(&self) -> *const usize {
        &self.outputs.0 as *const usize
    }
    
    fn print(&self) -> String {
        unsafe {
            let condition = *self.inputs.1;
            let should_jump = !condition.is_zero();
            format!(
                "JumpiNode: Conditional jump to PC {} if condition {} (Jump: {})",
                *self.inputs.0, condition, should_jump
            )
        }
    }
}

// --- RETURN Node (0xf3) / REVERT Node (0xfd) ---
// These mark the end of execution for the current frame.

/// Node for RETURN or REVERT operations. Signals end of frame execution.
pub struct ReturnRevertNode {
    /// Inputs:
    /// 0: *const U256 - Memory offset for return/revert data.
    /// 1: *const U256 - Length of return/revert data.
    /// 2: Rc<RefCell<SharedMemory>> - Shared memory reference.
    /// 3: InstructionResult - Indicates if it's RETURN (Ok) or REVERT (Revert).
    inputs: (
        *const U256,
        *const U256,
        Rc<RefCell<SharedMemory>>,
        InstructionResult,
    ),
    /// Output:
    /// 0: InstructionResult - The final execution status for this frame.
    /// 1: Bytes - The data returned or reverted with.
    outputs: (InstructionResult, Bytes),
}

// Define the specific input type tuple
type ReturnRevertInput = (
    *const U256,
    *const U256,
    Rc<RefCell<SharedMemory>>,
    InstructionResult,
);

impl HasInputType<ReturnRevertInput> for ReturnRevertNode {}
impl HasOutputType<(InstructionResult, Bytes)> for ReturnRevertNode {}

impl ReturnRevertNode {
    pub fn new(
        offset_ptr: *const U256,
        len_ptr: *const U256,
        memory: Rc<RefCell<SharedMemory>>,
        result: InstructionResult, // Pass Ok for RETURN, Revert for REVERT
    ) -> Self {
        Self {
            inputs: (offset_ptr, len_ptr, memory, result),
            outputs: (InstructionResult::Continue, Bytes::new()), // Initial state
        }
    }
}

impl TypedNode for ReturnRevertNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let offset = as_usize_saturated!(*self.inputs.0);
            let len = as_usize_saturated!(*self.inputs.1);
            let instruction_result = self.inputs.3; // The result (Ok/Revert) passed during creation

            // Borrow memory mutably as reading might trigger resize (via calc_memory_size)
            let mut memory = self.inputs.2.borrow_mut();

            // Ensure memory is large enough (mimics EVM memory expansion on read for RETURN/REVERT)
            let required_size = calc_memory_size(offset, len);
            if required_size > memory.len() {
                // EVM would revert with OutOfGas here if resize fails.
                // For TypedGraph, we might need a way to signal this.
                // For now, resize or handle error. Let's assume resize works.
                memory.resize(required_size);
            }

            // Read the data from memory
            let output_data = if len == 0 {
                Bytes::new()
            } else {
                // Read the slice safely after potential resize
                Bytes::copy_from_slice(memory.slice(offset, len))
            };

            // Set the outputs
            self.outputs.0 = instruction_result;
            self.outputs.1 = output_data;
        }
        Ok(())
    }

    fn get_instruction_result_output(&self) -> *const InstructionResult {
        &self.outputs.0 as *const InstructionResult
    }

    fn get_bytes_output(&self) -> Option<*const Bytes> {
        Some(&self.outputs.1 as *const Bytes) // Return pointer TO the Bytes struct
    }
    
    fn print(&self) -> String {
        unsafe {
            let op_type = match self.inputs.3 {
                InstructionResult::Return => "RETURN",
                InstructionResult::Revert => "REVERT",
                _ => "UNKNOWN",
            };
            
            format!(
                "ReturnRevertNode: {} data from offset {} with length {} (Result: {:?}, Data len: {})",
                op_type, 
                *self.inputs.0, 
                *self.inputs.1,
                self.outputs.0,
                self.outputs.1.len()
            )
        }
    }
}

// --- STOP Node (0x00) / INVALID Node (0xfe) ---
// These also mark the end of execution, similar to RETURN/REVERT but without data.

/// Node for STOP or INVALID operations. Signals end of frame execution.
pub struct StopInvalidNode {
    /// Input: The specific result (Ok for STOP, revert code for INVALID).
    _inputs: (InstructionResult,), // Just the result code
    /// Outputs: Same as inputs.
    outputs: (InstructionResult, Bytes), // Output Bytes is always empty
}

impl HasInputType<(InstructionResult,)> for StopInvalidNode {}
impl HasOutputType<(InstructionResult, Bytes)> for StopInvalidNode {}

impl StopInvalidNode {
    pub fn new(result: InstructionResult) -> Self {
        assert!(result == InstructionResult::Stop || result.is_revert() || result.is_error()); // Ensure valid type
        Self {
            _inputs: (result,),
            outputs: (result, Bytes::new()), // Set output directly
        }
    }
}

impl TypedNode for StopInvalidNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        // No actual execution needed, outputs are set in new()
        Ok(())
    }
    // Custom getters for output
    fn get_instruction_result_output(&self) -> *const InstructionResult {
        &self.outputs.0 as *const InstructionResult
    }
    fn get_bytes_output(&self) -> Option<*const Bytes> {
        Some(&self.outputs.1 as *const Bytes)
    }
    
    fn print(&self) -> String {
        let op_type = match self.outputs.0 {
            InstructionResult::Stop => "STOP",
            _ => if self.outputs.0.is_error() { "INVALID" } else { "UNKNOWN" },
        };
        
        format!(
            "StopInvalidNode: {} operation (Result: {:?})",
            op_type,
            self.outputs.0
        )
    }
}
