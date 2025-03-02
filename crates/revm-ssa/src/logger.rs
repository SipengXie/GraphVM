// TODO: we may need to consider something like the TxEnv, BlockEnv here.
use revm_primitives::bitvec::bitvec;
use revm_primitives::bitvec::order::Lsb0;
use revm_primitives::bitvec::vec::BitVec;
use revm_primitives::{AccountInfo, AccountStatus, Address, AnalysisKind, Bytecode, Bytes, FixedBytes, HashMap, HashSet, JumpTable, LegacyAnalyzedBytecode, Log, B256, U256};
use crate::shadow_stack::ShadowStack;
use crate::types::{SSALogEntry, StorageKey, ContractEnv, InternalOp, MemoryDep, SSAInput, SSAOutput, StorageValue};
use crate::{SSACallInput, SSACallOutcome, SSACallScheme, SSACreateInput, SSACreateOutcome, SSACreateScheme, SSAInstructionResult, SSAInterpreterResult};
use revm_primitives::Spec;
use std::cmp::min;
use std::sync::Arc;
// Update macro pop_stack_or_const to take two parameters: self and value
#[macro_export]
macro_rules! pop_stack_or_const {
    ($self:expr, $value:expr) => {{
        let src = $self.pop_stack_def().unwrap();
        if src == 0 {
            SSAInput::Constant($value)
        } else {
            SSAInput::Stack {
                source: src,
            }
        }
    }};
}

// Macro for pushing storage account info
#[macro_export]
macro_rules! input_account_info {
    ($self:expr, $address:expr) => {{
        SSAInput::Storage {
            key: Box::new(StorageKey::AccountInfo($address)),
            source: $self.get_storage_def(StorageKey::AccountInfo($address))
        }
    }};
}

// Macro for pushing storage account status 
#[macro_export]
macro_rules! input_account_status {
    ($self:expr, $address:expr) => {{
        SSAInput::Storage {
            key: Box::new(StorageKey::AccountStatus($address)),
            source: $self.get_storage_def(StorageKey::AccountStatus($address))
        }
    }};
}

// Macro for output storage account info
#[macro_export]
macro_rules! output_account_info {
    ($address:expr, $info:expr) => {{
        SSAOutput::Storage {
            key: Box::new(StorageKey::AccountInfo($address)),
            value: Box::new(StorageValue::AccountInfo($info))
        }
    }};
}

// Macro for output storage account status
#[macro_export]
macro_rules! output_account_status {
    ($address:expr, $status:expr) => {{
        SSAOutput::Storage {
            key: Box::new(StorageKey::AccountStatus($address)),
            value: Box::new(StorageValue::AccountStatus($status))
        }
    }};
}



#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SSALogger {
    // Current LSN
    pub current_lsn: u16,
    // Log entries
    logs: Vec<SSALogEntry>,
    // Shadow stack for tracking stack item definitions
    pub stack: ShadowStack,
    // Latest writes to storage slots
    // records the latest lsn of sstore that write to the slot
    // used for dependency tracking
    latest_writes: HashMap<StorageKey, u16>,
    // First reads of storage slots
    // records the first lsn of sload that read the slot
    // used for identifying storage conflicts
    first_reads: HashMap<StorageKey, u16>,
    // to record the latest lsn that modifies memory
    last_memory: u16,
    // to record the latest lsn that modifies return data buffer
    last_return_data_buffer: u16,
    // last_interpreter_return
    last_interpreter_return: u16,
    // last_call
    last_call: Vec<u16>,
    // last_create
    last_create: Vec<u16>,
    // last_call_return
    last_call_return: Vec<u16>,
    // last_create_return
    last_create_return: Vec<u16>,
    // Initial LSN
    // Use stack to track entry_lsn at different levels
    pub entry_lsn: Vec<u16>,
    // we need call_inputs to get return range
    pub call_inputs: Vec<SSACallInput>,
    pub first_call_input: Option<SSACallInput>,
    pub first_create_input: Option<SSACreateInput>,

    // memory buffer for storing inputs and outputs
    input_buf: Vec<SSAInput>,
    output_buf: Vec<SSAOutput>,
}

#[derive(Clone, Debug)]
pub struct SsaRwSet {
    pub read_set: HashMap<StorageKey, u16>,
    pub write_set: HashSet<StorageKey>,
}

impl SsaRwSet {
    /// Get all storage keys in the read set
    pub fn get_read_keys(&self) -> Vec<StorageKey> {
        self.read_set.keys().cloned().collect()
    }

    pub fn new_with_write_set(write_set: HashSet<StorageKey>) -> Self {
        Self {
            read_set: HashMap::default(),
            write_set,
        }
    }
}

impl SSALogger {

    fn get_entry_lsn(&mut self) -> u16 {
        if self.entry_lsn.len() > 0 {
            *self.entry_lsn.last().unwrap()
        } else {
            0
        }
    }

    pub fn new() -> Self {
        
        Self {
            current_lsn: 1,
            entry_lsn: Vec::new(),
            logs: Vec::with_capacity(512),
            stack: ShadowStack::new(),
            latest_writes: HashMap::default(),
            first_reads: HashMap::default(),
            last_memory: 0,
            last_return_data_buffer: 0,
            last_interpreter_return: 0,
            last_call: vec![],
            last_create: vec![],
            last_call_return: vec![],
            last_create_return: vec![],
            call_inputs: vec![],
            first_call_input: None,
            first_create_input: None,

            input_buf: vec![SSAInput::Constant(U256::ZERO); 3],
            output_buf: vec![SSAOutput::Stack(U256::ZERO); 1],
        }
    }

    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            current_lsn: 1,
            entry_lsn: Vec::new(),
            logs: Vec::with_capacity(capacity),
            stack: ShadowStack::new(),
            latest_writes: HashMap::default(),
            first_reads: HashMap::default(),
            last_memory: 0,
            last_return_data_buffer: 0,
            last_interpreter_return: 0,
            last_call: vec![],
            last_create: vec![],
            last_call_return: vec![],
            last_create_return: vec![],
            call_inputs: vec![],
            first_call_input: None,
            first_create_input: None,

