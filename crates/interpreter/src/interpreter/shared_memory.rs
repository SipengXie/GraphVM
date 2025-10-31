use crate::primitives::{hex, B256, U256};
use core::{cmp::min, fmt, ops::Range};
use revm_ssa::{logger::LsnWithIndex, MemoryDep};
use std::vec::Vec;
use core::ptr;

/// A tuple of (LSN, offset) representing where this byte was written from
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct MemoryDef {
    /// The LSN of the instruction that wrote this byte
    pub lsn: LsnWithIndex,
    /// The offset in the result of that instruction
    pub offset: usize,
}

const EMPTY_MEMORY_DEF: MemoryDef = MemoryDef {
    lsn: (0, 0),
    offset: 0,
};

/// A sequential memory shared between calls, which uses
/// a `Vec` for internal representation.
/// A [SharedMemory] instance should always be obtained using
/// the `new` static method to ensure memory safety.
#[derive(Clone, PartialEq, Eq, Hash)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct SharedMemory {
    /// The underlying buffer.
    buffer: Vec<u8>,
    /// Memory checkpoints for each depth.
    /// Invariant: these are always in bounds of `data`.
    checkpoints: Vec<usize>,
    /// Invariant: equals `self.checkpoints.last()`
    last_checkpoint: usize,
    /// Memory limit. See [`CfgEnv`](wiring::default::CfgEnv).
    #[cfg(feature = "memory_limit")]
    memory_limit: u64,
    /// Shadow memory buffer for tracking memory definitions
    shadow_buffer: Option<Vec<MemoryDef>>,
}

/// Empty shared memory.
///
/// Used as placeholder inside Interpreter when it is not running.
pub const EMPTY_SHARED_MEMORY: SharedMemory = SharedMemory {
    buffer: Vec::new(),
    checkpoints: Vec::new(),
    last_checkpoint: 0,
    #[cfg(feature = "memory_limit")]
    memory_limit: u64::MAX,
    shadow_buffer: None,
};

impl fmt::Debug for SharedMemory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedMemory")
            .field("current_len", &self.len())
            .field("context_memory", &hex::encode(self.context_memory()))
            .finish_non_exhaustive()
    }
}

impl Default for SharedMemory {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl SharedMemory {
    /// Creates a new memory instance that can be shared between calls.
    ///
    /// The default initial capacity is 4KiB.
    #[inline]
    pub fn new() -> Self {
        Self::with_capacity(4 * 1024) // from evmone
    }

