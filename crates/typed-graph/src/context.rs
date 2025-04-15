use core::ops::Range;

use revm_interpreter::InstructionResult;
// Add this struct definition, likely in host.rs or a shared types module
use revm_precompile::{PrecompileSpecId, Precompiles};
use revm_primitives::{
    AccountInfo, AccountStatus, Address, Bytes, Env, HashMap, PrecompileErrors, SpecId, B256, U256,
};
use revm_ssa::ContractEnv;

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
    /// Create a new ExternalContext with the given parameters
    pub fn new(
        env: Env,
        accounts: HashMap<Address, (AccountInfo, AccountStatus)>,
        storage: HashMap<(Address, U256), U256>,
        block_hashes: HashMap<u64, B256>,
    ) -> Self {
        Self {
            accounts,
            storage,
            block_hashes,
            env,
            precompiles: Precompiles::new(PrecompileSpecId::from_spec_id(SpecId::LATEST)),
        }
    }

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
    ) -> (InstructionResult, Bytes) {
        // Returning CallOutcome directly
        let precompile = self.precompiles.get(address);
        let outcome = precompile.unwrap().call_ref(input, gas_limit, &self.env);
        match outcome {
            Ok(output) => (InstructionResult::Return, output.bytes),
            Err(e) => {
                let result = match e {
                    PrecompileErrors::Error(_) => InstructionResult::Revert,
                    PrecompileErrors::Fatal { msg: _ } => InstructionResult::PrecompileError,
                };
                (result, Bytes::default())
            }
        }
    }
}

// Helper to get account info and status, returning default if not found
pub fn get_account_context(
    context: &ExternalContext,
    address: Address,
) -> (AccountInfo, AccountStatus) {
    context.accounts.get(&address).cloned().unwrap_or_else(|| {
        // Return default info and treat as ColdLoaded if not present
        (AccountInfo::default(), AccountStatus::Loaded)
    })
}

// Helper to get storage slot, returning zero if not found
pub fn get_storage_slot_context(context: &ExternalContext, address: Address, index: U256) -> U256 {
    context
        .storage
        .get(&(address, index))
        .cloned()
        .unwrap_or(U256::ZERO)
}

pub type FrameContext = ContractEnv;

/// Outcome of a CALL-like operation.
#[derive(Clone, Debug, Default)]
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
