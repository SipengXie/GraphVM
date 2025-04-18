use crate::typed_graph::TypedNode;
use revm_interpreter::{as_usize_saturated, SharedMemory};
use revm_primitives::U256;
use std::{cell::RefCell, cmp::max};
use std::rc::Rc;
use super::types::{MemoryStoreInputs, MemoryLoadInputs, U256Output, MemoryCopyInputs};

#[cfg(feature = "metrics")]
use metrics::histogram;
#[cfg(feature = "metrics")]
use std::time::Instant;

// --- MLOAD Node ---

/// Node for MLOAD operation: reads 32 bytes from memory into a U256 value.
pub struct MloadNode {
    inputs: MemoryLoadInputs,
    outputs: U256Output,
}


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
            let required_size = offset.saturating_add(32);
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
    inputs: MemoryStoreInputs,
    _outputs: (),
}


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
        #[cfg(feature = "metrics")]
        let start = Instant::now();
        unsafe {
            let offset = as_usize_saturated!(*self.inputs.0);
            let value = *self.inputs.1;
            let mut memory = self.inputs.2.borrow_mut();

            let required_size = offset.saturating_add(32);
            if required_size > memory.len() {
                memory.resize(required_size);
            }

            memory.set_u256(offset, value);
        }

        #[cfg(feature = "metrics")]
        histogram!("mstore_time", start.elapsed());
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
    inputs: MemoryStoreInputs,
    _outputs: (),
}


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

            let required_size = offset.saturating_add(1);
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
    inputs: (Rc<RefCell<SharedMemory>>,),
    outputs: U256Output,
}


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
    inputs: MemoryCopyInputs,
    _outputs: (),
}


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

            let required_size = max(dst, src).saturating_add(len);

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
