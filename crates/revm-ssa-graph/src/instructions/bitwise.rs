use std::cmp::Ordering;
use revm_primitives::db::DatabaseRef;
use super::i256::i256_cmp;
use revm_primitives::{Spec, U256};
use revm_ssa::{SSAInput, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result};
use super::utils::as_usize_saturated;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute LT operation
    #[inline]
    pub fn execute_lt(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "LT requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(a < b))])
    }

    /// Execute GT operation
    #[inline]
    pub fn execute_gt(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "GT requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(a > b))])
    }

    /// Execute SLT operation
    #[inline]
    pub fn execute_slt(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SLT requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(i256_cmp(a, b) == Ordering::Less))])
    }

    /// Execute SGT operation
    #[inline]
    pub fn execute_sgt(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SGT requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(i256_cmp(a, b) == Ordering::Greater))])
    }

    /// Execute EQ operation
    #[inline]
    pub fn execute_eq(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "EQ requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(a == b))])
    }

    /// Execute ISZERO operation
    #[inline]
    pub fn execute_iszero(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "ISZERO requires exactly 1 operand".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(U256::from(a.is_zero()))])
    }

    /// Execute AND operation
    #[inline]
    pub fn execute_and(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "AND requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(a & b)])
    }

    /// Execute OR operation
    #[inline]
    pub fn execute_or(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "OR requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(a | b)])
    }

    /// Execute XOR operation
    #[inline]
    pub fn execute_xor(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "XOR requires exactly 2 operands".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let b = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(a ^ b)])
    }

    /// Execute NOT operation
    #[inline]
    pub fn execute_not(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 1 {
            return Err(ExecutionError::ExecutionError(
                "NOT requires exactly 1 operand".to_string()
            ));
        }

        let a = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Operand must be Stack value".to_string()
            )),
        };

        Ok(vec![SSAOutput::Stack(!a)])
    }

    /// Execute BYTE operation
    #[inline]
    pub fn execute_byte(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "BYTE requires exactly 2 operands".to_string()
            ));
        }

        let index = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand must be Stack value".to_string()
            )),
        };
        let word = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand must be Stack value".to_string()
            )),
        };

        let index = as_usize_saturated(*index);
        let result = if index < 32 {
            U256::from(word.byte(31 - index))
        } else {
            U256::ZERO
        };

        Ok(vec![SSAOutput::Stack(result)])
    }

    /// Execute SHL operation (left shift)
    #[inline]
    pub fn execute_shl(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SHL requires exactly 2 operands".to_string()
            ));
        }

        let shift = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand (shift) must be Stack value".to_string()
            )),
        };
        let value = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand (value) must be Stack value".to_string()
            )),
        };

        if *shift >= U256::from(256) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            let shift_amount = as_usize_saturated(*shift);
            Ok(vec![SSAOutput::Stack(*value << shift_amount)])
        }
    }

    /// Execute SHR operation (logical right shift)
    #[inline]
    pub fn execute_shr(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SHR requires exactly 2 operands".to_string()
            ));
        }

        let shift = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand (shift) must be Stack value".to_string()
            )),
        };
        let value = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand (value) must be Stack value".to_string()
            )),
        };

        if *shift >= U256::from(256) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            let shift_amount = as_usize_saturated(*shift);
            Ok(vec![SSAOutput::Stack(*value >> shift_amount)])
        }
    }

    /// Execute SAR operation (arithmetic right shift)
    #[inline]
    pub fn execute_sar(&self, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SAR requires exactly 2 operands".to_string()
            ));
        }
    
        let shift = match &inputs[0] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "First operand (shift) must be Stack value".to_string()
            )),
        };
        let value = match &inputs[1] {
            SSAInput::Stack { value, .. } => value,
            _ => return Err(ExecutionError::ExecutionError(
                "Second operand (value) must be Stack value".to_string()
            )),
        };
    
        let shift_amount = as_usize_saturated(*shift);
        let result = if shift_amount < 256 {
            value.arithmetic_shr(shift_amount)
        } else if value.bit(255) {
            U256::MAX
        } else {
            U256::ZERO
        };
        
        Ok(vec![SSAOutput::Stack(result)])
    }
}
