use std::cmp::Ordering;
use revm_primitives::db::DatabaseRef;
use super::i256::i256_cmp;
use revm_primitives::{Spec, U256};
use revm_ssa::{SSAInput, SSALogEntry, SSAOutput};
use crate::{ExecutionContext, ExecutionError, Result, get_ssa_output_stack_or_const, SsaGraph};
use crate::as_usize_saturated;

impl<'a, DB: DatabaseRef + Send + Sync, SPEC: Spec> ExecutionContext<'a, DB, SPEC> {
    /// Execute LT operation
    #[inline]
    pub fn execute_lt(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(if a < b { U256::from(1) } else { U256::from(0) });
        Ok(())
    }

    /// Execute GT operation
    #[inline]
    pub fn execute_gt(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(if a > b { U256::from(1) } else { U256::from(0) });
        Ok(())
    }

    /// Execute SLT operation
    #[inline]
    pub fn execute_slt(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(if i256_cmp(&a, &b) == Ordering::Less { U256::from(1) } else { U256::from(0) });
        Ok(())
    }

    /// Execute SGT operation
    #[inline]
    pub fn execute_sgt(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(if i256_cmp(&a, &b) == Ordering::Greater { U256::from(1) } else { U256::from(0) });
        Ok(())
    }

    /// Execute EQ operation
    #[inline]
    pub fn execute_eq(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(if a == b { U256::from(1) } else { U256::from(0) });
        Ok(())
    }

    /// Execute ISZERO operation
    #[inline]
    pub fn execute_iszero(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        node.outputs[0] = SSAOutput::Stack(if a.is_zero() { U256::from(1) } else { U256::from(0) });
        Ok(())
    }

    /// Execute AND operation
    #[inline]
    pub fn execute_and(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(a & b);
        Ok(())
    }

    /// Execute OR operation
    #[inline]
    pub fn execute_or(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(a | b);
        Ok(())
    }

    /// Execute XOR operation
    #[inline]
    pub fn execute_xor(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(a ^ b);
        Ok(())
    }

    /// Execute NOT operation
    #[inline]
    pub fn execute_not(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        node.outputs[0] = SSAOutput::Stack(!a);
        Ok(())
    }

    /// Execute BYTE operation
    #[inline]
    pub fn execute_byte(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let index = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let word = get_ssa_output_stack_or_const!(graph, node.inputs[1]);

        let index = as_usize_saturated!(index);
        let result = if index < 32 {
            U256::from(word.byte(31 - index))
        } else {
            U256::ZERO
        };

        node.outputs[0] = SSAOutput::Stack(result);
        Ok(())
    }

    /// Execute SHL operation (left shift)
    #[inline]
    pub fn execute_shl(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let shift = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[1]);

        node.outputs[0] = if shift >= U256::from(256) {
            SSAOutput::Stack(U256::from(0))
        } else {
            let shift_amount = as_usize_saturated!(shift);
            SSAOutput::Stack(value << shift_amount)
        };
        Ok(())
    }

    /// Execute SHR operation (logical right shift)
    #[inline]
    pub fn execute_shr(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let shift = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[1]);

        node.outputs[0] = if shift >= U256::from(256) {
            SSAOutput::Stack(U256::from(0))
        } else {
            let shift_amount = as_usize_saturated!(shift);
            SSAOutput::Stack(value >> shift_amount)
        };
        Ok(())
    }

    /// Execute SAR operation (arithmetic right shift)
    #[inline]
    pub fn execute_sar(&self, node: &mut SSALogEntry, graph: & SsaGraph) -> Result<()> {
        let shift = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
        let value = get_ssa_output_stack_or_const!(graph, node.inputs[1]);

        let shift_amount = as_usize_saturated!(shift);
        let result = if shift_amount < 256 {
            value.arithmetic_shr(shift_amount)
        } else if value.bit(255) {
            U256::MAX
        } else {
            U256::ZERO
        };
        
        node.outputs[0] = SSAOutput::Stack(result);
        Ok(())
    }
}
