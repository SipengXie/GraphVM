use revm_primitives::{db::DatabaseRef, Bytes, Spec};
use revm_ssa::{SSAInstructionResult, SSAInterpreterResult, SSALogEntry, SSAOutput, SSAInput
};
use crate::{get_ssa_output_stack_or_const, get_memory, as_usize_saturated,ExecutionContext, ExecutionError, Result, SsaGraph};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute JUMP operation
    #[inline(always)]
    pub fn execute_jump(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let target = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let current_pc = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        // Calculate relative offset
        let target_usize = target.as_limbs()[0] as usize;
        let relative_offset = target_usize as isize - current_pc.as_limbs()[0] as isize;

        // Control flow verification
        if let SSAOutput::Jump(old_jump) = node.outputs[0] {
            if old_jump != relative_offset {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::control_flow_not_deterministic(node, old_jump, relative_offset)
                ));
            }
        }

        node.outputs[0] = SSAOutput::Jump(relative_offset);

        Ok(())
    }

    /// Execute JUMPI operation
    #[inline(always)]
    pub fn execute_jumpi(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let target = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let condition = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let current_pc = get_ssa_output_stack_or_const!(graph, node.inputs[2]);

        // If condition is 0, no jump, relative offset is 0
        let new_jump = if condition.is_zero() {
            0
        } else {
            // Calculate relative offset
            let target_usize = target.as_limbs()[0] as usize;
            let relative_offset = target_usize as isize - current_pc.as_limbs()[0] as isize;
            relative_offset
        };

        // Control flow verification
        if let SSAOutput::Jump(old_jump) = node.outputs[0] {
            if old_jump != new_jump {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::control_flow_not_deterministic(node, old_jump, new_jump)
                ));
            }
        }

        node.outputs[0] = SSAOutput::Jump(new_jump);

        Ok(())
    }

    /// Execute RETURN/REVERT operation
    #[inline(always)]
    pub fn execute_ret(&mut self, node: &mut SSALogEntry, graph: & SsaGraph, instruction_result: SSAInstructionResult) -> Result<()> {
        let offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let length = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let output = get_memory!(graph, &node.inputs[2]);

        let result = instruction_result;
        let offset = as_usize_saturated!(offset);
        let length = as_usize_saturated!(length);
        let new_size = self.check_memory_size(offset, length);

        node.outputs[0] = SSAOutput::InterpreterResult(
            SSAInterpreterResult {
                result,
                output: output.into(),
            }
        );
        
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

    #[inline(always)]
    pub fn execute_change_instruction_result(&self, node: &mut SSALogEntry, _graph: & SsaGraph, opcode: u8) -> Result<()> {
        let result = match opcode {
            0x00 => SSAInstructionResult::Ok,      // STOP
            0xFE => SSAInstructionResult::Error,   // INVALID
            0xFF => SSAInstructionResult::Error,   // UNKNOWN
            _ => return Err(ExecutionError::ExecutionError(
                ExecutionError::INVALID_OPCODE_FOR_RESULT_CHANGE.to_string()
            )),
        };

        node.outputs[0] = SSAOutput::InterpreterResult(SSAInterpreterResult {
            result,
            output: Bytes::new(),
        });

        Ok(())
    }
}
