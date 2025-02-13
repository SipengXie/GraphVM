use revm_primitives::{Address, Bytes, Log, U256};
use smallvec::SmallVec;
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
    pub lsn: usize,
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

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum StorageValue {
    Balance(U256),
    Nonce(u64),
    CodeSize(u64),
    Code(Bytes),
    CodeHash(U256),
    Slot(U256),
}

impl StorageValue {
    /// Get balance value, returns None if not Balance type
    pub fn as_balance(&self) -> Option<U256> {
        match self {
            StorageValue::Balance(value) => Some(*value),
            _ => None
        }
    }

    /// Get nonce value, returns None if not Nonce type
    pub fn as_nonce(&self) -> Option<u64> {
        match self {
            StorageValue::Nonce(value) => Some(*value),
            _ => None
        }
    }

    /// Get code size, returns None if not CodeSize type
    pub fn as_code_size(&self) -> Option<u64> {
        match self {
            StorageValue::CodeSize(value) => Some(*value),
            _ => None
        }
    }

    /// Get code content, returns None if not Code type
    pub fn as_code(&self) -> Option<&Bytes> {
        match self {
            StorageValue::Code(value) => Some(value),
            _ => None
        }
    }

    /// Get code hash, returns None if not CodeHash type
    pub fn as_code_hash(&self) -> Option<U256> {
        match self {
            StorageValue::CodeHash(value) => Some(*value),
            _ => None
        }
    }

    /// Get storage slot value, returns None if not Slot type
    pub fn as_slot(&self) -> Option<U256> {
        match self {
            StorageValue::Slot(value) => Some(*value),
            _ => None
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSAInput {
    Constant(U256),
    Stack {
        value: U256,
        source: Option<usize>,
    },
    Memory {
        value: Bytes,
        source: Vec<MemoryDep>,
    },
    Storage {
        key: StorageKey,
        value: StorageValue,
        source: Option<usize>,
    },
    ReturnDataBuffer {
        value: Bytes,
        source: Option<usize>,
    },
    InterpreterResult {
        result: SSAInterpreterResult,
        source: Option<usize>,
    },
    CallOutcome {
        outcome: SSACallOutcome,
        source: Option<usize>,
    },
    CreateOutcome {
        outcome: SSACreateOutcome,
        source: Option<usize>,
    },
    ContractEntry {
        value: ContractEnv,
        entry_lsn: Option<usize>,
    },
    MemorySizeChange {
        size: usize,
        last_memory: Option<usize>,
    },
    CreateInput {
        input: SSACreateInput,
        entry: Option<usize>,
    },
    CallInput {
        input: SSACallInput,
        entry: Option<usize>,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSAOutput {
    Stack(U256),
    Memory(Bytes),
    Storage {
        key: StorageKey,
        value: StorageValue,
    },
    Jump {
        relative_offset: isize,
    },
    ReturnDataBuffer(Bytes),
    InterpreterResult(SSAInterpreterResult),
    MemorySize(usize),
    Address(Address),
    CreateFrame(SSACreateInput),
    CreateOutcome(SSACreateOutcome),
    CallFrame(SSACallInput),
    CallOutcome(SSACallOutcome),
    Log(Log),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SSALogEntry {
    pub lsn: usize,
    pub opcode: u8,
    pub inputs: SmallVec<[SSAInput; 8]>,
    pub outputs: SmallVec<[SSAOutput; 3]>,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum StorageKey {
    Slot(Address, U256),
    Balance(Address),
    Nonce(Address),
    CodeSize(Address),
    Code(Address),
    CodeHash(Address),
}

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSAValue {
    U256(U256),
    I32(i32),
    U64(u64),
    Bytes(Bytes),
    Address(Address),
    LOG(Log),
    CallInput(SSACallInput),
    CallOutcome(SSACallOutcome),
    CreateInput(SSACreateInput),
    CreateOutcome(SSACreateOutcome),
}

impl SSAValue {
    pub fn as_u256(&self) -> Option<U256> {
        match self {
            SSAValue::U256(u) => Some(*u),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            SSAValue::I32(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_bytes(&self) -> Option<Bytes> {
        match self {
            SSAValue::Bytes(b) => Some(b.clone()),
            _ => None,
        }
    }

    pub fn as_address(&self) -> Option<Address> {
        match self {
            SSAValue::Address(a) => Some(*a),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            SSAValue::U64(u) => Some(*u),
            _ => None,
        }
    }

    pub fn as_log(&self) -> Option<Log> {
        match self {
            SSAValue::LOG(l) => Some(l.clone()),
            _ => None,
        }
    }

    pub fn as_call_input(&self) -> Option<SSACallInput> {
        match self {
            SSAValue::CallInput(input) => Some(input.clone()),
            _ => None,
        }
    }

    pub fn as_call_outcome(&self) -> Option<SSACallOutcome> {
        match self {
            SSAValue::CallOutcome(outcome) => Some(outcome.clone()),
            _ => None,
        }
    }

    pub fn as_create_input(&self) -> Option<SSACreateInput> {
        match self {
            SSAValue::CreateInput(input) => Some(input.clone()),
            _ => None,
        }
    }

    pub fn as_create_outcome(&self) -> Option<SSACreateOutcome> {
        match self {
            SSAValue::CreateOutcome(outcome) => Some(outcome.clone()),
            _ => None,
        }
    }
} 