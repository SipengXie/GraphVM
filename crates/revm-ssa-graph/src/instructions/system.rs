use std::cmp::min;

use revm_primitives::{B256, U256};
use revm_ssa::{SSAInput, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result, match_ssa_input_stack_or_const};
use super::utils::as_usize_saturated;
use revm_primitives::db::DatabaseRef;
use revm_primitives::Spec;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute GAS operation
    #[inline]
    pub fn execute_gas(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {  
            return Err(ExecutionError::ExecutionError(
                "GAS requires exactly 1 operand".to_string()
            ));
        }
        let gas = match &inputs[0] {
            SSAInput::Constant(value) => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Constant value".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(*gas)])
    }

    /// Execute ADDRESS operation
    #[inline]
    pub fn execute_address(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "ADDRESS requires exactly 1 operand".to_string()
            ));
        }
        let address = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be CallInput".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(address.into_word().into())])
    }

    /// Execute CALLER operation
    #[inline]
    pub fn execute_caller(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "CALLER requires exactly 1 operand".to_string()
            ));
        }
        let caller = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_caller().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be CallInput".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(caller.into_word().into())])
    }

    /// Execute CODESIZE operation
    #[inline]
    pub fn execute_codesize(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "CODESIZE requires exactly 1 operand".to_string()
            ));
        }
        let len = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_size().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be CallInput".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(U256::from(len))])
    }

    /// Execute CODECOPY operation
    #[inline]
    pub fn execute_codecopy(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 4 {
            return Err(ExecutionError::ExecutionError(
                "CODECOPY requires exactly 4 operands".to_string()
            ));
        }

        let memory_offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let code_offset = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let len = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let code = match &inputs[3] {
            SSAInput::ContractEntry { value, .. } => value.as_code().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };
        let memory_offset = as_usize_saturated(*memory_offset);
        let code_offset = as_usize_saturated(*code_offset);
        let len = as_usize_saturated(*len);

        // Prevent code from being too short
        let code_len = min(code_offset + len, code.len());
        let code_slice = code.slice(code_offset..code_len);
        let new_size = self.check_memory_size(memory_offset, len);
        // Pad code to len length
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code_slice.len()].copy_from_slice(&code_slice);
        let mut outputs = vec![SSAOutput::Memory(padded_code_slice.into())];
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }
        Ok(outputs)
    }

    /// Execute CALLDATALOAD operation
    #[inline]
    pub fn execute_calldataload(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "CALLDATALOAD requires exactly 2 operands".to_string()
            ));
        }
        let offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let call_data = match &inputs[1] {
            SSAInput::ContractEntry { value, .. } => value.as_call_data().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };
        let offset = as_usize_saturated(*offset);
        let mut word = [0u8; 32];
        if offset < call_data.len() {
            let length = 32.min(call_data.len() - offset);
            word[..length].copy_from_slice(&call_data[offset..offset+length]);
        }
        Ok(vec![SSAOutput::Stack(B256::from_slice(&word).into())])
    }

    /// Execute CALLDATASIZE operation
    #[inline]
    pub fn execute_calldatasize(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "CALLDATASIZE requires exactly 1 operand".to_string()
            ));
        }
        let len = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_call_data_size().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(U256::from(len))])
    }

    /// Execute CALLVALUE operation
    #[inline]
    pub fn execute_callvalue(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "CALLVALUE requires exactly 1 operand".to_string()
            ));
        }
        let call_value = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_call_value().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(call_value.into())])
    }

    /// Execute CALLDATACOPY operation
    #[inline]
    pub fn execute_calldatacopy(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 4 {
            return Err(ExecutionError::ExecutionError(
                "CALLDATACOPY requires exactly 4 operands".to_string()
            ));
        }
        let memory_offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let data_offset = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let len = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let call_data = match &inputs[3] {
            SSAInput::ContractEntry { value, .. } => value.as_call_data().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };

        let memory_offset = as_usize_saturated(*memory_offset);
        let data_offset = as_usize_saturated(*data_offset);
        let len = as_usize_saturated(*len);

        // Prevent data from being too short
        let data_len = min(data_offset + len, call_data.len());
        let data_slice = call_data.slice(data_offset..data_len);
        let new_size = self.check_memory_size(memory_offset, len);
        // Pad data to len length
        let mut padded_data_slice = vec![0u8; len];
        padded_data_slice[..data_slice.len()].copy_from_slice(&data_slice);
        let mut outputs = vec![SSAOutput::Memory(padded_data_slice.into())];
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }
        Ok(outputs)
    }

    /// Execute RETURNDATASIZE operation
    #[inline]
    pub fn execute_returndatasize(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "RETURNDATASIZE requires exactly 1 operand".to_string()
            ));
        }
        let return_data = match &inputs[0] {
            SSAInput::ReturnDataBuffer { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ReturnDataBuffer".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(U256::from(return_data.len()))])
    }

    /// Execute RETURNDATACOPY operation
    #[inline]
    pub fn execute_returndatacopy(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 4 {
            return Err(ExecutionError::ExecutionError(
                "RETURNDATACOPY requires exactly 4 operands".to_string()
            ));
        }
        let memory_offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let data_offset = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let len = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let return_data = match &inputs[3] {
            SSAInput::ReturnDataBuffer { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ReturnData".to_string()
            )),
        };

        let memory_offset = as_usize_saturated(*memory_offset);
        let data_offset = as_usize_saturated(*data_offset);
        let len = as_usize_saturated(*len);

        // Prevent data from being too short
        let data_len = min(data_offset + len, return_data.len());
        let data_slice = return_data.slice(data_offset..data_len);
        let new_size = self.check_memory_size(memory_offset, len);
        // Pad data to len length
        let mut padded_data_slice = vec![0u8; len];
        padded_data_slice[..data_slice.len()].copy_from_slice(&data_slice);
        let mut outputs = vec![SSAOutput::Memory(padded_data_slice.into())];
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }
        Ok(outputs)
    }

    /// Execute RETURNDATALOAD operation
    #[inline]
    pub fn execute_returndataload(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "RETURNDATALOAD requires exactly 2 operands".to_string()
            ));
        }
        let offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let return_data = match &inputs[1] {
            SSAInput::ReturnDataBuffer { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ReturnData".to_string()
            )),
        };
        let offset = as_usize_saturated(*offset);
        let mut word = [0u8; 32];
        if offset < return_data.len() {
            let length = 32.min(return_data.len() - offset);
            word[..length].copy_from_slice(&return_data[offset..offset+length]);
        }
        Ok(vec![SSAOutput::Stack(B256::from(word).into())])
    }

    /// Execute KECCAK256 operation
    #[inline]
    pub fn execute_keccak256(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "KECCAK256 requires exactly 3 operands".to_string()
            ));
        }

        let offset = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let len = match_ssa_input_stack_or_const!(&inputs[1], "Second");

        let data = match &inputs[2] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Third operand must be Memory value".to_string()
            )),
        };
        let offset = as_usize_saturated(*offset);
        let len = as_usize_saturated(*len);

        // Calculate new memory size
        let new_size = self.check_memory_size(offset, len);
        let data_slice = data.slice(..len);
        let hash = revm_primitives::keccak256(data_slice);
        let mut outputs = vec![SSAOutput::Stack(hash.into())];

        // If memory size changes, add MemorySize output
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }

        Ok(outputs)
    }
}
