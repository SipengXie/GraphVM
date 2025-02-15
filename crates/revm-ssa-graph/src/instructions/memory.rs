use revm_primitives::U256;
use revm_ssa::{SSAInput, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result, match_ssa_input_stack_or_const};
use super::utils::as_usize_saturated;
use revm_primitives::db::DatabaseRef;
use revm_primitives::Spec;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Check if memory size needs to be extended, return new memory size
    pub fn check_memory_size(&self, offset: usize, size: usize) -> usize {
        let required_size = if size == 0 {
            offset
        } else {
            offset.saturating_add(size)
        };

        // Round up to 32-byte alignment
        ((required_size + 31) / 32) * 32
    }

    /// Execute MLOAD operation
    #[inline]
    pub fn execute_mload(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "MLOAD requires exactly 2 operands (offset, memory)".to_string()
            ));
        }

        let offset = match_ssa_input_stack_or_const!(&inputs[0], "First");

        let memory = match &inputs[1] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Memory value".to_string()
            )),
        };

        // Convert 32 bytes of memory value to U256
        let mut value = [0u8; 32];
        let len = memory.len().min(32);
        value[..len].copy_from_slice(&memory[..len]);
        let value = U256::from_be_bytes(value);

        // Calculate required memory size
        let offset = as_usize_saturated(*offset);
        let new_size = self.check_memory_size(offset, 32);

        let mut outputs = vec![SSAOutput::Stack(value)];
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }

        Ok(outputs)
    }

    /// Execute MSTORE operation
    #[inline]
    pub fn execute_mstore(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "MSTORE requires exactly 2 operands (offset, value)".to_string()
            ));
        }

        let offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let value = match_ssa_input_stack_or_const!(&inputs[1], "Second");

        let offset = as_usize_saturated(*offset);
        let value_bytes = value.to_be_bytes::<32>();
        
        // Calculate required memory size
        let new_size = self.check_memory_size(offset, 32);

        let mut outputs = vec![SSAOutput::Memory(value_bytes.to_vec().into())];
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }

        Ok(outputs)
    }

    /// Execute MSTORE8 operation
    #[inline]
    pub fn execute_mstore8(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "MSTORE8 requires exactly 2 operands (offset, value)".to_string()
            ));
        }

        let offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let value = match_ssa_input_stack_or_const!(&inputs[1], "Second");

        let offset = as_usize_saturated(*offset);
        let value_bytes = vec![value.byte(0)];

        // Calculate required memory size
        let new_size = self.check_memory_size(offset, 1);

        let mut outputs = vec![SSAOutput::Memory(value_bytes.to_vec().into())];
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }

        Ok(outputs)
    }

    /// Execute MSIZE operation
    #[inline]
    pub fn execute_msize(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "MSIZE requires exactly 1 operand (memory size change)".to_string()
            ));
        }

        let size = match &inputs[0] {
            SSAInput::MemorySizeChange { size, .. } => size,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be MemorySizeChange".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(*size))])
    }

    /// Execute MCOPY operation
    #[inline]
    pub fn execute_mcopy(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 4 {
            return Err(ExecutionError::ExecutionError(
                "MCOPY requires exactly 4 operands (dst, src, len, memory)".to_string()
            ));
        }

        let dst = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let src = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let len = match_ssa_input_stack_or_const!(&inputs[2], "Third");

        let memory = match &inputs[3] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Fourth operand must be Memory value".to_string()
            )),
        };

        // If length is 0, return directly
        if len.is_zero() {
            return Ok(vec![]);
        }

        let dst = as_usize_saturated(*dst);
        let src = as_usize_saturated(*src);
        let len = as_usize_saturated(*len);

        // Calculate required memory size
        let new_size = self.check_memory_size(dst.max(src), len);
        // Pad memory to len length
        let mut outputs = vec![SSAOutput::Memory(memory.clone())];
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }

        Ok(outputs)
    }
}
