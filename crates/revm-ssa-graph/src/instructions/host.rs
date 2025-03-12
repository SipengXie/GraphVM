use std::cmp::min;
use revm_primitives::db::DatabaseRef;
use revm_primitives::{
    AccountStatus, Address, Bytecode, Bytes, FixedBytes, Log, LogData, Spec, U256
};
use revm_ssa::{
    output_account_info, output_account_status, SSAInstructionResult, SSAInterpreterResult, SSAOutput, StorageKey, StorageValue
};
use crate::{match_input, match_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result};

use super::{as_u64_saturated, as_usize_saturated, u256_to_bool};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute SLOAD operation
    #[inline]
    pub fn execute_sload(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 4 {
            return Err(ExecutionError::ExecutionError(
                "SLOAD requires exactly 1 operand".to_string()
            ));
        }

        let value = match_input!(inputs, 2, SSAOutput::Storage { value, .. } => value, "Third");
        let account_status = match_input!(inputs, 3, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Fourth");
        let value = if account_status.contains(AccountStatus::Created) {
            U256::ZERO
        } else {
            *value.as_slot().unwrap()
        };
        Ok(vec![SSAOutput::Stack(value)])
    }

    /// Execute SSTORE operation
    #[inline]
    pub fn execute_sstore(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "SSTORE requires exactly 2 operands".to_string()
            ));
        }

        let address = match_input!(inputs, 0, SSAOutput::ContractEnv(value) => value.target_address, "First");

        let index = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let value = match_ssa_output_stack_or_const!(&inputs[2], "Third");

        Ok(vec![SSAOutput::Storage {
            key: Box::new(StorageKey::Slot(address, *index)),
            value: Box::new(StorageValue::Slot(*value)),
        }])
    }

    /// Execute BALANCE operation
    #[inline]
    pub fn execute_balance(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "BALANCE requires exactly 2 operand".to_string()
            ));
        }
        // check first operand is stack/constant value
        match_ssa_output_stack_or_const!(&inputs[0], "First");
        let account_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Second");
        Ok(vec![SSAOutput::Stack(account_info.balance)])
    }

    /// Execute SELFBALANCE operation
    #[inline]
    pub fn execute_selfbalance(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SELFBALANCE requires exactly 2 operand".to_string()
            ));
        }
        let _target = match_input!(inputs, 0, SSAOutput::ContractEnv(value) => value.target_address, "First");
        let account_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Second");
        Ok(vec![SSAOutput::Stack(account_info.balance)])
    }

    /// Execute EXTCODESIZE operation
    #[inline]
    pub fn execute_extcodesize(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "EXTCODESIZE requires exactly 2 operand".to_string()
            ));
        }
        let _address = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let account_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Second");
        // we ignore EIP 7702 here
        let code = match &account_info.code {
            Some(code) => code,
            None => {
                &Bytecode::default()
            }
        };
        Ok(vec![SSAOutput::Stack(U256::from(code.len()))])
    }

    /// Execute EXTCODEHASH operation
    #[inline]
    pub fn execute_extcodehash(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "EXTCODEHASH requires exactly 2 operand".to_string()
            ));
        }
        match_ssa_output_stack_or_const!(&inputs[0], "First");
        let account_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Second");
        Ok(vec![SSAOutput::Stack(account_info.code_hash.into())])
    }

    /// Execute EXTCODECOPY operation
    #[inline]
    pub fn execute_extcodecopy(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 5 {
            return Err(ExecutionError::ExecutionError(
                "CODECOPY requires exactly 4 operands".to_string()
            ));
        }
        match_ssa_output_stack_or_const!(&inputs[0], "First");
        let mem_offset = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let code_offset = match_ssa_output_stack_or_const!(&inputs[2], "Third");
        let len = match_ssa_output_stack_or_const!(&inputs[3], "Fourth");
        let account_info = match_input!(inputs, 4, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Fifth");
        let code = account_info.code.as_ref().unwrap().original_bytes();
        
        let mem_offset = as_usize_saturated(*mem_offset);
        let code_offset = as_usize_saturated(*code_offset);
        let len = as_usize_saturated(*len);

        // When len is 0, return an empty vector
        let padded_code_slice = if len == 0 {
            Vec::new()
        } else {
            let code_len = min(code.len(), code_offset+len);
            let code_slice = &code[code_offset..code_len];
            // Pad code_slice to len
            let mut padded_data = vec![0u8; len];
            padded_data[..code_slice.len()].copy_from_slice(&code_slice);
            padded_data
        };
        
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
    pub fn execute_blockhash(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "BLOCKHASH requires exactly 2 operand".to_string()
            ));
        }

        let number = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let number = as_u64_saturated(*number);
        let blockhash = self.get_blockhash(number);
        Ok(vec![SSAOutput::Stack(blockhash)])
    }

    /// Execute LOG operation
    #[inline]
    pub fn execute_log(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() < 4 {
            return Err(ExecutionError::ExecutionError(
                "LOG requires at least 4 operands".to_string()
            ));
        }
        let address = match_input!(inputs, 0, SSAOutput::ContractEnv(value) => value.target_address, "First");
        let _ = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let _ = match_ssa_output_stack_or_const!(&inputs[2], "Third");
        let memory = match_input!(inputs, 3, SSAOutput::Memory(value) => value, "Fourth");

        let mut topics: Vec<FixedBytes<32>> = vec![];
        for i in 4..inputs.len() {
            let topic = match_ssa_output_stack_or_const!(&inputs[i], format!("Topic {}", i-3).as_str());
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
    pub fn execute_selfdestruct(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 6 {
            return Err(ExecutionError::ExecutionError(
                "SELFDESTRUCT requires exactly 6 operands".to_string()
            ));
        }
        let contract_address = match_input!(inputs, 0, SSAOutput::ContractEnv(value) => value.target_address, "First");
        let target = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let address_info = match_input!(inputs, 2, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Storage");
        let target_info = match_input!(inputs, 3, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Storage");
        let address_status = match_input!(inputs, 4, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Storage");
        let is_cancun_enabled = match_ssa_output_stack_or_const!(&inputs[5], "Fifth");

        
        let target = Address::from_word(target.to_be_bytes::<32>().into());
        let is_created = address_status.contains(AccountStatus::Created);
        let is_cancun_enabled = u256_to_bool(*is_cancun_enabled).unwrap();

        let mut outputs = Vec::with_capacity(4);

        if contract_address != target {
            let mut new_target_info = target_info.clone();
            new_target_info.balance = new_target_info.balance.saturating_add(address_info.balance);
            outputs.push(output_account_info!(target, new_target_info));
        }

        if is_created || !is_cancun_enabled {
            let new_address_status = *address_status | AccountStatus::SelfDestructed;
            let mut new_address_info = address_info.clone();
            new_address_info.balance = U256::ZERO;
            outputs.push(output_account_info!(contract_address, new_address_info));
            outputs.push(output_account_status!(contract_address, new_address_status));
        } else if contract_address != target {
            let mut new_address_info = address_info.clone();
            new_address_info.balance = U256::ZERO;
            outputs.push(output_account_info!(contract_address, new_address_info));
        }

        let result = SSAOutput::InterpreterResult(SSAInterpreterResult{
            result: SSAInstructionResult::Ok,
            output: Bytes::default(),
        });
        outputs.push(result);

        Ok(outputs)
    }
}

