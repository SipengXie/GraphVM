use crate::db::{AccountState, CacheDB, DatabaseCommit, DatabaseRef, DbAccount, EmptyDB};
use crate::primitives::{Account, AccountInfo, Address, Bytecode, HashMap, B256, U256};
use parking_lot::RwLock;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
/// ParallelDB is a wrapper around a read-only database that provides caching and performance metrics
/// It is designed to be used in parallel execution environments where multiple threads
/// need to access the same underlying database.

/// Statistics for database operations
#[derive(Default)]
pub struct DbStats {
    /// Counter for cache hits
    pub cache_hits: AtomicU64,
    /// Counter for cache misses
    pub cache_misses: AtomicU64,
    /// Total time spent reading from database
    pub db_read_time: AtomicU64,
    /// Total time spent accessing cache
    pub cache_time: AtomicU64,
    /// Maximum time for a single database read
    pub max_db_read_time: AtomicU64,
    /// Count of database reads
    pub db_read_count: AtomicU64,
}

impl DbStats {
    /// Get statistics in a human-readable format
    pub fn get_metrics(&self) -> (f64, u64, u64, Duration, Duration, Duration, Duration) {
        let hits = self.cache_hits.load(Ordering::Relaxed);
        let misses = self.cache_misses.load(Ordering::Relaxed);
        let total = hits + misses;
        let hit_rate = if total > 0 {
            hits as f64 / total as f64 * 100.0
        } else {
            0.0
        };

        let db_time = Duration::from_nanos(self.db_read_time.load(Ordering::Relaxed));
        let cache_time = Duration::from_nanos(self.cache_time.load(Ordering::Relaxed));
        let max_read = Duration::from_nanos(self.max_db_read_time.load(Ordering::Relaxed));

        let read_count = self.db_read_count.load(Ordering::Relaxed);
        let avg_read = if read_count > 0 {
            Duration::from_nanos(self.db_read_time.load(Ordering::Relaxed) / read_count)
        } else {
            Duration::from_nanos(0)
        };

        (
            hit_rate, hits, misses, db_time, cache_time, max_read, avg_read,
        )
    }

    /// Reset all statistics
    pub fn reset(&self) {
        self.cache_hits.store(0, Ordering::Relaxed);
        self.cache_misses.store(0, Ordering::Relaxed);
        self.db_read_time.store(0, Ordering::Relaxed);
        self.cache_time.store(0, Ordering::Relaxed);
        self.max_db_read_time.store(0, Ordering::Relaxed);
        self.db_read_count.store(0, Ordering::Relaxed);
    }

    /// Update database read time metrics
    pub fn update_db_read_time(&self, elapsed: Duration) {
        let nanos = elapsed.as_nanos() as u64;
        self.db_read_time.fetch_add(nanos, Ordering::Relaxed);
        self.db_read_count.fetch_add(1, Ordering::Relaxed);

        let mut current_max = self.max_db_read_time.load(Ordering::Relaxed);
        while current_max < nanos {
            match self.max_db_read_time.compare_exchange(
                current_max,
                nanos,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => break,
                Err(actual) => current_max = actual,
            }
        }
    }
}

pub struct ParallelDB<DB> {
    /// Cache layer implemented with RwLock for thread-safe access
    /// Using RwLock instead of Mutex as we expect many concurrent reads
    pub cache: RwLock<CacheDB<EmptyDB>>,

    /// The underlying read-only database wrapped in Arc for thread-safe sharing
    pub read_only_db: Arc<DB>,

    // Separate statistics for parallel and sequential execution
    parallel_stats: DbStats,
    sequential_stats: DbStats,
    is_parallel: AtomicBool,
}

impl<DB: DatabaseRef> ParallelDB<DB> {
    /// Creates a new ParallelDB instance with an empty cache
    /// TODO: Consider adding cache size configuration
    /// TODO: Consider adding cache eviction policies
    pub fn new(db: Arc<DB>) -> Self {
        Self {
            cache: RwLock::new(CacheDB::new(EmptyDB::default())),
            read_only_db: db,
            parallel_stats: DbStats::default(),
            sequential_stats: DbStats::default(),
            is_parallel: AtomicBool::new(true),
        }
    }

    /// Set execution mode
    pub fn set_parallel_mode(&self, is_parallel: bool) {
        self.is_parallel.store(is_parallel, Ordering::Relaxed);
    }

    /// Get current stats based on execution mode
    pub fn get_stats(&self) -> (f64, u64, u64, Duration, Duration, Duration, Duration) {
        if self.is_parallel.load(Ordering::Relaxed) {
            self.parallel_stats.get_metrics()
        } else {
            self.sequential_stats.get_metrics()
        }
    }

    pub fn reset_stats(&self) {
        self.parallel_stats.reset();
        self.sequential_stats.reset();
    }

    fn get_current_stats(&self) -> &DbStats {
        if self.is_parallel.load(Ordering::Relaxed) {
            &self.parallel_stats
        } else {
            &self.sequential_stats
        }
    }
}

/// Implementation of DatabaseRef trait for ParallelDB
/// Each method follows a similar pattern:
/// 1. Try to read from cache first
/// 2. If cache miss, read from underlying database
/// 3. Update cache with new data
/// 4. Track metrics for both cache and database operations
impl<DB: DatabaseRef> DatabaseRef for ParallelDB<DB> {
    type Error = DB::Error;

