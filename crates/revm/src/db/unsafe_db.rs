use crate::db::Database;
use crate::primitives::{AccountInfo, Address, Bytecode, B256, U256};
pub struct UnsafeDB<DB> {
    db: DB,
}

impl<DB: Database> UnsafeDB<DB> {
    pub fn new(db: DB) -> Self {
        Self { db }
    }
}

impl<DB: Database> Database for UnsafeDB<DB> {
    type Error = DB::Error;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.db.basic(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.db.code_by_hash(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.db.storage(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.db.block_hash(number)
    }
}