    /// Creates a new memory instance that can be shared between calls with the given `capacity`.
    #[inline]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            buffer: Vec::with_capacity(capacity),
            checkpoints: Vec::with_capacity(32),
            last_checkpoint: 0,
            #[cfg(feature = "memory_limit")]
            memory_limit: u64::MAX,
            shadow_buffer: None,
        }
    }

    /// Enable shadow memory tracking
    #[inline]
    pub fn enable_shadow(&mut self) {
        if self.shadow_buffer.is_none() {
            self.shadow_buffer = Some(Vec::with_capacity(4 * 1024));
        }
    }

    /// Disable shadow memory tracking
    #[inline]
    pub fn disable_shadow(&mut self) {
        self.shadow_buffer = None;
    }

    /// Detect if shadow memory is enabled
    #[inline]
    pub fn is_shadow_enabled(&self) -> bool {
        self.shadow_buffer.is_some()
    }

    /// Record a memory write operation in the shadow memory
    #[inline]
    pub fn record_shadow_write(&mut self, addr: usize, size: usize, lsn: LsnWithIndex) {
        // short circuit for empty write
        if size == 0 {
            return;
        }
        if let Some(ref mut shadow) = self.shadow_buffer {
            // Ensure capacity
            let required_len = self.last_checkpoint + addr + size;
            if required_len > shadow.len() {
                shadow.resize(required_len, EMPTY_MEMORY_DEF);
            }

            // SAFETY: We've ensured the capacity above
            unsafe {
                let shadow_ptr = shadow.as_mut_ptr().add(self.last_checkpoint + addr);
                for i in 0..size {
                    *shadow_ptr.add(i) = MemoryDef { lsn, offset: i };
                }
            }
        }
    }

    /// Get the shadow memory definition for a range of memory
    #[inline]
    pub fn get_shadow_defs(&self, range: Range<usize>) -> Vec<MemoryDef> {
        if let Some(ref shadow) = self.shadow_buffer {
            let start = self.last_checkpoint + range.start;
            let end = (self.last_checkpoint + range.end).min(shadow.len());
            if start >= end {
                return Vec::new();
            }

            // SAFETY: We've checked the bounds above
            unsafe {
                let mut result = Vec::with_capacity(end - start);
                result.set_len(end - start);
                ptr::copy_nonoverlapping(
                    shadow.as_ptr().add(start),
                    result.as_mut_ptr(),
                    end - start,
                );
                result
            }
        } else {
            Vec::new()
        }
    }

    /// Get the shadow memory definitions for a word (32 bytes)
    #[inline]
    pub fn get_shadow_word_defs(&self, offset: usize) -> Vec<MemoryDef> {
        self.get_shadow_defs(offset..offset + 32)
    }

    /// Convert the given memory range to memory dependencies logs
    #[inline]
    pub fn get_shadow_deps(&self, range: Range<usize>) -> Vec<MemoryDep> {
        // short circuit for empty range
        if range.len() == 0 {
            return Vec::new();
        }
        let defs = self.get_shadow_defs(range.clone());
        if defs.is_empty() {
            return Vec::new();
        }

        let mut result = Vec::new();
        let mut start = 0;
        let mut current_lsn = (0, 0);
        let mut current_offset = 0;
        let mut len = 0;

        // Helper function to push current memory segment
        let mut push_segment = |start: usize, len: usize, lsn: LsnWithIndex, offset: usize| {
            if lsn.0 != 0 {
                result.push(MemoryDep {
                    // self_offset..self_offset+len is set by lsn's output[lsn_offset..lsn_offset+len]
                    lsn,
                    self_offset: start,
                    lsn_offset: offset,
                    length: len,
                });
            }
        };

        // Iterate through definitions to find continuous segments
        for (i, &mem_def) in defs.iter().enumerate() {
            if mem_def == EMPTY_MEMORY_DEF {
                // End current segment if exists
                push_segment(start, len, current_lsn, current_offset);
                current_lsn = (0, 0);
                continue;
            }

            if current_lsn.0 == 0 {
                // Start new segment
                start = i;
                current_lsn = mem_def.lsn;
                current_offset = mem_def.offset;
                len = 1;
            } else if current_lsn == mem_def.lsn && current_offset + len == mem_def.offset {
                // Continue current segment
                len += 1;
            } else {
                // End current segment and start new one
                push_segment(start, len, current_lsn, current_offset);
                start = i;
                current_lsn = mem_def.lsn;
                current_offset = mem_def.offset;
                len = 1;
            }
        }

        // Push final segment if exists
        push_segment(start, len, current_lsn, current_offset);

        result
    }

    /// Creates a new memory instance that can be shared between calls,
    /// with `memory_limit` as upper bound for allocation size.
    ///
    /// The default initial capacity is 4KiB.
    #[cfg(feature = "memory_limit")]
    #[inline]
    pub fn new_with_memory_limit(memory_limit: u64) -> Self {
        Self {
            memory_limit,
            ..Self::new()
        }
    }

    /// Returns `true` if the `new_size` for the current context memory will
    /// make the shared buffer length exceed the `memory_limit`.
    #[cfg(feature = "memory_limit")]
    #[inline]
    pub fn limit_reached(&self, new_size: usize) -> bool {
        self.last_checkpoint.saturating_add(new_size) as u64 > self.memory_limit
    }

    /// Prepares the shared memory for a new context.
    #[inline]
    pub fn new_context(&mut self) {
        let new_checkpoint = self.buffer.len();
        self.checkpoints.push(new_checkpoint);
        self.last_checkpoint = new_checkpoint;
    }

    /// Prepares the shared memory for returning to the previous context.
    #[inline]
    pub fn free_context(&mut self) {
        if let Some(old_checkpoint) = self.checkpoints.pop() {
            self.buffer.truncate(old_checkpoint);
            self.last_checkpoint = self.checkpoints.last().copied().unwrap_or(0);

            // Also handle shadow buffer if enabled
            if let Some(ref mut shadow) = self.shadow_buffer {
                shadow.truncate(old_checkpoint);
            }
        }
    }

    /// Clear all memory and reset to initial state.
    #[inline]
    pub fn clear(&mut self) {
        self.buffer.clear();
        self.checkpoints.clear();
        self.last_checkpoint = 0;

        // Also clear shadow buffer if enabled
        if let Some(ref mut shadow) = self.shadow_buffer {
            shadow.clear();
        }
    }

    /// Returns the length of the current memory range.
    #[inline]
    pub fn len(&self) -> usize {
        self.buffer.len() - self.last_checkpoint
    }

    /// Returns `true` if the current memory range is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the gas cost for the current memory expansion.
    #[inline]
    pub fn current_expansion_cost(&self) -> u64 {
        crate::gas::memory_gas_for_len(self.len())
    }

    /// Resizes the memory in-place so that `len` is equal to `new_len`.
    #[inline]
    pub fn resize(&mut self, new_size: usize) {
        self.buffer.resize(self.last_checkpoint + new_size, 0);
        // Also resize shadow buffer if enabled
        if let Some(ref mut shadow) = self.shadow_buffer {
            shadow.resize(self.last_checkpoint + new_size, EMPTY_MEMORY_DEF);
        }
    }

    /// Returns a byte slice of the memory region at the given offset.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn slice(&self, offset: usize, size: usize) -> &[u8] {
        self.slice_range(offset..offset + size)
    }

    /// Returns a byte slice of the memory region at the given offset.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn slice_range(&self, range @ Range { start, end }: Range<usize>) -> &[u8] {
        match self.context_memory().get(range) {
            Some(slice) => slice,
            None => debug_unreachable!("slice OOB: {start}..{end}; len: {}", self.len()),
        }
    }

    /// Returns a byte slice of the memory region at the given offset.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn slice_mut(&mut self, offset: usize, size: usize) -> &mut [u8] {
        let end = offset + size;
        match self.context_memory_mut().get_mut(offset..end) {
            Some(slice) => slice,
            None => debug_unreachable!("slice OOB: {offset}..{end}"),
        }
    }

    /// Returns the byte at the given offset.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    pub fn get_byte(&self, offset: usize) -> u8 {
        self.slice(offset, 1)[0]
    }

    /// Returns a 32-byte slice of the memory region at the given offset.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    pub fn get_word(&self, offset: usize) -> B256 {
        self.slice(offset, 32).try_into().unwrap()
    }

    /// Returns a U256 of the memory region at the given offset.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    pub fn get_u256(&self, offset: usize) -> U256 {
        self.get_word(offset).into()
    }

    /// Sets the `byte` at the given `index`.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set_byte(&mut self, offset: usize, byte: u8) {
        self.set(offset, &[byte]);
    }

    /// Sets the given 32-byte `value` to the memory region at the given `offset`.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set_word(&mut self, offset: usize, value: &B256) {
        self.set(offset, &value[..]);
    }

    /// Sets the given U256 `value` to the memory region at the given `offset`.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set_u256(&mut self, offset: usize, value: U256) {
        self.set(offset, &value.to_be_bytes::<32>());
    }

    /// Set memory region at given `offset`.
    ///
    /// # Panics
    ///
    /// Panics on out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set(&mut self, offset: usize, value: &[u8]) {
        if !value.is_empty() {
            self.slice_mut(offset, value.len()).copy_from_slice(value);
        }
    }

    /// Set memory from data. Our memory offset+len is expected to be correct but we
    /// are doing bound checks on data/data_offeset/len and zeroing parts that is not copied.
    ///
    /// # Panics
    ///
    /// Panics if memory is out of bounds.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn set_data(&mut self, memory_offset: usize, data_offset: usize, len: usize, data: &[u8]) {
        if data_offset >= data.len() {
            // nullify all memory slots
            self.slice_mut(memory_offset, len).fill(0);
            return;
        }
        let data_end = min(data_offset + len, data.len());
        let data_len = data_end - data_offset;
        debug_assert!(data_offset < data.len() && data_end <= data.len());
        let data = unsafe { data.get_unchecked(data_offset..data_end) };
        self.slice_mut(memory_offset, data_len)
            .copy_from_slice(data);

        // nullify rest of memory slots
        // SAFETY: Memory is assumed to be valid, and it is commented where this assumption is made.
        self.slice_mut(memory_offset + data_len, len - data_len)
            .fill(0);
    }

    /// Copies elements from one part of the memory to another part of itself.
    #[inline]
    #[cfg_attr(debug_assertions, track_caller)]
    pub fn copy(&mut self, dst: usize, src: usize, len: usize) {
        self.context_memory_mut().copy_within(src..src + len, dst);

        // Also copy shadow memory if enabled
        if let Some(ref mut shadow) = self.shadow_buffer {
            let src = self.last_checkpoint + src;
            let dst = self.last_checkpoint + dst;
            let end = dst + len;

            // Ensure capacity
            if end > shadow.len() {
                shadow.resize(end, EMPTY_MEMORY_DEF);
            }

            // SAFETY: We've ensured the capacity above
            unsafe {
                ptr::copy(
                    shadow.as_ptr().add(src),
                    shadow.as_mut_ptr().add(dst),
                    len,
                );
            }
        }
    }

    /// Returns a reference to the memory of the current context, the active memory.
    #[inline]
    pub fn context_memory(&self) -> &[u8] {
        // SAFETY: access bounded by buffer length
        unsafe {
            self.buffer
                .get_unchecked(self.last_checkpoint..self.buffer.len())
        }
    }

    /// Returns a mutable reference to the memory of the current context.
    #[inline]
    pub fn context_memory_mut(&mut self) -> &mut [u8] {
        let buf_len = self.buffer.len();
        // SAFETY: access bounded by buffer length
        unsafe { self.buffer.get_unchecked_mut(self.last_checkpoint..buf_len) }
    }
}

