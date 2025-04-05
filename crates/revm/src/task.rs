use core::default::Default;
use revm_ssa::{SSAOutput, StorageKey};

use crate::primitives::{Env, EvmState, ExecutionResult, SpecId, B256};

use crate::journaled_state::{AccessType, ReadWriteSet};
use std::cmp::Ordering;

#[derive(Default)]
pub struct Task {
    pub tid: i32,
    pub sid: i32,
    pub gas: u64,
    pub spec_id: SpecId,
    pub env: Box<Env>,
    pub tx_hash: Option<B256>,
}

impl Task {
    pub fn new(env: Box<Env>, tid: i32, sid: i32, spec_id: SpecId, tx_hash: Option<B256>) -> Self {
        Self {
            tid,
            sid,
            gas: env.tx.gas_limit,
            spec_id,
            env,
            tx_hash,
        }
    }
}

pub struct SidOrderedTask(pub Task);
pub struct TidOrderedTask(pub Task);
pub struct GasOrderedTask(pub Task);

impl Ord for SidOrderedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .sid
            .cmp(&other.0.sid)
            .then_with(|| self.0.tid.cmp(&other.0.tid))
    }
}

impl PartialOrd for SidOrderedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for SidOrderedTask {
    fn eq(&self, other: &Self) -> bool {
        self.0.sid == other.0.sid && self.0.tid == other.0.tid
    }
}

impl Eq for SidOrderedTask {}

impl Ord for TidOrderedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.tid.cmp(&other.0.tid)
    }
}

impl PartialOrd for TidOrderedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for TidOrderedTask {
    fn eq(&self, other: &Self) -> bool {
        self.0.tid == other.0.tid
    }
}

impl Eq for TidOrderedTask {}

impl Ord for GasOrderedTask {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0
            .gas
            .cmp(&other.0.gas)
            .then_with(|| self.0.tid.cmp(&other.0.tid))
    }
}

impl PartialOrd for GasOrderedTask {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for GasOrderedTask {
    fn eq(&self, other: &Self) -> bool {
        self.0.gas == other.0.gas && self.0.tid == other.0.tid
    }
}

impl Eq for GasOrderedTask {}

pub struct TaskResultItem<I> {
    pub gas_limit: u64,
    pub result: Option<ExecutionResult>,
    pub inspector: Option<I>,
    pub read_write_set: Option<ReadWriteSet>,
    pub state: Option<EvmState>,
    pub ssa_output: Option<Vec<SSAOutput>>,
}

impl<I> Default for TaskResultItem<I> {
    fn default() -> Self {
        Self {
            gas_limit: 0,
            result: None,
            inspector: None,
            read_write_set: None,
            state: None,
            ssa_output: None,
        }
    }
}

impl<I> TaskResultItem<I> {
    pub fn get_read_write_set(&self) -> ReadWriteSet {
        if let Some(rw_set) = &self.read_write_set {
            return rw_set.clone();
        }

        if let Some(ssa_state) = &self.ssa_output {
            let mut read_write_set = ReadWriteSet::new();

            for output in ssa_state {
                if let SSAOutput::Storage { key, .. } = output {
                    match &**key {
                        StorageKey::AccountInfo(address) => {
                            read_write_set.add_write(*address, AccessType::AccountInfo);
                        }
                        StorageKey::AccountStatus(address) => {
                            read_write_set.add_write(*address, AccessType::AccountStatus);
                        }
                        StorageKey::Slot(address, key) => {
                            read_write_set.add_write(*address, AccessType::StorageSlot(*key));
                        }
                    }
                }
            }
            return read_write_set;
        }

        ReadWriteSet::new()
    }
}
