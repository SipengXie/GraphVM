use core::{fmt, ptr};
use std::vec::Vec;

pub const STACK_LIMIT: usize = 1024;

#[derive(Debug, PartialEq, Eq)]
pub enum InstructionResult {
    StackOverflow,
    StackUnderflow,
}

/// Shadow stack for tracking LSN definitions, with same capacity as EVM stack
#[derive(Debug, PartialEq, Eq, Hash, Clone)]
#[cfg_attr(feature = "serde", derive(serde::Serialize))]
pub struct ShadowStack {
    /// The underlying data of the shadow stack, storing LSN definitions
    data: Vec<u16>, // 0 means constant, else means lsn
}

impl fmt::Display for ShadowStack {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for (i, x) in self.data.iter().enumerate() {
            if i > 0 {
                f.write_str(", ")?;
            }
            if *x > 0 {
                write!(f, "LSN({})", x)?;
            } else {
                write!(f, "Const")?;
            }
        }
        f.write_str("]")
    }
}

impl Default for ShadowStack {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl ShadowStack {
    /// Instantiate a new stack with the [default stack limit][STACK_LIMIT].
    #[inline]
    pub fn new() -> Self {
        Self {
            data: Vec::with_capacity(4*STACK_LIMIT),
        }
    }

    /// Returns the length of the stack in words.
    #[inline]
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Returns whether the stack is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Removes the topmost element from the stack and returns it, or `StackUnderflow` if it is
    /// empty.
    #[inline]
    pub fn pop(&mut self) -> Result<u16, InstructionResult> {
        self.data.pop().ok_or(InstructionResult::StackUnderflow)
    }

    /// Push a new value onto the stack.
    ///
    /// If it will exceed the stack limit, returns `StackOverflow` error and leaves the stack
    /// unchanged.
    #[inline]
    pub fn push(&mut self, value: u16) -> Result<(), InstructionResult> {
        self.data.push(value);
        Ok(())
    }

    /// Duplicates the `N`th value from the top of the stack.
    #[inline]
    pub fn dup(&mut self, n: usize) -> Result<(), InstructionResult> {
        let len = self.data.len();
        unsafe {
            let ptr = self.data.as_mut_ptr().add(len);
            ptr::copy_nonoverlapping(ptr.sub(n), ptr, 1);
            self.data.set_len(len + 1);
        }
        Ok(())
    }

    /// Swaps the topmost value with the `N`th value from the top.
    #[inline]
    pub fn swap(&mut self, n: usize) -> Result<(), InstructionResult> {
        self.exchange(0, n)
    }

    /// Exchange two values on the stack.
    ///
    /// `n` is the first index, and the second index is calculated as `n + m`.
    #[inline]
    pub fn exchange(&mut self, n: usize, m: usize) -> Result<(), InstructionResult> {
        let len = self.data.len();
        let n_m_index = n + m;
        unsafe {
            let top = self.data.as_mut_ptr().add(len - 1);
            core::ptr::swap_nonoverlapping(top.sub(n), top.sub(n_m_index), 1);
        }
        Ok(())
    }

}

#[cfg(feature = "serde")]
impl<'de> serde::Deserialize<'de> for ShadowStack {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut data = Vec::<u16>::deserialize(deserializer)?;
        if data.len() > STACK_LIMIT {
            return Err(serde::de::Error::custom(std::format!(
                "stack size exceeds limit: {} > {}",
                data.len(),
                STACK_LIMIT
            )));
        }
        data.reserve(STACK_LIMIT - data.len());
        Ok(Self { data })
    }
} 