/// Returns number of words what would fit to provided number of bytes,
/// i.e. it rounds up the number bytes to number of words.
#[inline]
pub const fn num_words(len: u64) -> u64 {
    len.saturating_add(31) / 32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_num_words() {
        assert_eq!(num_words(0), 0);
        assert_eq!(num_words(1), 1);
        assert_eq!(num_words(31), 1);
        assert_eq!(num_words(32), 1);
        assert_eq!(num_words(33), 2);
        assert_eq!(num_words(63), 2);
        assert_eq!(num_words(64), 2);
        assert_eq!(num_words(65), 3);
        assert_eq!(num_words(u64::MAX), u64::MAX / 32);
    }

    #[test]
    fn new_free_context() {
        let mut shared_memory = SharedMemory::new();
        shared_memory.new_context();

        assert_eq!(shared_memory.buffer.len(), 0);
        assert_eq!(shared_memory.checkpoints.len(), 1);
        assert_eq!(shared_memory.last_checkpoint, 0);

        unsafe { shared_memory.buffer.set_len(32) };
        assert_eq!(shared_memory.len(), 32);
        shared_memory.new_context();

        assert_eq!(shared_memory.buffer.len(), 32);
        assert_eq!(shared_memory.checkpoints.len(), 2);
        assert_eq!(shared_memory.last_checkpoint, 32);
        assert_eq!(shared_memory.len(), 0);

        unsafe { shared_memory.buffer.set_len(96) };
        assert_eq!(shared_memory.len(), 64);
        shared_memory.new_context();

        assert_eq!(shared_memory.buffer.len(), 96);
        assert_eq!(shared_memory.checkpoints.len(), 3);
        assert_eq!(shared_memory.last_checkpoint, 96);
        assert_eq!(shared_memory.len(), 0);

        // free contexts
        shared_memory.free_context();
        assert_eq!(shared_memory.buffer.len(), 96);
        assert_eq!(shared_memory.checkpoints.len(), 2);
        assert_eq!(shared_memory.last_checkpoint, 32);
        assert_eq!(shared_memory.len(), 64);

        shared_memory.free_context();
        assert_eq!(shared_memory.buffer.len(), 32);
        assert_eq!(shared_memory.checkpoints.len(), 1);
        assert_eq!(shared_memory.last_checkpoint, 0);
        assert_eq!(shared_memory.len(), 32);

        shared_memory.free_context();
        assert_eq!(shared_memory.buffer.len(), 0);
        assert_eq!(shared_memory.checkpoints.len(), 0);
        assert_eq!(shared_memory.last_checkpoint, 0);
        assert_eq!(shared_memory.len(), 0);
    }

    #[test]
    fn resize() {
        let mut shared_memory = SharedMemory::new();
        shared_memory.new_context();

        shared_memory.resize(32);
        assert_eq!(shared_memory.buffer.len(), 32);
        assert_eq!(shared_memory.len(), 32);
        assert_eq!(shared_memory.buffer.get(0..32), Some(&[0_u8; 32] as &[u8]));

        shared_memory.new_context();
        shared_memory.resize(96);
        assert_eq!(shared_memory.buffer.len(), 128);
        assert_eq!(shared_memory.len(), 96);
        assert_eq!(
            shared_memory.buffer.get(32..128),
            Some(&[0_u8; 96] as &[u8])
        );

        shared_memory.free_context();
        shared_memory.resize(64);
        assert_eq!(shared_memory.buffer.len(), 64);
        assert_eq!(shared_memory.len(), 64);
        assert_eq!(shared_memory.buffer.get(0..64), Some(&[0_u8; 64] as &[u8]));
    }

    #[test]
    fn test_shadow_memory_basic() {
        let mut memory = SharedMemory::new();

        // Initially shadow memory should be disabled
        assert!(memory.shadow_buffer.is_none());

        // Enable shadow memory
        memory.enable_shadow();
        assert!(memory.shadow_buffer.is_some());

        // Record a write
        memory.record_shadow_write(0, 32, (1, 0));

        // Check the write was recorded
        let defs = memory.get_shadow_defs(0..32);
        assert_eq!(defs.len(), 32);
        for (i, def) in defs.iter().enumerate() {
            assert_eq!(def.lsn, (1, 0));
            assert_eq!(def.offset, i);
        }

        // Disable shadow memory
        memory.disable_shadow();
        assert!(memory.shadow_buffer.is_none());
    }

    #[test]
    fn test_shadow_memory_context() {
        let mut memory = SharedMemory::new();
        memory.enable_shadow();

        // Write in parent context
        memory.resize(32); // First resize memory
        memory.set_u256(0, U256::from(0x123)); // Write actual memory
        memory.record_shadow_write(0, 32, (1, 0)); // Record in shadow memory

        // Verify parent context state
        assert_eq!(memory.get_u256(0), U256::from(0x123));
        let defs = memory.get_shadow_defs(0..32);
        assert_eq!(defs.len(), 32);
        for def in defs.iter() {
            assert_eq!(def.lsn, (1, 0));
        }

        // Create new context
        memory.new_context();

        // Write in child context
        memory.resize(32); // Resize in child context
        memory.set_u256(0, U256::from(0x456)); // Write actual memory
        memory.record_shadow_write(0, 32, (2, 0)); // Record in shadow memory

        // Verify child context state
        assert_eq!(memory.get_u256(0), U256::from(0x456));
        let defs = memory.get_shadow_defs(0..32);
        assert_eq!(defs.len(), 32);
        for def in defs.iter() {
            assert_eq!(def.lsn, (2, 0));
        }

        // Free child context
        memory.free_context();

        // Verify parent context is preserved
        assert_eq!(memory.get_u256(0), U256::from(0x123));
        let defs = memory.get_shadow_defs(0..32);
        assert_eq!(defs.len(), 32);
        for def in defs.iter() {
            assert_eq!(def.lsn, (1, 0));
        }
    }

    #[test]
    fn test_shadow_memory_copy() {
        let mut memory = SharedMemory::new();
        memory.enable_shadow();

        // Write source data
        memory.resize(64); // Resize for both source and destination
        memory.set_u256(0, U256::from(0x123)); // Write actual memory
        memory.record_shadow_write(0, 32, (1, 0)); // Record in shadow memory

        // Verify source data before copy
        assert_eq!(memory.get_u256(0), U256::from(0x123));
        let src_defs = memory.get_shadow_defs(0..32);
        for (i, def) in src_defs.iter().enumerate() {
            assert_eq!(def.lsn, (1, 0));
            assert_eq!(def.offset, i);
        }

        // Copy memory
        memory.copy(32, 0, 32);

        // Verify source data after copy
        assert_eq!(memory.get_u256(0), U256::from(0x123));
        let src_defs = memory.get_shadow_defs(0..32);
        for (i, def) in src_defs.iter().enumerate() {
            assert_eq!(def.lsn, (1, 0));
            assert_eq!(def.offset, i);
        }

        // Verify copied data
        assert_eq!(memory.get_u256(32), U256::from(0x123));
        let dst_defs = memory.get_shadow_defs(32..64);
        for (i, def) in dst_defs.iter().enumerate() {
            assert_eq!(def.lsn, (1, 0));
            assert_eq!(def.offset, i);
        }
    }

    #[test]
    fn test_shadow_memory_resize() {
        let mut memory = SharedMemory::new();
        memory.enable_shadow();

        // Write initial data
        memory.resize(32);
        memory.set_u256(0, U256::from(0x123));
        memory.record_shadow_write(0, 32, (1, 0));

        // Verify initial data
        assert_eq!(memory.get_u256(0), U256::from(0x123));
        let defs = memory.get_shadow_defs(0..32);
        for (i, def) in defs.iter().enumerate() {
            assert_eq!(def.lsn, (1, 0));
            assert_eq!(def.offset, i);
        }

        // Resize larger
        memory.resize(64);

        // Check original data preserved
        assert_eq!(memory.get_u256(0), U256::from(0x123));
        let defs = memory.get_shadow_defs(0..32);
        for (i, def) in defs.iter().enumerate() {
            assert_eq!(def.lsn, (1, 0));
            assert_eq!(def.offset, i);
        }

        // Check new space initialized to zero and None
        assert_eq!(memory.get_u256(32), U256::ZERO);
        let defs = memory.get_shadow_defs(32..64);
        for def in defs.iter() {
            assert!(def == &EMPTY_MEMORY_DEF);
        }

        // Resize smaller
        memory.resize(16);

        // Check truncated data
        let data = memory.slice(0, 16);
        let expected_data = U256::from(0x123).to_be_bytes::<32>();
        assert_eq!(data, &expected_data[..16]);
        let defs = memory.get_shadow_defs(0..16);
        for (i, def) in defs.iter().enumerate() {
            assert_eq!(def.lsn, (1, 0));
            assert_eq!(def.offset, i);
        }
    }

    #[test]
    fn test_convert_shadow_to_deps() {
        let mut memory = SharedMemory::new();
        memory.enable_shadow();

        // Test case 1: Two continuous regions with different LSNs
        memory.resize(32); // Resize for first write
        memory.set_u256(0, U256::from(0x123));
        memory.record_shadow_write(0, 16, (1, 0)); // lsn 1 set 0..16

        memory.resize(48); // Resize for second write
        memory.set_u256(16, U256::from(0x456));
        memory.record_shadow_write(16, 16, (2, 0)); // lsn 2 set 16..32

        // Convert to deps for continuous regions
        let deps = memory.get_shadow_deps(0..32);
        assert_eq!(
            deps.len(),
            2,
            "Should have two segments for continuous regions"
        );

        // Check first segment
        assert_eq!(
            deps[0],
            MemoryDep {
                lsn: (1, 0),
                self_offset: 0,
                lsn_offset: 0,
                length: 16
            }
        );

        // Check second segment
        assert_eq!(
            deps[1],
            MemoryDep {
                lsn: (2, 0),
                self_offset: 16,
                lsn_offset: 0,
                length: 16
            }
        );

        // Test case 2: Non-continuous regions with a gap
        memory.resize(96); // Resize for third write
        memory.set_u256(64, U256::from(0x789));
        memory.record_shadow_write(64, 32, (3, 0)); // lsn 3 set 64..96

        // Convert to deps including the gap
        let deps = memory.get_shadow_deps(16..96); // memory 16..96 is self 0..80, 0..16 is set by lsn 2, 48..80 is set by lsn 3
        assert_eq!(deps.len(), 2, "Should have two segments with gap");

        // Check first segment (part of second write)
        assert_eq!(
            deps[0],
            MemoryDep {
                lsn: (2, 0),
                self_offset: 0,
                lsn_offset: 0,
                length: 16
            }
        );

        // Check second segment (third write)
        assert_eq!(
            deps[1],
            MemoryDep {
                lsn: (3, 0),
                self_offset: 48,
                lsn_offset: 0,
                length: 32
            }
        );

        // Test case 3: Partial word with specific offset
        memory.resize(128); // Resize for fourth write
        memory.set_byte(96, 0x42);
        memory.record_shadow_write(96, 1, (4, 0));

        memory.resize(128); // Resize for fifth write
        memory.set_byte(97, 0x43);
        memory.record_shadow_write(97, 1, (4, 0));

        // Convert to deps for partial word
        let deps = memory.get_shadow_deps(96..98);
        assert_eq!(
            deps.len(),
            2,
            "Should have two segments for separate byte writes"
        );

        // Check first segment
        assert_eq!(
            deps[0],
            MemoryDep {
                lsn: (4, 0),
                self_offset: 0,
                lsn_offset: 0,
                length: 1
            }
        );

        // Check second segment
        assert_eq!(
            deps[1],
            MemoryDep {
                lsn: (4, 0),
                self_offset: 1,
                lsn_offset: 0,
                length: 1
            }
        );

        // Test case 4: Empty range
        let deps = memory.get_shadow_deps(32..32);
        assert!(deps.is_empty(), "Empty range should return empty deps");

        // Test case 5: Range with no definitions
        let deps = memory.get_shadow_deps(32..64);
        assert!(
            deps.is_empty(),
            "Range with no definitions should return empty deps"
        );
    }

    #[test]
    fn test_shadow_memory_clear() {
        let mut memory = SharedMemory::new();
        memory.enable_shadow();

        // Write some data
        memory.resize(32);
        memory.set_u256(0, U256::from(0x123));
        memory.record_shadow_write(0, 32, (1, 0));

        // Verify initial data
        assert_eq!(memory.get_u256(0), U256::from(0x123));
        let defs = memory.get_shadow_defs(0..32);
        for def in defs.iter() {
            assert_eq!(def.lsn, (1, 0));
        }

        // Clear memory
        memory.clear();

        // Verify memory is cleared
        assert_eq!(memory.len(), 0);
        assert!(memory.shadow_buffer.is_some());
        assert!(memory.get_shadow_defs(0..32).is_empty());

        // Write after clear should work
        memory.resize(32);
        memory.set_u256(0, U256::from(0x456));
        memory.record_shadow_write(0, 32, (2, 0));

        // Verify new data
        assert_eq!(memory.get_u256(0), U256::from(0x456));
        let defs = memory.get_shadow_defs(0..32);
        for def in defs.iter() {
            assert_eq!(def.lsn, (2, 0));
        }
    }
}
