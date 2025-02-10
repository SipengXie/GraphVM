/// SsaAccessTracker is responsible for tracking access events of storage keys within the SSA context.
/// It uses an internal HashMap to map each StorageKey to a sorted list of transaction IDs (tids).

use std::collections::HashMap;
use revm_primitives::HashSet;
use revm_ssa::StorageKey;

/// The SsaAccessTracker tracks access events of storage keys within the SSA context.
pub struct SsaAccessTracker {
    accesses: HashMap<StorageKey, Vec<i32>>,
}

impl SsaAccessTracker {
    /// Creates a new SsaAccessTracker with an empty accesses hashmap.
    pub fn new() -> Self {
        Self {
            accesses: HashMap::new(),
        }
    }

    /// Records an access event for the given storage key with the specified transaction id (tid).
    ///
    /// The tid is appended to the vector corresponding to the storage key. It is assumed that tids
    /// are recorded in increasing order. If not guaranteed, one might need to sort the vector after insertion.
    pub fn record_access(&mut self, write_set: &HashSet<StorageKey>, tid: i32) {
        for key in write_set {
            self.accesses.entry(*key).or_insert_with(Vec::new).push(tid);
        }
    }



    /// Given a slice of storage keys and a transaction id range [start_tid, end_tid),
    /// queries and returns the list of storage keys that have at least one recorded tid within that range.
    ///
    /// It performs a binary search on the sorted vector of tids for each storage key to efficiently
    /// determine if any tid falls within the range.
    pub fn query_conflicts(&self, storage_keys: &[StorageKey], start_tid: i32, end_tid: i32) -> Vec<StorageKey> {
        let mut conflicts = Vec::new();
        for &key in storage_keys {

            if let Some(tids) = self.accesses.get(&key) {
                // Perform a binary search to find the first tid >= start_tid.
                let pos = tids.binary_search_by(|&tid| tid.cmp(&start_tid)).unwrap_or_else(|x| x);
                // Check if the found position is within bounds and the tid is less than end_tid.
                if pos < tids.len() && tids[pos] < end_tid {
                    conflicts.push(key);
                }
            }
        }
        conflicts
    }
}