    /// Retrieves account information for a given address
    ///
    /// This method implements a two-layer caching strategy:
    /// 1. First checks the in-memory cache using a read lock
    /// 2. If cache miss, reads from the underlying database
    ///
    /// Performance tracking:
    /// - Measures cache access time
    /// - Tracks cache hits/misses
    /// - Monitors database read time
    ///
    /// Thread safety:
    /// - Uses RwLock for cache access
    /// - Minimizes lock hold time by using scoped blocks
    /// - Uses atomic counters for metrics
    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        let stats = self.get_current_stats();
        {
            let cache = self.cache.read();
            let cache_start = Instant::now();
            if let Some(acc) = cache.accounts.get(&address) {
                stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                stats
                    .cache_time
                    .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                return Ok(acc.info());
            }
            stats
                .cache_time
                .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }

        stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        let db_start = Instant::now();
        let from_db = self.read_only_db.basic_ref(address)?;
        stats.update_db_read_time(db_start.elapsed());
        let mut cache = self.cache.write();
        cache.insert_account_info(address, from_db.clone().unwrap_or_default());
        Ok(from_db)
    }

    /// Retrieves bytecode for a given code hash
    ///
    /// Caching strategy:
    /// - Contracts are immutable once deployed
    /// - High cache hit rates expected for popular contracts
    /// - Cache entries never need invalidation
    ///
    /// Performance considerations:
    /// - Bytecode can be large, making cache beneficial
    /// - Clone operations might be expensive
    ///
    /// TODO: Consider implementing bytecode size limits in cache
    /// TODO: Add separate metrics for bytecode size distribution
    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        let stats = self.get_current_stats();
        {
            let cache = self.cache.read();
            let cache_start = Instant::now();
            if let Some(code) = cache.contracts.get(&code_hash) {
                stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                stats
                    .cache_time
                    .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                return Ok(code.clone());
            }
            stats
                .cache_time
                .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }

        stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        let db_start = Instant::now();
        let from_db = self.read_only_db.code_by_hash_ref(code_hash)?;
        stats.update_db_read_time(db_start.elapsed());
        let mut cache = self.cache.write();
        cache.contracts.insert(code_hash, from_db.clone());
        Ok(from_db)
    }

    /// Retrieves storage value for a given address and index
    ///
    /// Complex caching logic:
    /// 1. Checks if account exists in cache
    /// 2. If account exists, checks storage value
    /// 3. Handles special cases (cleared/non-existing accounts)
    ///
    /// Performance implications:
    /// - Multiple cache lookups (account + storage)
    /// - More complex than other ref methods
    /// - Higher chance of cache misses
    ///
    /// TODO: Consider separate caching for frequently accessed storage slots
    /// TODO: Add metrics for storage access patterns
    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let stats = self.get_current_stats();
        {
            let cache = self.cache.read();
            let cache_start = Instant::now();
            if let Some(acc) = cache.accounts.get(&address) {
                if let Some(value) = acc.storage.get(&index) {
                    stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                    stats
                        .cache_time
                        .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    return Ok(*value);
                } else if matches!(
                    acc.account_state,
                    AccountState::StorageCleared | AccountState::NotExisting
                ) {
                    stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                    stats
                        .cache_time
                        .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                    return Ok(U256::ZERO);
                }
            }
            stats
                .cache_time
                .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }

        stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        let db_start = Instant::now();
        let from_db = self.read_only_db.storage_ref(address, index)?;
        stats.update_db_read_time(db_start.elapsed());
        let mut cache = self.cache.write();
        let acc = cache
            .accounts
            .entry(address)
            .or_insert_with(DbAccount::new_not_existing);

        if !matches!(
            acc.account_state,
            AccountState::StorageCleared | AccountState::NotExisting
        ) {
            acc.storage.insert(index, from_db);
        }
        Ok(from_db)
    }

    /// Retrieves block hash for a given block number
    ///
    /// Caching characteristics:
    /// - Block hashes are immutable
    /// - Limited working set (only recent blocks needed)
    /// - Perfect for caching
    ///
    /// Implementation notes:
    /// - Converts block number to U256 for consistency
    /// - Uses same caching pattern as other methods
    ///
    /// TODO: Consider implementing LRU cache for block hashes
    /// TODO: Add pruning for very old block hashes
    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        let u_number = U256::from(number);
        let stats = self.get_current_stats();

        {
            let cache = self.cache.read();
            let cache_start = Instant::now();
            if let Some(existing) = cache.block_hashes.get(&u_number) {
                stats.cache_hits.fetch_add(1, Ordering::Relaxed);
                stats
                    .cache_time
                    .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
                return Ok(*existing);
            }
            stats
                .cache_time
                .fetch_add(cache_start.elapsed().as_nanos() as u64, Ordering::Relaxed);
        }

        stats.cache_misses.fetch_add(1, Ordering::Relaxed);
        let db_start = Instant::now();
        let from_db = self.read_only_db.block_hash_ref(number)?;
        stats.update_db_read_time(db_start.elapsed());
        let mut cache = self.cache.write();
        cache.block_hashes.insert(u_number, from_db);
        Ok(from_db)
    }
}

impl<DB> DatabaseCommit for ParallelDB<DB> {
    fn commit(&mut self, changes: HashMap<Address, Account>) {
        let mut cache = self.cache.write();
        cache.commit(changes);
    }
}
