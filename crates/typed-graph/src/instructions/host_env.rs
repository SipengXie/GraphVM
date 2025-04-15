use crate::typed_graph::{HasInputType, HasOutputType, TypedNode};
use revm_interpreter::as_usize_saturated;
use revm_primitives::{Env, U256}; // Reusing this helper macro

// --- Common Input Type for Env Nodes ---

/// Type alias for Env pointer input
type EnvInput = (*const Env,);
/// Trait implementation for Env pointer input
impl HasInputType<EnvInput> for ChainIdNode {}
impl HasInputType<EnvInput> for CoinbaseNode {}
impl HasInputType<EnvInput> for TimestampNode {}
impl HasInputType<EnvInput> for NumberNode {}
impl HasInputType<EnvInput> for DifficultyNode {} // PREVRANDAO is handled internally
impl HasInputType<EnvInput> for GasLimitNode {}
impl HasInputType<EnvInput> for GasPriceNode {}
impl HasInputType<EnvInput> for BaseFeeNode {}
impl HasInputType<EnvInput> for OriginNode {}
impl HasInputType<EnvInput> for BlobBaseFeeNode {}

// --- Common Output Type ---

/// Type alias for U256 output
type U256Output = (U256,);
/// Trait implementation for U256 output
impl HasOutputType<U256Output> for ChainIdNode {}
impl HasOutputType<U256Output> for CoinbaseNode {}
impl HasOutputType<U256Output> for TimestampNode {}
impl HasOutputType<U256Output> for NumberNode {}
impl HasOutputType<U256Output> for DifficultyNode {}
impl HasOutputType<U256Output> for GasLimitNode {}
impl HasOutputType<U256Output> for GasPriceNode {}
impl HasOutputType<U256Output> for BaseFeeNode {}
impl HasOutputType<U256Output> for OriginNode {}
impl HasOutputType<U256Output> for BlobBaseFeeNode {}
impl HasOutputType<U256Output> for BlobHashNode {} // Also outputs U256

// --- CHAINID Node (0x46) ---

/// Node for CHAINID operation: gets the chain ID.
pub struct ChainIdNode {
    inputs: EnvInput, // Use 'static lifetime for pointer type simplicity
    outputs: U256Output,
}

impl ChainIdNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ChainIdNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = U256::from((*self.inputs.0).cfg.chain_id);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- COINBASE Node (0x41) ---

/// Node for COINBASE operation: gets the beneficiary address.
pub struct CoinbaseNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl CoinbaseNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for CoinbaseNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            // Convert Address (B160) to U256
            self.outputs.0 =
                U256::from_be_bytes((*self.inputs.0).block.coinbase.into_word().into());
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- TIMESTAMP Node (0x42) ---

/// Node for TIMESTAMP operation: gets the block timestamp.
pub struct TimestampNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl TimestampNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for TimestampNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).block.timestamp;
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- NUMBER Node (0x43) ---

/// Node for NUMBER operation: gets the block number.
pub struct NumberNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl NumberNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for NumberNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).block.number;
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- DIFFICULTY / PREVRANDAO Node (0x44) ---

/// Node for DIFFICULTY/PREVRANDAO operation.
pub struct DifficultyNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl DifficultyNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for DifficultyNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let env = &*self.inputs.0;
            // After Shanghai, DIFFICULTY opcode reads PREVRANDAO.
            if let Some(prevrandao) = env.block.prevrandao {
                self.outputs.0 = U256::from_be_bytes(prevrandao.0);
            } else {
                // Before Shanghai, it reads difficulty.
                self.outputs.0 = env.block.difficulty;
            }
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- GASLIMIT Node (0x45) ---

/// Node for GASLIMIT operation: gets the block gas limit.
pub struct GasLimitNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl GasLimitNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for GasLimitNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = U256::from((*self.inputs.0).block.gas_limit);
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- GASPRICE Node (0x3a) ---

/// Node for GASPRICE operation: gets the effective gas price of the transaction.
pub struct GasPriceNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl GasPriceNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for GasPriceNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).effective_gas_price();
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- BASEFEE Node (0x48) ---

/// Node for BASEFEE operation: gets the block base fee.
pub struct BaseFeeNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl BaseFeeNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for BaseFeeNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).block.basefee;
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- ORIGIN Node (0x32) ---

/// Node for ORIGIN operation: gets the transaction originator address.
pub struct OriginNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl OriginNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for OriginNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            // Convert Address (B160) to U256
            self.outputs.0 = U256::from_be_bytes((*self.inputs.0).tx.caller.into_word().into());
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- BLOBBASEFEE Node (0x4a) --- Requires EIP-4844

/// Node for BLOBBASEFEE operation: gets the blob base fee.
pub struct BlobBaseFeeNode {
    inputs: EnvInput,
    outputs: U256Output,
}

impl BlobBaseFeeNode {
    pub fn new(env_ptr: *const Env) -> Self {
        Self {
            inputs: (env_ptr,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for BlobBaseFeeNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            // .block.get_blob_gasprice() returns Option<u128>
            self.outputs.0 = U256::from(
                (*self.inputs.0)
                    .block
                    .get_blob_gasprice()
                    .unwrap_or_default(),
            );
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}

// --- BLOBHASH Node (0x4f) --- Requires EIP-4844

/// Node for BLOBHASH operation: gets the versioned hash of a blob.
pub struct BlobHashNode {
    /// Inputs:
    /// 0: *const U256 - Index of the blob.
    /// 1: *const Env - Environment reference.
    inputs: (*const U256, *const Env),
    /// Output:
    /// 0: U256 - The blob hash or zero if index is out of bounds.
    outputs: U256Output,
}

impl HasInputType<(*const U256, *const Env)> for BlobHashNode {}
// Output type already implemented above

impl BlobHashNode {
    pub fn new(index_ptr: *const U256, env_ptr: *const Env) -> Self {
        Self {
            inputs: (index_ptr, env_ptr),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for BlobHashNode {
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let index = as_usize_saturated!(*self.inputs.0);
            let env = &*self.inputs.1;
            let tx = &env.tx;

            // Get the hash from the transaction's blob hashes list.
            self.outputs.0 = match tx.blob_hashes.get(index) {
                Some(hash) => U256::from_be_bytes(hash.0), // B256 to U256
                None => U256::ZERO,
            };
        }
        Ok(())
    }
    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
}
