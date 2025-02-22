use core::ops::Range;

use revm_primitives::{Address, Bytes, U256};
#[cfg(feature = "serde")]
use serde::{Serialize, Deserialize};

/// Simplified Call input structure
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SSACallInput {
    /// Input data for the call
    pub input: Bytes,
    /// Bytecode address
    pub bytecode_address: Address,
    /// Target address
    pub target_address: Address,
    /// Caller address
    pub caller: Address,
    /// Call value
    pub transfer_value: U256,
    /// Call scheme
    pub scheme: SSACallScheme,
    /// Return range
    pub ret_range: Range<usize>,
    /// Contract code
    pub code: Option<Bytes>,
}

/// Simplified Call output structure
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SSACallOutcome {
    /// Call execution result
    pub result: SSAInterpreterResult,
    /// Call output data range
    pub ret_range: Range<usize>,
}

/// Simplified Create input structure
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SSACreateInput {
    /// Creator address
    pub caller: Address,
    /// Creation value
    pub value: U256,
    /// Initialization code
    pub init_code: Bytes,
    /// Creation scheme
    pub scheme: SSACreateScheme,
    /// Target address
    pub target: Address,
}

/// Simplified Create output structure
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SSACreateOutcome {
    /// Creation execution result
    pub result: SSAInterpreterResult,
    /// Created contract address (if successful)
    pub address: Option<Address>,
}

/// Simplified Call scheme enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSACallScheme {
    Call,
    CallCode,
    DelegateCall,
    StaticCall,
    ExtCall,
    ExtStaticCall,
    ExtDelegateCall,
} 

/// Simplified Create scheme enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSACreateScheme {
    Create,
    Create2 {
        salt: U256,
    },
} 

#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SSAInterpreterResult {
    /// Execution result
    pub result: SSAInstructionResult,
    /// Execution output data
    pub output: Bytes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub enum SSAInstructionResult {
    /// Successful execution
    Ok,
    /// Execution reverted (e.g., REVERT instruction)
    Revert,
    /// Execution error (e.g., OutOfGas, StackOverflow)
    Error,
}

impl SSAInstructionResult {
    /// Check if execution was successful
    pub fn is_ok(&self) -> bool {
        matches!(self, SSAInstructionResult::Ok)
    }

    /// Check if execution was reverted
    pub fn is_revert(&self) -> bool {
        matches!(self, SSAInstructionResult::Revert)
    }

    /// Check if execution resulted in error
    pub fn is_error(&self) -> bool {
        matches!(self, SSAInstructionResult::Error)
    }
} 