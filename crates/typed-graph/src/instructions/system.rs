use crate::context::FrameContext;
use crate::typed_graph::{HasInputType, HasOutputType, TypedNode};
use revm_interpreter::{as_usize_saturated, SharedMemory};
use revm_primitives::{Bytes, KECCAK_EMPTY, U256};
use std::cell::RefCell;
use std::cmp::min;
use std::rc::Rc;

use super::memory::calc_memory_size;

// --- GAS Node (0x5a) ---
// Simplified: Assumes gas value is passed as input. Gas logic is complex.

/// Node for GAS operation (Simplified: passes through gas value).
pub struct GasNode {
    /// Input: Pointer to the current gas value.
    inputs: (*const U256,),
    /// Output: The current gas value.
    outputs: (U256,),
}

impl HasInputType<(*const U256,)> for GasNode {}
impl HasOutputType<(U256,)> for GasNode {}

impl GasNode {
    pub fn new(gas_ptr: *const U256) -> Self {
        Self {
            inputs: (gas_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for GasNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = *self.inputs.0;
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        unsafe {
            format!(
                "GasNode: Gas remaining = {}",
                *self.inputs.0
            )
        }
    }
}

// --- ADDRESS Node (0x30) ---

/// Node for ADDRESS operation: gets the address of the current contract.
pub struct AddressNode {
    /// Input: Pointer to the current frame context.
    inputs: (*const FrameContext,),
    /// Output: Address as U256.
    outputs: (U256,),
}

impl HasInputType<(*const FrameContext,)> for AddressNode {}
impl HasOutputType<(U256,)> for AddressNode {}

impl AddressNode {
    pub fn new(frame_ptr: *const FrameContext) -> Self {
        Self {
            inputs: (frame_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for AddressNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let address = (*self.inputs.0).frame_input.target_address;
            self.outputs.0 = U256::from_be_bytes(address.into_word().0);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        format!(
            "AddressNode: Current address = {}",
            self.outputs.0
        )
    }
}

// --- CALLER Node (0x33) ---

/// Node for CALLER operation: gets the address of the message sender.
pub struct CallerNode {
    /// Input: Pointer to the current frame context.
    inputs: (*const FrameContext,),
    /// Output: Caller address as U256.
    outputs: (U256,),
}

impl HasInputType<(*const FrameContext,)> for CallerNode {}
impl HasOutputType<(U256,)> for CallerNode {}

impl CallerNode {
    pub fn new(frame_ptr: *const FrameContext) -> Self {
        Self {
            inputs: (frame_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for CallerNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let caller = (*self.inputs.0).frame_input.caller;
            self.outputs.0 = U256::from_be_bytes(caller.into_word().0);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        format!(
            "CallerNode: Caller address = {}",
            self.outputs.0
        )
    }
}

// --- CODESIZE Node (0x38) ---

/// Node for CODESIZE operation: gets the size of the current contract's code.
pub struct CodesizeNode {
    /// Input: Pointer to the current frame context.
    inputs: (*const FrameContext,),
    /// Output: Code size in bytes.
    outputs: (U256,),
}

impl HasInputType<(*const FrameContext,)> for CodesizeNode {}
impl HasOutputType<(U256,)> for CodesizeNode {}

impl CodesizeNode {
    pub fn new(frame_ptr: *const FrameContext) -> Self {
        Self {
            inputs: (frame_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for CodesizeNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let code_size = (*self.inputs.0).bytecode.len();
            self.outputs.0 = U256::from(code_size);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        format!(
            "CodesizeNode: Code size = {}",
            self.outputs.0
        )
    }
}

// --- CODECOPY Node (0x39) ---

/// Node for CODECOPY operation: copies code of the current contract to memory.
pub struct CodecopyNode {
    /// Inputs:
    /// 0: *const U256 - Memory destination offset.
    /// 1: *const U256 - Code source offset.
    /// 2: *const U256 - Length of bytes to copy.
    /// 3: *const FrameContext - Contains the bytecode.
    /// 4: Rc<RefCell<SharedMemory>> - Shared memory reference.
    inputs: (
        *const U256,
        *const U256,
        *const U256,
        *const FrameContext,
        Rc<RefCell<SharedMemory>>,
    ),
    /// Outputs: None. Modifies memory.
    _outputs: (),
}

impl
    HasInputType<(
        *const U256,
        *const U256,
        *const U256,
        *const FrameContext,
        Rc<RefCell<SharedMemory>>,
    )> for CodecopyNode
{
}
impl HasOutputType<()> for CodecopyNode {}

impl CodecopyNode {
    pub fn new(
        mem_offset_ptr: *const U256,
        code_offset_ptr: *const U256,
        len_ptr: *const U256,
        frame_ptr: *const FrameContext,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (mem_offset_ptr, code_offset_ptr, len_ptr, frame_ptr, memory),
            _outputs: (),
        }
    }
}

impl TypedNode for CodecopyNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let mem_offset = as_usize_saturated!(*self.inputs.0);
            let code_offset = as_usize_saturated!(*self.inputs.1);
            let len = as_usize_saturated!(*self.inputs.2);

            if len == 0 {
                return Ok(());
            }

            let frame = &*self.inputs.3;
            let code = frame.bytecode.original_byte_slice();
            let mut memory = self.inputs.4.borrow_mut();

            // Resize memory if needed
            let required_size = calc_memory_size(mem_offset, len);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            // Perform the copy with padding
            let mut data_to_copy = vec![0u8; len];
            if code_offset < code.len() {
                let copy_len = min(len, code.len() - code_offset);
                data_to_copy[..copy_len]
                    .copy_from_slice(&code[code_offset..code_offset + copy_len]);
            }
            // else: data_to_copy remains all zeros

            memory.set(mem_offset, &data_to_copy);
        }
        Ok(())
    }
    fn print(&self) -> String {
        unsafe {
            format!(
                "CodecopyNode: Copy code from offset {} with length {} to memory offset {}",
                *self.inputs.1, *self.inputs.2, *self.inputs.0
            )
        }
    }
}

// --- CALLDATALOAD Node (0x35) ---

/// Node for CALLDATALOAD operation: loads 32 bytes from calldata.
pub struct CalldataloadNode {
    /// Inputs:
    /// 0: *const U256 - Calldata offset.
    /// 1: *const FrameContext - Contains the calldata (input).
    inputs: (*const U256, *const FrameContext),
    /// Output:
    /// 0: U256 - The 32 bytes loaded from calldata.
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const FrameContext)> for CalldataloadNode {}
impl HasOutputType<(U256,)> for CalldataloadNode {}

impl CalldataloadNode {
    pub fn new(offset_ptr: *const U256, frame_ptr: *const FrameContext) -> Self {
        Self {
            inputs: (offset_ptr, frame_ptr),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for CalldataloadNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let offset = as_usize_saturated!(*self.inputs.0);
            let call_data = &(*self.inputs.1).frame_input.input;

            let mut word = [0u8; 32];
            if offset < call_data.len() {
                let copy_len = min(32, call_data.len() - offset);
                word[..copy_len].copy_from_slice(&call_data[offset..offset + copy_len]);
            }
            // else: word remains all zeros

            self.outputs.0 = U256::from_be_bytes(word);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        unsafe {
            format!(
                "CalldataloadNode: Load calldata from offset {} = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

// --- CALLDATASIZE Node (0x36) ---

/// Node for CALLDATASIZE operation: gets the size of calldata.
pub struct CalldatasizeNode {
    /// Input: Pointer to the current frame context.
    inputs: (*const FrameContext,),
    /// Output: Calldata size in bytes.
    outputs: (U256,),
}

impl HasInputType<(*const FrameContext,)> for CalldatasizeNode {}
impl HasOutputType<(U256,)> for CalldatasizeNode {}

impl CalldatasizeNode {
    pub fn new(frame_ptr: *const FrameContext) -> Self {
        Self {
            inputs: (frame_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for CalldatasizeNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let data_size = (*self.inputs.0).frame_input.input.len();
            self.outputs.0 = U256::from(data_size);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        format!(
            "CalldatasizeNode: Calldata size = {}",
            self.outputs.0
        )
    }
}

// --- CALLVALUE Node (0x34) ---

/// Node for CALLVALUE operation: gets the value sent with the message call.
pub struct CallvalueNode {
    /// Input: Pointer to the current frame context.
    inputs: (*const FrameContext,),
    /// Output: Call value.
    outputs: (U256,),
}

impl HasInputType<(*const FrameContext,)> for CallvalueNode {}
impl HasOutputType<(U256,)> for CallvalueNode {}

impl CallvalueNode {
    pub fn new(frame_ptr: *const FrameContext) -> Self {
        Self {
            inputs: (frame_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for CallvalueNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).frame_input.transfer_value;
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        format!(
            "CallvalueNode: Call value = {}",
            self.outputs.0
        )
    }
}

// --- CALLDATACOPY Node (0x37) ---

/// Node for CALLDATACOPY operation: copies calldata to memory.
pub struct CalldatacopyNode {
    /// Inputs:
    /// 0: *const U256 - Memory destination offset.
    /// 1: *const U256 - Calldata source offset.
    /// 2: *const U256 - Length of bytes to copy.
    /// 3: *const FrameContext - Contains the calldata.
    /// 4: Rc<RefCell<SharedMemory>> - Shared memory reference.
    inputs: (
        *const U256,
        *const U256,
        *const U256,
        *const FrameContext,
        Rc<RefCell<SharedMemory>>,
    ),
    /// Outputs: None. Modifies memory.
    _outputs: (),
}

impl
    HasInputType<(
        *const U256,
        *const U256,
        *const U256,
        *const FrameContext,
        Rc<RefCell<SharedMemory>>,
    )> for CalldatacopyNode
{
}
impl HasOutputType<()> for CalldatacopyNode {}

impl CalldatacopyNode {
    pub fn new(
        mem_offset_ptr: *const U256,
        data_offset_ptr: *const U256,
        len_ptr: *const U256,
        frame_ptr: *const FrameContext,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (mem_offset_ptr, data_offset_ptr, len_ptr, frame_ptr, memory),
            _outputs: (),
        }
    }
}

impl TypedNode for CalldatacopyNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let mem_offset = as_usize_saturated!(*self.inputs.0);
            let data_offset = as_usize_saturated!(*self.inputs.1);
            let len = as_usize_saturated!(*self.inputs.2);

            if len == 0 {
                return Ok(());
            }

            let frame = &*self.inputs.3;
            let call_data = &frame.frame_input.input;
            let mut memory = self.inputs.4.borrow_mut();

            // Resize memory if needed
            let required_size = calc_memory_size(mem_offset, len);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            // Perform the copy with padding
            let mut data_to_copy = vec![0u8; len];
            if data_offset < call_data.len() {
                let copy_len = min(len, call_data.len() - data_offset);
                data_to_copy[..copy_len]
                    .copy_from_slice(&call_data[data_offset..data_offset + copy_len]);
            }
            // else: data_to_copy remains all zeros

            memory.set(mem_offset, &data_to_copy);
        }
        Ok(())
    }
    fn print(&self) -> String {
        unsafe {
            format!(
                "CalldatacopyNode: Copy calldata from offset {} with length {} to memory offset {}",
                *self.inputs.1, *self.inputs.2, *self.inputs.0
            )
        }
    }
}

// --- RETURNDATASIZE Node (0x3d) ---

/// Node for RETURNDATASIZE operation: gets the size of the last call's return data.
pub struct ReturndatasizeNode {
    /// Input: Pointer to the return data buffer.
    inputs: (*const Bytes,),
    /// Output: Size of the return data buffer.
    outputs: (U256,),
}

impl HasInputType<(*const Bytes,)> for ReturndatasizeNode {}
impl HasOutputType<(U256,)> for ReturndatasizeNode {}

impl ReturndatasizeNode {
    // Assume the return data buffer is managed externally and passed as input
    pub fn new(return_data_ptr: *const Bytes) -> Self {
        Self {
            inputs: (return_data_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ReturndatasizeNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let return_data_size = (*self.inputs.0).len();
            self.outputs.0 = U256::from(return_data_size);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        format!(
            "ReturndatasizeNode: Return data size = {}",
            self.outputs.0
        )
    }
}

// --- RETURNDATACOPY Node (0x3e) ---

/// Node for RETURNDATACOPY operation: copies return data to memory.
pub struct ReturndatacopyNode {
    /// Inputs:
    /// 0: *const U256 - Memory destination offset.
    /// 1: *const U256 - Return data source offset.
    /// 2: *const U256 - Length of bytes to copy.
    /// 3: *const Bytes - Pointer to the return data buffer.
    /// 4: Rc<RefCell<SharedMemory>> - Shared memory reference.
    inputs: (
        *const U256,
        *const U256,
        *const U256,
        *const Bytes,
        Rc<RefCell<SharedMemory>>,
    ),
    /// Outputs: None. Modifies memory.
    _outputs: (),
}

impl
    HasInputType<(
        *const U256,
        *const U256,
        *const U256,
        *const Bytes,
        Rc<RefCell<SharedMemory>>,
    )> for ReturndatacopyNode
{
}
impl HasOutputType<()> for ReturndatacopyNode {}

impl ReturndatacopyNode {
    pub fn new(
        mem_offset_ptr: *const U256,
        data_offset_ptr: *const U256,
        len_ptr: *const U256,
        return_data_ptr: *const Bytes,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (
                mem_offset_ptr,
                data_offset_ptr,
                len_ptr,
                return_data_ptr,
                memory,
            ),
            _outputs: (),
        }
    }
}

impl TypedNode for ReturndatacopyNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let mem_offset = as_usize_saturated!(*self.inputs.0);
            let data_offset = as_usize_saturated!(*self.inputs.1);
            let len = as_usize_saturated!(*self.inputs.2);

            if len == 0 {
                return Ok(());
            }

            let return_data = &*self.inputs.3;
            let mut memory = self.inputs.4.borrow_mut();

            // Check bounds for return data read BEFORE resizing memory
            if data_offset.saturating_add(len) > return_data.len() {
                // This should ideally result in a revert in a real EVM.
                // For TypedGraph, we might need an error mechanism or specific handling.
                // For now, just proceed (which will copy zeros for out-of-bounds part).
                // Or return an error:
                // return Err(anyhow::anyhow!("RETURNDATACOPY out of bounds"));
            }

            // Resize memory if needed
            let required_size = calc_memory_size(mem_offset, len);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            // Perform the copy with padding
            let mut data_to_copy = vec![0u8; len];
            if data_offset < return_data.len() {
                let copy_len = min(len, return_data.len() - data_offset);
                data_to_copy[..copy_len]
                    .copy_from_slice(&return_data[data_offset..data_offset + copy_len]);
            }
            // else: data_to_copy remains all zeros

            memory.set(mem_offset, &data_to_copy);
        }
        Ok(())
    }
    fn print(&self) -> String {
        unsafe {
            format!(
                "ReturndatacopyNode: Copy return data from offset {} with length {} to memory offset {}",
                *self.inputs.1, *self.inputs.2, *self.inputs.0
            )
        }
    }
}

// --- KECCAK256 Node (0x20) ---

/// Node for KECCAK256 operation: computes Keccak-256 hash of a memory region.
pub struct Keccak256Node {
    /// Inputs:
    /// 0: *const U256 - Memory offset.
    /// 1: *const U256 - Length of data to hash.
    /// 2: Rc<RefCell<SharedMemory>> - Shared memory reference.
    inputs: (*const U256, *const U256, Rc<RefCell<SharedMemory>>),
    /// Output:
    /// 0: U256 - The Keccak-256 hash result.
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256, Rc<RefCell<SharedMemory>>)> for Keccak256Node {}
impl HasOutputType<(U256,)> for Keccak256Node {}

impl Keccak256Node {
    pub fn new(
        offset_ptr: *const U256,
        len_ptr: *const U256,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (offset_ptr, len_ptr, memory),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for Keccak256Node {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let offset = as_usize_saturated!(*self.inputs.0);
            let len = as_usize_saturated!(*self.inputs.1);

            // Borrow memory mutably because hashing might trigger memory expansion
            let mut memory = self.inputs.2.borrow_mut();

            // Ensure memory is large enough
            let required_size = calc_memory_size(offset, len);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            // Perform the hash
            let hash = if len == 0 {
                KECCAK_EMPTY // Hash of empty bytes is specific constant
            } else {
                // Need immutable borrow *after* potential resize
                // Re-borrow immutably or read data before dropping mutable borrow
                // Simple approach: read into a temp buffer
                let data_to_hash = memory.slice(offset, len).to_vec(); // Read data
                revm_primitives::keccak256(&data_to_hash)
            };

            self.outputs.0 = U256::from_be_bytes(hash.0);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    fn print(&self) -> String {
        unsafe {
            format!(
                "Keccak256Node: Hash data at offset {} with length {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}
