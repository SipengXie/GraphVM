// TODO: we may need to consider something like the TxEnv, BlockEnv here.
use crate::shadow_stack::ShadowStack;
use crate::types::{
    ContractEnv, InternalOp, MemoryDep, SSAInput, SSALogEntry, SSAOutput, StorageKey, StorageValue,
};
use crate::{
    FrameInput, SSACallOutcome, TxScheme, SSACreateOutcome,
    SSAInstructionResult, SSAInterpreterResult,
};
use revm_primitives::bitvec::bitvec;
use revm_primitives::bitvec::order::Lsb0;
use revm_primitives::bitvec::vec::BitVec;
use revm_primitives::Spec;
use revm_primitives::{
    AccountInfo, AccountStatus, Address, AnalysisKind, Bytecode, Bytes, FixedBytes, HashMap,
    HashSet, JumpTable, LegacyAnalyzedBytecode, Log, B256, U256,
};
use std::cmp::min;
use core::ops::Range;
use std::sync::Arc;
// Update macro pop_stack_or_const to take two parameters: self and value
#[macro_export]
macro_rules! pop_stack_or_const {
    ($self:expr, $value:expr) => {{
        let src = $self.pop_stack_def().unwrap();
        if src.0 == 0 {
            SSAInput::Constant($value)
        } else {
            SSAInput::Stack(src)
        }
    }};
}

// Macro for pushing storage account info
#[macro_export]
macro_rules! input_account_info {
    ($self:expr, $address:expr) => {{
        SSAInput::Storage(
            StorageKey::AccountInfo($address),
            $self.get_storage_def(StorageKey::AccountInfo($address)),
        )
    }};
}

// Macro for pushing storage account status
#[macro_export]
macro_rules! input_account_status {
    ($self:expr, $address:expr) => {{
        SSAInput::Storage(
            StorageKey::AccountStatus($address),
            $self.get_storage_def(StorageKey::AccountStatus($address)),
        )
    }};
}

// Macro for output storage account info
#[macro_export]
macro_rules! output_account_info {
    ($address:expr, $info:expr) => {{
        SSAOutput::Storage {
            key: Box::new(StorageKey::AccountInfo($address)),
            value: Box::new(StorageValue::AccountInfo($info)),
        }
    }};
}

// Macro for output storage account status
#[macro_export]
macro_rules! output_account_status {
    ($address:expr, $status:expr) => {{
        SSAOutput::Storage {
            key: Box::new(StorageKey::AccountStatus($address)),
            value: Box::new(StorageValue::AccountStatus($status)),
        }
    }};
}

#[macro_export]
macro_rules! is_constant {
    ($value:expr) => {{
        matches!($value, SSAInput::Constant(_))
    }};
    ($first:expr, $($rest:expr),+) => {{
        is_constant!($first) $(&& is_constant!($rest))*
    }};
}

/// Type alias for Log Sequence Number (LSN)
///
/// LSN is used to uniquely identify each operation in the execution trace.
/// It helps with dependency tracking and conflict detection in the SSA form.
pub type LsnType = u32;
pub type LsnWithIndex = (LsnType, u8);

