use revm_primitives::{AccountInfo, AccountStatus, Address, Bytecode, Bytes, Log, B256, U256};
use crate::{call_types::{SSACallInput, SSACallOutcome, SSACreateInput, SSACreateOutcome}, logger::{LsnType, LsnWithIndex}, SSAInterpreterResult};
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};

#[allow(non_camel_case_types)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(u8)]
pub enum InternalOp {
    // CREATE operations
    MAKE_CREATE_FRAME = 0xD4,
    CREATE_RETURN = 0xD5,
    INSERT_CREATE_OUTCOME = 0xD6,
    
    // CALL operations
    MAKE_CALL_FRAME = 0xD7,
    CALL_RETURN = 0xD8,
    INSERT_CALL_OUTCOME = 0xD9,

    // Pre_verify operations
    DEDUCT_CALLER = 0xDA,

    // Post Execution operation
    REFUND_GAS = 0xDB,
    REWARD_BENEFICIARY = 0xDC,
}

impl From<u8> for InternalOp {

    fn from(value: u8) -> Self {
        match value {
            0xD4 => Self::MAKE_CREATE_FRAME,
            0xD5 => Self::CREATE_RETURN,
            0xD6 => Self::INSERT_CREATE_OUTCOME,
            0xD7 => Self::MAKE_CALL_FRAME,
            0xD8 => Self::CALL_RETURN,
            0xD9 => Self::INSERT_CALL_OUTCOME,
            0xDA => Self::DEDUCT_CALLER,
            0xDB => Self::REFUND_GAS,
            _ => panic!("Invalid internal opcode: {value:02x}"),
        }
    }

}

impl From<InternalOp> for u8 {
    fn from(op: InternalOp) -> Self {
        op as u8
    }
}

/// mem[self_offset:self_offset+length] = mem[lsn_offset:lsn_offset+length]
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MemoryDep {
    pub lsn: LsnWithIndex,
    pub self_offset: usize,
    pub lsn_offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ContractEnv {
    /// Contracts data
    pub input: Bytes,
    /// Bytecode contains contract code, size of original code, analysis with gas block and jump table.
    /// Note that current code is extended with push padding and STOP at end.
    pub bytecode: Bytecode,
    /// Bytecode hash for legacy. For EOF this would be None.
    pub hash: Option<B256>,
    /// Target address of the account. Storage of this address is going to be modified.
    pub target_address: Address,
    /// Address of the account the bytecode was loaded from. This can be different from target_address
    /// in the case of DELEGATECALL or CALLCODE
    pub bytecode_address: Option<Address>,
    /// Caller of the EVM.
    pub caller: Address,
    /// Value send to contract from transaction or from CALL opcodes.
    pub call_value: U256,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum StorageKey {
    Slot(Address, U256),
    AccountInfo(Address),
    AccountStatus(Address),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum StorageValue {
    AccountInfo(AccountInfo),
    AccountStatus(AccountStatus),
    Slot(U256),
}

impl StorageValue {
    /// Get account info, returns None if not AccountInfo type
    pub fn as_account_info(&self) -> Option<&AccountInfo> {
        match self {
            StorageValue::AccountInfo(info) => Some(info),
            _ => None
        }
    }

    /// Get account status, returns None if not AccountStatus type 
    pub fn as_account_status(&self) -> Option<&AccountStatus> {
        match self {
            StorageValue::AccountStatus(status) => Some(status),
            _ => None
        }
    }

    /// Get slot value, returns None if not Slot type
    pub fn as_slot(&self) -> Option<&U256> {
        match self {
            StorageValue::Slot(value) => Some(value),
            _ => None
        }
    }
}


#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
// SSAInput is the input of the SSA instruction, we only log the source
// as the value is unnecessary.
pub enum SSAInput {
    Constant(U256),
    ConstantI64(i64), // for gas_refunded
    Stack (LsnWithIndex),
    Memory(Vec<MemoryDep>),
    Storage (StorageKey, LsnWithIndex),
    Transient (LsnWithIndex),
    ReturnDataBuffer (LsnWithIndex),
    InterpreterResult(LsnWithIndex),
    CallOutcome(LsnWithIndex),
    CreateOutcome(LsnWithIndex),
    MemorySizeChange (LsnWithIndex),
    CreateInput(LsnWithIndex),
    CallInput(LsnWithIndex),
    ContractEnv(LsnWithIndex),
    GasCost(LsnWithIndex),
    GasRefund(LsnWithIndex),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
// SSAOutput is the output of the SSA instruction, we only log the value
// The SSAOutput also acts like the value of the SSA instruction, when we
// execute the SSA graph, we can use the SSAOutput as the input of the next
// instruction.
pub enum SSAOutput {
    Constant(U256),
    Stack(U256),
    Memory(Bytes),
    Storage {
        key: Box<StorageKey>,
        value: Box<StorageValue>,
    },
    Transient(U256),
    Jump(isize),
    ReturnDataBuffer(Bytes),
    InterpreterResult(SSAInterpreterResult),
    MemorySize(usize),
    CreateInput(Box<SSACreateInput>),
    CreateOutcome(Box<SSACreateOutcome>),
    CallInput(Box<SSACallInput>),
    CallOutcome(Box<SSACallOutcome>),
    Log(Box<Log>),
    ContractEnv(Box<ContractEnv>),
    GasCost(u64),
    GasRefund(i64),
}

// Implement TryFrom trait to convert SSAOutput to Bytes
// This is specifically for Memory type outputs
impl TryFrom<SSAOutput> for Bytes {
    type Error = &'static str;

    fn try_from(output: SSAOutput) -> Result<Self, Self::Error> {
        match output {
            SSAOutput::Memory(bytes) => Ok(bytes),
            _ => Err("Cannot convert non-memory SSAOutput to Bytes"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[repr(align(64))]
pub struct SSALogEntry {
    // The LSN of the log entry
    pub lsn: LsnType,
    // The opcode of the instruction 
    pub opcode: u8,
    // The inputs of the instruction
    pub inputs: Vec<SSAInput>,
    // The outputs of the instruction, it is necessary to record the value
    // because when we construct the SSA graph, the paritially executed nodes may
    // access some nodes unnecessary to execute, thus we can give them the same value
    pub outputs: Vec<SSAOutput>,
}

impl std::fmt::Display for SSALogEntry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "SSALogEntry {{")?;
        writeln!(f, "  LSN: {}", self.lsn)?;
        writeln!(f, "  OPCODE: 0x{:02X}", self.opcode)?;
        writeln!(f, "  Inputs:")?;
        for (i, input) in self.inputs.iter().enumerate() {
            writeln!(f, "    {}: {:?}", i, input)?;
        }
        writeln!(f, "  Outputs:")?;
        for (i, output) in self.outputs.iter().enumerate() {
            writeln!(f, "    {}: {:?}", i, output)?;
        }
        write!(f, "}}")
    }
}