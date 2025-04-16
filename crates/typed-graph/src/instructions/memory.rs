use crate::typed_graph::{HasInputType, HasOutputType, TypedNode};
use revm_interpreter::{as_usize_saturated, SharedMemory};
use revm_primitives::U256;
use std::cell::RefCell;
use std::rc::Rc;

/// Calculates the required memory size rounded up to the nearest 32-byte word.
/// This function mimics the memory expansion logic in EVM.
#[inline]
pub fn calc_memory_size(offset: usize, size: usize) -> usize {
    if size == 0 {
        0
    } else {
        // Calculate the highest byte accessed
        let end = offset.saturating_add(size);
        // Round up to the nearest word (32 bytes)
        (end.saturating_add(31)) / 32 * 32
    }
}

// --- MLOAD Node ---

/// Node for MLOAD operation: reads 32 bytes from memory into a U256 value.
pub struct MloadNode {
    /// Inputs:
    /// 0: *const U256 - Memory offset.
    /// 1: Rc<RefCell<SharedMemory>> - Reference to the shared memory.
    inputs: (*const U256, Rc<RefCell<SharedMemory>>),
    /// Output:
    /// 0: U256 - The value loaded from memory.
    outputs: (U256,),
}

impl HasInputType<(*const U256, Rc<RefCell<SharedMemory>>)> for MloadNode {}
impl HasOutputType<(U256,)> for MloadNode {}

impl MloadNode {
    pub fn new(offset_ptr: *const U256, memory: Rc<RefCell<SharedMemory>>) -> Self {
        Self {
            inputs: (offset_ptr, memory),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for MloadNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let offset = as_usize_saturated!(*self.inputs.0);
            let mut memory = self.inputs.1.borrow_mut();

            let required_size = calc_memory_size(offset, 32);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            self.outputs.0 = memory.get_u256(offset);
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "MloadNode: Load from offset {} = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

// --- MSTORE Node ---

/// Node for MSTORE operation: writes a 32-byte U256 value to memory.
pub struct MstoreNode {
    /// Inputs:
    /// 0: *const U256 - Memory offset.
    /// 1: *const U256 - Value to store.
    /// 2: Rc<RefCell<SharedMemory>> - Reference to the shared memory.
    inputs: (*const U256, *const U256, Rc<RefCell<SharedMemory>>),
    /// Outputs: None. Modifies shared memory state directly.
    _outputs: (),
}

impl HasInputType<(*const U256, *const U256, Rc<RefCell<SharedMemory>>)> for MstoreNode {}
impl HasOutputType<()> for MstoreNode {}

impl MstoreNode {
    pub fn new(
        offset_ptr: *const U256,
        value_ptr: *const U256,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (offset_ptr, value_ptr, memory),
            _outputs: (),
        }
    }
}

impl TypedNode for MstoreNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let offset = as_usize_saturated!(*self.inputs.0);
            let value = *self.inputs.1;
            let mut memory = self.inputs.2.borrow_mut();

            let required_size = calc_memory_size(offset, 32);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            memory.set_u256(offset, value);
        }
        Ok(())
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "MstoreNode: Store {} at offset {}",
                *self.inputs.1, *self.inputs.0
            )
        }
    }
}

// --- MSTORE8 Node ---

/// Node for MSTORE8 operation: writes a single byte to memory.
pub struct Mstore8Node {
    /// Inputs:
    /// 0: *const U256 - Memory offset.
    /// 1: *const U256 - Value to store (only the least significant byte is used).
    /// 2: Rc<RefCell<SharedMemory>> - Reference to the shared memory.
    inputs: (*const U256, *const U256, Rc<RefCell<SharedMemory>>),
    /// Outputs: None. Modifies shared memory state directly.
    _outputs: (),
}

impl HasInputType<(*const U256, *const U256, Rc<RefCell<SharedMemory>>)> for Mstore8Node {}
impl HasOutputType<()> for Mstore8Node {}