/// Macro for padding data with zeros
macro_rules! pad_data {
    ($source:expr, $offset:expr, $len:expr) => {{
        if $offset >= $source.len() {
            vec![0u8; $len]
        } else {
            let end = std::cmp::min($source.len(), $offset + $len);
            let slice = $source.slice($offset..end);
            let mut padded_data = vec![0u8; $len];
            padded_data[..slice.len()].copy_from_slice(&slice);
            padded_data
        }
    }};
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SSALogger {
    // Current LSN
    pub current_lsn: LsnType,
    // Log entries
    logs: Vec<SSALogEntry>,
    // Shadow stack for tracking stack item definitions
    pub stack_pool: Vec<ShadowStack>,
    // Latest writes to storage slots
    // records the latest lsn of sstore that write to the slot
    // used for dependency tracking
    latest_writes: HashMap<StorageKey, LsnWithIndex>,

    // Latest writes to transient storage
    latest_transient_writes: HashMap<(Address, U256), LsnWithIndex>,

    // First reads of storage slots
    // records the first lsn of sload that read the slot
    // used for identifying storage conflicts
    first_reads: HashMap<StorageKey, LsnType>,
    // to record the latest lsn that modifies memory
    last_memory: LsnWithIndex,
    // to record the latest lsn that modifies return data buffer
    last_return_data_buffer: LsnWithIndex,
    // last_interpreter_return
    last_interpreter_return: LsnWithIndex,
    // last_call
    last_sub_call: Vec<LsnWithIndex>,
    // last_create
    last_sub_create: Vec<LsnWithIndex>,
    // last_call_return
    last_call_return: Vec<LsnWithIndex>,
    // last_create_return
    last_create_return: Vec<LsnWithIndex>,
    // Initial LSN
    // Use stack to track contract_env at different levels
    pub contract_env: Vec<LsnWithIndex>,
    // First frame's input
    pub first_frame_input: Option<FrameInput>,

    // memory buffer for storing inputs and outputs
    input_buf: Vec<SSAInput>,
    output_buf: Vec<SSAOutput>,

    // for gas calculation
    gas_cost: Vec<(LsnWithIndex, u64)>,
    gas_refund: Vec<(LsnWithIndex, i64)>,
}

#[derive(Clone, Debug)]
pub struct SsaRwSet {
    pub read_set: HashMap<StorageKey, LsnType>,
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
    fn get_entry_lsn(&mut self) -> LsnWithIndex {
        if self.contract_env.len() > 0 {
            *self.contract_env.last().unwrap()
        } else {
            panic!("entry_lsn is empty");
        }
    }

    pub fn new() -> Self {
        Self {
            current_lsn: 1,
            contract_env: Vec::new(),
            logs: Vec::with_capacity(512),
            stack_pool: vec![ShadowStack::new()],
            latest_writes: HashMap::default(),
            latest_transient_writes: HashMap::default(),
            first_reads: HashMap::default(),
            last_memory: (0, 0),
            last_return_data_buffer: (0, 0),
            last_interpreter_return: (0, 0),
            last_sub_call: vec![],
            last_sub_create: vec![],
            last_call_return: vec![],
            last_create_return: vec![],
            first_frame_input: None,

            input_buf: vec![SSAInput::Constant(U256::ZERO); 3],
            output_buf: vec![SSAOutput::Stack(U256::ZERO); 1],

            gas_cost: vec![],
            gas_refund: vec![],
        }
    }

    pub fn new_with_capacity(capacity: usize) -> Self {
        Self {
            current_lsn: 1,
            contract_env: Vec::new(),
            logs: Vec::with_capacity(capacity),
            stack_pool: vec![ShadowStack::new()],
            latest_writes: HashMap::default(),
            latest_transient_writes: HashMap::default(),
            first_reads: HashMap::default(),
            last_memory: (0, 0),
            last_return_data_buffer: (0, 0),
            last_interpreter_return: (0, 0),
            last_sub_call: vec![],
            last_sub_create: vec![],
            last_call_return: vec![],
            last_create_return: vec![],
            first_frame_input: None,

            input_buf: vec![SSAInput::Constant(U256::ZERO); 3],
            output_buf: vec![SSAOutput::Stack(U256::ZERO); 1],

            gas_cost: vec![],
            gas_refund: vec![],
        }
    }

    /// Check if the logger is empty (has no logs)
    pub fn is_empty(&self) -> bool {
        self.logs.is_empty()
    }

    /// Get the current LSN
    pub fn get_current_lsn(&self) -> LsnType {
        self.current_lsn
    }

    #[inline(always)]
    pub fn log_operation(
        &mut self,
        opcode: u8,
        inputs: Vec<SSAInput>,
        outputs: Vec<SSAOutput>,
    ) -> LsnType {
        if self.current_lsn == LsnType::MAX {
            panic!("LSN overflow: reached maximum LsnType value");
        }

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

    #[inline(always)]
    pub fn log_operation_with_buffer(
        &mut self,
        opcode: u8,
        input_size: usize,
        output_size: usize,
    ) -> LsnType {
        if self.current_lsn == LsnType::MAX {
            panic!("LSN overflow: reached maximum LsnType value");
        }

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

    /// Corresponding Execution Function
    /// [execute_deduct_caller](revm_ssa_graph/instructions/contract.rs -> execute_deduct_caller)
    #[inline(always)]
    pub fn log_deduct_caller(
        &mut self,
        caller: Address,
        new_info: AccountInfo,
        gas_cost: U256,
        is_create: bool,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(SSAInput::Constant(caller.into_word().into()));
        ssa_inputs.push(SSAInput::Constant(U256::from(is_create)));
        ssa_inputs.push(SSAInput::Constant(gas_cost));
        ssa_inputs.push(input_account_info!(self, caller));
        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(output_account_info!(caller, new_info));
        self.log_storage_write(StorageKey::AccountInfo(caller), lsn, 0); // this should also log the index of the ssa output

        self.log_operation(0xDA, ssa_inputs, ssa_outputs);
    }

    // TODO: the refund_gas may not be constant, some extra work is needed here.
    // Corresponding Execution Function
    /// [execute_refund_gas](revm_ssa_graph/instructions/contract.rs -> execute_refund_gas)
    #[inline(always)]
    pub fn log_refund_gas(
        &mut self,
        caller: Address,
        new_info: AccountInfo,
        effective_gas_price: U256,
        origin_gas_remaining: u64,
        origin_gas_refunded: i64,
        computed_gas_remaining: u64,
        computed_gas_refunded: i64,
        eip7702_gas_refund: i64,
        gas_limit: u64,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(5 + self.gas_cost.len() + self.gas_refund.len());
        let mut gas_remaining = origin_gas_remaining;
        let mut gas_refunded = origin_gas_refunded;
        for gas_cost in self.gas_cost.iter() {
            ssa_inputs.push(SSAInput::GasCost(gas_cost.0)); // dependency to the dynamic gas cost
            gas_remaining += gas_cost.1; // we ignore the sstore gas cost, and will re-calculate it in the execution phase
        }
        for gas_refund in self.gas_refund.iter() {
            ssa_inputs.push(SSAInput::GasRefund(gas_refund.0)); // dependency to the dynamic gas refund
            gas_refunded -= gas_refund.1; // we ignore the sstore gas refund, and will re-calculate it in the execution phase
        }

        ssa_inputs.push(SSAInput::Constant(caller.into_word().into()));
        ssa_inputs.push(SSAInput::Constant(effective_gas_price));
        ssa_inputs.push(SSAInput::Constant(U256::from(gas_remaining))); // base gas remaining
        ssa_inputs.push(SSAInput::ConstantI64(gas_refunded)); // base gas refunded
        ssa_inputs.push(SSAInput::ConstantI64(eip7702_gas_refund)); // eip7702 gas refund
        ssa_inputs.push(SSAInput::Constant(U256::from(gas_limit))); // gas limit
        ssa_inputs.push(input_account_info!(self, caller));

        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);

        let mut ssa_outputs = Vec::with_capacity(3);
        ssa_outputs.push(output_account_info!(caller, new_info));
        self.log_storage_write(StorageKey::AccountInfo(caller), lsn, 0);
        ssa_outputs.push(SSAOutput::Gas(computed_gas_remaining));
        ssa_outputs.push(SSAOutput::GasRefund(computed_gas_refunded));

        self.log_operation(0xDB, ssa_inputs, ssa_outputs);
    }

    // TODO: the reward may not be constant, some extra work is needed here.
    #[inline(always)]
    pub fn log_reward_beneficiary(
        &mut self,
        beneficiary: Address,
        new_info: AccountInfo,
        reward: U256,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(SSAInput::Constant(beneficiary.into_word().into()));
        ssa_inputs.push(SSAInput::Constant(reward));
        ssa_inputs.push(input_account_info!(self, beneficiary));
        self.log_storage_read(StorageKey::AccountInfo(beneficiary), lsn);

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(output_account_info!(beneficiary, new_info));
        self.log_storage_write(StorageKey::AccountInfo(beneficiary), lsn, 0);

        self.log_operation(0xDC, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_monotonic_operation(&mut self, opcode: u8, operand1: U256, result: U256) {
        let lsn = self.current_lsn;
        let operand1_ssa_input = pop_stack_or_const!(self, operand1);
        if is_constant!(operand1_ssa_input) {
            self.push_stack_def((0, 0)).unwrap();
        } else {
            self.input_buf[0] = operand1_ssa_input;
            self.output_buf[0] = SSAOutput::Stack(result);
            self.push_stack_def((lsn, 0)).unwrap();
            self.log_operation_with_buffer(opcode, 1, 1);
        }
    }

    #[inline(always)]
    pub fn log_binary_operation(
        &mut self,
        opcode: u8,
        operand1: U256,
        operand2: U256,
        result: U256,
    ) {
        let lsn = self.current_lsn;
        let operand1_ssa_input = pop_stack_or_const!(self, operand1);
        let operand2_ssa_input = pop_stack_or_const!(self, operand2);
        if is_constant!(operand1_ssa_input, operand2_ssa_input) {
            self.push_stack_def((0, 0)).unwrap();
        } else {
            self.input_buf[0] = operand1_ssa_input;
            self.input_buf[1] = operand2_ssa_input;
            self.output_buf[0] = SSAOutput::Stack(result);
            self.push_stack_def((lsn, 0)).unwrap();
            self.log_operation_with_buffer(opcode, 2, 1);
        }
    }

    #[inline(always)]
    pub fn log_trinary_operation(
        &mut self,
        opcode: u8,
        operand1: U256,
        operand2: U256,
        operand3: U256,
        result: U256,
    ) {
        let lsn = self.current_lsn;
        let operand1_ssa_input = pop_stack_or_const!(self, operand1);
        let operand2_ssa_input = pop_stack_or_const!(self, operand2);
        let operand3_ssa_input = pop_stack_or_const!(self, operand3);
        if is_constant!(operand1_ssa_input, operand2_ssa_input, operand3_ssa_input) {
            self.push_stack_def((0, 0)).unwrap();
        } else {
            self.input_buf[0] = operand1_ssa_input;
            self.input_buf[1] = operand2_ssa_input;
            self.input_buf[2] = operand3_ssa_input;
            self.output_buf[0] = SSAOutput::Stack(result);
            self.push_stack_def((lsn, 0)).unwrap();
            self.log_operation_with_buffer(opcode, 3, 1);
        }
    }

    #[inline(always)]
    pub fn log_pop_operation(&mut self, _opcode: u8) {
        self.pop_stack_def().unwrap();
    }

    #[inline(always)]
    pub fn log_push_operation(&mut self, _opcode: u8, _result: &[u8]) {
        self.push_stack_def((0, 0)).unwrap();
    }

    #[inline(always)]
    pub fn log_dup_operation(&mut self, _opcode: u8, n: usize) {
        self.dup_stack_def(n).unwrap();
    }

    #[inline(always)]
    pub fn log_swap_operation(&mut self, _opcode: u8, n: usize) {
        self.swap_stack_def(n).unwrap();
    }

    #[inline(always)]
    pub fn log_exchange_operation(&mut self, opcode: u8, n: usize, m: usize) {
        let ssa_inputs = Vec::new();
        let ssa_outputs = Vec::new();
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
        self.exchange_stack_def(n, m).unwrap();
    }

    #[inline(always)]
    pub fn log_jump(
        &mut self,
        opcode: u8,
        target: usize,
        current_pc: usize,
        relative_offset: isize,
    ) {
        let target_ssa_input = pop_stack_or_const!(self, U256::from(target));
        if !is_constant!(target_ssa_input) {
            self.input_buf[0] = target_ssa_input;
            self.input_buf[1] = SSAInput::Constant(U256::from(current_pc));
            self.output_buf[0] = SSAOutput::Jump(relative_offset);
            self.log_operation_with_buffer(opcode, 2, 1);
        }
    }

    #[inline(always)]
    pub fn log_jumpi(
        &mut self,
        opcode: u8,
        target: usize,
        cond: U256,
        current_pc: usize,
        relative_offset: isize,
    ) {
        let target_ssa_input = pop_stack_or_const!(self, U256::from(target));
        let cond_ssa_input = pop_stack_or_const!(self, cond);
        if !is_constant!(target_ssa_input, cond_ssa_input) {
            self.input_buf[0] = target_ssa_input;
            self.input_buf[1] = cond_ssa_input;
            self.input_buf[2] = SSAInput::Constant(U256::from(current_pc));
            self.output_buf[0] = SSAOutput::Jump(relative_offset);
            self.log_operation_with_buffer(opcode, 3, 1);
        }
    }

    #[inline(always)]
    pub fn log_pc_operation(&mut self, _opcode: u8, _result: usize) {
        self.push_stack_def((0, 0)).unwrap();
    }

    #[inline(always)]
    pub fn log_mload_operation(
        &mut self,
        opcode: u8,
        offset: usize,
        result: U256,
        memory_deps: Vec<MemoryDep>,
        mem_length: Option<usize>,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(SSAInput::Memory(memory_deps));

        let mut ssa_outputs = Vec::with_capacity(2);
        ssa_outputs.push(SSAOutput::Stack(result));
        self.push_stack_def((lsn, 0)).unwrap();
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    // TODO: need to cooperate with shadow memory
    #[inline(always)]
    pub fn log_mstore_operation(
        &mut self,
        opcode: u8,
        offset: usize,
        value: U256,
        mem_length: Option<usize>,
    ) -> LsnType {
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, value));

        let mut ssa_outputs = Vec::with_capacity(2);
        // Index is 0
        if opcode == 0x52 {
            ssa_outputs.push(SSAOutput::Memory(value.to_be_bytes::<32>().into()));
        } else {
            ssa_outputs.push(SSAOutput::Memory([value.byte(0)].into()));
        }

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (self.current_lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline(always)]
    pub fn log_msize_operation(&mut self, opcode: u8, mem_length: usize) {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(SSAInput::MemorySizeChange(self.last_memory));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(U256::from(mem_length)));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_mcopy_operation(
        &mut self,
        opcode: u8,
        dst: usize,
        src: usize,
        len: usize,
        result: Bytes,
        memory_deps: Vec<MemoryDep>,
        mem_length: Option<usize>,
    ) -> LsnType {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(dst)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(src)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory(memory_deps));

        let mut ssa_outputs = Vec::with_capacity(2);
        // Index is 0
        ssa_outputs.push(SSAOutput::Memory(result.clone()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline(always)]
    pub fn log_return(
        &mut self,
        opcode: u8,
        offset: usize,
        len: usize,
        output: Bytes,
        mem_deps: Vec<MemoryDep>,
        mem_length: Option<usize>,
        result: SSAInstructionResult,
    ) {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory(mem_deps));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::InterpreterResult(SSAInterpreterResult {
            result: result,
            output: output,
        }));
        self.last_interpreter_return = (lsn, 0);

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_instruction_result_change(&mut self, opcode: u8, result: SSAInstructionResult) {
        let lsn = self.current_lsn;
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::InterpreterResult(SSAInterpreterResult {
            result,
            output: Bytes::new(), // Empty output for stop/invalid/unknown cases
        }));
        self.last_interpreter_return = (lsn, 0);

        self.log_operation(opcode, Vec::new(), ssa_outputs);
    }

    #[inline(always)]
    pub fn log_host_env_operation(&mut self, opcode: u8, result: U256) {
        let lsn = self.current_lsn;
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(result));
        self.push_stack_def((lsn, 0)).unwrap();
        self.log_operation(opcode, Vec::new(), ssa_outputs);
    }

    #[inline(always)]
    pub fn log_blobhash_operation(&mut self, opcode: u8, index: usize, result: U256) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index)));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(result));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_system_operation(&mut self, opcode: u8, contract_env: ContractEnv) {
        let lsn = self.current_lsn;
        let mut ssa_input = Vec::with_capacity(1);
        ssa_input.push(SSAInput::ContractEnv(self.get_entry_lsn()));

        let mut ssa_output = Vec::with_capacity(1);
        match opcode {
            0x30 => ssa_output.push(SSAOutput::Stack(
                contract_env.frame_input.target_address.into_word().into(),
            )), // ADDRESS
            0x33 => ssa_output.push(SSAOutput::Stack(contract_env.frame_input.caller.into_word().into())), // CALLER
            0x34 => ssa_output.push(SSAOutput::Stack(contract_env.frame_input.transfer_value)), // CALLVALUE
            0x36 => ssa_output.push(SSAOutput::Stack(U256::from(contract_env.frame_input.input.len()))), // CALLDATASIZE
            0x38 => ssa_output.push(SSAOutput::Stack(U256::from(contract_env.bytecode.len()))), // CODESIZE
            _ => unreachable!("Unsupported system operation: {}", opcode),
        }
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_input, ssa_output);
    }

    #[inline(always)]
    pub fn log_return_data_size(&mut self, opcode: u8, value: Bytes) {
        let lsn = self.current_lsn;

        let len = value.len();
        let mut ssa_input = Vec::with_capacity(1);
        ssa_input.push(SSAInput::ReturnDataBuffer(self.last_return_data_buffer));

        let mut ssa_output = Vec::with_capacity(1);
        ssa_output.push(SSAOutput::Stack(U256::from(len)));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_input, ssa_output);
    }

    // Corresponding Execution Function
    // [execute_returndatacopy]
    #[inline(always)]
    pub fn log_return_data_cpy_operation(
        &mut self,
        opcode: u8,
        meme_offset: usize,
        data_offset: usize,
        len: usize,
        return_data: Bytes,
        mem_length: Option<usize>,
    ) -> LsnType {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(meme_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(data_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::ReturnDataBuffer(self.last_return_data_buffer));

        // When len is 0, return an empty vector
        let padded_return_data_slice = if len == 0 {
            Vec::new()
        } else {
            pad_data!(return_data, data_offset, len)
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        // Index is 0
        ssa_outputs.push(SSAOutput::Memory(padded_return_data_slice.into()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    // Corresponding Execution Function
    // [execute_codecopy]
    #[inline(always)]
    pub fn log_codecopy(
        &mut self,
        opcode: u8,
        memory_offset: usize,
        code_offset: usize,
        len: usize,
        code: Bytes,
        mem_length: Option<usize>,
    ) -> LsnType {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(memory_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(code_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));

        // When len is 0, return an empty vector
        let padded_code_slice = if len == 0 {
            Vec::new()
        } else {
            pad_data!(code, code_offset, len)
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        // Index is 0
        ssa_outputs.push(SSAOutput::Memory(padded_code_slice.into()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    // Corresponding Execution Function
    // [execute_calldatacopy]
    #[inline(always)]
    pub fn log_call_data_copy(
        &mut self,
        opcode: u8,
        memory_offset: usize,
        data_offset: usize,
        len: usize,
        data: Bytes,
        mem_length: Option<usize>,
    ) -> LsnType {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(memory_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(data_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));

        // When len is 0, return an empty vector
        let padded_data_slice = if len == 0 {
            Vec::new()
        } else {
            pad_data!(data, data_offset, len)
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        // Index is 0
        ssa_outputs.push(SSAOutput::Memory(padded_data_slice.into()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline(always)]
    pub fn log_return_data_load(&mut self, opcode: u8, offset: usize, return_data: Bytes) {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(SSAInput::ReturnDataBuffer(self.last_return_data_buffer));

        let mut output = [0u8; 32];
        if let Some(available) = return_data.len().checked_sub(offset) {
            let copy_len = available.min(32);
            output[..copy_len].copy_from_slice(&return_data[offset..offset + copy_len]);
        }

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(B256::from(output).into()));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_call_data_load(&mut self, opcode: u8, offset: usize, data: Bytes) {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));

        let mut word = [0u8; 32];
        if offset < data.len() {
            let length = 32.min(data.len() - offset);
            word[..length].copy_from_slice(&data[offset..offset + length]);
        }

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(B256::from_slice(&word).into()));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    // ! IN SSA, this function is simple!
    // ! For a formal implementation, we should consider all front-loaded dynamic gas commands
    pub fn log_gas(&mut self, opcode: u8, gas: u64) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(SSAInput::Constant(U256::from(gas)));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(U256::from(gas)));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_keccak256(
        &mut self,
        opcode: u8,
        offset: usize,
        len: usize,
        data: &[u8],
        mem_deps: Vec<MemoryDep>,
        mem_length: Option<usize>,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory(mem_deps));

        let hash = revm_primitives::keccak256(data);
        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(hash.into()));
        self.push_stack_def((lsn, 0)).unwrap();

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_create(
        &mut self,
        opcode: u8,
        value: U256,
        code_offset: usize,
        len: usize,
        code: Bytes,
        code_deps: Vec<MemoryDep>,
        target: Address,
        salt: Option<U256>,
        mem_length: Option<usize>,
    ) {
        let lsn = self.current_lsn;
        // inputs
        let mut ssa_inputs = Vec::with_capacity(6);
        ssa_inputs.push(pop_stack_or_const!(self, value)); // value
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(code_offset))); // code_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len))); // len
        ssa_inputs.push(SSAInput::Memory(code_deps)); // code
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // target_address
        if let Some(salt) = salt {
            ssa_inputs.push(pop_stack_or_const!(self, salt)); // salt
        }

        let mut padded_code_slice = vec![0u8; len];
        padded_code_slice[..code.len()].copy_from_slice(&code);

        // outputs
        let ssa_create_input = FrameInput {
            input: padded_code_slice.into(),
            transfer_value: value,
            caller: target,
            scheme: if opcode == 0xF0 {
                TxScheme::Create
            } else {
                TxScheme::Create2 {
                    salt: salt.unwrap(),
                }
            },
            ..Default::default()
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::FrameInput(Box::new(ssa_create_input)));
        self.last_sub_create.push((lsn, 0));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_make_create_frame(
        &mut self,
        new_caller_info: AccountInfo,
        new_target_info: AccountInfo,
        new_target_status: AccountStatus, // needed as it is marked created
        contract_env: ContractEnv, // which contains create_input
    ) {
        let opcode = InternalOp::MAKE_CREATE_FRAME;
        let lsn = self.current_lsn;
        let caller = contract_env.frame_input.caller;
        let created_address = contract_env.frame_input.target_address;

        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(SSAInput::FrameInput(
            self.last_sub_create.pop().unwrap_or_default(),
        ));
        ssa_inputs.push(input_account_info!(self, caller));
        ssa_inputs.push(input_account_info!(self, created_address));

        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);
        self.log_storage_read(StorageKey::AccountInfo(created_address), lsn);

        if self.first_frame_input.is_none() {
            self.first_frame_input = Some(contract_env.frame_input.clone());
        }

        let mut ssa_outputs = Vec::with_capacity(4);
        ssa_outputs.push(output_account_info!(caller, new_caller_info));
        self.log_storage_write(StorageKey::AccountInfo(caller), lsn, 0);
        ssa_outputs.push(output_account_info!(created_address, new_target_info));
        self.log_storage_write(StorageKey::AccountInfo(created_address), lsn, 1);
        ssa_outputs.push(output_account_status!(created_address, new_target_status));
        self.log_storage_write(StorageKey::AccountStatus(created_address), lsn, 2);
        ssa_outputs.push(SSAOutput::ContractEnv(Box::new(contract_env)));
        self.contract_env.push((lsn, 3));
        self.generate_new_stack();

        
        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_create_return_failed(&mut self, result: &SSAInterpreterResult) {
        let opcode = InternalOp::CREATE_RETURN;
        let lsn = self.current_lsn;
        self.contract_env.pop();
        let create_outcome = SSACreateOutcome {
            result: result.clone(),
            address: None,
        };
        let mut ssa_outputs = Vec::with_capacity(1);

        ssa_outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
        self.last_create_return.push((lsn, 0));
        self.remove_last_stack();
        self.log_operation(opcode.into(), vec![], ssa_outputs);
    }

    #[inline(always)]
    pub fn log_create_return<SPEC: Spec>(
        &mut self,
        result: &SSAInterpreterResult,
        address: Address,
        target_info: AccountInfo,
        analysis_kind: &AnalysisKind,
    ) {
        let opcode = InternalOp::CREATE_RETURN;
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(SSAInput::InterpreterResult(self.last_interpreter_return)); // interpreter_result
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // address
        ssa_inputs.push(input_account_info!(self, address));
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
        ssa_inputs.push(match analysis_kind {
            AnalysisKind::Raw => SSAInput::Constant(U256::from(0)),
            AnalysisKind::Analyse => SSAInput::Constant(U256::from(1)),
        });

        self.contract_env.pop();

        let create_outcome = SSACreateOutcome {
            result: result.clone(),
            address: Some(address),
        };

        let mut ssa_outputs = Vec::with_capacity(2);

        ssa_outputs.push(SSAOutput::CreateOutcome(Box::new(create_outcome)));
        self.last_create_return.push((lsn, 0));
        self.remove_last_stack();
        ssa_outputs.push(output_account_info!(address, target_info));
        self.log_storage_write(StorageKey::AccountInfo(address), lsn, 1);

        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
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
        ssa_inputs.push(SSAInput::CreateOutcome(
            self.last_create_return.pop().unwrap_or_default(),
        ));

        let mut ssa_outputs = Vec::with_capacity(2);
        ssa_outputs.push(SSAOutput::ReturnDataBuffer(return_data_buffer.clone()));
        self.last_return_data_buffer = (lsn, 0);
        match instruction_result {
            SSAInstructionResult::Ok => {
                let address = address.unwrap();
                ssa_outputs.push(SSAOutput::Stack(address.into_word().into()));
            }
            SSAInstructionResult::Revert => {
                ssa_outputs.push(SSAOutput::Stack(U256::ZERO.into()));
            }
            SSAInstructionResult::Error => {
                panic!("Error in insert_create_outcome");
            }
        }

        self.push_stack_def((lsn, 1)).unwrap();

        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_call(
        &mut self,
        opcode: u8,
        local_gas_limit: u64,
        to: Address,
        value: U256,
        in_offset: usize,
        in_len: usize,
        out_offset: usize,
        out_len: usize,
        input: Bytes,
        mem_deps: Vec<MemoryDep>,
        target_address: Address,
        mem_length: Option<usize>,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(7);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(value))); // value
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory(mem_deps)); // memory
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // target_address

        // Create SSACallInput
        let ssa_call_input = FrameInput {
            input,
            target_address: to,
            bytecode_address: to,
            caller: target_address,
            transfer_value: value,
            scheme: match opcode {
                0xF1 => TxScheme::Call,
                _ => panic!("Invalid opcode"),
            },
            ret_range: out_offset..out_offset + out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::FrameInput(Box::new(ssa_call_input)));
        self.last_sub_call.push((lsn, 0));
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    pub fn log_call_code(
        &mut self,
        opcode: u8,
        local_gas_limit: u64,
        to: Address,
        value: U256,
        in_offset: usize,
        in_len: usize,
        out_offset: usize,
        out_len: usize,
        input: Bytes,
        mem_deps: Vec<MemoryDep>,
        target_address: Address,
        mem_length: Option<usize>,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(8);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(value))); // value
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory(mem_deps)); // memory
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // target_address

        // Create SSACallInput
        let ssa_call_input = FrameInput {
            input,
            target_address,
            bytecode_address: to,
            caller: target_address,
            transfer_value: value,
            scheme: match opcode {
                0xF2 => TxScheme::CallCode,
                _ => panic!("Invalid opcode"),
            },
            ret_range: out_offset..out_offset + out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::FrameInput(Box::new(ssa_call_input)));
        self.last_sub_call.push((lsn, 0));
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    pub fn log_delegatecall(
        &mut self,
        opcode: u8,
        local_gas_limit: u64,
        to: Address,
        in_offset: usize,
        in_len: usize,
        out_offset: usize,
        out_len: usize,
        input: Bytes,
        mem_deps: Vec<MemoryDep>,
        mem_length: Option<usize>,
        contract_caller: Address,
        contract_target: Address,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(9);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory(mem_deps)); // memory
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // contract_caller
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // contract_target

        // Create SSACallInput
        let ssa_call_input = FrameInput {
            input,
            target_address: contract_target,
            bytecode_address: to,
            caller: contract_caller,
            transfer_value: U256::ZERO,
            scheme: match opcode {
                0xF4 => TxScheme::DelegateCall,
                _ => panic!("Invalid opcode"),
            },
            ret_range: out_offset..out_offset + out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);

        ssa_outputs.push(SSAOutput::FrameInput(Box::new(ssa_call_input)));
        self.last_sub_call.push((lsn, 0));
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    pub fn log_staticcall(
        &mut self,
        opcode: u8,
        local_gas_limit: u64,
        to: Address,
        in_offset: usize,
        in_len: usize,
        out_offset: usize,
        out_len: usize,
        input: Bytes,
        mem_deps: Vec<MemoryDep>,
        mem_length: Option<usize>,
        contract_target: Address,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(7);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(local_gas_limit))); // local_gas_limit
        ssa_inputs.push(pop_stack_or_const!(self, to.into_word().into())); // to
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_offset))); // in_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(in_len))); // in_len
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_offset))); // out_offset
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(out_len))); // out_len
        ssa_inputs.push(SSAInput::Memory(mem_deps)); // memory
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // contract_target

        // Create SSACallInput
        let ssa_call_input = FrameInput {
            input,
            target_address: to,
            bytecode_address: to,
            caller: contract_target,
            transfer_value: U256::ZERO,
            scheme: match opcode {
                0xFA => TxScheme::StaticCall,
                _ => panic!("Invalid opcode"),
            },
            ret_range: out_offset..out_offset + out_len,
            gas_limit: local_gas_limit,
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::FrameInput(Box::new(ssa_call_input)));
        self.last_sub_call.push((lsn, 0));
        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_make_call_frame(
        &mut self,
        call_input: FrameInput,
        new_caller_info: AccountInfo,
        new_target_info: AccountInfo,
        contract_env: Option<ContractEnv>,
        is_precompile: bool,
        ssa_interpreter_result: Option<SSAInterpreterResult>,
    ) {
        let opcode = InternalOp::MAKE_CALL_FRAME;
        let lsn = self.current_lsn;
        let value = call_input.transfer_value;
        let caller = call_input.caller;
        let target_address = call_input.target_address;
        let bytecode_address = call_input.bytecode_address;

        let mut ssa_inputs = Vec::with_capacity(6);
        ssa_inputs.push(
            SSAInput::FrameInput(self.last_sub_call.pop().unwrap_or_default()), // 0 if it is the first call
        );
        ssa_inputs.push(input_account_info!(self, caller));
        ssa_inputs.push(input_account_info!(self, target_address));
        ssa_inputs.push(input_account_info!(self, bytecode_address));

        // log the read operations
        self.log_storage_read(StorageKey::AccountInfo(caller), lsn);
        self.log_storage_read(StorageKey::AccountInfo(bytecode_address), lsn);
        self.log_storage_read(StorageKey::AccountInfo(target_address), lsn);

        let mut ssa_outputs = Vec::with_capacity(3);

        if !value.is_zero() {
            ssa_outputs.push(output_account_info!(caller, new_caller_info));
            ssa_outputs.push(output_account_info!(target_address, new_target_info));
            self.log_storage_write(StorageKey::AccountInfo(caller), lsn, 0);
            self.log_storage_write(StorageKey::AccountInfo(target_address), lsn, 1);
        }

        if is_precompile {
            // If the call is a precompile, we should log it
            // return result
            ssa_outputs.push(SSAOutput::InterpreterResult(
                ssa_interpreter_result.unwrap(),
            ));
            self.last_interpreter_return = (lsn, (ssa_outputs.len() - 1) as u8);
        } else if contract_env.is_none() {
            // if the call is a transfer, we should generate a result
            ssa_outputs.push(SSAOutput::InterpreterResult(
                ssa_interpreter_result.unwrap(),
            ));
            self.last_interpreter_return = (lsn, (ssa_outputs.len() - 1) as u8);
        } else {
            // if the call is a contract call, we should generate a result
            ssa_outputs.push(SSAOutput::ContractEnv(Box::new(contract_env.unwrap())));
            self.contract_env.push((lsn, (ssa_outputs.len() - 1) as u8));
            self.generate_new_stack();
        }

        if self.first_frame_input.is_none() {
            self.first_frame_input = Some(call_input);
        }

        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_call_return(&mut self, interpreter_result: SSAInterpreterResult, ret_range: Range<usize>) {
        let lsn = self.current_lsn;
        let opcode = InternalOp::CALL_RETURN;

        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(SSAInput::InterpreterResult(self.last_interpreter_return)); // interpreter_result
        ssa_inputs.push(SSAInput::ContractEnv(self.contract_env.pop().unwrap_or_default()));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::CallOutcome(Box::new(SSACallOutcome {
            result: interpreter_result,
            ret_range,
        })));
        self.last_call_return.push((lsn, 0));
        
        self.remove_last_stack();
        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs);
    }

    // Corresponding Execution Function
    // [execute_insert_call_outcome]
    #[inline(always)]
    pub fn log_insert_call_outcome(&mut self, call_outcome: SSACallOutcome) -> LsnType {
        let opcode = InternalOp::INSERT_CALL_OUTCOME;
        let lsn = self.current_lsn;
        let return_data_buffer = call_outcome.result.output.clone();

        let target_len = min(call_outcome.ret_range.len(), return_data_buffer.len());
        let data_slice = &return_data_buffer[..target_len];

        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(SSAInput::CallOutcome(
            self.last_call_return.pop().unwrap_or_default(),
        ));

        let mut ssa_outputs = Vec::with_capacity(3);
        ssa_outputs.push(SSAOutput::ReturnDataBuffer(return_data_buffer.clone()));
        self.last_return_data_buffer = (lsn, 0);

        // Add memory output and stack value based on result
        // Index is 1
        ssa_outputs.push(SSAOutput::Memory(data_slice.to_vec().into()));
        let success = match call_outcome.result.result {
            SSAInstructionResult::Ok => U256::from(1),
            SSAInstructionResult::Revert => U256::ZERO,
            SSAInstructionResult::Error => panic!("Error in insert_call_outcome"),
        };
        ssa_outputs.push(SSAOutput::Stack(success));
        self.push_stack_def((lsn, 2)).unwrap();

        self.log_operation(opcode.into(), ssa_inputs, ssa_outputs)
    }

    #[inline(always)]
    pub fn log_balance_operation(&mut self, opcode: u8, address: Address, value: U256) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(value));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_self_balance(&mut self, opcode: u8, target: Address, value: U256) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));
        ssa_inputs.push(input_account_info!(self, target));
        self.log_storage_read(StorageKey::AccountInfo(target), lsn);

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(value));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_extcodesize(&mut self, opcode: u8, address: Address, len: usize) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(U256::from(len)));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_extcodehash(&mut self, opcode: u8, address: Address, code_hash: U256) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(2);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(code_hash));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    // Corresponding Execution Function
    // [execute_extcodecopy]
    #[inline(always)]
    pub fn log_extcodecopy(
        &mut self,
        opcode: u8,
        address: Address,
        mem_offset: usize,
        code_offset: usize,
        len: usize,
        code: Bytes,
        mem_length: Option<usize>,
    ) -> LsnType {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(5);
        ssa_inputs.push(pop_stack_or_const!(self, address.into_word().into()));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(mem_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(code_offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(input_account_info!(self, address));
        self.log_storage_read(StorageKey::AccountInfo(address), lsn);

        // When len is 0, return an empty vector
        let padded_code_slice = if len == 0 {
            Vec::new()
        } else {
            pad_data!(code, code_offset, len)
        };

        let mut ssa_outputs = Vec::with_capacity(1);
        // Index is 0
        ssa_outputs.push(SSAOutput::Memory(padded_code_slice.into()));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 0);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs)
    }

    #[inline(always)]
    pub fn log_blockhash_operation(&mut self, opcode: u8, number: u64, hash: U256) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(1);
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(number)));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(hash));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_sload(&mut self, opcode: u8, address: Address, index: U256, value: U256) {
        let lsn = self.current_lsn;

        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index)));
        ssa_inputs.push(SSAInput::Storage(
            StorageKey::Slot(address, index),
            self.get_storage_def(StorageKey::Slot(address, index)),
        ));
        self.log_storage_read(StorageKey::Slot(address, index), lsn);
        ssa_inputs.push(input_account_status!(self, address)); // identify if it is created
        self.log_storage_read(StorageKey::AccountStatus(address), lsn);

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(value));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_sstore(
        &mut self,
        opcode: u8,
        address: Address,
        index: U256,
        value: U256,
        gas_cost: u64,
        gas_refund: i64,
    ) {
        let lsn = self.current_lsn;
        let key = StorageKey::Slot(address, index);
        let is_read = self.first_reads.contains_key(&key);
        let mut ssa_inputs = Vec::with_capacity(6);
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index)));
        ssa_inputs.push(pop_stack_or_const!(self, value));
        ssa_inputs.push(SSAInput::Storage(key, (0, 0))); // origin value
        ssa_inputs.push(SSAInput::Storage(key, self.get_storage_def(key))); // present value
        ssa_inputs.push(SSAInput::Constant(U256::from(is_read)));

        let mut ssa_outputs = Vec::with_capacity(3);
        ssa_outputs.push(SSAOutput::Storage {
            key: Box::new(key),
            value: Box::new(StorageValue::Slot(value)),
        });
        ssa_outputs.push(SSAOutput::Gas(gas_cost));
        ssa_outputs.push(SSAOutput::GasRefund(gas_refund));
        self.log_storage_write(StorageKey::Slot(address, index), lsn, 0);
        self.gas_cost.push(((lsn, 1), gas_cost));
        self.gas_refund.push(((lsn, 2), gas_refund));
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_tstore(&mut self, opcode: u8, address: Address, index: U256, value: U256) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // address
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index))); // index
        ssa_inputs.push(pop_stack_or_const!(self, value)); // value

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Transient(value));
        self.log_transient_write((address, index), lsn, 0);
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_tload(&mut self, opcode: u8, address: Address, index: U256, value: U256) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(3);
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn())); // address
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(index))); // index
        ssa_inputs.push(SSAInput::Transient(
            self.get_transient_def((address, index)),
        ));

        let mut ssa_outputs = Vec::with_capacity(1);
        ssa_outputs.push(SSAOutput::Stack(value));
        self.push_stack_def((lsn, 0)).unwrap();

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_log_opcode(
        &mut self,
        opcode: u8,
        offset: usize,
        len: usize,
        topics: Vec<FixedBytes<32>>,
        mem_deps: Vec<MemoryDep>,
        log: Log,
        mem_length: Option<usize>,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(4);
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(offset)));
        ssa_inputs.push(pop_stack_or_const!(self, U256::from(len)));
        ssa_inputs.push(SSAInput::Memory(mem_deps));

        for topic in topics {
            ssa_inputs.push(pop_stack_or_const!(self, topic.into()));
        }

        let mut ssa_outputs = Vec::with_capacity(1);
        // log index is 0
        ssa_outputs.push(SSAOutput::Log(Box::new(log)));

        if let Some(mem_length) = mem_length {
            ssa_outputs.push(SSAOutput::MemorySize(mem_length));
            self.last_memory = (lsn, 1);
        }

        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_selfdestruct(
        &mut self,
        opcode: u8,
        address: Address,
        target: Address,
        is_created: bool,
        is_cancun_enabled: bool,
        address_info: AccountInfo,
        address_status: AccountStatus,
        target_info: AccountInfo,
    ) {
        let lsn = self.current_lsn;
        let mut ssa_inputs = Vec::with_capacity(6);
        ssa_inputs.push(SSAInput::ContractEnv(self.get_entry_lsn()));
        ssa_inputs.push(pop_stack_or_const!(self, target.into_word().into()));
        ssa_inputs.push(input_account_info!(self, address));
        ssa_inputs.push(input_account_info!(self, target));
        ssa_inputs.push(input_account_status!(self, address)); // identify if it is created
        ssa_inputs.push(SSAInput::Constant(U256::from(is_cancun_enabled)));

        self.log_storage_read(StorageKey::AccountInfo(address), lsn);
        self.log_storage_read(StorageKey::AccountStatus(address), lsn);
        self.log_storage_read(StorageKey::AccountInfo(target), lsn);

        let mut ssa_outputs = Vec::with_capacity(4);
        if address != target {
            ssa_outputs.push(output_account_info!(target, target_info));
            self.log_storage_write(StorageKey::AccountInfo(target), lsn, 0); //  add balance
        }

        if is_created || !is_cancun_enabled {
            ssa_outputs.push(output_account_info!(address, address_info));
            self.log_storage_write(
                StorageKey::AccountInfo(address),
                lsn,
                (ssa_outputs.len() - 1) as u8,
            ); // mark as selfdestruct, it is used when calculate gas
            ssa_outputs.push(output_account_status!(address, address_status));
            self.log_storage_write(
                StorageKey::AccountStatus(address),
                lsn,
                (ssa_outputs.len() - 1) as u8,
            ); // clear balance
        } else if address != target {
            ssa_outputs.push(output_account_info!(address, address_info));
            self.log_storage_write(
                StorageKey::AccountInfo(address),
                lsn,
                (ssa_outputs.len() - 1) as u8,
            ); // clear balance
        }

        let result = SSAOutput::InterpreterResult(SSAInterpreterResult {
            result: SSAInstructionResult::Ok,
            output: Bytes::default(),
        });
        ssa_outputs.push(result);

        self.last_interpreter_return = (lsn, (ssa_outputs.len() - 1) as u8);
        self.log_operation(opcode, ssa_inputs, ssa_outputs);
    }

    #[inline(always)]
    pub fn log_storage_write(&mut self, key: StorageKey, lsn: LsnType, index: u8) {
        self.latest_writes.insert(key, (lsn, index));
    }

    #[inline(always)]
    pub fn log_transient_write(&mut self, key: (Address, U256), lsn: LsnType, index: u8) {
        self.latest_transient_writes.insert(key, (lsn, index));
    }

    #[inline(always)]
    pub fn log_storage_read(&mut self, key: StorageKey, lsn: LsnType) {
        if !self.latest_writes.contains_key(&key) {
            self.first_reads.entry(key).or_insert(lsn);
        }
    }

    #[inline(always)]
    pub fn get_storage_def(&self, key: StorageKey) -> LsnWithIndex {
        *self.latest_writes.get(&key).unwrap_or(&(0, 0))
    }

    #[inline(always)]
    pub fn get_transient_def(&self, key: (Address, U256)) -> LsnWithIndex {
        *self.latest_transient_writes.get(&key).unwrap_or(&(0, 0))
    }

    #[inline(always)]
    pub fn generate_new_stack(&mut self) {
        self.stack_pool.push(ShadowStack::new());
    }

    #[inline(always)]
    pub fn remove_last_stack(&mut self) {
        self.stack_pool.pop();
    }

    #[inline(always)]
    pub fn push_stack_def(
        &mut self,
        def: LsnWithIndex,
    ) -> Result<(), crate::shadow_stack::InstructionResult> {
        let stack = self.stack_pool.last_mut().unwrap();
        stack.push(def)
    }

    #[inline(always)]
    pub fn pop_stack_def(
        &mut self,
    ) -> Result<LsnWithIndex, crate::shadow_stack::InstructionResult> {
        let stack = self.stack_pool.last_mut().unwrap();
        stack.pop()
    }

    #[inline(always)]
    pub fn dup_stack_def(
        &mut self,
        n: usize,
    ) -> Result<(), crate::shadow_stack::InstructionResult> {
        let stack = self.stack_pool.last_mut().unwrap();
        stack.dup(n)
    }

    #[inline(always)]
    pub fn swap_stack_def(
        &mut self,
        n: usize,
    ) -> Result<(), crate::shadow_stack::InstructionResult> {
        let stack = self.stack_pool.last_mut().unwrap();
        stack.swap(n)
    }

    #[inline(always)]
    pub fn exchange_stack_def(
        &mut self,
        n: usize,
        m: usize,
    ) -> Result<(), crate::shadow_stack::InstructionResult> {
        let stack = self.stack_pool.last_mut().unwrap();
        stack.exchange(n, m)
    }

    pub fn take_logs(&mut self) -> Vec<SSALogEntry> {
        std::mem::take(&mut self.logs)
    }

    pub fn get_log(&self, lsn: usize) -> &SSALogEntry {
        &self.logs[lsn]
    }

    pub fn get_latest_writes(&self) -> &HashMap<StorageKey, LsnWithIndex> {
        &self.latest_writes
    }

    pub fn get_first_reads(&self) -> &HashMap<StorageKey, LsnType> {
        &self.first_reads
    }

    pub fn take_first_reads(&mut self) -> HashMap<StorageKey, LsnType> {
        std::mem::take(&mut self.first_reads)
    }

    pub fn take_first_frame_input(&mut self) -> Option<FrameInput> {
        std::mem::take(&mut self.first_frame_input)
    }

    pub fn clear(&mut self) {
        self.current_lsn = 0;
        self.logs.clear();
        self.stack_pool = vec![ShadowStack::new()];
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
#[inline(always)]
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
