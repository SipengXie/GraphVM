use revm_interpreter::{InstructionResult, SharedMemory};
use revm_primitives::{AccountInfo, AccountStatus, Bytes, Env, U256};
use crate::context::{ExternalContext, FrameContext, CallOutcome, CreateOutcome};
use revm_ssa::FrameInput;
use std::{cell::RefCell, rc::Rc};

// --- Basic U256 Input Types ---
/// Input type for nodes that take one U256 parameter
pub type UnaryU256Inputs = (*const U256,);

/// Input type for nodes that take two U256 parameters
pub type BinaryU256Inputs = (*const U256, *const U256);

/// Input type for nodes that take three U256 parameters
pub type TernaryU256Inputs = (*const U256, *const U256, *const U256);

// --- Basic Output Types ---
/// Output type for nodes that return a single U256 value
pub type U256Output = (U256,);

/// Output type for nodes that return bytes
pub type BytesOutput = (Bytes,);

/// Output type for nodes that return instruction result
pub type InstructionResultOutput = (InstructionResult,);

// --- Memory Related Types ---
/// Input type for memory operations with offset and value
pub type MemoryStoreInputs = (*const U256, *const U256, Rc<RefCell<SharedMemory>>);

/// Input type for memory operations with only offset
pub type MemoryLoadInputs = (*const U256, Rc<RefCell<SharedMemory>>);

/// Input type for memory copy operations
pub type MemoryCopyInputs = (*const U256, *const U256, *const U256, Rc<RefCell<SharedMemory>>);

// --- Environment Related Types ---
/// Input type for environment access
pub type EnvInput = (*const Env,);

/// Input type for frame context access
pub type FrameContextInput = (*const FrameContext,);

/// Input type for blob hash operation
pub type BlobHashInputs = (*const U256, *const Env);

// --- Account Related Types ---
/// Input type for account info access
pub type AccountInfoInput = (*const AccountInfo,);

/// Output type for account info
pub type AccountInfoOutput = (AccountInfo,);

/// Output type for account status
pub type AccountStatusOutput = (AccountStatus,);

// --- External Context Types ---
/// Input type for external context access
pub type ExternalContextInput = (Rc<RefCell<ExternalContext>>,);

// --- Combined Types ---
/// Input type for operations that need both frame context and external context
pub type FrameAndExternalContextInputs = (*const FrameContext, Rc<RefCell<ExternalContext>>);

/// Output type for operations that return both instruction result and bytes
pub type InterpreterResultOutputs = (InstructionResult, Bytes);

/// Output type for operations that return both U256 and bytes
pub type U256AndBytesOutputs = (U256, Bytes);

// --- Optional Types ---
/// Input type for operations with optional account info
pub type OptionalAccountInfoInput = (Option<*const AccountInfo>,);

/// Input type for operations with optional external context
pub type OptionalExternalContextInput = (Option<Rc<RefCell<ExternalContext>>>,);

// --- Control Flow Types ---
/// Input type for return/revert operations
pub type ReturnRevertInputs = (*const U256, *const U256, Rc<RefCell<SharedMemory>>, InstructionResult);

/// Input type for jump operations
pub type JumpInputs = UnaryU256Inputs;

/// Input type for conditional jump operations
pub type JumpiInputs = BinaryU256Inputs;

/// Output type for jump operations
pub type JumpOutput = (usize,);

/// Input type for stop/invalid operations
pub type StopInvalidInputs = (InstructionResult,);

// --- System Operation Types ---
/// Input type for operations that need frame context
pub type FrameContextInputs = (*const FrameContext,);

/// Input type for operations that need both memory and frame context
pub type MemoryAndFrameInputs = (*const U256, *const U256, Rc<RefCell<SharedMemory>>, *const FrameContext);

/// Input type for code copy operations
pub type CodeCopyInputs = (*const U256, *const U256, *const U256, *const FrameContext, Rc<RefCell<SharedMemory>>);

/// Input type for calldata operations
pub type CallDataInputs = (*const U256, *const FrameContext);

/// Input type for operations that need bytes data
pub type BytesDataInput = (*const Bytes,);

/// Input type for return data copy operations
pub type ReturnDataCopyInputs = (*const U256, *const U256, *const U256, *const Bytes, Rc<RefCell<SharedMemory>>);

/// Input type for keccak256 operations
pub type Keccak256Inputs = (*const U256, *const U256, Rc<RefCell<SharedMemory>>);

// --- Host Operation Types ---
/// Input type for storage load operations
pub type StorageLoadInputs = (
    *const FrameContext,
    *const U256,
    Option<*const U256>,
    Rc<RefCell<ExternalContext>>,
);

/// Input type for storage store operations
pub type StorageStoreInputs = (
    *const FrameContext,
    *const U256,
    *const U256,
    Rc<RefCell<ExternalContext>>,
);

/// Input type for balance check operations
pub type BalanceCheckInputs = (
    *const U256,
    Option<*const AccountInfo>,
    Rc<RefCell<ExternalContext>>,
);

/// Input type for code size/hash operations
pub type CodeInfoInputs = (
    *const U256,
    Option<*const AccountInfo>,
    Rc<RefCell<ExternalContext>>,
);

