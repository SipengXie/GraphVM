use revm_primitives::db::DatabaseRef;
use revm_primitives::{keccak256, AccountInfo, AccountStatus, Bytecode};
use revm_primitives::{
    Address, Bytes, B256, Spec, U256,
};
use revm_ssa::logger::to_analysed;
use revm_ssa::{
    output_account_info, output_account_status, ContractEnv, SSACallInput, SSACallOutcome, SSACallScheme, SSACreateInput, SSACreateOutcome, SSACreateScheme, SSAInstructionResult, SSAInterpreterResult, SSAOutput, StorageKey, StorageValue
};
use crate::{match_input, match_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result};

use super::{as_u64_saturated, as_usize_saturated, u256_to_bool};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {

    /// Execute deduct caller operation
    #[inline]
    pub fn execute_deduct_caller(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 5 {
            return Err(ExecutionError::ExecutionError(
                "DEDUCT_CALLER requires at least 3 operands".to_string()
            ));
        }
        let caller = match_input!(inputs, 0, SSAOutput::Constant(value) => value, "First");
        let is_call = match_input!(inputs, 1, SSAOutput::Constant(value, ..) => u256_to_bool(*value)?, "Second");
        let caller_info = match_input!(inputs, 2, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Third");
        let caller_status = match_input!(inputs, 3, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Fourth");
        let gas_cost = match_input!(inputs, 4, SSAOutput::Constant ( value, .. )=> value, "Fifth");

        let caller = Address::from_word(B256::from(*caller));
        let new_caller_info = AccountInfo {
            balance: caller_info.balance - gas_cost,
            nonce: if is_call { caller_info.nonce } else { caller_info.nonce + 1 },
            code: caller_info.code.clone(),
            code_hash: caller_info.code_hash,
        };
        let new_caller_status = *caller_status | AccountStatus::Touched;

        let outputs = vec![
            SSAOutput::Storage {
                key: Box::new(StorageKey::AccountInfo(caller)),
                value: Box::new(StorageValue::AccountInfo(new_caller_info)),
            },
            SSAOutput::Storage {
                key: Box::new(StorageKey::AccountStatus(caller)),
                value: Box::new(StorageValue::AccountStatus(new_caller_status)),
            },
        ];

        Ok(outputs)
    }

    #[inline]
    pub fn execute_refund_gas(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        // eprintln!("Refund Gas");
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "REFUND_GAS requires exactly 2 operands (caller, refund_gas)".to_string()
            ));
        }
        let caller = match_input!(inputs, 0, SSAOutput::Constant(value) => value, "First");
        let caller_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Second");
        let refund_gas = match_input!(inputs, 2, SSAOutput::Constant(value)=> value, "Third");
        
        let caller = Address::from_word(B256::from(*caller));
        let new_caller_info = AccountInfo {
            balance: caller_info.balance + refund_gas,
            nonce: caller_info.nonce,
            code: caller_info.code.clone(),
            code_hash: caller_info.code_hash,
        };
        
        let outputs = vec![
            SSAOutput::Storage {
                key: Box::new(StorageKey::AccountInfo(caller)),
                value: Box::new(StorageValue::AccountInfo(new_caller_info)),
            },
        ];
        Ok(outputs)
    }

    #[inline]
    pub fn execute_reward_beneficiary(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 4 {
            return Err(ExecutionError::ExecutionError(
                "REWARD_BENEFICIARY requires exactly 2 operands".to_string()
            ));
        }
        let beneficiary = match_input!(inputs, 0, SSAOutput::Constant (value) => value, "First");
        let beneficiary_account_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Second");
        let beneficiary_account_status = match_input!(inputs, 2, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Third");
        let reward = match_input!(inputs, 3, SSAOutput::Constant(value)=> value, "Fourth");

        let beneficiary = Address::from_word(B256::from(*beneficiary));
        let new_beneficiary_account_info = AccountInfo {
            balance: beneficiary_account_info.balance + reward,
            nonce: beneficiary_account_info.nonce,
            code: beneficiary_account_info.code.clone(),
            code_hash: beneficiary_account_info.code_hash,
        };
        let new_beneficiary_account_status = *beneficiary_account_status | AccountStatus::Touched;

        let outputs = vec![
            SSAOutput::Storage {
                key: Box::new(StorageKey::AccountInfo(beneficiary)),
                value: Box::new(StorageValue::AccountInfo(new_beneficiary_account_info)),
            },
            SSAOutput::Storage {
                key: Box::new(StorageKey::AccountStatus(beneficiary)),
                value: Box::new(StorageValue::AccountStatus(new_beneficiary_account_status)),
            },
        ];
        Ok(outputs)
    }
    /// Execute call operation
    #[inline]
    pub fn execute_call(&mut self, inputs: Vec<SSAOutput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 9 {
            return Err(ExecutionError::ExecutionError(
                "CALL requires exactly 9 operands (gas, to, value, in_offset, in_len, out_offset, out_len, input)".to_string()
            ));
        }
        let gas_limit = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let value = match_ssa_output_stack_or_const!(&inputs[2], "Third");
        let in_offset = match_ssa_output_stack_or_const!(&inputs[3], "Fourth");
        let in_len = match_ssa_output_stack_or_const!(&inputs[4], "Fifth");
        let out_offset = match_ssa_output_stack_or_const!(&inputs[5], "Sixth");
        let out_len = match_ssa_output_stack_or_const!(&inputs[6], "Seventh");
        let input = match &inputs[7] {
            SSAOutput::Memory (value) => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Eighth operand must be Memory".to_string()
            )),
        };
        let target_address = match &inputs[8] {
            SSAOutput::ContractEnv(value) => value.target_address,
            _ => return Err(ExecutionError::ExecutionError(
                "Ninth operand must be ContractEnv".to_string()
            )),
        };
        let gas_limit = as_u64_saturated(*gas_limit);
        let out_offset = as_usize_saturated(*out_offset);
        let out_len = as_usize_saturated(*out_len);
        let in_offset = as_usize_saturated(*in_offset);
        let in_len = as_usize_saturated(*in_len);

        let ssa_call_input = SSACallInput {
            input: input.clone(),
            target_address: Address::from_word(B256::from(*to)),
            bytecode_address: Address::from_word(B256::from(*to)),
            caller: target_address,
            transfer_value: *value,
            scheme: match opcode {
                0xF1 => SSACallScheme::Call,
                _ => return Err(ExecutionError::ExecutionError(
                    "Invalid opcode".to_string()
                )),
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: gas_limit,
        };
        
        let mut outputs = vec![SSAOutput::CallInput(Box::new(ssa_call_input))];
        let new_size_1 = if in_len == 0 {
            0
        } else {
            self.check_memory_size(in_offset, in_len)
        };
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
        if new_size > self.memory_size() {
            self.set_memory_size(new_size);
            outputs.push(SSAOutput::MemorySize(new_size));
        }

        Ok(outputs)
    }

    /// Execute make call frame operation
    /// The initial call frame is created by the evm, we should take from the ssa_logger
    /// TODO: achieve precompile in this function
    #[inline]
    pub fn execute_make_call_frame(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 6 {
            return Err(ExecutionError::ExecutionError(
                "MAKE_CALL_FRAME requires at least 6 operands".to_string()
            ));

        }

        let call_input = match_input!(inputs, 0, SSAOutput::CallInput(value) => value, "CallFrame");
        let caller_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Storage(AccountInfo)");
        let target_info = match_input!(inputs, 2, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Storage(AccountInfo)");
        let bytecode_info = match_input!(inputs, 3, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Storage(AccountInfo)");
        let caller_status = match_input!(inputs, 4, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Storage(AccountStatus)");
        let target_status = match_input!(inputs, 5, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Storage(AccountStatus)");
        
        let value = call_input.transfer_value;
        let caller = call_input.caller;
        let target_address = call_input.target_address;
        let bytecode_address = call_input.bytecode_address;

        let mut outputs = Vec::with_capacity(5);

        if !value.is_zero() {
            let new_caller_info = AccountInfo {
                nonce: caller_info.nonce,
                balance: caller_info.balance.saturating_sub(value),
                code: caller_info.code.clone(),
                code_hash: caller_info.code_hash,
            };
            let new_caller_status = *caller_status | AccountStatus::Touched;
            let new_target_info = AccountInfo {
                nonce: target_info.nonce,
                balance: target_info.balance.saturating_add(value),
                code: target_info.code.clone(),
                code_hash: target_info.code_hash,
            };
            let new_target_status = *target_status | AccountStatus::Touched;
            outputs.push(output_account_info!(caller, new_caller_info));
            outputs.push(output_account_status!(caller, new_caller_status));
            outputs.push(output_account_info!(target_address, new_target_info));
            outputs.push(output_account_status!(target_address, new_target_status));
        } else {
            let new_target_status = *target_status | AccountStatus::Touched;
            outputs.push(output_account_status!(target_address, new_target_status));
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
                bytecode_address: Some(target_address),
                caller: caller,
                call_value: value,
            };
            outputs.push(SSAOutput::ContractEnv(Box::new(contract)));
        }
        
        Ok(outputs)
    }

    /// Execute call return operation
    #[inline]
    pub fn execute_call_return(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "CALL_RETURN requires exactly 2 operands (interpreter_result, call_input)".to_string()
            ));
        }

        let interpreter_result = match_input!(inputs, 0, SSAOutput::InterpreterResult(result) => result, "InterpreterResult");
        let call_input = match_input!(inputs, 1, SSAOutput::CallInput(input) => input, "CallFrame");

        let ret_range = call_input.ret_range.clone();

        Ok(vec![
            SSAOutput::CallOutcome(Box::new(SSACallOutcome {
                result: interpreter_result.clone(),
                ret_range: ret_range,
            }))
        ])
    }

    /// Execute insert call outcome operation
    #[inline]
    pub fn execute_insert_call_outcome(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "INSERT_CALL_OUTCOME requires exactly 1 operand (call_outcome)".to_string()
            ));
        }

        let call_outcome = match_input!(inputs, 0, SSAOutput::CallOutcome(outcome) => outcome, "CallOutcome");

        let out_len = call_outcome.ret_range.len();
        let return_data_buffer = call_outcome.result.output.clone();
        let mut outputs = vec![
            SSAOutput::ReturnDataBuffer(return_data_buffer.clone()),
        ];

        let target_len = std::cmp::min(out_len, return_data_buffer.len());
        let data_slice = &return_data_buffer[..target_len];
        match call_outcome.result.result {
            SSAInstructionResult::Ok => {
                outputs.push(SSAOutput::Memory(data_slice.to_vec().into()));
                outputs.push(SSAOutput::Stack(U256::from(1)));
            },
            SSAInstructionResult::Revert => {
                outputs.push(SSAOutput::Memory(data_slice.to_vec().into()));
                outputs.push(SSAOutput::Stack(U256::ZERO));
            },
            SSAInstructionResult::Error => {
                return Err(ExecutionError::ExecutionError(
                    "Error in insert_call_outcome".to_string()
                ));
            }
        }
        // eprintln!("outputs: {:?}", outputs);
        Ok(outputs)
    }

    /// Execute create operation
    #[inline]
    pub fn execute_create(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() < 5 {
            return Err(ExecutionError::ExecutionError(
                "CREATE requires at least 5 operands (value, code_offset, len, code, caller)".to_string()
            ));
        }
        let value = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let code_offset = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let len = match_ssa_output_stack_or_const!(&inputs[2], "Third");
        let code = match_input!(inputs, 3, SSAOutput::Memory(value) => value, "Fourth");
        let contract_address = match_input!(inputs, 4, SSAOutput::ContractEnv(value) => value.target_address, "Fifth");
        let salt = if inputs.len() == 6 {
            Some(match_ssa_output_stack_or_const!(&inputs[5], "Sixth"))
        } else {
            None
        };

        let len = as_usize_saturated(*len);
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code.len()].copy_from_slice(&code);


        let ssa_create_input = SSACreateInput {
            init_code: padded_code_slice.into(),
            value: *value,
            caller: contract_address,
            scheme: if salt.is_some() {
                SSACreateScheme::Create2 { salt: *salt.unwrap() }
            } else {
                SSACreateScheme::Create
            },
        };
        let mut outputs = vec![SSAOutput::CreateInput(Box::new(ssa_create_input))];

        let new_size = self.check_memory_size(as_usize_saturated(*code_offset), len);
        if new_size > self.memory_size() {
            self.set_memory_size(new_size);
            outputs.push(SSAOutput::MemorySize(new_size));
        }


        Ok(outputs)
    }

    /// Execute make create frame operation
    #[inline]
    pub fn execute_make_create_frame(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        // eprintln!("execute_make_create_frame: {:?}", inputs);cl
        if inputs.len() != 5 {
            return Err(ExecutionError::ExecutionError(
                "MAKE_CREATE_FRAME requires exactly 5 operands".to_string()
            ));
        }

        let create_input = match_input!(inputs, 0, SSAOutput::CreateInput(input) => input, "First");
        let caller_info = match_input!(inputs, 1, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Second");
        let created_info = match_input!(inputs, 2, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Third");
        let caller_status = match_input!(inputs, 3, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Fourth");
        let created_status = match_input!(inputs, 4, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Fifth");

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

        let new_caller_status = *caller_status | AccountStatus::Touched;
        let new_created_status = *created_status | AccountStatus::Created;

        let bytecode = Bytecode::new_legacy(create_input.init_code.clone());
        let contract_env = ContractEnv {
            input: Bytes::new(),
            bytecode: bytecode,
            caller: caller,
            hash: Some(init_code_hash),
            target_address: target,
            bytecode_address: None,
            call_value: create_input.value,
        };

        let mut outputs = Vec::with_capacity(5);
        outputs.push(output_account_info!(caller, new_caller_info));
        outputs.push(output_account_info!(target, new_created_info));
        outputs.push(output_account_status!(caller, new_caller_status));
        outputs.push(output_account_status!(target, new_created_status));
        outputs.push(SSAOutput::ContractEnv(Box::new(contract_env)));

        Ok(outputs)
    }

    /// Execute create return operation
    #[inline]
    pub fn execute_create_return(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 5 {
            return Err(ExecutionError::ExecutionError(
                "CREATE_RETURN requires exactly 5 operands".to_string()
            ));
        }

        let interpreter_result = match_input!(inputs, 0, SSAOutput::InterpreterResult(result) => result, "First");
        let address = match_input!(inputs, 1, SSAOutput::ContractEnv(input) => input.target_address, "Second");
        let target_info = match_input!(inputs, 2, SSAOutput::Storage { value, .. } => value.as_account_info().unwrap(), "Third");
        let target_status = match_input!(inputs, 3, SSAOutput::Storage { value, .. } => value.as_account_status().unwrap(), "Fourth");
        let analysis_kind = match_input!(inputs, 4, SSAOutput::Constant(value) => value, "Fifth");
        let analysis_kind = u256_to_bool(*analysis_kind)?;

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
        let new_target_status = *target_status | AccountStatus::Touched;

        let mut outputs = Vec::with_capacity(3);
        outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
        outputs.push(output_account_info!(address, new_target_info));
        outputs.push(output_account_status!(address, new_target_status));

        Ok(outputs)
    }

    /// Execute insert create outcome operation
    #[inline]
    pub fn execute_insert_create_outcome(&mut self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "INSERT_CREATE_OUTCOME requires exactly 1 operand (create_outcome)".to_string()
            ));
        }

        let create_outcome = match_input!(inputs, 0, SSAOutput::CreateOutcome(outcome) => outcome, "First");

        let address = create_outcome.address;
        let instruction_result = create_outcome.result.result;
        let return_data_buffer = if instruction_result.is_revert() {
            create_outcome.result.output.clone()
        } else {
            Bytes::new()
        };

        let mut outputs = vec![
            SSAOutput::ReturnDataBuffer(return_data_buffer.clone()),
        ];

        match instruction_result {
            SSAInstructionResult::Ok => {
                let address = address.unwrap();
                outputs.push(SSAOutput::Stack(address.into_word().into()));
            }
            SSAInstructionResult::Revert => {
                outputs.push(SSAOutput::Stack(U256::ZERO));
            }
            SSAInstructionResult::Error => {
                return Err(ExecutionError::ExecutionError(
                    "Error in insert_create_outcome".to_string()
                ));
            }
        }

        Ok(outputs)
    }

    /// Execute callcode operation
    #[inline]
    pub fn execute_callcode(&mut self, inputs: Vec<SSAOutput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 9 {
            return Err(ExecutionError::ExecutionError(
                "CALLCODE requires exactly 9 operands (gas, to, value, in_offset, in_len, out_offset, out_len, input)".to_string()
            ));
        }
        let gas_limit = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let value = match_ssa_output_stack_or_const!(&inputs[2], "Third");
        let in_offset = match_ssa_output_stack_or_const!(&inputs[3], "Fourth");
        let in_len = match_ssa_output_stack_or_const!(&inputs[4], "Fifth");
        let out_offset = match_ssa_output_stack_or_const!(&inputs[5], "Sixth");
        let out_len = match_ssa_output_stack_or_const!(&inputs[6], "Seventh");
        let input = match_input!(inputs, 7, SSAOutput::Memory(value) => value, "Eighth");
        let target = match_input!(inputs, 8, SSAOutput::ContractEnv(value) => value.target_address, "Ninth");

        let gas_limit = as_u64_saturated(*gas_limit);
        let out_offset = as_usize_saturated(*out_offset);
        let out_len = as_usize_saturated(*out_len);
        let in_offset = as_usize_saturated(*in_offset);
        let in_len = as_usize_saturated(*in_len);
        let ssa_call_input = SSACallInput {
            input: input.clone(),
            target_address: target,
            bytecode_address: Address::from_word(B256::from(*to)),
            caller: target,
            transfer_value: *value,
            scheme: match opcode {
                0xF2 => SSACallScheme::CallCode,
                _ => return Err(ExecutionError::ExecutionError(
                    "Invalid opcode".to_string()
                )),
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: gas_limit,
        };
        
        let mut outputs = vec![SSAOutput::CallInput(Box::new(ssa_call_input))];
        let new_size_1 = self.check_memory_size(in_offset, in_len);
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
        if new_size > self.memory_size() {
            self.set_memory_size(new_size);
            outputs.push(SSAOutput::MemorySize(new_size));
        }

        Ok(outputs)
    }

    /// Execute delegatecall operation
    #[inline]
    pub fn execute_delegatecall(&mut self, inputs: Vec<SSAOutput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 9 {
            return Err(ExecutionError::ExecutionError(
                "DELEGATECALL requires exactly 10 operands (gas, to, in_offset, in_len, out_offset, out_len, input, value, caller, target)".to_string()
            ));
        }
        let gas_limit = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let in_offset = match_ssa_output_stack_or_const!(&inputs[2], "Fourth");
        let in_len = match_ssa_output_stack_or_const!(&inputs[3], "Fifth");
        let out_offset = match_ssa_output_stack_or_const!(&inputs[4], "Sixth");
        let out_len = match_ssa_output_stack_or_const!(&inputs[5], "Seventh");
        let input = match_input!(inputs, 6, SSAOutput::Memory(value) => value, "Eighth");
        let contract_address: Address = match_input!(inputs, 7, SSAOutput::ContractEnv(value) => value.target_address, "Ninth");
        let caller = match_input!(inputs, 8, SSAOutput::ContractEnv(value) => value.caller, "Tenth");

        let gas_limit = as_u64_saturated(*gas_limit);
        let out_offset = as_usize_saturated(*out_offset);
        let out_len = as_usize_saturated(*out_len);
        let in_offset = as_usize_saturated(*in_offset);
        let in_len = as_usize_saturated(*in_len);

        let ssa_call_input = SSACallInput {
            input: input.clone(),
            target_address: contract_address,
            bytecode_address: Address::from_word(B256::from(*to)),
            caller: caller,
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
        
        let mut outputs = vec![SSAOutput::CallInput(Box::new(ssa_call_input))];
        let new_size_1 = self.check_memory_size(in_offset, in_len);
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
        if new_size > self.memory_size() {
            self.set_memory_size(new_size);
            outputs.push(SSAOutput::MemorySize(new_size));
        }

        Ok(outputs)
    }

    /// Execute staticcall operation
    #[inline]
    pub fn execute_staticcall(&mut self, inputs: Vec<SSAOutput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 8 {
            return Err(ExecutionError::ExecutionError(
                "STATICCALL requires exactly 8 operands (gas, to, in_offset, in_len, out_offset, out_len, input, target)".to_string()
            ));
        }
        let gas_limit = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let in_offset = match_ssa_output_stack_or_const!(&inputs[2], "Third");
        let in_len = match_ssa_output_stack_or_const!(&inputs[3], "Fourth");
        let out_offset = match_ssa_output_stack_or_const!(&inputs[4], "Fifth");
        let out_len = match_ssa_output_stack_or_const!(&inputs[5], "Sixth");
        let input = match_input!(inputs, 6, SSAOutput::Memory(value) => value, "Seventh");
        let target = match_input!(inputs, 7, SSAOutput::ContractEnv(value) => value.target_address, "Eighth");

        let gas_limit = as_u64_saturated(*gas_limit);
        let out_offset = as_usize_saturated(*out_offset);
        let out_len = as_usize_saturated(*out_len);
        let in_offset = as_usize_saturated(*in_offset);
        let in_len = as_usize_saturated(*in_len);
        let to_addr = Address::from_word(B256::from(*to));

        let ssa_call_input = SSACallInput {
            input: input.clone(),
            target_address: to_addr,
            bytecode_address: to_addr,
            caller: target,
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
        
        let mut outputs = vec![SSAOutput::CallInput(Box::new(ssa_call_input))];
        let new_size_1 = self.check_memory_size(in_offset, in_len);
        let new_size_2 = self.check_memory_size(out_offset, out_len);
        let new_size = std::cmp::max(new_size_1, new_size_2);
        if new_size > self.memory_size() {
            self.set_memory_size(new_size);
            outputs.push(SSAOutput::MemorySize(new_size));
        }

        Ok(outputs)
    }

}