            input_buf: vec![SSAInput::Constant(U256::ZERO); 3],
            output_buf: vec![SSAOutput::Stack(U256::ZERO); 1],
        }
    }

    /// Check if the logger is empty (has no logs)
    pub fn is_empty(&self) -> bool {
        self.logs.is_empty()
    }

    /// Get the current LSN
    pub fn get_current_lsn(&self) -> u16 {
        self.current_lsn
    }

    #[inline]
    pub fn log_operation(&mut self, opcode: u8, inputs: Vec<SSAInput>, outputs: Vec<SSAOutput>) -> u16 {
        let entry = SSALogEntry {
            lsn: self.current_lsn,
            opcode,
            inputs,
            outputs,
        };
        
        self.logs.push(entry);
        self.current_lsn += 1;
        self.current_lsn - 1
    }

    #[inline]
    pub fn log_operation_with_buffer(&mut self, opcode: u8, input_size: usize, output_size: usize) -> u16 {
        let entry = SSALogEntry {
            lsn: self.current_lsn,
            opcode,
            inputs: self.input_buf[0..input_size].to_vec(),
            outputs: self.output_buf[0..output_size].to_vec(),
        };
        self.logs.push(entry);
        self.current_lsn += 1;
        self.current_lsn - 1
    }

    /// Corresponding Execution Function: file://./../../revm-ssa-graph/src/instructions/contract.rs#L18
    #[inline]
    pub fn log_deduct_caller(&mut self, caller: Address, new_info: AccountInfo, gas_cost: U256, is_create: bool) {
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(SSAInput::Constant(caller.into_word().into()));
        ssa_inputs.push(SSAInput::Constant(U256::from(is_create)));
        ssa_inputs.push(input_account_info!(self, caller));
        ssa_inputs.push(SSAInput::Constant(gas_cost));

        let mut ssa_outputs = Vec::with_capacity(1);

        ssa_outputs.push(output_account_info!(caller, new_info));

        let lsn = self.log_operation(0xDA, ssa_inputs, ssa_outputs);
        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);
        self.log_storage_write(StorageKey::AccountInfo(caller), lsn);
    }

    // TODO: the refund_gas may not be constant, some extra work is needed here.
    #[inline]
    pub fn log_refund_gas(&mut self, caller: Address, new_info: AccountInfo, refund_gas : U256) {
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(SSAInput::Constant(caller.into_word().into()));
        ssa_inputs.push(input_account_info!(self, caller));
        ssa_inputs.push(SSAInput::Constant(refund_gas));

        let mut ssa_outputs = Vec::with_capacity(1);

        ssa_outputs.push(output_account_info!(caller, new_info));

        let lsn = self.log_operation(0xDB, ssa_inputs, ssa_outputs);
        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);
        self.log_storage_write(StorageKey::AccountInfo(caller), lsn);
    }

    // TODO: the reward may not be constant, some extra work is needed here.
    #[inline]
    pub fn log_reward_beneficiary(&mut self, beneficiary: Address, new_info: AccountInfo, reward: U256) {
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(SSAInput::Constant(beneficiary.into_word().into()));
        ssa_inputs.push(input_account_info!(self, beneficiary));
        ssa_inputs.push(SSAInput::Constant(reward));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(output_account_info!(beneficiary, new_info));

        let lsn = self.log_operation(0xDC, ssa_inputs, ssa_outputs);
        self.log_storage_read(StorageKey::AccountInfo(beneficiary), lsn);
        self.log_storage_write(StorageKey::AccountInfo(beneficiary), lsn);
    }

    #[inline]
    pub fn log_monotonic_operation(&mut self, opcode: u8, operand1: U256, result: U256) {
        let operand1_ssa_input = pop_stack_or_const!(self, operand1);
        self.input_buf[0] = operand1_ssa_input;
        self.output_buf[0] = SSAOutput::Stack(result);
        let lsn = self.log_operation_with_buffer(opcode, 1, 1);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_binary_operation(&mut self, opcode: u8, operand1: U256, operand2: U256, result: U256) {
        let operand1_ssa_input = pop_stack_or_const!(self, operand1);
        let operand2_ssa_input = pop_stack_or_const!(self, operand2);
        self.input_buf[0] = operand1_ssa_input;
        self.input_buf[1] = operand2_ssa_input;
        self.output_buf[0] = SSAOutput::Stack(result);
        let lsn = self.log_operation_with_buffer(opcode, 2, 1);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_trinary_operation(&mut self, opcode: u8, operand1: U256, operand2: U256, operand3: U256, result: U256) {
        let operand1_ssa_input = pop_stack_or_const!(self, operand1);
        let operand2_ssa_input = pop_stack_or_const!(self, operand2);
        let operand3_ssa_input = pop_stack_or_const!(self, operand3);
        self.input_buf[0] = operand1_ssa_input;
        self.input_buf[1] = operand2_ssa_input;
        self.input_buf[2] = operand3_ssa_input;
        self.output_buf[0] = SSAOutput::Stack(result);
        let lsn = self.log_operation_with_buffer(opcode, 3, 1);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_pop_operation(&mut self, _opcode: u8) {
        self.pop_stack_def().unwrap();
    }

    #[inline]
    pub fn log_push_operation(&mut self, _opcode: u8, _result: &[u8]) {
        self.push_stack_def(0).unwrap();
    }

    #[inline]
    pub fn log_dup_operation(&mut self, _opcode: u8, n: usize) {
        self.dup_stack_def(n).unwrap();
    }

    #[inline]
    pub fn log_swap_operation(&mut self, _opcode: u8, n: usize) {
        self.swap_stack_def(n).unwrap();
    }

    #[inline]
    pub fn log_exchange_operation(&mut self, opcode: u8, n: usize, m: usize) {
        let ssa_inputs = Vec::new();
        let ssa_outputs = Vec::new();
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.exchange_stack_def(n, m).unwrap();
    }

    #[inline]
    pub fn log_jump(&mut self, opcode: u8, target: usize, current_pc: usize, relative_offset: isize) {
        let target_ssa_input = pop_stack_or_const!(self, U256::from(target));
        self.input_buf[0] = target_ssa_input;
        self.input_buf[1] = SSAInput::Constant(U256::from(current_pc));
        self.output_buf[0] = SSAOutput::Jump { relative_offset };
        self.log_operation_with_buffer(opcode, 2, 1);
    }

    #[inline]
    pub fn log_jumpi(&mut self, opcode: u8, target: usize, cond: U256, current_pc: usize, relative_offset: isize) {
        let target_ssa_input = pop_stack_or_const!(self, U256::from(target));
        let cond_ssa_input = pop_stack_or_const!(self, cond);
        self.input_buf[0] = target_ssa_input;
        self.input_buf[1] = cond_ssa_input;
        self.input_buf[2] = SSAInput::Constant(U256::from(current_pc));
        self.output_buf[0] = SSAOutput::Jump { relative_offset };
        self.log_operation_with_buffer(opcode, 3, 1);
    }
    
    #[inline]
    pub fn log_pc_operation(&mut self, _opcode: u8, _result: usize) {
        self.push_stack_def(0).unwrap();
    }

    #[inline]
    pub fn log_mload_operation(&mut self, opcode: u8, offset: usize, result: U256, memory_deps: Vec<MemoryDep>, mem_length: Option<usize>) {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(SSAInput::Memory {
            source: memory_deps,
        });

        let mut ssa_outputs = Vec::with_capacity(2);
        ssa_outputs.push(SSAOutput::Stack(result));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_mstore_operation(&mut self, opcode: u8, offset: usize, value: U256, mem_length: Option<usize>) -> u16 {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, value));

        let mut ssa_outputs = Vec::with_capacity(2);
        if opcode == 0x52 {
            ssa_outputs.push(SSAOutput::Memory(value.to_be_bytes::<32>().into()));
        } else {
            ssa_outputs.push(SSAOutput::Memory([value.byte(0)].into()));
        }

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline]
    pub fn log_msize_operation(&mut self, opcode: u8, mem_length: usize) {
        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(SSAInput::MemorySizeChange {
            source: self.last_memory,
        });
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(U256::from(mem_length)));
        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_mcopy_operation(&mut self, opcode: u8, dst: usize, src: usize, len: usize, result: Bytes, memory_deps: Vec<MemoryDep>, mem_length: Option<usize>) -> u16 {
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(dst)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(src)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory {
            source: memory_deps,
        });
        assert_eq!(result.len(), len,"mcopy result len not equal to len");
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Memory(result.clone()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline]
    pub fn log_return(&mut self, 
        opcode: u8, 
        offset: usize, 
        len: usize, 
        output: Bytes, 
        mem_deps: Vec<MemoryDep>,
        mem_length: Option<usize>,
        result: SSAInstructionResult)
    {

        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory {
            source: mem_deps,
        });
      
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::InterpreterResult(
            SSAInterpreterResult {
                result: result,
                output: output,
            })
        );

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.last_interpreter_return = lsn;
    }


    #[inline]
    pub fn log_instruction_result_change(&mut self, opcode: u8, result: SSAInstructionResult) {
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::InterpreterResult(
                SSAInterpreterResult {
                    result: result,
                    output: Bytes::new(), // Empty output for stop/invalid/unknown cases
                }
            )
        );

        let lsn = self.log_operation(opcode, Vec::new(), ssa_outputs);
        self.last_interpreter_return = lsn;
    }

    #[inline]
    pub fn log_host_env_operation(&mut self, opcode: u8, result: U256) {
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(result));
        let lsn = self.log_operation(opcode, Vec::new(), ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_blobhash_operation(&mut self, opcode: u8, index: usize, result: U256) {
        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index)));
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(result));
        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_system_operation(&mut self, opcode: u8, contract_env: ContractEnv) {
        let mut ssa_input = Vec::with_capacity(1);
        ssa_input.push(SSAInput::ContractEnv{source: self.get_entry_lsn()});
        let mut ssa_output = Vec::with_capacity(1);
        match opcode {
             0x30 => ssa_output.push(SSAOutput::Stack(contract_env.target_address.into_word().into())), // ADDRESS
             0x33 => ssa_output.push(SSAOutput::Stack(contract_env.caller.into_word().into())), // CALLER
             0x34 => ssa_output.push(SSAOutput::Stack(contract_env.call_value)), // CALLVALUE
             0x36 => ssa_output.push(SSAOutput::Stack(U256::from(contract_env.input.len()))), // CALLDATASIZE
             0x38 => ssa_output.push(SSAOutput::Stack(U256::from(contract_env.bytecode.len()))), // CODESIZE
             _ => unreachable!("Unsupported system operation: {}", opcode),
        }
        let lsn = self.log_operation(opcode, ssa_input, ssa_output);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_return_data_size(&mut self, opcode: u8, value: Bytes) {
        let len = value.len();
        let mut ssa_input = Vec::with_capacity(1);
        ssa_input.push(SSAInput::ReturnDataBuffer {
            source: self.last_return_data_buffer,  
        });
        let mut ssa_output = Vec::with_capacity(1);
        ssa_output.push(SSAOutput::Stack(U256::from(len)));
        let lsn = self.log_operation(opcode, ssa_input, ssa_output);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_return_data_cpy_operation(&mut self, opcode: u8, 
        meme_offset: usize, 
        data_offset: usize, 
        len: usize, 
        return_data: Bytes, 
        mem_length: Option<usize>) -> u16 {

        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(meme_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(data_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::ReturnDataBuffer {
            source: self.last_return_data_buffer,
        });
        
        let return_data_len = min(data_offset + len, return_data.len());
        let return_data_slice = return_data.slice(data_offset..return_data_len);
        // Pad return_data_slice to len
        let mut padded_return_data_slice = vec![0u8; len];
        padded_return_data_slice[..return_data_slice.len()].copy_from_slice(&return_data_slice);
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Memory(padded_return_data_slice.into()));
        
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline]
    pub fn log_code_copy(&mut self, 
        opcode: u8, 
        memory_offset: usize, 
        code_offset: usize, 
        len: usize, 
        code: Bytes, 
        mem_length: Option<usize>) -> u16 {
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(memory_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(code_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::ContractEnv { 
            source: self.get_entry_lsn() 
        });
        let code_end = min(code.len(), code_offset+len);
        let code_slice = code.slice(code_offset..code_end);
        // Pad code_slice to len
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code_slice.len()].copy_from_slice(&code_slice);
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Memory(padded_code_slice.into()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline]
    pub fn log_call_data_copy(&mut self, opcode: u8, memory_offset: usize, data_offset: usize, len: usize, data: Bytes, mem_length: Option<usize>) -> u16 {
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(memory_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(data_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::ContractEnv {
            source: self.get_entry_lsn(),
        });
        let data_len = min(data_offset + len, data.len());
        let data_slice = data.slice(data_offset..data_len);
        // Pad data_slice to len
        let mut padded_data_slice = vec![0u8; len];
        padded_data_slice[..data_slice.len()].copy_from_slice(&data_slice);
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Memory(padded_data_slice.into()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline]
    pub fn log_return_data_load(&mut self, opcode: u8, offset: usize, return_data: Bytes) {

        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(SSAInput::ReturnDataBuffer {
            source: self.last_return_data_buffer,
        });

        let mut output = [0u8; 32];
        if let Some(available) = return_data.len()
            .checked_sub(offset)
        {
            let copy_len = available.min(32);
            output[..copy_len].copy_from_slice(
                &return_data[offset..offset + copy_len],
            );
        }

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(B256::from(output).into()));
        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_call_data_load(&mut self, opcode: u8, offset: usize, data: Bytes) {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(SSAInput::ContractEnv { 
            source: self.get_entry_lsn() 
        });

        let mut word = [0u8; 32];
        if offset < data.len() {
            let length = 32.min(data.len() - offset);
            word[..length].copy_from_slice(&data[offset..offset+length]);
        }

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(B256::from_slice(&word).into())
        );

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    // ! IN SSA, this function is simple!
    pub fn log_gas(&mut self, opcode: u8, gas: u64) {
        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(SSAInput::Constant(U256::from(gas)));
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(U256::from(gas))
        );
        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_keccak256(&mut self, opcode: u8, offset: usize, len: usize, data: &[u8], mem_deps: Vec<MemoryDep>, mem_length: Option<usize>) {
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory {
            source: mem_deps,
        });

        let hash = revm_primitives::keccak256(data);
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(hash.into())
        );

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_create(&mut self, opcode: u8, 
        value: U256, 
        code_offset: usize, 
        len: usize, 
        code: Bytes, 
        code_deps: Vec<MemoryDep>, 
        target: Address, 
        salt: Option<U256>, 
        mem_length: Option<usize>) {
        // inputs
        let mut ssa_inputs = Vec::with_capacity(6);
        ssa_inputs.push(pop_stack_or_const!(self, value)); // value
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(code_offset))); // code_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len))); // len
        ssa_inputs.push(SSAInput::Memory { 
            source: code_deps 
        });// code
        ssa_inputs.push(SSAInput::ContractEnv { 
            source: self.get_entry_lsn() 
        }); // target_address

        if let Some(salt) = salt {
            ssa_inputs.push(
                pop_stack_or_const!(self, salt)
            ); // salt
        }
        
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code.len()].copy_from_slice(&code);

        // outputs
        let ssa_create_input = SSACreateInput {
            init_code: padded_code_slice.into(),
            value,
            caller: target,
            scheme: if opcode == 0xF0 {
                SSACreateScheme::Create
            } else {
                SSACreateScheme::Create2 { salt: salt.unwrap() }
            },
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::CreateInput(Box::new(ssa_create_input)));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }
        self.last_create.push(self.current_lsn);
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_make_create_frame(&mut self, 
        create_input: SSACreateInput, 
        new_caller_info: AccountInfo,
        new_target_info: AccountInfo,
        new_target_status: AccountStatus, // needed as it is marked created
        contract_env: ContractEnv
    ) 
    {
        let opcode = InternalOp::MAKE_CREATE_FRAME;
        let lsn = self.current_lsn;
        self.entry_lsn.push(lsn);
        let caller = create_input.caller;
        let created_address = contract_env.target_address;

        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(
            SSAInput::CreateInput { 
                source: self.last_create.pop().unwrap_or_default()
            }
        );
        ssa_inputs.push(input_account_info!(self, caller));
        ssa_inputs.push(input_account_info!(self, created_address));

        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);
        self.log_storage_read(StorageKey::AccountInfo(created_address), lsn);

        let mut ssa_outputs = Vec::with_capacity(4);
        ssa_outputs.push(output_account_info!(caller, new_caller_info));
        ssa_outputs.push(output_account_info!(created_address, new_target_info));
        ssa_outputs.push(output_account_status!(created_address, new_target_status));
        ssa_outputs.push(SSAOutput::ContractEnv(Box::new(contract_env)));

        if self.first_create_input.is_none() {
            self.first_create_input = Some(create_input.clone());
        }

        self.log_storage_write(StorageKey::AccountInfo(caller), lsn);
        self.log_storage_write(StorageKey::AccountInfo(created_address), lsn);
        self.log_storage_write(StorageKey::AccountStatus(created_address), lsn);

        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_create_return<SPEC: Spec>(&mut self, 
        result: &SSAInterpreterResult, 
        address: Address,
        target_info: AccountInfo, 
        analysis_kind: &AnalysisKind
    ) {
        let opcode = InternalOp::CREATE_RETURN;
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(
            SSAInput::InterpreterResult { 
                source: self.last_interpreter_return
            }
        );
        ssa_inputs.push(
            SSAInput::ContractEnv { source: self.get_entry_lsn() } // address
        );
        ssa_inputs.push(input_account_info!(self, address));
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
        ssa_inputs.push(
            match analysis_kind {
                AnalysisKind::Raw => SSAInput::Constant(U256::from(0)),
                AnalysisKind::Analyse => SSAInput::Constant(U256::from(1)),
            }
        );

        self.entry_lsn.pop();

        let create_outcome = SSACreateOutcome {
            result: result.clone(),
            address: Some(address),
        };

        let mut ssa_outputs = Vec::with_capacity(2);

        ssa_outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
        self.last_create_return.push(lsn);
        ssa_outputs.push(output_account_info!(address, target_info));
        self.log_storage_write(StorageKey::AccountInfo(address), lsn);

        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_insert_create_outcome(&mut self, create_outcome: SSACreateOutcome) {
        let opcode = InternalOp::INSERT_CREATE_OUTCOME;
        let lsn = self.current_lsn;
        let address = create_outcome.address;
        let instruction_result = create_outcome.result.result;
        let return_data_buffer = if instruction_result.is_revert() {
            create_outcome.result.output.clone()
        } else {
            Bytes::new()
        };

        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(
            SSAInput::CreateOutcome { 
                source: self.last_create_return.pop().unwrap_or_default()
            }
        );

        let mut ssa_outputs = Vec::with_capacity(2);
        ssa_outputs.push(SSAOutput::ReturnDataBuffer(return_data_buffer.clone()));
        match instruction_result {
            SSAInstructionResult::Ok => {
                let address = address.unwrap();
                ssa_outputs.push(SSAOutput::Stack(address.into_word().into()));
            }
            SSAInstructionResult::Revert => {
               ssa_outputs.push(SSAOutput::Stack(U256::ZERO.into()));
            },
            SSAInstructionResult::Error => {
                panic!("Error in insert_create_outcome");
            }
        }

        self.last_return_data_buffer = lsn;
        
        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_call(&mut self, opcode: u8, 
        local_gas_limit: u64, 
        to: Address, 
        value: U256, 
        in_offset: usize, in_len: usize, 
        out_offset: usize, out_len: usize, 
        input:Bytes, mem_deps: Vec<MemoryDep>, 
        target_address: Address,
        mem_length: Option<usize>) {
        let mut ssa_inputs = Vec::with_capacity(7);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(value))); // value
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory { 
            source: mem_deps
        }); // memory
        ssa_inputs.push(SSAInput::ContractEnv { 
            source: self.get_entry_lsn() 
        }); // target_address

        // Create SSACallInput
        let ssa_call_input = SSACallInput {
            input: input,
            target_address: to,
            bytecode_address: to,
            caller: target_address,
            transfer_value: value,
            scheme: match opcode {
                0xF1 => SSACallScheme::Call,
                _ => panic!("Invalid opcode")
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::CallInput(Box::new(ssa_call_input))
        );
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(
                SSAOutput::MemorySize(mem_length)
            );
            self.last_memory = self.current_lsn;
        }
        self.last_call.push(self.current_lsn);
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    pub fn log_call_code(&mut self, opcode: u8, 
        local_gas_limit: u64, 
        to: Address, 
        value: U256, 
        in_offset: usize, 
        in_len: usize, 
        out_offset: usize, 
        out_len: usize, 
        input:Bytes, 
        mem_deps: Vec<MemoryDep>, 
        target_address: Address,
        mem_length: Option<usize>) {
        let mut ssa_inputs = Vec::with_capacity(8);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(value))); // value
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory { 
            source: mem_deps
        }); // memory
        ssa_inputs.push(SSAInput::ContractEnv { 
            source: self.get_entry_lsn() 
        }); // target_address

        // Create SSACallInput
        let ssa_call_input = SSACallInput {
            input: input,
            target_address: target_address,
            bytecode_address: to,
            caller: target_address,
            transfer_value: value,
            scheme: match opcode {
                0xF2 => SSACallScheme::CallCode,
                _ => panic!("Invalid opcode")
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::CallInput(Box::new(ssa_call_input))
        );
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(
                SSAOutput::MemorySize(mem_length)
            );
            self.last_memory = self.current_lsn;
        }

        self.last_call.push(self.current_lsn);
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
        
    }   

    pub fn log_delegatecall(&mut self, opcode: u8, 
        local_gas_limit: u64, 
        to: Address, 
        in_offset: usize, 
        in_len: usize, 
        out_offset: usize, 
        out_len: usize, 
        input:Bytes, 
        mem_deps: Vec<MemoryDep>, 
        mem_length: Option<usize>,
        contract_caller: Address,
        contract_target: Address) {
        let mut ssa_inputs = Vec::with_capacity(9);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset   
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory { 
            source: mem_deps
        }); // memory
        ssa_inputs.push(SSAInput::ContractEnv {  // contract_caller
            source: self.get_entry_lsn() 
        });
        ssa_inputs.push(SSAInput::ContractEnv {  // contract_target
            source: self.get_entry_lsn() 
        });

        // Create SSACallInput
        let ssa_call_input = SSACallInput {
            input: input,
            target_address: contract_target,
            bytecode_address: to,
            caller: contract_caller,
            transfer_value: U256::ZERO,
            scheme: match opcode {
                0xF4 => SSACallScheme::DelegateCall,
                _ => panic!("Invalid opcode")
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::CallInput(Box::new(ssa_call_input))
        );
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(
                SSAOutput::MemorySize(mem_length)
            );
            self.last_memory = self.current_lsn;
        }
        self.last_call.push(self.current_lsn);
        self.log_operation(opcode, ssa_inputs, ssa_outputs);

    }

    pub fn log_staticcall(&mut self, opcode: u8, 
        local_gas_limit: u64, 
        to: Address, 
        in_offset: usize, 
        in_len: usize, 
        out_offset: usize, 
        out_len: usize, 
        input:Bytes, 
        mem_deps: Vec<MemoryDep>, 
        mem_length: Option<usize>,
        contract_target: Address ) {
        let mut ssa_inputs = Vec::with_capacity(7);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory { 
            source: mem_deps
        }); // memory
        ssa_inputs.push(SSAInput::ContractEnv {  // contract_target
            source: self.get_entry_lsn() 
        });

        // Create SSACallInput
        let ssa_call_input = SSACallInput {
            input: input,
            target_address: to,
            bytecode_address: to,
            caller: contract_target,
            transfer_value: U256::ZERO,
            scheme: match opcode {
                0xFA => SSACallScheme::StaticCall,
                _ => panic!("Invalid opcode")
            },
            ret_range: out_offset..out_offset+out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::CallInput(Box::new(ssa_call_input))
        );
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(
                SSAOutput::MemorySize(mem_length)
            );
            self.last_memory = self.current_lsn;
        }
        self.last_call.push(self.current_lsn);
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_make_call_frame(&mut self, 
        call_input: SSACallInput, 
        new_caller_info: AccountInfo,
        new_target_info: AccountInfo,
        contract_env: Option<ContractEnv>,
        is_precompile: bool,
        ssa_interpreter_result: Option<SSAInterpreterResult>,
        ) 
    {
        let opcode = InternalOp::MAKE_CALL_FRAME;
        let lsn = self.current_lsn;
        self.entry_lsn.push(lsn);
        let value = call_input.transfer_value;
        let caller = call_input.caller;
        let target_address = call_input.target_address;
        let bytecode_address = call_input.bytecode_address;
        
        let mut ssa_inputs = Vec::with_capacity(6);
        ssa_inputs.push(
            SSAInput::CallInput {
                source: self.last_call.pop().unwrap_or_default(),
            }
        );
        ssa_inputs.push(input_account_info!(self, caller));
        ssa_inputs.push(input_account_info!(self, target_address));
        ssa_inputs.push(input_account_info!(self, bytecode_address));

        // log the read operations
        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);
        self.log_storage_read(StorageKey::AccountInfo(bytecode_address), lsn);
        self.log_storage_read(StorageKey::AccountInfo(target_address), lsn);

        let mut ssa_outputs = Vec::with_capacity(5);

        if !value.is_zero() {
            ssa_outputs.push(output_account_info!(caller, new_caller_info));
            ssa_outputs.push(output_account_info!(target_address, new_target_info));
            self.log_storage_write(StorageKey::AccountInfo(caller), lsn);
            self.log_storage_write(StorageKey::AccountInfo(target_address), lsn);
        }

        if is_precompile {
            // If the call is a precompile, we should log it
            // return result
            ssa_outputs.push(SSAOutput::InterpreterResult(ssa_interpreter_result.unwrap()));
            self.last_interpreter_return = lsn;
        } else if contract_env.is_none() {
            // if the call is a transfer, we should generate a result 
            ssa_outputs.push(SSAOutput::InterpreterResult(ssa_interpreter_result.unwrap()));
            self.last_interpreter_return = lsn;
        } else {
            // if the call is a contract call, we should generate a result
            ssa_outputs.push(SSAOutput::ContractEnv(Box::new(contract_env.unwrap())));
        }

        if self.call_inputs.is_empty() {
            self.first_call_input = Some(call_input.clone());
        }

        self.call_inputs.push(call_input);
        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_call_return(&mut self, interpreter_result: SSAInterpreterResult) {
        let opcode = InternalOp::CALL_RETURN;
        let call_input = self.call_inputs.pop().unwrap();
        let ret_range = call_input.ret_range.clone();

        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(
            SSAInput::InterpreterResult {
                source: self.last_interpreter_return,
            }
        );
        ssa_inputs.push(
            SSAInput::CallInput {
                source: self.last_call.pop().unwrap_or_default(),
            }
        );

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::CallOutcome(Box::new(SSACallOutcome{
                result: interpreter_result,
                ret_range: ret_range,
            }))
        );
        self.last_call_return.push(self.current_lsn);
        self.entry_lsn.pop();
        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_insert_call_outcome(&mut self, call_outcome: SSACallOutcome) -> u16 {
        let opcode = InternalOp::INSERT_CALL_OUTCOME;
        let lsn = self.current_lsn;
        let out_len = call_outcome.ret_range.len();
        let out_result = call_outcome.result.result;
        let return_data_buffer = call_outcome.result.output.clone();

        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(
            SSAInput::CallOutcome { 
                source: self.last_call_return.pop().unwrap_or_default()
            }
        );

        let mut ssa_outputs = Vec::with_capacity(2);
        ssa_outputs.push(SSAOutput::ReturnDataBuffer(return_data_buffer.clone()));

        let target_len = min(out_len, return_data_buffer.len());
        let data_slice = &return_data_buffer[..target_len];
        match out_result {
            SSAInstructionResult::Ok => {
                ssa_outputs.push(SSAOutput::Memory(data_slice.to_vec().into()));
                ssa_outputs.push(SSAOutput::Stack(U256::from(1)));
            },
            SSAInstructionResult::Revert => {
               ssa_outputs.push(SSAOutput::Memory(data_slice.to_vec().into()));
               ssa_outputs.push(SSAOutput::Stack(U256::ZERO));
            },
            SSAInstructionResult::Error => {
                panic!("Error in insert_call_outcome");
            }
        }

        self.last_return_data_buffer = lsn;
        
        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();

        lsn
    }

    #[inline]
    pub fn log_balance_operation(&mut self, opcode: u8, address: Address, value: U256) {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(
                value
            )
        );

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
        // We should also read account_status here, but we can't get it from the host.
        // We assume the difference in account_status won't affect the formal logic much.
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
    }

    #[inline]
    pub fn log_self_balance(&mut self, opcode: u8, target: Address, value: U256) {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(
            SSAInput::ContractEnv { 
                source: self.get_entry_lsn() 
            }
        );
        ssa_inputs.push(input_account_info!(self, target));
        
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(value),
        );
        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
        self.log_storage_read(StorageKey::AccountInfo(target), lsn);
    }

    #[inline]
    pub fn log_extcodesize(&mut self, opcode: u8, address: Address, len: usize) {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(U256::from(len))
        );
        
        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
    }

    #[inline]
    pub fn log_extcodehash(&mut self, opcode: u8, address: Address, code_hash: U256) {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(code_hash)
        );

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
    }

    #[inline]
    pub fn log_extcodecopy(&mut self, 
        opcode: u8, 
        address: Address, 
        mem_offset: usize, 
        code_offset: usize, 
        len: usize, 
        code: Bytes,
        mem_length: Option<usize>
    ) -> u16 {
        let mut ssa_inputs = Vec::with_capacity(5);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(mem_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(code_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(input_account_info!(self, address));

        let code_end = min(code.len(), code_offset+len);
        let code_slice = &code[code_offset..code_end];
        // pad code_slice to len
        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code_slice.len()].copy_from_slice(&code_slice);
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Memory(padded_code_slice.into()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
        lsn
    }

    #[inline]
    pub fn log_blockhash_operation(&mut self, opcode: u8, number: u64, hash: U256) {
        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(number)));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Stack(hash)
        );

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
    }

    #[inline]
    pub fn log_sload(&mut self, opcode: u8, address: Address, index: U256, value: U256) {
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(SSAInput::ContractEnv { source: self.get_entry_lsn() });
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index)));
        ssa_inputs.push(SSAInput::Storage { 
            key: Box::new(StorageKey::Slot(address, index)), 
            source: self.get_storage_def(StorageKey::Slot(address, index)) 
        });
        ssa_inputs.push(input_account_status!(self, address)); // identify if it is created

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(value));
        let lsn =  self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.push_stack_def(lsn).unwrap();
        self.log_storage_read(StorageKey::AccountStatus(address), lsn);
        self.log_storage_read(StorageKey::Slot(address, index), lsn);
    }

    #[inline]
    pub fn log_sstore(&mut self, opcode: u8, address: Address, index: U256, value: U256) {
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(SSAInput::ContractEnv {source: self.get_entry_lsn() });
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index)));
        ssa_inputs.push(pop_stack_or_const!(self, value));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push( 
            SSAOutput::Storage { 
                key: Box::new(StorageKey::Slot(address, index)), 
                value: Box::new(StorageValue::Slot(value)), 
            }
        );

        let lsn = self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.log_storage_write(StorageKey::Slot(address, index), lsn);
    }

    #[inline]
    pub fn log_log_opcode(&mut self, opcode: u8,
        offset: usize,
        len: usize,
        topics: Vec<FixedBytes<32>>,
        mem_deps: Vec<MemoryDep>, 
        log: Log,
        mem_length: Option<usize>) 
    {    
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(SSAInput::ContractEnv {source: self.get_entry_lsn()});
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory {source: mem_deps});

        for topic in topics {
            ssa_inputs.push(pop_stack_or_const!(self, topic.into()));
        }

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(
            SSAOutput::Log(Box::new(log))
        );

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = self.current_lsn;
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_selfdestruct(&mut self, 
        opcode: u8, 
        address: Address, 
        target: Address, 
        is_created: bool, 
        is_cancun_enabled: bool,
        address_info: AccountInfo,
        address_status: AccountStatus,
        target_info: AccountInfo,
    
    ) {
        let mut ssa_inputs = Vec::with_capacity(6);
        ssa_inputs.push(SSAInput::ContractEnv {source: self.get_entry_lsn() });
        ssa_inputs.push(pop_stack_or_const!(self, target.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));
        ssa_inputs.push(input_account_info!(self, target));
        ssa_inputs.push(input_account_status!(self, address)); // identify if it is created
        ssa_inputs.push(SSAInput::Constant(U256::from(is_cancun_enabled)));
        

        let lsn = self.current_lsn;
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
        self.log_storage_read(StorageKey::AccountStatus(address), lsn);
        self.log_storage_read(StorageKey::AccountInfo(target), lsn);

        let mut ssa_outputs = Vec::with_capacity(4);
        if address != target {
            ssa_outputs.push(output_account_info!(target,target_info));
            self.log_storage_write(StorageKey::AccountInfo(target), lsn); //  add balance
        }

        if is_created || !is_cancun_enabled {
            ssa_outputs.push(output_account_info!(address,address_info));
            ssa_outputs.push(output_account_status!(address,address_status));
            self.log_storage_write(StorageKey::AccountStatus(address), lsn); // mark as selfdestruct, it is used when calculate gas
            self.log_storage_write(StorageKey::AccountInfo(address), lsn); // clear balance
        } else if address != target {
            ssa_outputs.push(output_account_info!(address,address_info));
            self.log_storage_write(StorageKey::AccountInfo(address), lsn); // clear balance
        }

        let result = SSAOutput::InterpreterResult(SSAInterpreterResult{
            result: SSAInstructionResult::Ok,
            output: Bytes::default(),
        });
        ssa_outputs.push(result);

        self.last_interpreter_return = lsn;
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline]
    pub fn log_storage_write(&mut self, key: StorageKey, lsn: u16) {
        // eprintln!("log_storage_write: {:?}, {}", key, lsn);
        self.latest_writes.insert(key, lsn);
    }

    #[inline]
    pub fn log_storage_read(&mut self, key: StorageKey, lsn: u16) {
        if !self.latest_writes.contains_key(&key) {
            self.first_reads.entry(key).or_insert(lsn);
        }
    }
    
    #[inline]
    pub fn get_storage_def(&self, key: StorageKey) -> u16 {
        *self.latest_writes.get(&key).unwrap_or(&0)
    }

    #[inline]
    pub fn push_stack_def(&mut self, def: u16) -> Result<(), crate::shadow_stack::InstructionResult> {
        self.stack.push(def)
    }

    #[inline]
    pub fn pop_stack_def(&mut self) -> Result<u16, crate::shadow_stack::InstructionResult> {
        self.stack.pop()
    }

    #[inline]
    pub fn dup_stack_def(&mut self, n: usize) -> Result<(), crate::shadow_stack::InstructionResult> {
        self.stack.dup(n)
    }

    #[inline]
    pub fn swap_stack_def(&mut self, n: usize) -> Result<(), crate::shadow_stack::InstructionResult> {
        self.stack.swap(n)
    }

    #[inline]
    pub fn exchange_stack_def(&mut self, n: usize, m: usize) -> Result<(), crate::shadow_stack::InstructionResult> {
        self.stack.exchange(n, m)
    }

    pub fn take_logs(&mut self) -> Vec<SSALogEntry> {
        std::mem::take(&mut self.logs)
    }

    pub fn get_log(&self, lsn: usize) -> &SSALogEntry {
        &self.logs[lsn]
    }

    pub fn get_latest_writes(&self) -> &HashMap<StorageKey, u16> {
        &self.latest_writes
    }

    pub fn get_first_reads(&self) -> &HashMap<StorageKey, u16> {
        &self.first_reads
    }

    pub fn take_first_reads(&mut self) -> HashMap<StorageKey, u16> {
        std::mem::take(&mut self.first_reads)
    }

    pub fn take_first_call_input(&mut self) -> Option<SSACallInput> {
        std::mem::take(&mut self.first_call_input)
    }

    pub fn take_first_create_input(&mut self) -> Option<SSACreateInput> {
        std::mem::take(&mut self.first_create_input)
    }

    pub fn clear(&mut self) {
        self.current_lsn = 0;
        self.logs.clear();
        self.stack = ShadowStack::new();
        self.latest_writes.clear();
        self.first_reads.clear();
    }

    /// Get the read and write sets of storage accesses
    /// Returns a tuple: (read_set, write_set)
    /// read_set contains storage keys and their LSNs for first reads
    /// write_set contains storage keys and their LSNs for latest writes
    pub fn get_read_write_set(&self) -> SsaRwSet {
        SsaRwSet {
            read_set: self.first_reads.clone(),
            write_set: self.latest_writes.keys().cloned().collect(),
        }
    }
}

