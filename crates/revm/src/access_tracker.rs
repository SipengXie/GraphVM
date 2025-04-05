use crate::journaled_state::AccessType;
use crate::primitives::{Address, HashMap, HashSet};

/// Track write access records for addresses
#[derive(Default)]
pub struct AccessTracker {
    // Record the write access history for each address
    // The outer HashMap's key is the address, the inner HashMap's key is AccessType, Vec stores all written tids (ordered)
    writes: HashMap<Address, HashMap<AccessType, Vec<i32>>>,
}

impl AccessTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record the write set of a task
    pub fn record_write_set(
        &mut self,
        tid: i32,
        write_set: &HashMap<Address, HashSet<AccessType>>,
    ) {
        for (addr, access_types) in write_set {
            let addr_writes = self.writes.entry(*addr).or_default();
            for access_type in access_types {
                let tid_vec = addr_writes.entry(access_type.clone()).or_default();
                tid_vec.push(tid);
                // Since they are submitted in order, the new tid must be greater than all previous tids, no need to sort
            }
        }
    }

    /// Check if there is a write access within the range
    /// Return all conflicting keys found
    /// !! Potential overhead
    pub fn check_conflict_in_range(
        &self,
        read_set: &HashMap<Address, HashSet<AccessType>>,
        start_tid: i32, // sid + 1
        end_tid: i32,   // tid
        enable_ssa: bool,
    ) -> Vec<(Address, AccessType)> {
        let mut conflicts = Vec::new();
        for (addr, access_types) in read_set {
            if let Some(addr_writes) = self.writes.get(addr) {
                for access_type in access_types {
                    if let Some(tid_vec) = addr_writes.get(access_type) {
                        // Use binary search to find the first position greater than start_tid
                        let has_conflict = match tid_vec.binary_search(&start_tid) {
                            // Found exact match at start_tid
                            Ok(_) => true,
                            // Check first position after start_tid
                            Err(idx) => idx < tid_vec.len() && tid_vec[idx] < end_tid,
                        };

                        if has_conflict {
                            conflicts.push((*addr, access_type.clone()));
                            // Early exit if SSA not enabled
                            if !enable_ssa {
                                return conflicts;
                            }
                        }
                    }
                }
            }
        }
        conflicts
    }
}
