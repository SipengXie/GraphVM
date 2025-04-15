use crate::typed_graph::{HasInputType, HasOutputType, TypedNode};
use revm_primitives::U256;

/// Node for performing addition operation
pub struct AddNode {
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for AddNode {}
impl HasOutputType<(U256,)> for AddNode {}

impl AddNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for AddNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for MulNode {}
impl HasOutputType<(U256,)> for MulNode {}

impl MulNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for MulNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for SubNode {}
impl HasOutputType<(U256,)> for SubNode {}

impl SubNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SubNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for DivNode {}
impl HasOutputType<(U256,)> for DivNode {}

impl DivNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for DivNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for ModNode {}
impl HasOutputType<(U256,)> for ModNode {}

impl ModNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ModNode {
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
    inputs: (*const U256, *const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256, *const U256)> for AddModNode {}
impl HasOutputType<(U256,)> for AddModNode {}

impl AddModNode {
    pub fn new(input1: *const U256, input2: *const U256, input3: *const U256) -> Self {
        Self {
            inputs: (input1, input2, input3),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for AddModNode {
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
    inputs: (*const U256, *const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256, *const U256)> for MulModNode {}
impl HasOutputType<(U256,)> for MulModNode {}

impl MulModNode {
    pub fn new(input1: *const U256, input2: *const U256, input3: *const U256) -> Self {
        Self {
            inputs: (input1, input2, input3),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for MulModNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for ExpNode {}
impl HasOutputType<(U256,)> for ExpNode {}

impl ExpNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for ExpNode {
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
    inputs: (*const U256, *const U256),
    outputs: (U256,),
}

impl HasInputType<(*const U256, *const U256)> for SignExtendNode {}
impl HasOutputType<(U256,)> for SignExtendNode {}

impl SignExtendNode {
    pub fn new(input1: *const U256, input2: *const U256) -> Self {
        Self {
            inputs: (input1, input2),
            outputs: (U256::ZERO,),
        }
    }
}

impl TypedNode for SignExtendNode {
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
