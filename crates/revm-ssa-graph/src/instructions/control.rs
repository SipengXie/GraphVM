use revm_primitives::{db::DatabaseRef, Bytes, Spec, U256};
use revm_ssa::{
    as_usize_saturated,
    SSAInput, SSAOutput,
    SSAInstructionResult, SSAInterpreterResult,
};
use crate::{ExecutionContext, ExecutionError, Result};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute JUMP operation
    #[inline]
    pub fn execute_jump(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "JUMP requires exactly 2 operands".to_string()
            ));
        }

        let target = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Stack value".to_string()
            )),
        };

        let current_pc = match &inputs[1] {
            SSAInput::Constant(value) => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Constant value".to_string()
            )),
        };
        // Calculate relative offset
        let target_usize = target.as_limbs()[0] as usize;
        let relative_offset = target_usize as isize - current_pc.as_limbs()[0] as isize;

        Ok(vec![SSAOutput::Jump {
            relative_offset,
        }])
    }

    /// Execute JUMPI operation
    #[inline]
    pub fn execute_jumpi(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "JUMPI requires exactly 3 operands".to_string()
            ));
        }

        let target = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };

        let condition = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        let current_pc = match &inputs[2] {
            SSAInput::Constant(value) => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Third operand must be Constant value".to_string()
            )),
        };

        // If condition is 0, no jump, relative offset is 0
        if condition.is_zero() {
            return Ok(vec![SSAOutput::Jump {
                relative_offset: 0,
            }]);
        }

        // Calculate relative offset
        let target_usize = target.as_limbs()[0] as usize;
        let relative_offset = target_usize as isize - current_pc.as_limbs()[0] as isize;

        Ok(vec![SSAOutput::Jump {
            relative_offset,
        }])
    }

    /// Execute PC operation
    #[inline]
    pub fn execute_pc(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "PC requires exactly 1 operand".to_string()
            ));
        }

        let pc = match &inputs[0] { 
            SSAInput::Constant(value) => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Constant value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(pc.as_limbs()[0] as usize))])
    }

    /// Execute RETURN/REVERT operation
    #[inline]
    pub fn execute_ret(&mut self, inputs: Vec<SSAInput>, instruction_result: SSAInstructionResult) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "RETURN/REVERT requires exactly 3 operands".to_string()
            ));
        }

        let offset = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };

        let length = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        let output = match &inputs[2] {
            SSAInput::Memory { value, .. } => value.clone(),
            _ => return Err(ExecutionError::ExecutionError(
                "Third operand must be Memory value".to_string()
            )),
        };

        let result = instruction_result;
        let mut ssa_outputs = vec![
            SSAOutput::InterpreterResult(
            SSAInterpreterResult {
                result,
                output,
            })];
        // eprintln!("return: {:?}", ssa_outputs);
        let offset = as_usize_saturated(*offset);
        let length = as_usize_saturated(*length);
        let new_size = self.check_memory_size(offset, length);
        if new_size > self.memory_size() {
            ssa_outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }
        Ok(ssa_outputs)
    }

    pub fn execute_change_instruction_result(&self, opcode: u8) -> Result<Vec<SSAOutput>> {
        let result = match opcode {
            0x00 => SSAInstructionResult::Ok,      // STOP
            0xFE => SSAInstructionResult::Error,   // INVALID
            0xFF => SSAInstructionResult::Error,   // UNKNOWN
            _ => return Err(ExecutionError::ExecutionError(
                "Invalid opcode for instruction result change".to_string()
            )),
        };

        Ok(vec![
            SSAOutput::InterpreterResult(SSAInterpreterResult {
                result,
                output: Bytes::new(),
            })
        ])
    }

}