impl Mstore8Node {
    pub fn new(
        offset_ptr: *const U256,
        value_ptr: *const U256,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (offset_ptr, value_ptr, memory),
            _outputs: (),
        }
    }
}

impl TypedNode for Mstore8Node {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let offset = as_usize_saturated!(*self.inputs.0);
            let value_byte = (*self.inputs.1).byte(0);
            let mut memory = self.inputs.2.borrow_mut();

            let required_size = calc_memory_size(offset, 1);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            memory.set_byte(offset, value_byte);
        }
        Ok(())
    }
    
    fn print(&self) -> String {
        unsafe {
            let byte_value = (*self.inputs.1).byte(0);
            format!(
                "Mstore8Node: Store byte {} at offset {}",
                byte_value, *self.inputs.0
            )
        }
    }
}

// --- MSIZE Node ---

/// Node for MSIZE operation: gets the current size of the active memory in bytes.
pub struct MsizeNode {
    /// Input:
    /// 0: Rc<RefCell<SharedMemory>> - Reference to the shared memory.
    inputs: (Rc<RefCell<SharedMemory>>,),
    /// Output:
    /// 0: U256 - Current memory size in bytes.
    outputs: (U256,),
}

impl HasInputType<(Rc<RefCell<SharedMemory>>,)> for MsizeNode {}
impl HasOutputType<(U256,)> for MsizeNode {}

impl MsizeNode {
    pub fn new(memory: Rc<RefCell<SharedMemory>>) -> Self {
        Self {
            inputs: (memory,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for MsizeNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        let memory = self.inputs.0.borrow();
        let size = memory.len();
        self.outputs.0 = U256::from(size);
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    
    fn print(&self) -> String {
        format!(
            "MsizeNode: Memory size = {}",
            self.outputs.0
        )
    }
}

// --- MCOPY Node ---

/// Node for MCOPY operation: copies a region of memory to another location.
pub struct McopyNode {
    /// Inputs:
    /// 0: *const U256 - Destination offset.
    /// 1: *const U256 - Source offset.
    /// 2: *const U256 - Length of data to copy in bytes.
    /// 3: Rc<RefCell<SharedMemory>> - Reference to the shared memory.
    inputs: (
        *const U256,
        *const U256,
        *const U256,
        Rc<RefCell<SharedMemory>>,
    ),
    /// Outputs: None. Modifies shared memory state directly.
    _outputs: (),
}

impl
    HasInputType<(
        *const U256,
        *const U256,
        *const U256,
        Rc<RefCell<SharedMemory>>,
    )> for McopyNode
{
}
impl HasOutputType<()> for McopyNode {}

impl McopyNode {
    pub fn new(
        dst_ptr: *const U256,
        src_ptr: *const U256,
        len_ptr: *const U256,
        memory: Rc<RefCell<SharedMemory>>,
    ) -> Self {
        Self {
            inputs: (dst_ptr, src_ptr, len_ptr, memory),
            _outputs: (),
        }
    }
}

impl TypedNode for McopyNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let dst = as_usize_saturated!(*self.inputs.0);
            let src = as_usize_saturated!(*self.inputs.1);
            let len = as_usize_saturated!(*self.inputs.2);

            if len == 0 {
                return Ok(());
            }

            let mut memory = self.inputs.3.borrow_mut();

            let highest_byte_accessed = dst.saturating_add(len).max(src.saturating_add(len));
            let required_size = if highest_byte_accessed > 0 {
                calc_memory_size(highest_byte_accessed - 1, 1)
            } else {
                0
            };

            if required_size > memory.len() {
                memory.resize(required_size);
            }

            memory.copy(dst, src, len);
        }
        Ok(())
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "McopyNode: Copy {} bytes from offset {} to offset {}",
                *self.inputs.2, *self.inputs.1, *self.inputs.0
            )
        }
    }
}
