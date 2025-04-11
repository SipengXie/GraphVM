use revm_primitives::U256;
use crate::typed_graph::{TypedNode, HasInputType, HasOutputType};

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
    
    fn get_u256_output(&self, index: usize) -> Option<*const U256> {
        match index {
            0 => Some(&self.outputs.0),
            _ => None,
        }
    }
} 