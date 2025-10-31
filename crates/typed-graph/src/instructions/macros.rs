/// Converts a U256 input (typically from the stack) to an Address.
/// Assumes the U256 represents the lower 160 bits of the address.
#[macro_export]
macro_rules! u256_to_address {
    ($u256_val:expr) => {
        // Convert U256 to B256, then take the lower 160 bits (Address::from_word does this)
        revm_primitives::Address::from_word(revm_primitives::B256::from($u256_val))
    };
}

/// Re-export as_usize_saturated for convenience within this crate if needed elsewhere.
/// Or ensure it's imported correctly where used.
pub use revm_interpreter::as_usize_saturated;
