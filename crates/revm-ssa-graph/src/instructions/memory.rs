use revm_primitives::U256;
use revm_ssa::{SSAInput, SSALogEntry, SSAOutput};
use crate::{get_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result, SsaGraph};
use super::get_memory;
use super::utils::as_usize_saturated;
use revm_primitives::db::DatabaseRef;
use revm_primitives::Spec;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Check if memory size needs to be extended, return new memory size
    #[inline]
    pub fn check_memory_size(&self, offset: usize, size: usize) -> usize {
        let required_size = if size == 0 {
            0
        } else {
            offset.saturating_add(size)
        };

        // Round up to 32-byte alignment
        ((required_size + 31) / 32) * 32
    }

    /// Execute MLOAD operation
    #[inline]
    pub fn execute_mload(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let memory = get_memory!(graph, &node.inputs[1]);
        let offset = as_usize_saturated!(offset);

        

        // Convert 32 bytes of memory value to U256
        let mut value = [0u8; 32];
        let len = memory.len().min(32);
        value[..len].copy_from_slice(&memory[..len]);
        let value = U256::from_be_bytes(value);

        // Calculate required memory size
        let new_size = self.check_memory_size(offset, 32);

        node.outputs[0] = SSAOutput::Stack(value);
        if new_size > self.memory_size() {
            if node.outputs.len() < 2 {
                node.outputs.push(SSAOutput::MemorySize(new_size));
            } else {
                node.outputs[1] = SSAOutput::MemorySize(new_size);
            }
            self.set_memory_size(new_size);
        }

        Ok(())
    }

    /// Execute MSTORE operation
    #[inline]
    pub fn execute_mstore(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let offset = as_usize_saturated!(offset);
        let value_bytes = value.to_be_bytes::<32>();
        
        // Calculate required memory size
        let new_size = self.check_memory_size(offset, 32);

        node.outputs[0] = SSAOutput::Memory(value_bytes.to_vec().into());
        if new_size > self.memory_size() {
            if node.outputs.len() < 2 {
                node.outputs.push(SSAOutput::MemorySize(new_size));
            } else {
                node.outputs[1] = SSAOutput::MemorySize(new_size);
            }
            self.set_memory_size(new_size);
        }

        Ok(())
    }

    /// Execute MSTORE8 operation
    #[inline]
    pub fn execute_mstore8(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let offset = as_usize_saturated!(offset);
        let value_bytes = vec![value.byte(0)];

        // Calculate required memory size
        let new_size = self.check_memory_size(offset, 1);

        node.outputs[0] = SSAOutput::Memory(value_bytes.to_vec().into());
        if new_size > self.memory_size() {
            if node.outputs.len() < 2 {
                node.outputs.push(SSAOutput::MemorySize(new_size));
            } else {
                node.outputs[1] = SSAOutput::MemorySize(new_size);
            }
            self.set_memory_size(new_size);
        }

        Ok(())
    }

    /// Execute MSIZE operation
    #[inline]
    pub fn execute_msize(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let size = match node.inputs[0] {
            SSAInput::MemorySizeChange((lsn, index)) => {
                let dep_node = graph.get_node(lsn)?;
                match dep_node.outputs[index as usize] {
                    SSAOutput::MemorySize(value) => value,
                    _ => return Err(ExecutionError::ExecutionError(
                        "Expected MemorySize output value".to_string()
                    ))
                }
            }
            _ => return Err(ExecutionError::ExecutionError(
                "MSIZE requires exactly 1 operand (memory size change)".to_string()
            ))
        };

        node.outputs[0] = SSAOutput::Stack(U256::from(size));

        Ok(())
    }

    /// Execute MCOPY operation
    #[inline]
    pub fn execute_mcopy(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {

        let dst = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let src = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let len = get_ssa_output_stack_or_const!(graph, node.inputs[2]);

        let memory = get_memory!(graph, &node.inputs[3]);

        // If length is 0, return directly
        if len.is_zero() {
            return Ok(());
        }

        let dst = as_usize_saturated!(dst);
        let src = as_usize_saturated!(src);
        let len = as_usize_saturated!(len);

        // Calculate required memory size
        let new_size = self.check_memory_size(dst.max(src), len);
        
        node.outputs[0] = SSAOutput::Memory(memory.into());
        if new_size > self.memory_size() {
            if node.outputs.len() < 2 {
                node.outputs.push(SSAOutput::MemorySize(new_size));
            } else {
                node.outputs[1] = SSAOutput::MemorySize(new_size);
            }
            self.set_memory_size(new_size);
        }

        Ok(())
    }
}
