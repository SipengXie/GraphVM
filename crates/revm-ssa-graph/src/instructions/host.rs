use crate::{get_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result, SsaGraph};
use revm_interpreter::{gas, SStoreResult};
use revm_primitives::db::DatabaseRef;
use revm_primitives::{
    AccountStatus, Address, Bytecode, Bytes, FixedBytes, Log, LogData, Spec, U256,
};
use revm_ssa::{
    output_account_info, output_account_status, SSAInput, SSAInstructionResult,
    SSAInterpreterResult, SSALogEntry, SSAOutput, StorageKey, StorageValue,
};
use std::cmp::min;

use super::{
    as_u64_saturated, as_usize_saturated, get_contract_env, get_memory, get_storage_value,
    u256_to_bool,
};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute SLOAD operation
    #[inline(always)]
    pub fn execute_sload(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let value = get_storage_value!(graph, node.inputs[2], |key| self.get_state(key));
        let account_status = get_storage_value!(graph, node.inputs[3], |key| self.get_state(key));

        let value = if account_status
            .as_account_status()
            .unwrap()
            .contains(AccountStatus::Created)
        {
            U256::ZERO
        } else {
            *value.as_slot().unwrap()
        };
        node.outputs[0] = SSAOutput::Stack(value);

        Ok(())
    }

    /// Execute SSTORE operation
    #[inline(always)]
    pub fn execute_sstore(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let address = get_contract_env!(graph, node.inputs[0]).target_address;

        let index = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let origin_value = get_storage_value!(graph, node.inputs[3], |key| self.get_state(key));
        let present_value = get_storage_value!(graph, node.inputs[4], |key| self.get_state(key));
        let is_read = get_ssa_output_stack_or_const!(graph, node.inputs[5]);

        let origin_value = origin_value.as_slot().unwrap();
        let present_value = present_value.as_slot().unwrap();
        let is_read = u256_to_bool!(is_read).unwrap();

        let sstore_result = SStoreResult {
            original_value: origin_value.clone(),
            present_value: present_value.clone(),
            new_value: value.clone(),
        };

        let is_cold = match node.inputs[4] {
            SSAInput::Storage(_, lsn_with_index) => {
                lsn_with_index.0 == 0 // not been written before
            }
            _ => panic!("present value of sstore input is not a storage key"),
        } && !is_read; // not been read before

        let gas_cost = gas::sstore_cost(
            SPEC::SPEC_ID,
            &sstore_result,
            2301, /* just to bypass the gas check */
            is_cold,
        )
        .unwrap();
        let gas_refund = gas::sstore_refund(SPEC::SPEC_ID, &sstore_result);

        node.outputs[0] = SSAOutput::Storage {
            key: Box::new(StorageKey::Slot(address, index)),
            value: Box::new(StorageValue::Slot(value)),
        };
        node.outputs[1] = SSAOutput::Gas(gas_cost);
        node.outputs[2] = SSAOutput::GasRefund(gas_refund);

        Ok(())
    }

    #[inline(always)]
    pub fn execute_tstore(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let _address = get_contract_env!(graph, node.inputs[0]).target_address;
        let _index = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[2]);

        node.outputs[0] = SSAOutput::Transient(value);

        Ok(())
    }

    #[inline(always)]
    pub fn execute_tload(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let _address = get_contract_env!(graph, node.inputs[0]).target_address;
        let _index = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let value = match node.inputs[2] {
            SSAInput::Transient((lsn, index)) => {
                let dep_node = graph.get_node(lsn)?;
                match dep_node.outputs[index as usize] {
                    SSAOutput::Transient(value) => value,
                    _ => {
                        return Err(ExecutionError::ExecutionError(
                            ExecutionError::EXPECTED_TRANSIENT_VALUE.to_string(),
                        ))
                    }
                }
            }
            _ => {
                return Err(ExecutionError::ExecutionError(
                    ExecutionError::EXPECTED_TRANSIENT_VALUE.to_string(),
                ))
            }
        };

        node.outputs[0] = SSAOutput::Stack(value);

        Ok(())
    }

    /// Execute BALANCE operation
    #[inline(always)]
    pub fn execute_balance(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let account = get_storage_value!(graph, node.inputs[1], |key| self.get_state(key));
        let balance = account.as_account_info().unwrap().balance;
        node.outputs[0] = SSAOutput::Stack(balance);
        Ok(())
    }

    /// Execute SELFBALANCE operation
    #[inline(always)]
    pub fn execute_selfbalance(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let account = get_storage_value!(graph, node.inputs[1], |key| self.get_state(key));
        let balance = account.as_account_info().unwrap().balance;
        node.outputs[0] = SSAOutput::Stack(balance);
        Ok(())
    }

    /// Execute EXTCODESIZE operation
    #[inline(always)]
    pub fn execute_extcodesize(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let account = get_storage_value!(graph, node.inputs[1], |key| self.get_state(key));
        // we ignore EIP 7702 here
        let code = match &account.as_account_info().unwrap().code {
            Some(code) => code,
            None => &Bytecode::default(),
        };
        node.outputs[0] = SSAOutput::Stack(U256::from(code.len()));
        Ok(())
    }

    /// Execute EXTCODEHASH operation
    #[inline(always)]
    pub fn execute_extcodehash(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let account = get_storage_value!(graph, node.inputs[1], |key| self.get_state(key));
        let code_hash = account.as_account_info().unwrap().code_hash;
        node.outputs[0] = SSAOutput::Stack(code_hash.into());
        Ok(())
    }

    /// Execute EXTCODECOPY operation
    #[inline(always)]
    pub fn execute_extcodecopy(&mut self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let mem_offset = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let code_offset = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let len = get_ssa_output_stack_or_const!(graph, node.inputs[3]);
        let account_info = get_storage_value!(graph, node.inputs[4], |key| self.get_state(key));

        let mem_offset = as_usize_saturated!(mem_offset);
        let code_offset = as_usize_saturated!(code_offset);
        let len = as_usize_saturated!(len);
        let code = account_info
            .as_account_info()
            .unwrap()
            .code
            .as_ref()
            .unwrap()
            .original_bytes();

        // When len is 0, return an empty vector
        let padded_code_slice = if len == 0 {
            Vec::new()
        } else {
            let code_len = min(code.len(), code_offset + len);
            let code_slice = &code[code_offset..code_len];
            // Pad code_slice to len
            let mut padded_data = vec![0u8; len];
            padded_data[..code_slice.len()].copy_from_slice(&code_slice);
            padded_data
        };

        node.outputs[0] = SSAOutput::Memory(padded_code_slice.into());

        let new_size = self.check_memory_size(mem_offset, len);
        if new_size > self.memory_size() {
            if node.outputs.len() == 2 {
                // original memory size
                node.outputs[1] = SSAOutput::MemorySize(new_size);
            } else {
                node.outputs.push(SSAOutput::MemorySize(new_size));
            }
            self.set_memory_size(new_size);
        }

        Ok(())
    }

    /// Execute BLOCKHASH operation
    #[inline(always)]
    pub fn execute_blockhash(&mut self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let number = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let number = as_u64_saturated!(number);
        let blockhash = self.get_blockhash(number)?;
        node.outputs[0] = SSAOutput::Stack(blockhash);
        Ok(())
    }

    /// Execute LOG operation
    #[inline(always)]
    pub fn execute_log(&self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let address = get_contract_env!(graph, node.inputs[0]).target_address;
        let memory = get_memory!(graph, &node.inputs[3]);

        let mut topics: Vec<FixedBytes<32>> = vec![];
        for i in 4..node.inputs.len() {
            let topic = get_ssa_output_stack_or_const!(graph, node.inputs[i]);
            topics.push(topic.to_be_bytes::<32>().into());
        }

        let log = Log {
            address: address,
            data: LogData::new(topics, memory.into()).expect("LogData should have <=4 topics"),
        };

        node.outputs[0] = SSAOutput::Log(Box::new(log));

        Ok(())
    }

    /// Execute SELFDESTRUCT operation
    #[inline(always)]
    pub fn execute_selfdestruct(&mut self, node: &mut SSALogEntry, graph: &SsaGraph) -> Result<()> {
        let contract_address = get_contract_env!(graph, node.inputs[0]).target_address;
        let target = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let address_info = get_storage_value!(graph, node.inputs[2], |key| self.get_state(key));
        let target_info = get_storage_value!(graph, node.inputs[3], |key| self.get_state(key));
        let address_status = get_storage_value!(graph, node.inputs[4], |key| self.get_state(key));
        let is_cancun_enabled = get_ssa_output_stack_or_const!(graph, node.inputs[5]);

        let address_info = address_info.as_account_info().unwrap();
        let target_info = target_info.as_account_info().unwrap();
        let address_status = address_status.as_account_status().unwrap();
        let is_cancun_enabled = u256_to_bool!(is_cancun_enabled).unwrap();

        let target = Address::from_word(target.to_be_bytes::<32>().into());
        let is_created = address_status.contains(AccountStatus::Created);

        let mut index = 0;
        let outputs = &mut node.outputs;

        // Calculate the required number of outputs
        let mut required_outputs = 1; // At least one InterpreterResult output is needed

        // If contract address is not equal to target, we need one output to update target account info
        if contract_address != target {
            required_outputs += 1;
        }

        // If it's a newly created contract or Cancun is not enabled, we need two outputs
        // to update contract address account info and status
        if is_created || !is_cancun_enabled {
            required_outputs += 2;
        } else if contract_address != target {
            // If not newly created and Cancun is enabled, but contract address is not equal to target,
            // we need one output to update contract address account info
            required_outputs += 1;
        }

        // Ensure outputs has enough space
        if outputs.len() < required_outputs {
            outputs.resize(required_outputs, SSAOutput::Constant(U256::ZERO));
        }

        if contract_address != target {
            let mut new_target_info = target_info.clone();
            new_target_info.balance = new_target_info.balance.saturating_add(address_info.balance);
            outputs[index] = output_account_info!(target, new_target_info);
            index += 1;
        }

        if is_created || !is_cancun_enabled {
            let new_address_status = *address_status | AccountStatus::SelfDestructed;
            let mut new_address_info = address_info.clone();
            new_address_info.balance = U256::ZERO;
            outputs[index] = output_account_info!(contract_address, new_address_info);
            index += 1;
            outputs[index] = output_account_status!(contract_address, new_address_status);
            index += 1;
        } else if contract_address != target {
            let mut new_address_info = address_info.clone();
            new_address_info.balance = U256::ZERO;
            outputs[index] = output_account_info!(contract_address, new_address_info);
            index += 1;
        }

        let result = SSAOutput::InterpreterResult(SSAInterpreterResult {
            result: SSAInstructionResult::Ok,
            output: Bytes::default(),
        });
        outputs[index] = result;

        Ok(())
    }
}
