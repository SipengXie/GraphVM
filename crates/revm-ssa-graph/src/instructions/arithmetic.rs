use revm_primitives::{Spec, U256, db::DatabaseRef};
use revm_ssa::SSAOutput;
use crate::{ExecutionContext, ExecutionError, Result, match_ssa_output_stack_or_const};

use super::i256::{i256_div, i256_mod};

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute addition operation
    #[inline]
    pub fn execute_add(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "ADD requires exactly 2 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        Ok(vec![SSAOutput::Stack(a.overflowing_add(*b).0)])
    }

    /// Execute multiplication operation
    #[inline]
    pub fn execute_mul(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "MUL requires exactly 2 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        Ok(vec![SSAOutput::Stack(a.overflowing_mul(*b).0)])
    }

    /// Execute subtraction operation
    #[inline]
    pub fn execute_sub(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SUB requires exactly 2 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        Ok(vec![SSAOutput::Stack(a.overflowing_sub(*b).0)])
    }

    /// Execute division operation
    #[inline]
    pub fn execute_div(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "DIV requires exactly 2 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        if *b == U256::from(0) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            Ok(vec![SSAOutput::Stack(a.wrapping_div(*b))])
        }
    }

    /// Execute modulo operation
    #[inline]
    pub fn execute_mod(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "MOD requires exactly 2 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        if *b == U256::from(0) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            Ok(vec![SSAOutput::Stack(a.wrapping_rem(*b))])
        }
    }

    /// Execute addition modulo operation
    #[inline]
    pub fn execute_addmod(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "ADDMOD requires exactly 3 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let n = match_ssa_output_stack_or_const!(&inputs[2], "Third");

        if *n == U256::from(0) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            Ok(vec![SSAOutput::Stack(a.add_mod(*b, *n))])
        }
    }

    /// Execute multiplication modulo operation
    #[inline]
    pub fn execute_mulmod(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 3 {
            return Err(ExecutionError::ExecutionError(
                "MULMOD requires exactly 3 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");
        let n = match_ssa_output_stack_or_const!(&inputs[2], "Third");

        if *n == U256::from(0) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            Ok(vec![SSAOutput::Stack(a.mul_mod(*b, *n))])
        }
    }

    /// Execute signed division operation
    #[inline]
    pub fn execute_sdiv(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SDIV requires exactly 2 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        if *b == U256::from(0) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            Ok(vec![SSAOutput::Stack(i256_div(*a, *b))])
        }
    }

    /// Execute signed modulo operation
    #[inline]
    pub fn execute_smod(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SMOD requires exactly 2 operands".to_string()
            ));
        }

        let a = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let b = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        if *b == U256::from(0) {
            Ok(vec![SSAOutput::Stack(U256::from(0))])
        } else {
            Ok(vec![SSAOutput::Stack(i256_mod(*a, *b))])
        }
    }

    /// Execute exponentiation operation
    #[inline]
    pub fn execute_exp(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "EXP requires exactly 2 operands".to_string()
            ));
        }

        let base = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let exponent = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        Ok(vec![SSAOutput::Stack(base.pow(*exponent))])
    }

    /// Execute sign extension operation
    #[inline]
    pub fn execute_signextend(&self, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        if inputs.len() != 2 {
            return Err(ExecutionError::ExecutionError(
                "SIGNEXTEND requires exactly 2 operands".to_string()
            ));
        }

        let ext = match_ssa_output_stack_or_const!(&inputs[0], "First");
        let word = match_ssa_output_stack_or_const!(&inputs[1], "Second");

        // Completely follow the interpreter's logic
        let ext = ext.as_limbs()[0];
        let bit_index = (8 * ext + 7) as usize;
        let bit = word.bit(bit_index);
        let mask = (U256::from(1) << bit_index) - U256::from(1);
        let value = if bit { 
            *word | !mask 
        } else { 
            *word & mask 
        };

        Ok(vec![SSAOutput::Stack(value)])
    }
}
