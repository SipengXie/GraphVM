use crate::typed_graph::{HasInputType, HasOutputType, TypedNode};
use revm_primitives::U256;
use std::cmp::Ordering;

/// Node for performing less than operation
pub struct LtNode {
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for LtNode {}
impl HasOutputType<(U256,)> for LtNode {}

impl LtNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for LtNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for GtNode {}
impl HasOutputType<(U256,)> for GtNode {}

impl GtNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for GtNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for EqNode {}
impl HasOutputType<(U256,)> for EqNode {}

impl EqNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for EqNode {
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
    inputs: (*const U256,),
    outputs: (U256,),
}

impl HasInputType<(*const U256,)> for IsZeroNode {}
impl HasOutputType<(U256,)> for IsZeroNode {}

impl IsZeroNode {
    pub fn new(input: *const U256) -> Self {
        Self {
            inputs: (input,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for IsZeroNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for AndNode {}
impl HasOutputType<(U256,)> for AndNode {}

impl AndNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for AndNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for OrNode {}
impl HasOutputType<(U256,)> for OrNode {}

impl OrNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for OrNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for XorNode {}
impl HasOutputType<(U256,)> for XorNode {}

impl XorNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for XorNode {
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
    inputs: (*const U256,),
    outputs: (U256,),
}

impl HasInputType<(*const U256,)> for NotNode {}
impl HasOutputType<(U256,)> for NotNode {}

impl NotNode {
    pub fn new(input: *const U256) -> Self {
        Self {
            inputs: (input,),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for NotNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for ByteNode {}
impl HasOutputType<(U256,)> for ByteNode {}

impl ByteNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ByteNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for ShlNode {}
impl HasOutputType<(U256,)> for ShlNode {}

impl ShlNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ShlNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for ShrNode {}
impl HasOutputType<(U256,)> for ShrNode {}

impl ShrNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ShrNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for SarNode {}
impl HasOutputType<(U256,)> for SarNode {}

impl SarNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SarNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for SltNode {}
impl HasOutputType<(U256,)> for SltNode {}

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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for SgtNode {}
impl HasOutputType<(U256,)> for SgtNode {}

impl SgtNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SgtNode {
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
