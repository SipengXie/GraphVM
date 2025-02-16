use std::cmp::min;
use revm_primitives::db::DatabaseRef;
use revm_primitives::{
    Address, Bytes, FixedBytes, Log, LogData, 
    Spec, B256, U256
};
use revm_ssa::{
    SSAInput, SSAOutput, StorageKey, StorageValue,
    SSAInstructionResult, SSAInterpreterResult
};
use crate::{ExecutionContext, ExecutionError, Result, match_ssa_input_stack_or_const};

use super::{as_u64_saturated, as_usize_saturated};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute SLOAD operation
    #[inline]
    pub fn execute_sload(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "SLOAD requires exactly 1 operand".to_string()
            ));
        }

        let value = match &inputs[2] {
            SSAInput::Storage { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(value.as_slot().unwrap())])
    }

    /// Execute SSTORE operation
    #[inline]
    pub fn execute_sstore(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "SSTORE requires exactly 2 operands".to_string()
            ));
        }

        let address = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };

        let index = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let value = match_ssa_input_stack_or_const!(&inputs[2], "Third");

        Ok(vec![SSAOutput::Storage {
            key: Box::new(StorageKey::Slot(address, *index)),
            value: Box::new(StorageValue::Slot(*value)),
        }])
    }

    /// Execute BALANCE operation
    #[inline]
    pub fn execute_balance(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "BALANCE requires exactly 1 operand".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let balance = match &inputs[1] {
            SSAInput::Storage {value, .. } => value.as_balance().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(balance)])
    }

    /// Execute SELFBALANCE operation
    #[inline]
    pub fn execute_selfbalance(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SELFBALANCE requires exactly 1 operand".to_string()
            ));
        }
        let _ = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_caller().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Stack value".to_string()
            )),
        };
        let balance = match &inputs[1] {
            SSAInput::Storage {value, .. } => value.as_balance().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(balance)])
    }

    /// Execute EXTCODESIZE operation
    #[inline]
    pub fn execute_extcodesize(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "EXTCODESIZE requires exactly 1 operand".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let code_size = match &inputs[1] {
            SSAInput::Storage {value, .. } => value.as_code_size().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(U256::from(code_size))])
    }

    /// Execute EXTCODEHASH operation
    #[inline]
    pub fn execute_extcodehash(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "EXTCODEHASH requires exactly 1 operand".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let code_hash = match &inputs[1] {
            SSAInput::Storage {value, .. } => value.as_code_hash().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };
        Ok(vec![SSAOutput::Stack(code_hash)])
    }

    /// Execute EXTCODECOPY operation
    #[inline]
    pub fn execute_extcodecopy(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 5 {
            return Err(ExecutionError::ExecutionError(
                "CODECOPY requires exactly 4 operands".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let mem_offset = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let code_offset = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let len = match_ssa_input_stack_or_const!(&inputs[3], "Fourth");
        let code = match &inputs[4] {
            SSAInput::Storage {value, .. } => value.as_code().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };
        let mem_offset = as_usize_saturated(*mem_offset);
        let code_offset = as_usize_saturated(*code_offset);
        let len = as_usize_saturated(*len);
        let code_len = min(code.len(), code_offset+len);
        let code_slice = &code[code_offset..code_len];
        // Pad code_slice to len
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code_slice.len()].copy_from_slice(&code_slice);
        let mut outputs = vec![SSAOutput::Memory(padded_code_slice.into())];

        let new_size = self.check_memory_size(mem_offset, len);
        if new_size > self.memory_size() {
            outputs.push(SSAOutput::MemorySize(new_size));
            self.set_memory_size(new_size);
        }
        Ok(outputs)
    }

    /// Execute BLOCKHASH operation
    #[inline]
    pub fn execute_blockhash(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "BLOCKHASH requires exactly 2 operand".to_string()
            ));
        }

        let number = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let number = as_u64_saturated(*number);
        let blockhash = self.get_blockhash(number);
        Ok(vec![SSAOutput::Stack(blockhash)])
    }

    /// Execute LOG operation
    #[inline]
    pub fn execute_log(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() < 4 {
            return Err(ExecutionError::ExecutionError(
                "LOG requires at least 4 operands".to_string()
            ));
        }
        let address = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };
        let _ = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let _ = match_ssa_input_stack_or_const!(&inputs[2], "Third");

        let memory = match &inputs[3] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Memory value".to_string()
            )),
        };

        let mut topics: Vec<FixedBytes<32>> = vec![];
        for i in 4..inputs.len() {
            let topic = match_ssa_input_stack_or_const!(&inputs[i], format!("Topic {}", i-3).as_str());
            topics.push(topic.to_be_bytes::<32>().into());
        }

        let log = Log {
            address: address,
            data: LogData::new(topics, memory.clone()).expect("LogData should have <=4 topics"),
        };

        Ok(vec![SSAOutput::Log(Box::new(log))])
    }

    /// Execute SELFDESTRUCT operation
    #[inline]
    pub fn execute_selfdestruct(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 4 {
            return Err(ExecutionError::ExecutionError(
                "SELFDESTRUCT requires exactly 4 operands".to_string()
            ));
        }
        let caller = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_caller().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be ContractEntry".to_string()
            )),
        };
        let caller_balance = match &inputs[1] {
            SSAInput::Storage { value, .. } => value.as_balance().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };
        let target = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let target_balance = match &inputs[3] {
            SSAInput::Storage { value, .. } => value.as_balance().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Storage value".to_string()
            )),
        };

        let new_caller_balance = caller_balance.saturating_add(target_balance);
        let new_target_balance = U256::ZERO;

        Ok(vec![SSAOutput::Storage { 
            key: Box::new(StorageKey::Balance(caller)),
            value: Box::new(StorageValue::Balance(new_caller_balance))
        },
        SSAOutput::Storage { 
            key: Box::new(StorageKey::Balance(Address::from_word(B256::from(*target)))), 
            value: Box::new(StorageValue::Balance(new_target_balance))
        },
        SSAOutput::InterpreterResult(SSAInterpreterResult{
            result: SSAInstructionResult::Ok,
            output: Bytes::default(),
        })] )
    }
}

