use revm_primitives::{AccountInfo, AccountStatus, Address, Bytes, Log, U256};
use crate::{call_types::{SSACallInput, SSACallOutcome, SSACreateInput, SSACreateOutcome}, SSAInterpreterResult};
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
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct MemoryDep {
    pub lsn: u16,
    pub self_offset: usize,
    pub lsn_offset: usize,
    pub length: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum ContractEnv {
    Target(Address),
    Size(usize),
    Code(Bytes),
    Caller(Address),
    CallValue(U256),
    CallDataSize(usize),
    CallData(Bytes),
    CallDataLoad(U256),
}

impl ContractEnv {
    /// Get target address, returns None if not Target type
    pub fn as_target(&self) -> Option<Address> {
        match self {
            ContractEnv::Target(addr) => Some(*addr),
            _ => None
        }
    }

    /// Get size, returns None if not Size type
    pub fn as_size(&self) -> Option<usize> {
        match self {
            ContractEnv::Size(size) => Some(*size),
            _ => None
        }
    }

    /// Get code, returns None if not Code type
    pub fn as_code(&self) -> Option<&Bytes> {
        match self {
            ContractEnv::Code(code) => Some(code),
            _ => None
        }
    }

    /// Get caller address, returns None if not Caller type
    pub fn as_caller(&self) -> Option<Address> {
        match self {
            ContractEnv::Caller(addr) => Some(*addr),
            _ => None
        }
    }

    /// Get call value, returns None if not CallValue type
    pub fn as_call_value(&self) -> Option<U256> {
        match self {
            ContractEnv::CallValue(value) => Some(*value),
            _ => None
        }
    }

    /// Get call data size, returns None if not CallDataSize type
    pub fn as_call_data_size(&self) -> Option<usize> {
        match self {
            ContractEnv::CallDataSize(size) => Some(*size),
            _ => None
        }
    }

    /// Get call data, returns None if not CallData type
    pub fn as_call_data(&self) -> Option<&Bytes> {
        match self {
            ContractEnv::CallData(data) => Some(data),
            _ => None
        }
    }

    /// Get call data load value, returns None if not CallDataLoad type
    pub fn as_call_data_load(&self) -> Option<U256> {
        match self {
            ContractEnv::CallDataLoad(value) => Some(*value),
            _ => None
        }
    }
}

impl From<ContractEnv> for U256 {
    fn from(value: ContractEnv) -> Self {
        match value {
            ContractEnv::Target(address) => address.into_word().into(),
            ContractEnv::Size(size) => U256::from(size),
            ContractEnv::CallDataSize(size) => U256::from(size),
            ContractEnv::CallDataLoad(data) => U256::from(data),
            ContractEnv::CallValue(value) => value,
            ContractEnv::Caller(address) => address.into_word().into(),

            ContractEnv::Code(_) => U256::ZERO, // Code cannot be converted to U256, return 0
            ContractEnv::CallData(_) => U256::ZERO, // Call data cannot be converted to U256, return 0
        }
    }
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

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSAInput {
    Constant(U256),
    Stack {
        source: u16,
    },
    Memory {
        source: Vec<MemoryDep>,
    },
    Storage {
        key: Box<StorageKey>,
        source: u16,
    },
    ReturnDataBuffer {
        source: u16,
    },
    InterpreterResult {
        source: u16,
    },
    CallOutcome {
        source: u16,
    },
    CreateOutcome {
        source: u16,
    },
    ContractEntry {
        source: u16,
    },
    MemorySizeChange {
        source: u16,
    },
    CreateInput {
        source: u16,
    },
    CallInput {
        source: u16,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSAOutput {
    Stack(U256),
    Memory(Bytes),
    Storage {
        key: Box<StorageKey>,
        value: Box<StorageValue>,
    },
    Jump {
        relative_offset: isize,
    },
    ReturnDataBuffer(Bytes),
    InterpreterResult(SSAInterpreterResult),
    MemorySize(usize),
    Address(Address),
    CreateFrame(Box<SSACreateInput>),
    CreateOutcome(Box<SSACreateOutcome>),
    CallFrame(Box<SSACallInput>),
    CallOutcome(Box<SSACallOutcome>),
    Log(Box<Log>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SSALogEntry {
    pub lsn: u16,
    pub opcode: u8,
    pub inputs: Vec<SSAInput>,
    pub outputs: Vec<SSAOutput>,
}