use crate::typed_graph::TypedNode;
use revm_primitives::U256;
use std::cmp::Ordering;
use super::types::{BinaryU256Inputs, UnaryU256Inputs, U256Output};

/// Node for performing less than operation
pub struct LtNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl LtNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for LtNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = if *self.inputs.0 < *self.inputs.1 {
                U256::from(1)
            } else {
                U256::from(0)
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "LtNode: {} < {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing greater than operation
pub struct GtNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl GtNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for GtNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = if *self.inputs.0 > *self.inputs.1 {
                U256::from(1)
            } else {
                U256::from(0)
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "GtNode: {} > {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing equality comparison
pub struct EqNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl EqNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for EqNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = if *self.inputs.0 == *self.inputs.1 {
                U256::from(1)
            } else {
                U256::from(0)
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "EqNode: {} == {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing is zero check
pub struct IsZeroNode {
    inputs: UnaryU256Inputs,
    outputs: U256Output,
}


impl IsZeroNode {
    pub fn new(input: *const U256) -> Self {
        Self {
            inputs: (input,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for IsZeroNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = if (*self.inputs.0).is_zero() {
                U256::from(1)
            } else {
                U256::from(0)
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "IsZeroNode: {} == 0 = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

/// Node for performing bitwise AND operation
pub struct AndNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl AndNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for AndNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = *self.inputs.0 & *self.inputs.1;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "AndNode: {} & {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing bitwise OR operation
pub struct OrNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl OrNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for OrNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = *self.inputs.0 | *self.inputs.1;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "OrNode: {} | {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing bitwise XOR operation
pub struct XorNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl XorNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for XorNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = *self.inputs.0 ^ *self.inputs.1;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "XorNode: {} ^ {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing bitwise NOT operation
pub struct NotNode {
    inputs: UnaryU256Inputs,
    outputs: U256Output,
}


impl NotNode {
    pub fn new(input: *const U256) -> Self {
        Self {
            inputs: (input,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for NotNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = !*self.inputs.0;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "NotNode: ~{} = {}",
                *self.inputs.0, self.outputs.0
            )
        }
    }
}

/// Node for performing byte extraction operation
pub struct ByteNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl ByteNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ByteNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let index = (*self.inputs.0).as_limbs()[0] as usize;
            let word = *self.inputs.1;

            self.outputs.0 = if index < 32 {
                U256::from(word.byte(31 - index))
            } else {
                U256::ZERO
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "ByteNode: byte({}, {}) = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing left shift operation
pub struct ShlNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl ShlNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ShlNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let shift = *self.inputs.0;
            let value = *self.inputs.1;

            self.outputs.0 = if shift >= U256::from(256) {
                U256::ZERO
            } else {
                value << shift.as_limbs()[0] as usize
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "ShlNode: {} << {} = {}",
                *self.inputs.1, *self.inputs.0, self.outputs.0
            )
        }
    }
}

/// Node for performing logical right shift operation
pub struct ShrNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl ShrNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ShrNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let shift = *self.inputs.0;
            let value = *self.inputs.1;

            self.outputs.0 = if shift >= U256::from(256) {
                U256::ZERO
            } else {
                value >> shift.as_limbs()[0] as usize
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "ShrNode: {} >> {} = {}",
                *self.inputs.1, *self.inputs.0, self.outputs.0
            )
        }
    }
}

/// Node for performing arithmetic right shift operation
pub struct SarNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl SarNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SarNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let shift = *self.inputs.0;
            let value = *self.inputs.1;
            let shift_amount = shift.as_limbs()[0] as usize;

            self.outputs.0 = if shift_amount < 256 {
                value.arithmetic_shr(shift_amount)
            } else if value.bit(255) {
                U256::MAX
            } else {
                U256::ZERO
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "SarNode: {} >>> {} = {}",
                *self.inputs.1, *self.inputs.0, self.outputs.0
            )
        }
    }
}

/// Node for performing signed less than operation
pub struct SltNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl SltNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

/// Compare two U256 values as signed 256-bit integers.
#[inline]
fn i256_cmp(a: &U256, b: &U256) -> Ordering {
    let a_neg = a.bit(255);
    let b_neg = b.bit(255);

    match (a_neg, b_neg) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => a.cmp(b),
    }
}

impl TypedNode for SltNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = if i256_cmp(&*self.inputs.0, &*self.inputs.1) == Ordering::Less {
                U256::from(1)
            } else {
                U256::from(0)
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "SltNode: (signed){} < (signed){} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing signed greater than operation
pub struct SgtNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl SgtNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SgtNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = if i256_cmp(&*self.inputs.0, &*self.inputs.1) == Ordering::Greater {
                U256::from(1)
            } else {
                U256::from(0)
            };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }

    fn print(&self) -> String {
        unsafe {
            format!(
                "SgtNode: (signed){} > (signed){} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}
