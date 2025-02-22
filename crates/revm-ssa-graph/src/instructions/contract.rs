use revm_primitives::db::DatabaseRef;
use revm_primitives::{
    Address, Bytes, B256, Spec, U256,
    SpecId::{LONDON, SPURIOUS_DRAGON},
};
use revm_ssa::{
    SSACallInput, SSACallOutcome, SSACallScheme,
    SSACreateInput, SSACreateOutcome, SSACreateScheme,
    SSAInput, SSAInstructionResult, SSAOutput,
    StorageKey, StorageValue,
};
use crate::{ExecutionContext, ExecutionError, Result, match_ssa_input_stack_or_const};

use super::as_usize_saturated;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {

    /// Execute deduct caller operation
    #[inline]
    pub fn execute_deduct_caller(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() < 3 {
            return Err(ExecutionError::ExecutionError(
                "DEDUCT_CALLER requires at least 3 operands".to_string()
            ));
        }
        let is_call = inputs.len() == 4;
        let caller = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_caller().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be ContractEntry".to_string()
            )),
        };
        let origin_balance = match &inputs[1] {
            SSAInput::Storage { value, .. } => value.as_balance().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Storage value".to_string()
            )),
        };
        let deduct_balance = match &inputs[2] {
            SSAInput::Constant ( value, .. )=> value,
            _ => return Err(ExecutionError::ExecutionError(
                "Fourth operand must be Constant".to_string()
            )),
        };

        let new_balance = origin_balance - deduct_balance;

        let mut outputs = vec![
            SSAOutput::Storage {
                key: Box::new(StorageKey::Balance(caller)),
                value: Box::new(StorageValue::Balance(new_balance)),
            }
        ];

        if is_call {
            let origin_nonce = match &inputs[3] {
                SSAInput::Storage { value, .. } => value.as_nonce().unwrap(),
                _ => return Err(ExecutionError::ExecutionError(
                    "Third operand must be Storage value".to_string()
                )),
            };
            let new_nonce = origin_nonce + 1;
            outputs.push(SSAOutput::Storage {
                key: Box::new(StorageKey::Nonce(caller)),
                value: Box::new(StorageValue::Nonce(new_nonce)),
            });
        }

        Ok(outputs)
    }

    #[inline]
    pub fn execute_refund_gas(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        // eprintln!("Refund Gas");
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "REFUND_GAS requires exactly 2 operands (caller, refund_gas)".to_string()
            ));
        }
        let caller = match &inputs[0] {
            SSAInput::ContractEntry { value, .. } => value.as_caller().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be ContractEntry".to_string()
            )),
        };
        let origin_balance = match &inputs[1] {
            SSAInput::Storage { value, .. } => value.as_balance().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Storage value".to_string()
            )),
        };  
        let refund_gas = match &inputs[2] {
            SSAInput::Constant ( value, .. )=> value,
            _ => return Err(ExecutionError::ExecutionError(
                "Third operand must be Constant".to_string()
            )),
        };
        // Print original balance before reimbursement
        // eprintln!("Original balance before reimbursement: {}", origin_balance);
        let new_balance = origin_balance + refund_gas;
        // Print new balance after reimbursement
        // eprintln!("New balance after reimbursement: {}", new_balance);
        
        let outputs = vec![
            SSAOutput::Storage {
                key: Box::new(StorageKey::Balance(caller)),
                value: Box::new(StorageValue::Balance(new_balance)),
            },
        ];
        Ok(outputs)
    }

    /// Execute call operation
    #[inline]
    pub fn execute_call(&mut self, inputs: Vec<SSAInput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 9 {
            return Err(ExecutionError::ExecutionError(
                "CALL requires exactly 9 operands (gas, to, value, in_offset, in_len, out_offset, out_len, input)".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let value = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let in_offset = match_ssa_input_stack_or_const!(&inputs[3], "Fourth");
        let in_len = match_ssa_input_stack_or_const!(&inputs[4], "Fifth");
        let out_offset = match_ssa_input_stack_or_const!(&inputs[5], "Sixth");
        let out_len = match_ssa_input_stack_or_const!(&inputs[6], "Seventh");
        let input = match &inputs[7] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Eighth operand must be Memory".to_string()
            )),
        };
        let target_address = match &inputs[8] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Ninth operand must be ContractEntry".to_string()
            )),
        };
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
            code: None
        };
        
        let mut outputs = vec![SSAOutput::CallFrame(Box::new(ssa_call_input))];
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
    #[inline]
    pub fn execute_make_call_frame(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() < 2 {
            return Err(ExecutionError::ExecutionError(
                "MAKE_CALL_FRAME requires at least 2 operands (call_input, code)".to_string()
            ));

        }

        let call_input = match &inputs[0] {
            SSAInput::CallInput { input, .. } => input,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be CallInput".to_string()
            )),
        };

        let code = match &inputs[1] {
            SSAInput::Storage { value, .. } => value.as_code().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Storage value".to_string()
            )),
        };

        let mut new_call_input = call_input.clone();
        new_call_input.code = Some(code.clone());

        let mut outputs = vec![
            SSAOutput::CallFrame(new_call_input)
        ];

        if !call_input.transfer_value.is_zero() {
            let old_caller_balance = match &inputs[2] {
                SSAInput::Storage { value, .. } => value.as_balance().unwrap(),
                _ => return Err(ExecutionError::ExecutionError(
                    "Third operand must be Storage value".to_string()
                )),
            };
            let old_target_balance = match &inputs[3] {
                SSAInput::Storage { value, .. } => value.as_balance().unwrap(),
                _ => return Err(ExecutionError::ExecutionError(
                    "Fourth operand must be Storage value".to_string()
                )),
            };
            if old_caller_balance < call_input.transfer_value {
                return Err(ExecutionError::ExecutionError(
                    format!("Insufficient balance: need {}, had {}", call_input.transfer_value, old_caller_balance).to_string()
                ));
            }
            let new_caller_balance = old_caller_balance - call_input.transfer_value;
            let new_target_balance = old_target_balance + call_input.transfer_value;

            outputs.push(SSAOutput::Storage { 
                key: Box::new(StorageKey::Balance(call_input.caller)), 
                value: Box::new(StorageValue::Balance(new_caller_balance)), 
            });
            outputs.push(SSAOutput::Storage { 
                key: Box::new(StorageKey::Balance(call_input.target_address)), 
                value: Box::new(StorageValue::Balance(new_target_balance)), 
            });
        }

        Ok(outputs)
    }

    /// Execute call return operation
    #[inline]
    pub fn execute_call_return(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "CALL_RETURN requires exactly 2 operands (interpreter_result, call_input)".to_string()
            ));
        }

        let interpreter_result = match &inputs[0] {
            SSAInput::InterpreterResult { result, .. } => result,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be InterpreterResult".to_string()
            )),
        };

        let call_input = match &inputs[1] {
            SSAInput::CallInput { input, .. } => input,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be CallInput".to_string()
            )),
        };

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
    pub fn execute_insert_call_outcome(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "INSERT_CALL_OUTCOME requires exactly 1 operand (call_outcome)".to_string()
            ));
        }

        let call_outcome = match &inputs[0] {
            SSAInput::CallOutcome { outcome, .. } => outcome,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be CallOutcome".to_string()
            )),
        };

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
    pub fn execute_create(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() < 5 {
            return Err(ExecutionError::ExecutionError(
                "CREATE requires at least 5 operands (value, code_offset, len, code, caller)".to_string()
            ));
        }
        let value = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let code_offset = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let len = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let code = match &inputs[3] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Fourth operand must be Memory".to_string()
            )),
        };
        let target = match &inputs[4] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Fifth operand must be ContractEntry".to_string()
            )),
        };
        let salt = if inputs.len() == 6 {
            Some(match_ssa_input_stack_or_const!(&inputs[5], "Sixth"))
        } else {
            None
        };

        let len = as_usize_saturated(*len);
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code.len()].copy_from_slice(&code);


        let ssa_create_input = SSACreateInput {
            init_code: padded_code_slice.into(),
            value: *value,
            caller: target,
            scheme: if salt.is_some() {
                SSACreateScheme::Create2 { salt: *salt.unwrap() }
            } else {
                SSACreateScheme::Create
            },
            target: Address::ZERO,
        };
        let mut outputs = vec![SSAOutput::CreateFrame(Box::new(ssa_create_input))];

        let new_size = self.check_memory_size(as_usize_saturated(*code_offset), len);
        if new_size > self.memory_size() {
            self.set_memory_size(new_size);
            outputs.push(SSAOutput::MemorySize(new_size));
        }


        Ok(outputs)
    }

    /// Execute make create frame operation
    #[inline]
    pub fn execute_make_create_frame(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        // eprintln!("execute_make_create_frame: {:?}", inputs);cl
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "MAKE_CREATE_FRAME requires exactly 2 operands (create_input, code)".to_string()
            ));
        }

        let create_input = match &inputs[0] {
            SSAInput::CreateInput { input, .. } => input,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be CreateInput".to_string()
            )),
        };
        let nonce = match &inputs[1] {
            SSAInput::Storage { value, .. } => value.as_nonce().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Storage value".to_string()
            )),
        };

        let caller = create_input.caller;
        let created_address = match create_input.scheme {
            SSACreateScheme::Create => create_input.caller.create(nonce),
            SSACreateScheme::Create2 { salt } => {
                let init_code_hash = revm_primitives::keccak256(&create_input.init_code);
                create_input.caller.create2(salt.to_be_bytes(), init_code_hash)
            }
        };
        let mut create_input = create_input.clone();
        create_input.target = created_address;

        Ok(vec![
            SSAOutput::CreateFrame(create_input),
            SSAOutput::Storage {
                key: Box::new(StorageKey::Nonce(caller)),
                value: Box::new(StorageValue::Nonce(nonce+1)),
            }
        ])
    }

    /// Execute create return operation
    #[inline]
    pub fn execute_create_return(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "CREATE_RETURN requires exactly 2 operands (interpreter_result, create_input)".to_string()
            ));
        }

        let interpreter_result = match &inputs[0] {
            SSAInput::InterpreterResult { result, .. } => result,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be InterpreterResult".to_string()
            )),
        };

        let create_input = match &inputs[1] {
            SSAInput::CreateInput { input, .. } => input,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be CreateInput".to_string()
            )),
        };

        let address = create_input.target;
        let instruction_result = interpreter_result.result;
        let mut create_outcome = SSACreateOutcome {
            result: interpreter_result.clone(),
            address: Some(address),
        };

        let mut outputs = vec![];

        // Handle basic error cases
        if !instruction_result.is_ok() {
            create_outcome.address = None;
            outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
            return Ok(outputs);
        }

        // London fork: Check if the first byte is 0xEF
        if SPEC::enabled(LONDON) && interpreter_result.output.first() == Some(&0xEF) {
            create_outcome.address = None;
            create_outcome.result.result = SSAInstructionResult::Error;
            outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
            return Ok(outputs);
        }

        // Spurious Dragon fork: Contract size limit (0x6000 ~25kb)
        if SPEC::enabled(SPURIOUS_DRAGON) && interpreter_result.output.len() > 0x6000 {
            create_outcome.address = None;
            create_outcome.result.result = SSAInstructionResult::Error;
            outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
            return Ok(outputs);
        }

        // Handle successful case
        outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
        outputs.push(SSAOutput::Storage { 
            key: Box::new(StorageKey::Code(address)), 
            value: Box::new(StorageValue::Code(interpreter_result.output.clone())) 
        });

        Ok(outputs)
    }

    /// Execute insert create outcome operation
    #[inline]
    pub fn execute_insert_create_outcome(&mut self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "INSERT_CREATE_OUTCOME requires exactly 1 operand (create_outcome)".to_string()
            ));
        }

        let create_outcome = match &inputs[0] {
            SSAInput::CreateOutcome { outcome, .. } => outcome,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be CreateOutcome".to_string()
            )),
        };

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
    pub fn execute_callcode(&mut self, inputs: Vec<SSAInput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 9 {
            return Err(ExecutionError::ExecutionError(
                "CALLCODE requires exactly 9 operands (gas, to, value, in_offset, in_len, out_offset, out_len, input)".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let value = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let in_offset = match_ssa_input_stack_or_const!(&inputs[3], "Fourth");
        let in_len = match_ssa_input_stack_or_const!(&inputs[4], "Fifth");
        let out_offset = match_ssa_input_stack_or_const!(&inputs[5], "Sixth");
        let out_len = match_ssa_input_stack_or_const!(&inputs[6], "Seventh");
        let input = match &inputs[7] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Eighth operand must be Memory".to_string()
            )),
        };
        let target = match &inputs[8] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Ninth operand must be ContractEntry".to_string()
            )),
        };
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
            code: None
        };
        
        let mut outputs = vec![SSAOutput::CallFrame(Box::new(ssa_call_input))];
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
    pub fn execute_delegatecall(&mut self, inputs: Vec<SSAInput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 10 {
            return Err(ExecutionError::ExecutionError(
                "DELEGATECALL requires exactly 10 operands (gas, to, in_offset, in_len, out_offset, out_len, input, value, caller, target)".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let in_offset = match_ssa_input_stack_or_const!(&inputs[3], "Fourth");
        let in_len = match_ssa_input_stack_or_const!(&inputs[4], "Fifth");
        let out_offset = match_ssa_input_stack_or_const!(&inputs[5], "Sixth");
        let out_len = match_ssa_input_stack_or_const!(&inputs[6], "Seventh");
        let input = match &inputs[7] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Eighth operand must be Memory".to_string()
            )),
        };
        let target = match &inputs[8] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Tenth operand must be ContractEntry".to_string()
            )),
        };
        let caller = match &inputs[9] {
            SSAInput::ContractEntry { value, .. } => value.as_caller().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Ninth operand must be ContractEntry".to_string()
            )),
        };
        let out_offset = as_usize_saturated(*out_offset);
        let out_len = as_usize_saturated(*out_len);
        let in_offset = as_usize_saturated(*in_offset);
        let in_len = as_usize_saturated(*in_len);
        let ssa_call_input = SSACallInput {
            input: input.clone(),
            target_address: target,
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
            code: None
        };
        
        let mut outputs = vec![SSAOutput::CallFrame(Box::new(ssa_call_input))];
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
    pub fn execute_staticcall(&mut self, inputs: Vec<SSAInput>, opcode: u8) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 8 {
            return Err(ExecutionError::ExecutionError(
                "STATICCALL requires exactly 8 operands (gas, to, in_offset, in_len, out_offset, out_len, input, target)".to_string()
            ));
        }
        let _ = match_ssa_input_stack_or_const!(&inputs[0], "First");
        let to = match_ssa_input_stack_or_const!(&inputs[1], "Second");
        let in_offset = match_ssa_input_stack_or_const!(&inputs[2], "Third");
        let in_len = match_ssa_input_stack_or_const!(&inputs[3], "Fourth");
        let out_offset = match_ssa_input_stack_or_const!(&inputs[4], "Fifth");
        let out_len = match_ssa_input_stack_or_const!(&inputs[5], "Sixth");
        let input = match &inputs[6] {
            SSAInput::Memory { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Seventh operand must be Memory".to_string()
            )),
        };
        let target = match &inputs[7] {
            SSAInput::ContractEntry { value, .. } => value.as_target().unwrap(),
            _ => return Err(ExecutionError::ExecutionError(
                "Eighth operand must be ContractEntry".to_string()
            )),
        };
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
            code: None
        };
        
        let mut outputs = vec![SSAOutput::CallFrame(Box::new(ssa_call_input))];
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

