use crate::typed_graph::TypedNode;
use revm_primitives::U256;
use super::types::{BinaryU256Inputs, TernaryU256Inputs, U256Output};

/// Node for performing addition operation
pub struct AddNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl AddNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for AddNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).overflowing_add(*self.inputs.1).0;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "AddNode: {} + {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing multiplication operation
pub struct MulNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl MulNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for MulNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).overflowing_mul(*self.inputs.1).0;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "MulNode: {} * {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing subtraction operation
pub struct SubNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl SubNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SubNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).overflowing_sub(*self.inputs.1).0;
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "SubNode: {} - {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing division operation
pub struct DivNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl DivNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for DivNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let b = *self.inputs.1;
            self.outputs.0 = if b == U256::from(0) {
                U256::from(0)
            } else {
                (*self.inputs.0).wrapping_div(b)
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
                "DivNode: {} / {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing modulo operation
pub struct ModNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl ModNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ModNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let b = *self.inputs.1;
            self.outputs.0 = if b == U256::from(0) {
                U256::from(0)
            } else {
                (*self.inputs.0).wrapping_rem(b)
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
                "ModNode: {} % {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing addition modulo operation
pub struct AddModNode {
    inputs: TernaryU256Inputs,
    outputs: U256Output,
}


impl AddModNode {
    pub fn new(input1: *const U256, input2: *const U256, input3: *const U256) -> Self {
        Self {
            inputs: (input1, input2, input3),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for AddModNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let n = *self.inputs.2;
            self.outputs.0 = if n == U256::from(0) {
                U256::from(0)
            } else {
                (*self.inputs.0).add_mod(*self.inputs.1, n)
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
                "AddModNode: ({} + {}) % {} = {}",
                *self.inputs.0, *self.inputs.1, *self.inputs.2, self.outputs.0
            )
        }
    }
}

/// Node for performing multiplication modulo operation
pub struct MulModNode {
    inputs: TernaryU256Inputs,
    outputs: U256Output,
}


impl MulModNode {
    pub fn new(input1: *const U256, input2: *const U256, input3: *const U256) -> Self {
        Self {
            inputs: (input1, input2, input3),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for MulModNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let n = *self.inputs.2;
            self.outputs.0 = if n == U256::from(0) {
                U256::from(0)
            } else {
                (*self.inputs.0).mul_mod(*self.inputs.1, n)
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
                "MulModNode: ({} * {}) % {} = {}",
                *self.inputs.0, *self.inputs.1, *self.inputs.2, self.outputs.0
            )
        }
    }
}

/// Node for performing exponentiation operation
pub struct ExpNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl ExpNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ExpNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            self.outputs.0 = (*self.inputs.0).pow(*self.inputs.1);
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "ExpNode: {} ^ {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing sign extension operation
pub struct SignExtendNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl SignExtendNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SignExtendNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let ext = (*self.inputs.0).as_limbs()[0];
            let word = *self.inputs.1;
            let bit_index = (8 * ext + 7) as usize;
            let bit = word.bit(bit_index);
            let mask = (U256::from(1) << bit_index) - U256::from(1);
            self.outputs.0 = if bit { word | !mask } else { word & mask };
        }
        Ok(())
    }

    fn get_u256_output(&self) -> *const U256 {
        &self.outputs.0
    }
    
    fn print(&self) -> String {
        unsafe {
            format!(
                "SignExtendNode: SignExtend({}, {}) = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

// Helper function to determine the sign of a U256 interpreted as i256
// 0 for positive, 1 for negative
#[inline(always)]
fn i256_sign(val: U256) -> bool {
    val.bit(255)
}

// Helper function for signed division of U256 values
// Adapts logic from revm i256_div
#[inline(always)]
fn i256_div(mut num: U256, mut den: U256) -> U256 {
    let num_sign = i256_sign(num);
    let den_sign = i256_sign(den);

    if num_sign {
        num = num.wrapping_neg();
    }
    if den_sign {
        den = den.wrapping_neg();
    }

    let mut ret = num.wrapping_div(den);

    if num_sign != den_sign {
        ret = ret.wrapping_neg();
    }

    ret
}

// Helper function for signed modulo of U256 values
// Adapts logic from revm i256_mod
#[inline(always)]
fn i256_mod(mut num: U256, den: U256) -> U256 {
    let num_sign = i256_sign(num);

    if num_sign {
        num = num.wrapping_neg();
    }
    // Denominator sign doesn't matter for modulo result sign
    // let den_sign = i256_sign(den);
    // if den_sign {
    //     den = den.wrapping_neg();
    // }

    let mut ret = num.wrapping_rem(den);

    if num_sign {
        ret = ret.wrapping_neg();
    }

    ret
}

/// Node for performing signed division operation
pub struct SdivNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl SdivNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SdivNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let b = *self.inputs.1;
            self.outputs.0 = if b == U256::from(0) {
                U256::from(0)
            } else {
                // Handle the edge case INT_MIN / -1 = INT_MIN (which is U256::MAX / (U256::MAX.wrapping_neg()) )
                let a = *self.inputs.0;
                let int_min = U256::from(1) << 255;
                if a == int_min && b == U256::MAX.wrapping_add(U256::from(1)).wrapping_neg() {
                     // b is -1 in two's complement
                    int_min
                } else {
                   i256_div(a, b)
                }
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
                "SdivNode: {} / {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}

/// Node for performing signed modulo operation
pub struct SmodNode {
    inputs: BinaryU256Inputs,
    outputs: U256Output,
}


impl SmodNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SmodNode {
    #[inline(always)]
    fn execute(&mut self) -> anyhow::Result<()> {
        unsafe {
            let b = *self.inputs.1;
            self.outputs.0 = if b == U256::from(0) {
                U256::from(0)
            } else {
                let a = *self.inputs.0;
                i256_mod(a, b)
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
                "SmodNode: {} % {} = {}",
                *self.inputs.0, *self.inputs.1, self.outputs.0
            )
        }
    }
}