/// Perform bytecode analysis.
///
/// The analysis finds and caches valid jump destinations for later execution as an optimization step.
///
/// If the bytecode is already analyzed, it is returned as-is.
#[inline]
pub fn to_analysed(bytecode: Bytecode) -> Bytecode {
    let (bytes, len) = match bytecode {
        Bytecode::LegacyRaw(bytecode) => {
            let len = bytecode.len();
            let mut padded_bytecode = Vec::with_capacity(len + 33);
            padded_bytecode.extend_from_slice(&bytecode);
            padded_bytecode.resize(len + 33, 0);
            (Bytes::from(padded_bytecode), len)
        }
        n => return n,
    };
    let jump_table = analyze(bytes.as_ref());

    Bytecode::LegacyAnalyzed(LegacyAnalyzedBytecode::new(bytes, len, jump_table))
}

/// Analyze bytecode to build a jump map.
fn analyze(code: &[u8]) -> JumpTable {
    let mut jumps: BitVec<u8> = bitvec![u8, Lsb0; 0; code.len()];

    let range = code.as_ptr_range();
    let start = range.start;
    let mut iterator = start;
    let end = range.end;
    while iterator < end {
        let opcode = unsafe { *iterator };
        if 0x5B == opcode {
            // SAFETY: jumps are max length of the code
            unsafe { jumps.set_unchecked(iterator.offset_from(start) as usize, true) }
            iterator = unsafe { iterator.offset(1) };
        } else {
            let push_offset = opcode.wrapping_sub(0x60);
            if push_offset < 32 {
                // SAFETY: iterator access range is checked in the while loop
                iterator = unsafe { iterator.offset((push_offset + 2) as isize) };
            } else {
                // SAFETY: iterator access range is checked in the while loop
                iterator = unsafe { iterator.offset(1) };
            }
        }
    }

    JumpTable(Arc::new(jumps))
}