/// Input type for block hash operations
pub type BlockHashInputs = (
    *const U256,
    Rc<RefCell<ExternalContext>>,
    *const U256,
);

/// Input type for self balance operations
pub type SelfBalanceInputs = (
    *const FrameContext,
    Option<*const AccountInfo>,
    Rc<RefCell<ExternalContext>>,
);

/// Input type for external code copy operations
pub type ExtCodeCopyInputs = (
    *const U256,
    *const U256,
    *const U256,
    *const U256,
    Option<*const AccountInfo>,
    Rc<RefCell<ExternalContext>>,
    Rc<RefCell<SharedMemory>>,
);

/// Input type for self destruct operations
pub type SelfDestructInputs = (
    *const U256,
    *const FrameContext,
    Option<*const AccountInfo>,
    Option<*const AccountInfo>,
    Option<*const AccountStatus>,
    Rc<RefCell<ExternalContext>>,
    bool,
);

/// Output type for account info operations
pub type SelfDestructOutputs = (AccountInfo, AccountInfo, AccountStatus, InstructionResult);

// --- Contract Operation Types ---
/// Input type for deduct caller operations
pub type DeductCallerInputs = (*const U256, bool, *const U256, Rc<RefCell<ExternalContext>>);

/// Input type for call operations
pub type CallInputs = (
    *const U256,  // gas_limit
    *const U256,  // to
    *const U256,  // value
    *const U256,  // in_offset
    *const U256,  // in_len
    *const U256,  // out_offset
    *const U256,  // out_len
    Rc<RefCell<SharedMemory>>,
    *const FrameContext,
);

/// Input type for delegate call operations
pub type DelegateCallInputs = (
    *const U256,  // gas_limit
    *const U256,  // to
    *const U256,  // in_offset
    *const U256,  // in_len
    *const U256,  // out_offset
    *const U256,  // out_len
    Rc<RefCell<SharedMemory>>,
    *const FrameContext,
);

/// Input type for create operations
pub type CreateInputs = (
    *const U256,  // value
    *const U256,  // offset
    *const U256,  // length
    Rc<RefCell<SharedMemory>>,
    *const FrameContext,
);

/// Input type for create2 operations
pub type Create2Inputs = (
    *const U256,  // value
    *const U256,  // offset
    *const U256,  // length
    *const U256,  // salt
    Rc<RefCell<SharedMemory>>,
    *const FrameContext,
);

/// Input type for make call frame operations
pub type MakeCallFrameInputs = (
    *const FrameInput,
    Option<*const AccountInfo>,
    Option<*const AccountInfo>,
    Option<*const AccountInfo>,
    Rc<RefCell<ExternalContext>>,
);

/// Input type for make create frame operations
pub type MakeCreateFrameInputs = (
    *const FrameInput,
    Option<*const AccountInfo>,
    Option<Rc<RefCell<ExternalContext>>>,
);

/// Input type for create return operations
pub type CreateReturnInputs = (
    *const InstructionResult,
    Option<*const Bytes>,
    Option<*const FrameContext>,
    Option<Rc<RefCell<ExternalContext>>>,
    Option<*const AccountInfo>,
    Option<bool>,
);

/// Input type for call return operations
pub type CallReturnInputs = (
    *const InstructionResult,
    *const Bytes,
    *const FrameContext,
);

/// Input type for insert call outcome operations
pub type InsertCallOutcomeInputs = (
    *const CallOutcome,
    Rc<RefCell<SharedMemory>>,
    *const FrameContext,
);

/// Output type for frame context operations
pub type FrameContextOutput = (FrameContext,);

/// Output type for call outcome operations
pub type CallOutcomeOutput = (CallOutcome,);

/// Output type for create outcome operations
pub type CreateOutcomeOutput = (CreateOutcome,);

/// Output type for frame context and call outcome operations
pub type FrameContextAndCallOutcomeOutputs = (FrameContext, CallOutcome, AccountInfo, AccountInfo);

/// Output type for frame context and create outcome operations
pub type FrameContextAndCreateOutcomeOutputs = (AccountInfo, AccountInfo, AccountStatus, FrameContext);

/// Output type for create return operations
pub type CreateReturnOutputs = (CreateOutcome, AccountInfo);

/// Output type for call return operations
pub type CallReturnOutputs = (CallOutcome,);

/// Output type for insert call outcome operations
pub type InsertCallOutcomeOutputs = (U256, Bytes);

/// Output type for frame input operations
pub type FrameInputOutput = (FrameInput,);

/// Input type for base call operations
pub type BaseCallInputs = (
    *const U256,  // gas_limit
    *const U256,  // to
    *const U256,  // value
    *const U256,  // in_offset
    *const U256,  // in_len
    *const U256,  // out_offset
    *const U256,  // out_len
    Rc<RefCell<SharedMemory>>,
    *const FrameContext,
);

/// Input type for create outcome operations
pub type CreateOutcomeInputs = (*const CreateOutcome,); 

pub type InsertCreateOutcomeOutputs = (Bytes, U256);

pub type MakeCallFrameOutputs = (FrameContext, CallOutcome, AccountInfo, AccountInfo);

pub type MakeCreateFrameOutputs = (AccountInfo, AccountInfo, AccountStatus, FrameContext);