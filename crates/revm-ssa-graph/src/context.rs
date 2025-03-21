use std::marker::PhantomData;
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};

use revm_precompile::{PrecompileSpecId, Precompiles};
use revm_primitives::{
    db::DatabaseRef,
    AccountInfo, 
    AccountStatus, 
    Address, 
    Bytes, 
    Env, 
    PrecompileErrors, 
    Spec, 
    BLOCK_HASH_HISTORY, 
    U256
};
use revm_ssa::{
    SSACallInput, 
    SSACreateInput, 
    SSAInstructionResult, 
    SSAInterpreterResult, 
    StorageKey, 
    StorageValue
};

use crate::{instructions::as_u64_saturated, ExecutionError, Result};

/// Execution context
pub struct ExecutionContext<'a, DB: DatabaseRef, SPEC: Spec> {
    /// Environment
    env: Arc<&'a Env>,
    /// Database reference
    db: Arc<DB>,
    /// Virtual memory size
    memory_size: AtomicUsize,
    /// Hardfork specification
    spec: PhantomData<SPEC>,
    /// Precompiles
    precompiles: &'static Precompiles,
    /// First call input
    first_call_input: Option<SSACallInput>,
    /// First create input
    first_create_input: Option<SSACreateInput>,
}

impl<'a, DB: DatabaseRef, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    pub fn new(env: &'a Env, db: DB, first_call_input: Option<SSACallInput>, first_create_input: Option<SSACreateInput>) -> Self {
        Self {
            env: Arc::new(env),
            db: Arc::new(db),
            memory_size: AtomicUsize::new(0),
            spec: PhantomData,  
            precompiles: Precompiles::new(PrecompileSpecId::from_spec_id(SPEC::SPEC_ID)),
            first_call_input,
            first_create_input,
        }
    }

    /// Get environment
    #[inline(always)]
    pub fn env(&self) -> &'a Env {
        self.env.as_ref()
    }

    #[inline(always)]
    pub fn is_precompile(&mut self, address: &Address) -> bool {
        self.precompiles.contains(address)
    }

    #[inline(always)]
    pub fn call_precompile(&mut self, address: &Address, input_data: &Bytes, gas: u64) -> SSAInterpreterResult {
        let precompile = self.precompiles.get(address);

        let outcome = precompile.unwrap().call_ref(input_data, gas, self.env.as_ref());
        match outcome {
            Ok(output) => {
                let ssa_interpreter_result = SSAInterpreterResult {
                    result: SSAInstructionResult::Ok,
                    output: output.bytes,
                };
                return ssa_interpreter_result
            }
            Err(e) => {
                let ssa_interpreter_result = SSAInterpreterResult {
                    result: match e {
                        PrecompileErrors::Error(_) => SSAInstructionResult::Revert,
                        PrecompileErrors::Fatal{msg: _} => SSAInstructionResult::Error,
                    },
                    output: Bytes::default()
                };
                return ssa_interpreter_result
            }
        }
    }

    #[inline(always)]
    pub fn get_first_call_input(&self) -> Option<SSACallInput> {
        self.first_call_input.clone()
    }

    #[inline(always)]
    pub fn get_first_create_input(&self) -> Option<SSACreateInput> {
        self.first_create_input.clone()
    }

    /// Get state value based on storage key
    #[inline(always)]
    pub fn get_state(&self, storage_key: &StorageKey) -> Result<StorageValue> {
        match storage_key {
            StorageKey::Slot(address, slot) => {
                if let Ok(value) = self.db.storage_ref(*address, *slot) {
                    Ok(StorageValue::Slot(value))
                } else {
                    Ok(StorageValue::Slot(U256::ZERO))
                }
            },
            StorageKey::AccountInfo(address) => {
                if let Ok(Some(account)) = self.db.basic_ref(*address) {
                    Ok(StorageValue::AccountInfo(account))
                } else {
                    Ok(StorageValue::AccountInfo(AccountInfo::default()))
                }
            },
            StorageKey::AccountStatus(_address) => {
                Ok(StorageValue::AccountStatus(AccountStatus::default()))
            },
        }
    }

    /// Get blockhash
    #[inline(always)]
    pub fn get_blockhash(&mut self, requested_number: u64) -> Result<U256> {
        let block_number = as_u64_saturated!(self.env().block.number);
        let Some(diff) = block_number.checked_sub(requested_number) else {
            return Ok(U256::ZERO);
        };
        // blockhash should push zero if number is same as current block number.
        if diff == 0 {
            return Ok(U256::ZERO);
        }

        if diff <= BLOCK_HASH_HISTORY {
            let block_hash = self.db.block_hash_ref(requested_number)
            .map_err(|_e| {
                let str = format!("Failed to get block hash for number: {}", requested_number);
                ExecutionError::Database(str)
            })?;
            return Ok(block_hash.into());
        }

        Ok(U256::ZERO)
    }

    /// Get current memory size
    #[inline(always)]
    pub fn memory_size(&self) -> usize {
        self.memory_size.load(Ordering::Relaxed)
    }

    /// Set memory size
    #[inline(always)]
    pub fn set_memory_size(&mut self, size: usize) {
        self.memory_size.store(size, Ordering::Relaxed);
    }
    
}