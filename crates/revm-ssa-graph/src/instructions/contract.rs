use revm_primitives::db::DatabaseRef;
use revm_primitives::{keccak256, AccountInfo, AccountStatus, Bytecode};
use revm_primitives::{
    Address, Bytes, B256, Spec, U256,
};
use revm_ssa::logger::to_analysed;
use revm_ssa::{
    output_account_info, output_account_status, ContractEnv, SSAInput, SSACallInput, SSACallOutcome, SSACallScheme, SSACreateInput, SSACreateOutcome, SSACreateScheme, SSAInstructionResult, SSAInterpreterResult, SSALogEntry, SSAOutput, StorageKey, StorageValue
};
use crate::{get_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result, SsaGraph};

use super::{as_u64_saturated, as_usize_saturated, get_call_input, get_contract_env, get_interpreter_result, get_memory, get_storage_value, u256_to_bool};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {

    /// Execute deduct caller operation
    #[inline]
    pub fn execute_deduct_caller(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let caller = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let is_create = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let gas_cost = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let caller_info = get_storage_value!(graph, node.inputs[3], |key| self.get_state(key));

        let caller_info = caller_info.as_account_info().unwrap();
        let is_create = u256_to_bool!(is_create)?;
        let caller = Address::from_word(B256::from(caller));
        
        let new_caller_info = AccountInfo {
            balance: caller_info.balance - gas_cost,
            nonce: if is_create { caller_info.nonce } else { caller_info.nonce + 1 },
            code: caller_info.code.clone(),
            code_hash: caller_info.code_hash,
        };

        node.outputs[0] = SSAOutput::Storage {
            key: Box::new(StorageKey::AccountInfo(caller)),
            value: Box::new(StorageValue::AccountInfo(new_caller_info)),
        };

        Ok(())
    }

    #[inline]
    pub fn execute_refund_gas(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {

        let caller = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let refund_gas = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let caller_info = get_storage_value!(graph, node.inputs[2], |key| self.get_state(key));
        
        let caller = Address::from_word(B256::from(caller));
        let caller_info = caller_info.as_account_info().unwrap();
        let new_caller_info = AccountInfo {
            balance: caller_info.balance + refund_gas,
            nonce: caller_info.nonce,
            code: caller_info.code.clone(),
            code_hash: caller_info.code_hash,
        };
        
        node.outputs[0] = SSAOutput::Storage {
            key: Box::new(StorageKey::AccountInfo(caller)),
            value: Box::new(StorageValue::AccountInfo(new_caller_info)),
        };
        Ok(())
    }

    #[inline]
    pub fn execute_reward_beneficiary(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {

        let beneficiary = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let beneficiary_account_info = get_storage_value!(graph, node.inputs[1], |key| self.get_state(key));
        let reward = get_ssa_output_stack_or_const!(graph, node.inputs[2]);

        let beneficiary = Address::from_word(B256::from(beneficiary));
        let beneficiary_account_info = beneficiary_account_info.as_account_info().unwrap();
        let new_beneficiary_account_info = AccountInfo {
            balance: beneficiary_account_info.balance + reward,
            nonce: beneficiary_account_info.nonce,
            code: beneficiary_account_info.code.clone(),
            code_hash: beneficiary_account_info.code_hash,
        };

        node.outputs[0] = SSAOutput::Storage {
            key: Box::new(StorageKey::AccountInfo(beneficiary)),
            value: Box::new(StorageValue::AccountInfo(new_beneficiary_account_info)),
        };
        Ok(())
    }

    /// Execute call operation
    #[inline]
    pub fn execute_call(&mut self, node: &mut SSALogEntry, graph: & SsaGraph, opcode: u8) -> Result<()> {
        let gas_limit = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let to = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let in_offset = get_ssa_output_stack_or_const!(graph, node.inputs[3]);
        let in_len = get_ssa_output_stack_or_const!(graph, node.inputs[4]);
        let out_offset = get_ssa_output_stack_or_const!(graph, node.inputs[5]);
        let out_len = get_ssa_output_stack_or_const!(graph, node.inputs[6]);
        let input = get_memory!(graph, &node.inputs[7]);
        let target_address = get_contract_env!(graph, node.inputs[8]).target_address;

        let gas_limit = as_u64_saturated!(gas_limit);
        let out_offset = as_usize_saturated!(out_offset);
        let out_len = as_usize_saturated!(out_len);
        let in_offset = as_usize_saturated!(in_offset);
        let in_len = as_usize_saturated!(in_len);

        let ssa_call_input = SSACallInput {
            input: input.into(),
            target_address: Address::from_word(B256::from(to)),
            bytecode_address: Address::from_word(B256::from(to)),
            caller: target_address,
            transfer_value: value,
            scheme: match opcode {
                0xF1 => SSACallScheme::Call,
                _ => return Err(ExecutionError::ExecutionError(
                    "Invalid opcode".to_string()
                )),
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: gas_limit,
        };
        
        node.outputs[0] = SSAOutput::CallInput(Box::new(ssa_call_input));
        let new_size_1 = if in_len == 0 {
            0
        } else {
            self.check_memory_size(in_offset, in_len)
        };
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
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

    /// Execute callcode operation
    #[inline]
    pub fn execute_callcode(&mut self, node: &mut SSALogEntry, graph: & SsaGraph, opcode: u8) -> Result<()> {
        let gas_limit = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let to = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let in_offset = get_ssa_output_stack_or_const!(graph, node.inputs[3]);
        let in_len = get_ssa_output_stack_or_const!(graph, node.inputs[4]);
        let out_offset = get_ssa_output_stack_or_const!(graph, node.inputs[5]);
        let out_len = get_ssa_output_stack_or_const!(graph, node.inputs[6]);
        let input = get_memory!(graph, &node.inputs[7]);
        let contract_address = get_contract_env!(graph, node.inputs[8]).target_address;

        let gas_limit = as_u64_saturated!(gas_limit);
        let out_offset = as_usize_saturated!(out_offset);
        let out_len = as_usize_saturated!(out_len);
        let in_offset = as_usize_saturated!(in_offset);
        let in_len = as_usize_saturated!(in_len);
        let ssa_call_input = SSACallInput {
            input: input.into(),
            target_address: contract_address,
            bytecode_address: Address::from_word(B256::from(to)),
            caller: contract_address,
            transfer_value: value,
            scheme: match opcode {
                0xF2 => SSACallScheme::CallCode,
                _ => return Err(ExecutionError::ExecutionError(
                    "Invalid opcode".to_string()
                )),
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: gas_limit,
        };
        
        node.outputs[0] = SSAOutput::CallInput(Box::new(ssa_call_input));
        let new_size_1 = self.check_memory_size(in_offset, in_len);
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
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

    /// Execute delegatecall operation
    #[inline]
    pub fn execute_delegatecall(&mut self, node: &mut SSALogEntry, graph: & SsaGraph, opcode: u8) -> Result<()> {
        let gas_limit = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let to = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let in_offset = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let in_len = get_ssa_output_stack_or_const!(graph, node.inputs[3]);
        let out_offset = get_ssa_output_stack_or_const!(graph, node.inputs[4]);
        let out_len = get_ssa_output_stack_or_const!(graph, node.inputs[5]);
        let input = get_memory!(graph, &node.inputs[6]);
        let contract_address = get_contract_env!(graph, node.inputs[7]).target_address;
        let caller = get_contract_env!(graph, node.inputs[8]).caller;

        let gas_limit = as_u64_saturated!(gas_limit);
        let out_offset = as_usize_saturated!(out_offset);
        let out_len = as_usize_saturated!(out_len);
        let in_offset = as_usize_saturated!(in_offset);
        let in_len = as_usize_saturated!(in_len);

        let ssa_call_input = SSACallInput {
            input: input.into(),
            target_address: contract_address,
            bytecode_address: Address::from_word(B256::from(to)),
            caller,
            transfer_value: U256::ZERO,
            scheme: match opcode {
                0xF4 => SSACallScheme::DelegateCall,
                _ => return Err(ExecutionError::ExecutionError(
                    "Invalid opcode".to_string()
                )),
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: gas_limit,
        };
        
        node.outputs[0] = SSAOutput::CallInput(Box::new(ssa_call_input));
        let new_size_1 = self.check_memory_size(in_offset, in_len);
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
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

    /// Execute staticcall operation
    #[inline]
    pub fn execute_staticcall(&mut self, node: &mut SSALogEntry, graph: & SsaGraph, opcode: u8) -> Result<()> {
        let gas_limit = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let to = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let in_offset = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let in_len = get_ssa_output_stack_or_const!(graph, node.inputs[3]);
        let out_offset = get_ssa_output_stack_or_const!(graph, node.inputs[4]);
        let out_len = get_ssa_output_stack_or_const!(graph, node.inputs[5]);
        let input = get_memory!(graph, &node.inputs[6]);
        let contract_address = get_contract_env!(graph, node.inputs[7]).target_address;

        let gas_limit = as_u64_saturated!(gas_limit);
        let out_offset = as_usize_saturated!(out_offset);
        let out_len = as_usize_saturated!(out_len);
        let in_offset = as_usize_saturated!(in_offset);
        let in_len = as_usize_saturated!(in_len);
        let to_addr = Address::from_word(B256::from(to));

        let ssa_call_input = SSACallInput {
            input: input.into(),
            target_address: to_addr,
            bytecode_address: to_addr,
            caller: contract_address,
            transfer_value: U256::ZERO,
            scheme: match opcode {
                0xFA => SSACallScheme::StaticCall,
                _ => return Err(ExecutionError::ExecutionError(
                    "Invalid opcode".to_string()
                )),
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: gas_limit,
        };
        
        node.outputs[0] = SSAOutput::CallInput(Box::new(ssa_call_input));
        let new_size_1 = self.check_memory_size(in_offset, in_len);
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
        if new_size > self.memory_size() {
            self.set_memory_size(new_size);
            if node.outputs.len() < 2 {
                node.outputs.push(SSAOutput::MemorySize(new_size));
            } else {
                node.outputs[1] = SSAOutput::MemorySize(new_size);
            }
        }

        Ok(())
    }

    /// Execute make call frame operation
    /// The initial call frame is created by the evm, we should take from the ssa_logger
    #[inline]
    pub fn execute_make_call_frame(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let call_input = get_call_input!(graph, node.inputs[0], self.get_first_call_input().unwrap());
        let caller_info = get_storage_value!(graph, node.inputs[1], |key| self.get_state(key));
        let target_info = get_storage_value!(graph, node.inputs[2], |key| self.get_state(key));
        let bytecode_info = get_storage_value!(graph, node.inputs[3], |key| self.get_state(key));
        
        let caller_info = caller_info.as_account_info().unwrap();
        let target_info = target_info.as_account_info().unwrap();
        let bytecode_info = bytecode_info.as_account_info().unwrap();

        let value = call_input.transfer_value;
        let caller = call_input.caller;
        let target_address = call_input.target_address;
        let bytecode_address = call_input.bytecode_address;

        let outputs = &mut node.outputs;
        outputs.clear();

        if !value.is_zero() {
            let new_caller_info = AccountInfo {
                nonce: caller_info.nonce,
                balance: caller_info.balance.saturating_sub(value),
                code: caller_info.code.clone(),
                code_hash: caller_info.code_hash,
            };
            let new_target_info = AccountInfo {
                nonce: target_info.nonce,
                balance: target_info.balance.saturating_add(value),
                code: target_info.code.clone(),
                code_hash: target_info.code_hash,
            };
            outputs.push(output_account_info!(caller, new_caller_info));
            outputs.push(output_account_info!(target_address, new_target_info));
        }

        let bytecode = bytecode_info.code.clone().unwrap_or_default();

        if self.is_precompile(&bytecode_address) {
            // if is precompile ..
            let precompile = self.call_precompile(&bytecode_address, &call_input.input, call_input.gas_limit);
            outputs.push(SSAOutput::InterpreterResult(precompile));
        } else if bytecode.is_empty() {
            // if is simple transfer ..
            let ssa_interpreter_result = SSAInterpreterResult {
                result: SSAInstructionResult::Ok,
                output: Bytes::default(),
            };
            outputs.push(SSAOutput::InterpreterResult(ssa_interpreter_result));
        } else {
            // if is contract call
            let contract = ContractEnv {
                input: call_input.input.clone(),
                bytecode: bytecode_info.code.clone().unwrap_or_default(),
                hash: Some(bytecode_info.code_hash()),
                target_address: target_address,
                bytecode_address: Some(bytecode_address),
                caller: caller,
                call_value: value,
            };
            outputs.push(SSAOutput::ContractEnv(Box::new(contract)));
        }
        
        Ok(())
    }

    /// Execute call return operation
    #[inline]
    pub fn execute_call_return(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let interpreter_result = get_interpreter_result!(graph, node.inputs[0]);
        let call_input = get_call_input!(graph, node.inputs[1], self.get_first_call_input().unwrap());

        let ret_range = call_input.ret_range.clone();
        
        node.outputs[0] = SSAOutput::CallOutcome(Box::new(SSACallOutcome {
            result: interpreter_result.clone(),
            ret_range: ret_range,
        }));

        Ok(())
    }

    /// Execute insert call outcome operation
    #[inline]
    pub fn execute_insert_call_outcome(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {

        let call_outcome = match node.inputs[0] {
            SSAInput::CallOutcome((lsn, index)) => {
                let dep_node = graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::CallOutcome(outcome) => outcome,
                    _ => return Err(ExecutionError::ExecutionError("Expected CallOutcome output value".to_string()))
                }
            }
            _ => return Err(ExecutionError::ExecutionError("Expected CallOutcome input value".to_string()))
        };

        let out_len = call_outcome.ret_range.len();
        let return_data_buffer = call_outcome.result.output.clone();

        node.outputs[0] = SSAOutput::ReturnDataBuffer(return_data_buffer.clone());

        let data_slice = if out_len == 0 {
            &[] as &[u8]
        } else {
            let target_len = std::cmp::min(out_len, return_data_buffer.len());
            &return_data_buffer[..target_len]
        };
        match call_outcome.result.result {
            SSAInstructionResult::Ok => {
                node.outputs[1] = SSAOutput::Memory(data_slice.to_vec().into());
                node.outputs[2] = SSAOutput::Stack(U256::from(1));
            },
            SSAInstructionResult::Revert => {
                node.outputs[1] = SSAOutput::Memory(data_slice.to_vec().into());
                node.outputs[2] = SSAOutput::Stack(U256::ZERO);
            },
            SSAInstructionResult::Error => {
                return Err(ExecutionError::ExecutionError(
                    "Error in insert_call_outcome".to_string()
                ));
            }
        }
        Ok(())
    }

    /// Execute create operation
    #[inline]
    pub fn execute_create(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {

        let value = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let code_offset = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        let len = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
        let code = get_memory!(graph, &node.inputs[3]);
        let contract_address = get_contract_env!(graph, node.inputs[4]).target_address;
        let salt = if node.inputs.len() == 6 {
            Some(get_ssa_output_stack_or_const!(graph, node.inputs[5]))
        } else {
            None
        };

        let len = as_usize_saturated!(len);
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code.len()].copy_from_slice(&code);


        let ssa_create_input = SSACreateInput {
            init_code: padded_code_slice.into(),
            value: value,
            caller: contract_address,
            scheme: if salt.is_some() {
                SSACreateScheme::Create2 { salt: salt.unwrap() }
            } else {
                SSACreateScheme::Create
            },
        };

        node.outputs[0] = SSAOutput::CreateInput(Box::new(ssa_create_input));

        let new_size = self.check_memory_size(as_usize_saturated!(code_offset), len);

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

    /// Execute make create frame operation
    #[inline]
    pub fn execute_make_create_frame(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        
        let create_input = match node.inputs[0] {
            SSAInput::CreateInput((lsn, index)) => {
                let dep_node = graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::CreateInput(input) => input,
                    _ => return Err(ExecutionError::ExecutionError("Expected CreateInput output value".to_string()))
                }
            },
            _ => return Err(ExecutionError::ExecutionError("Expected CreateInput input value".to_string()))
        };
        
        let caller_info = get_storage_value!(graph, node.inputs[1], |key| self.get_state(key));
        let created_info = get_storage_value!(graph, node.inputs[2], |key| self.get_state(key));
        
        let caller_info = caller_info.as_account_info().unwrap();
        let created_info = created_info.as_account_info().unwrap();

        let caller = create_input.caller;
        let mut init_code_hash = B256::ZERO;
        let target = match create_input.scheme {
            SSACreateScheme::Create => caller.create(caller_info.nonce),
            SSACreateScheme::Create2 { salt } => {
                init_code_hash = keccak256(&create_input.init_code);
                caller.create2(salt.to_be_bytes(), init_code_hash)
            }
        };
        
        let new_caller_info = AccountInfo {
            balance: caller_info.balance - create_input.value,
            nonce: caller_info.nonce + 1,
            code_hash: caller_info.code_hash,
            code: caller_info.code.clone(),
        };

        let new_created_info = AccountInfo {
            balance: created_info.balance + create_input.value,
            nonce: 1,
            code_hash: created_info.code_hash,
            code: created_info.code.clone(),
        };

        let new_created_status = AccountStatus::Created;

        let bytecode = Bytecode::new_legacy(create_input.init_code.clone());
        let contract_env = ContractEnv {
            input: Bytes::new(),
            bytecode,
            caller,
            hash: Some(init_code_hash),
            target_address: target,
            bytecode_address: None,
            call_value: create_input.value,
        };

        node.outputs[0] = output_account_info!(caller, new_caller_info);
        node.outputs[1] = output_account_info!(target, new_created_info);
        node.outputs[2] = output_account_status!(target, new_created_status);
        node.outputs[3] = SSAOutput::ContractEnv(Box::new(contract_env));

        Ok(())
    }

    /// Execute create return operation
    #[inline]
    pub fn execute_create_return(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {

        if node.inputs.len() == 1 {
            let interpreter_result = get_interpreter_result!(graph, node.inputs[0]);
            node.outputs[0] = SSAOutput::CreateOutcome(Box::new(SSACreateOutcome {
                result: interpreter_result.clone(),
                address: None,
            }));
            return Ok(());
        }
        
        let interpreter_result = get_interpreter_result!(graph, node.inputs[0]);
        let address = get_contract_env!(graph, node.inputs[1]).target_address;
        let target_info = get_storage_value!(graph, node.inputs[2], |key| self.get_state(key));
        let analysis_kind = get_ssa_output_stack_or_const!(graph, node.inputs[3]);
        let analysis_kind = u256_to_bool!(analysis_kind)?;
        let target_info = target_info.as_account_info().unwrap();
        // TODO: Gas metering and error handling

        let create_outcome = SSACreateOutcome {
            result: interpreter_result.clone(),
            address: Some(address),
        };

        let raw_code = interpreter_result.output.clone();
        let bytecode = if analysis_kind {
            to_analysed(Bytecode::new_legacy(raw_code))
        } else {
            Bytecode::new_legacy(raw_code)
        };
        let codehash = bytecode.hash_slow();

        let new_target_info = AccountInfo {
            balance: target_info.balance,
            nonce: target_info.nonce,
            code_hash: codehash,
            code: Some(bytecode),
        };

        node.outputs[0] = SSAOutput::CreateOutcome(Box::new(create_outcome));
        node.outputs[1] = output_account_info!(address, new_target_info);

        Ok(())
    }

    /// Execute insert create outcome operation
    #[inline]
    pub fn execute_insert_create_outcome(&mut self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let create_outcome = match node.inputs[0] {
            SSAInput::CreateOutcome((lsn, index)) => {
                let dep_node = graph.get_node(lsn)?;
                match &dep_node.outputs[index as usize] {
                    SSAOutput::CreateOutcome(outcome) => outcome,
                    _ => return Err(ExecutionError::ExecutionError("Expected CreateOutcome output value".to_string()))
                }
            },
            _ => return Err(ExecutionError::ExecutionError("Expected CreateOutcome input value".to_string()))
        };
        let address = create_outcome.address;
        let instruction_result = create_outcome.result.result;
        let return_data_buffer = if instruction_result.is_revert() {
            create_outcome.result.output.clone()
        } else {
            Bytes::new()
        };

        node.outputs[0] = SSAOutput::ReturnDataBuffer(return_data_buffer.clone());

        match instruction_result {
            SSAInstructionResult::Ok => {
                let address = address.unwrap();
                node.outputs[1] = SSAOutput::Stack(address.into_word().into());
            }
            SSAInstructionResult::Revert => {
                node.outputs[1] = SSAOutput::Stack(U256::ZERO);
            }
            SSAInstructionResult::Error => {
                return Err(ExecutionError::ExecutionError(
                    "Error in insert_create_outcome".to_string()
                ));
            }
        }

        Ok(())
    }

}

