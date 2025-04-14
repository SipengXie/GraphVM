use core::ops::Range;

use revm_interpreter::InstructionResult;
// Add this struct definition, likely in host.rs or a shared types module
use revm_primitives::{AccountInfo, AccountStatus, Address, Bytecode, Bytes, Env, HashMap, B256, U256, PrecompileErrors};
use revm_ssa::TxScheme;
use revm_precompile::Precompiles;

// --- External State Context ---

/// Structure to hold external state (simulating DB/cache).
#[derive(Clone, Debug)]
pub struct ExternalContext {
    /// Account information and status cache.
    pub accounts: HashMap<Address, (AccountInfo, AccountStatus)>,
    /// Storage slot cache.
    pub storage: HashMap<(Address, U256), U256>,
    /// Block hash cache.
    pub block_hashes: HashMap<u64, B256>,
    /// Env
    pub env: Env,

    /// Precompiles
    precompiles: &'static Precompiles,
    // Add other external states if needed
}

impl ExternalContext {
    // Method to check if an address is a precompile
    pub fn is_precompile(&self, address: &Address) -> bool {
        self.precompiles.contains(address)
    }

    // Method to execute a precompile (simplified, ignores gas)
    pub fn call_precompile(
        &self,
        address: &Address,
        input: &Bytes,
        gas_limit: u64, // Gas ignored for now
    ) -> (InstructionResult, Bytes) { // Returning CallOutcome directly
        let precompile = self.precompiles.get(address);
        let outcome = precompile
            .unwrap()
            .call_ref(input, gas_limit, &self.env);
        match outcome {
            Ok(output) => {
                (InstructionResult::Return, output.bytes)
            }
            Err(e) => {
                let result = match e {
                    PrecompileErrors::Error(_) => InstructionResult::Revert,
                    PrecompileErrors::Fatal { msg: _ } => InstructionResult::PrecompileError,
                };
                (result, Bytes::default())
            },
        }
    }
}

// Helper to get account info and status, returning default if not found
pub fn get_account_context(context: &ExternalContext, address: Address) -> (AccountInfo, AccountStatus) {
    context.accounts.get(&address).cloned().unwrap_or_else(|| {
        // Return default info and treat as ColdLoaded if not present
        (AccountInfo::default(), AccountStatus::Loaded)
    })
}

// Helper to get storage slot, returning zero if not found
pub fn get_storage_slot_context(context: &ExternalContext, address: Address, index: U256) -> U256 {
    context.storage.get(&(address, index)).cloned().unwrap_or(U256::ZERO)
}


#[derive(Clone, Debug, Default)]
pub struct FrameContext {
    /// Bytecode contains contract code, size of original code, analysis with gas block and jump table.
    /// Note that current code is extended with push padding and STOP at end.
    pub bytecode: Bytecode,
    /// Bytecode hash for legacy. For EOF this would be None.
    pub hash: Option<B256>,
    /// FrameInput of this contractEnv
    pub frame_input: FrameInput,
}

/// Represents the input parameters for a new execution frame (CALL, CREATE, etc.).
/// This is produced by CALL/CREATE type nodes.
#[derive(Clone, Debug)]
pub struct FrameInput {
    pub target_address: Address, // Address being called or created address
    pub caller: Address,         // Address initiating the call/create
    pub transfer_value: U256,    // Value transferred
    pub input: Bytes,            // Input data (calldata or init code)
    pub gas_limit: u64,          // Gas limit for the sub-call/create
    pub scheme: TxScheme,        // Call, Create, Create2, etc.
    pub ret_range: Range<usize>, // Expected return memory range (for CALLs)
    pub bytecode_address: Address, // Address whose code to execute (differs for CALLCODE/DELEGATECALL)
    pub is_static: bool,         // Is this a static call?
}

impl Default for FrameInput {
    fn default() -> Self {
        Self {
            target_address: Address::ZERO,
            caller: Address::ZERO,
            transfer_value: U256::ZERO,
            input: Bytes::new(),
            gas_limit: 0,
            scheme: TxScheme::Call, // Default, should be set properly
            ret_range: 0..0,
            bytecode_address: Address::ZERO,
            is_static: false,
        }
    }
}

/// Outcome of a CALL-like operation.
#[derive(Clone, Debug)]
pub struct CallOutcome {
    pub result: InstructionResult, // Final status (Ok, Revert, Error)
    pub return_data: Bytes,        // Data returned by the sub-call
    pub ret_range: Range<usize>,   // Expected return memory range (for CALLs)
    // pub gas_used: u64,             // Gas used by the sub-call (optional for now)
}

/// Outcome of a CREATE-like operation.
#[derive(Clone, Debug, Default)]
pub struct CreateOutcome {
    pub result: InstructionResult, // Final status (Ok, Revert, Error)
    pub return_data: Bytes,        // Data returned on revert, or deployment bytecode on success
    pub created_address: Option<Address>, // Address of the created contract (if successful)
    // pub gas_used: u64,             // Gas used by the create operation (optional for now)
}


