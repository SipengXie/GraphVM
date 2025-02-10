use revm_ssa::{SSAInput, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result};
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

        let value = match &inputs[position - 1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Stack value".to_string()
            )),
        };

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

        let top = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };

        let swap = match &inputs[position] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                format!("Operand at position {} must be Stack value", position + 1).to_string()
            )),
        };

        Ok(vec![
            SSAOutput::Stack(*swap),
            SSAOutput::Stack(*top),
        ])
    }
}
