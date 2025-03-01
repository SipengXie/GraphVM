use std::marker::PhantomData;
use revm_precompile::{PrecompileSpecId, Precompiles};
use revm_primitives::{
    db::DatabaseRef, AccountInfo, AccountStatus, Address, Env, Spec, BLOCK_HASH_HISTORY, U256
};
use revm_ssa::{StorageKey, StorageValue};


use std::sync::{Arc, RwLock, atomic::{AtomicUsize, Ordering}};

use crate::instructions::as_u64_saturated;

/// Execution context
pub struct ExecutionContext<'a, DB: DatabaseRef, SPEC: Spec> {
    /// Environment
    env: Arc<&'a Env>,
    /// Database reference
    db: Arc<DB>,
    /// Virtual memory size
    memory_size: AtomicUsize,
    /// Error
    error: RwLock<Result<(), <DB as DatabaseRef>::Error>>,
    /// Hardfork specification
    spec: PhantomData<SPEC>,
    /// Precompiles
    precompiles: &'static Precompiles,
}

impl<'a, DB: DatabaseRef, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    pub fn new(env: &'a Env, db: DB) -> Self {
        Self {
            env: Arc::new(env),
            db: Arc::new(db),
            memory_size: AtomicUsize::new(0),
            error: RwLock::new(Ok(())),
            spec: PhantomData,  
            precompiles: Precompiles::new(PrecompileSpecId::from_spec_id(SPEC::SPEC_ID)),
        }
    }

    /// Get environment
    pub fn env(&self) -> &'a Env {
        self.env.as_ref()
    }

    /// Get account information
    pub fn get_account(&mut self, address: &Address) -> AccountInfo {
        // If not in cache, look up in database
        if let Ok(Some(account)) = self.db.basic_ref(*address) {
            return account;
        }
        AccountInfo::default()
    }

    /// Get account balance
    pub fn get_balance(&mut self, address: &Address) -> U256 {
        self.get_account(address).balance
    }

    /// Get storage value
    pub fn get_storage(&mut self, address: &Address, key: U256) -> U256 {
        // If not in cache, look up in database
        if let Ok(value) = self.db.storage_ref(*address, key) {
            return value;
        }
        U256::ZERO
    }

    pub fn get_blockhash(&mut self, requested_number: u64) -> U256 {
        let block_number = as_u64_saturated(self.env().block.number);
        let Some(diff) = block_number.checked_sub(requested_number) else {
            return U256::ZERO;
        };
        // blockhash should push zero if number is same as current block number.
        if diff == 0 {
            return U256::ZERO;
        }

        if diff <= BLOCK_HASH_HISTORY {
            let block_hash = self.db.block_hash_ref(requested_number)
            .map_err(|e| *self.error.write().unwrap() = Err(e))
            .ok()
            .unwrap();
            return block_hash.into();
        }

        U256::ZERO
    }

    /// Get current memory size
    pub fn memory_size(&self) -> usize {
        self.memory_size.load(Ordering::Relaxed)
    }

    /// Set memory size
    pub fn set_memory_size(&mut self, size: usize) {
        self.memory_size.store(size, Ordering::Relaxed);
    }

    /// Unified storage access interface
    pub fn get_storage_value_from_db(&mut self, key: &StorageKey) -> StorageValue {
        match key {
            StorageKey::Slot(address, slot) => {
                let value = self.get_storage(address, *slot);
                StorageValue::Slot(value)
            },
            StorageKey::AccountInfo(address) => {
                let account = self.get_account(address);
                StorageValue::AccountInfo(account)
            },
            StorageKey::AccountStatus(_) => {
                StorageValue::AccountStatus(AccountStatus::default())
            }
        }
    }
    
}