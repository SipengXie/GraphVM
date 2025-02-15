use revm_ssa::{SSAInput, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result, match_ssa_input_stack_or_const};
use revm_primitives::db::DatabaseRef;
use revm_primitives::Spec;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute generic PUSH operation
    #[inline]
    pub fn execute_push(&self, inputs: Vec<SSAInput>, size: usize) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                format!("PUSH{} requires exactly 1 operand", size).to_string()
            ));
        }

        let value = match &inputs[0] {
            SSAInput::Constant(value) => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Constant value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(value.clone())])
    }

    /// Execute POP operation
    #[inline]
    pub fn execute_pop(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "POP requires exactly 1 operand".to_string()
            ));
        }

        // POP operation produces no output
        Ok(vec![])
    }

    /// Execute DUP operation
    #[inline]
    pub fn execute_dup(&self, inputs: Vec<SSAInput>, position: usize) -> Result<Vec<SSAOutput>> {
        if inputs.len() != position {
            return Err(ExecutionError::ExecutionError(
                format!("DUP{} requires exactly {} operands", position, position).to_string()
            ));
        }

        let value = match_ssa_input_stack_or_const!(&inputs[position - 1], format!("Position {}", position).as_str());

        Ok(vec![SSAOutput::Stack(*value)])
    }

    /// Execute SWAP operation
    #[inline]
    pub fn execute_swap(&self, inputs: Vec<SSAInput>, position: usize) -> Result<Vec<SSAOutput>> {
        let required_inputs = position + 1;
        if inputs.len() != required_inputs {
            return Err(ExecutionError::ExecutionError(
                format!("SWAP{} requires exactly {} operands", position, required_inputs).to_string()
            ));
        }

        let top = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let swap = match_ssa_input_stack_or_const!(&inputs[position], format!("Position {}", position + 1).as_str());

        Ok(vec![
            SSAOutput::Stack(*swap),
            SSAOutput::Stack(*top),
        ])
    }
}
