use crate::{get_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result, SsaGraph};
use revm_primitives::{db::DatabaseRef, U256};
use revm_ssa::{SSAInput, SSALogEntry, SSAOutput};

use super::i256::{i256_div, i256_mod};

/// Execute addition operation
#[inline(always)]
pub fn execute_add<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = SSAOutput::Stack(a.overflowing_add(b).0);
    Ok(())
}

/// Execute multiplication operation
#[inline(always)]
pub fn execute_mul<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = SSAOutput::Stack(a.overflowing_mul(b).0);
    Ok(())
}

/// Execute subtraction operation
#[inline(always)]
pub fn execute_sub<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = SSAOutput::Stack(a.overflowing_sub(b).0);
    Ok(())
}

/// Execute division operation
#[inline(always)]
pub fn execute_div<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = if b == U256::from(0) {
        SSAOutput::Stack(U256::from(0))
    } else {
        SSAOutput::Stack(a.wrapping_div(b))
    };
    Ok(())
}

/// Execute modulo operation
#[inline(always)]
pub fn execute_mod<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = if b == U256::from(0) {
        SSAOutput::Stack(U256::from(0))
    } else {
        SSAOutput::Stack(a.wrapping_rem(b))
    };
    Ok(())
}

/// Execute addition modulo operation
#[inline(always)]
pub fn execute_addmod<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    let n = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
    node.outputs[0] = if n == U256::from(0) {
        SSAOutput::Stack(U256::from(0))
    } else {
        SSAOutput::Stack(a.add_mod(b, n))
    };
    Ok(())
}

/// Execute multiplication modulo operation
#[inline(always)]
pub fn execute_mulmod<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    let n = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
    node.outputs[0] = if n == U256::from(0) {
        SSAOutput::Stack(U256::from(0))
    } else {
        SSAOutput::Stack(a.mul_mod(b, n))
    };
    Ok(())
}

/// Execute signed division operation
#[inline(always)]
pub fn execute_sdiv<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = if b == U256::from(0) {
        SSAOutput::Stack(U256::from(0))
    } else {
        SSAOutput::Stack(i256_div(a, b))
    };
    Ok(())
}

/// Execute signed modulo operation
#[inline(always)]
pub fn execute_smod<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let a = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let b = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = if b == U256::from(0) {
        SSAOutput::Stack(U256::from(0))
    } else {
        SSAOutput::Stack(i256_mod(a, b))
    };
    Ok(())
}

/// Execute exponentiation operation
#[inline(always)]
pub fn execute_exp<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let base = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let exponent = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    node.outputs[0] = SSAOutput::Stack(base.pow(exponent));
    Ok(())
}

/// Execute sign extension operation
#[inline(always)]
pub fn execute_signextend<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let ext = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let word = get_ssa_output_stack_or_const!(graph, node.inputs[1]);

    let ext = ext.as_limbs()[0];
    let bit_index = (8 * ext + 7) as usize;
    let bit = word.bit(bit_index);
    let mask = (U256::from(1) << bit_index) - U256::from(1);
    let value = if bit { word | !mask } else { word & mask };

    node.outputs[0] = SSAOutput::Stack(value);
    Ok(())
}
