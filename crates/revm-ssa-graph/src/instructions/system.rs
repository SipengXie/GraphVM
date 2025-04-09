use std::cmp::min;

use super::memory::calc_mem_size;
use super::utils::as_usize_saturated;
use super::{get_contract_env, get_memory, get_return_data_buffer};
use crate::{get_ssa_output_stack_or_const, ExecutionContext, ExecutionError, Result, SsaGraph};
use revm_primitives::db::DatabaseRef;
use revm_primitives::{B256, U256};
use revm_ssa::{SSAInput, SSALogEntry, SSAOutput};

/// Execute GAS operation
/// ! For a formal implementation, we should consider all front-loaded dynamic gas commands
#[inline(always)]
pub fn execute_gas<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let gas = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    node.outputs[0] = SSAOutput::Stack(gas);
    Ok(())
}

/// Execute ADDRESS operation
#[inline(always)]
pub fn execute_address<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let contract_env = get_contract_env!(graph, node.inputs[0]);
    node.outputs[0] = SSAOutput::Stack(contract_env.frame_input.target_address.into_word().into());
    Ok(())
}

/// Execute CALLER operation
#[inline(always)]
pub fn execute_caller<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let contract_env = get_contract_env!(graph, node.inputs[0]);
    node.outputs[0] = SSAOutput::Stack(contract_env.frame_input.caller.into_word().into());
    Ok(())
}

/// Execute CODESIZE operation
#[inline(always)]
pub fn execute_codesize<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let code_length = get_contract_env!(graph, node.inputs[0]).bytecode.len();
    node.outputs[0] = SSAOutput::Stack(U256::from(code_length));
    Ok(())
}

/// Execute CODECOPY operation
#[inline(always)]
pub fn execute_codecopy<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let memory_offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let code_offset = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    let len = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
    let code = get_contract_env!(graph, node.inputs[3])
        .bytecode
        .original_byte_slice();
    let memory_offset = as_usize_saturated!(memory_offset);
    let code_offset = as_usize_saturated!(code_offset);
    let len = as_usize_saturated!(len);
    let new_size = calc_mem_size(memory_offset, len);

    // Handle the case when len is 0
    let padded_code_slice = if len == 0 {
        Vec::new()
    } else {
        // Prevent code from being too short
        let code_end = min(code_offset + len, code.len());
        let code_slice = &code[code_offset..code_end];
        // Pad code to len length
        let mut padded_slice = vec![0u8; len];
        padded_slice[..code_slice.len()].copy_from_slice(&code_slice);
        padded_slice
    };

    node.outputs[0] = SSAOutput::Memory(padded_code_slice.into());
    if new_size > _context.memory_size() {
        if node.outputs.len() < 2 {
            node.outputs.push(SSAOutput::MemorySize(new_size));
        } else {
            node.outputs[1] = SSAOutput::MemorySize(new_size);
        }
        _context.set_memory_size(new_size);
    }
    Ok(())
}

/// Execute CALLDATALOAD operation
#[inline(always)]
pub fn execute_calldataload<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let call_data = get_contract_env!(graph, node.inputs[1])
        .frame_input
        .input
        .clone();
    let offset = as_usize_saturated!(offset);
    let mut word = [0u8; 32];
    if offset < call_data.len() {
        let length = 32.min(call_data.len() - offset);
        word[..length].copy_from_slice(&call_data[offset..offset + length]);
    }
    node.outputs[0] = SSAOutput::Stack(B256::from_slice(&word).into());
    Ok(())
}

/// Execute CALLDATASIZE operation
#[inline(always)]
pub fn execute_calldatasize<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let input = get_contract_env!(graph, node.inputs[0])
        .frame_input
        .input
        .clone();
    node.outputs[0] = SSAOutput::Stack(U256::from(input.len()));
    Ok(())
}

/// Execute CALLVALUE operation
#[inline(always)]
pub fn execute_callvalue<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let call_value = get_contract_env!(graph, node.inputs[0])
        .frame_input
        .transfer_value;
    node.outputs[0] = SSAOutput::Stack(call_value);
    Ok(())
}

/// Execute CALLDATACOPY operation
#[inline(always)]
pub fn execute_calldatacopy<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let memory_offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let data_offset = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    let len = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
    let call_data = get_contract_env!(graph, node.inputs[3])
        .frame_input
        .input
        .clone();

    let memory_offset = as_usize_saturated!(memory_offset);
    let data_offset = as_usize_saturated!(data_offset);
    let len = as_usize_saturated!(len);
    let new_size = calc_mem_size(memory_offset, len);

    let padded_data_slice = if len == 0 {
        Vec::new()
    } else {
        // Prevent data from being too short
        let data_end = min(data_offset + len, call_data.len());
        let data_slice = call_data.slice(data_offset..data_end);
        // Pad data to len length
        let mut padded_data = vec![0u8; len];
        padded_data[..data_slice.len()].copy_from_slice(&data_slice);
        padded_data
    };

    node.outputs[0] = SSAOutput::Memory(padded_data_slice.into());
    if new_size > _context.memory_size() {
        if node.outputs.len() < 2 {
            node.outputs.push(SSAOutput::MemorySize(new_size));
        } else {
            node.outputs[1] = SSAOutput::MemorySize(new_size);
        }
        _context.set_memory_size(new_size);
    }
    Ok(())
}

/// Execute RETURNDATASIZE operation
#[inline(always)]
pub fn execute_returndatasize<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let return_data = get_return_data_buffer!(graph, node.inputs[0]);
    node.outputs[0] = SSAOutput::Stack(U256::from(return_data.len()));
    Ok(())
}

/// Execute RETURNDATACOPY operation
#[inline(always)]
pub fn execute_returndatacopy<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let memory_offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let data_offset = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    let len = get_ssa_output_stack_or_const!(graph, node.inputs[2]);
    let return_data = get_return_data_buffer!(graph, node.inputs[3]);

    let memory_offset = as_usize_saturated!(memory_offset);
    let data_offset = as_usize_saturated!(data_offset);
    let len = as_usize_saturated!(len);
    let new_size = calc_mem_size(memory_offset, len);

    // When len is 0, return an empty vector
    let padded_data_slice = if len == 0 {
        Vec::new()
    } else {
        // Prevent data from being too short
        let data_end = min(data_offset + len, return_data.len());
        let data_slice = return_data.slice(data_offset..data_end);
        // Pad data to len length
        let mut padded_data = vec![0u8; len];
        padded_data[..data_slice.len()].copy_from_slice(&data_slice);
        padded_data
    };

    node.outputs[0] = SSAOutput::Memory(padded_data_slice.into());
    if new_size > _context.memory_size() {
        if node.outputs.len() < 2 {
            node.outputs.push(SSAOutput::MemorySize(new_size));
        } else {
            node.outputs[1] = SSAOutput::MemorySize(new_size);
        }
        _context.set_memory_size(new_size);
    }
    Ok(())
}

/// Execute RETURNDATALOAD operation
#[inline(always)]
pub fn execute_returndataload<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let return_data = get_return_data_buffer!(graph, node.inputs[1]);
    let offset = as_usize_saturated!(offset);
    let mut word = [0u8; 32];
    if offset < return_data.len() {
        let length = 32.min(return_data.len() - offset);
        word[..length].copy_from_slice(&return_data[offset..offset + length]);
    }
    node.outputs[0] = SSAOutput::Stack(B256::from(word).into());
    Ok(())
}

/// Execute KECCAK256 operation
#[inline(always)]
pub fn execute_keccak256<DB: DatabaseRef + Send + Sync>(
    _context: &mut ExecutionContext<DB>,
    node: &mut SSALogEntry,
    graph: &SsaGraph,
) -> Result<()> {
    let offset = get_ssa_output_stack_or_const!(graph, node.inputs[0]);
    let len = get_ssa_output_stack_or_const!(graph, node.inputs[1]);
    let data = get_memory!(graph, &node.inputs[2]);
    let offset = as_usize_saturated!(offset);
    let len = as_usize_saturated!(len);

    // Calculate new memory size
    let new_size = calc_mem_size(offset, len);
    let data_slice = &data[..len];
    let hash = revm_primitives::keccak256(data_slice);
    node.outputs[0] = SSAOutput::Stack(hash.into());

    // If memory size changes, add MemorySize output
    if new_size > _context.memory_size() {
        if node.outputs.len() < 2 {
            node.outputs.push(SSAOutput::MemorySize(new_size));
        } else {
            node.outputs[1] = SSAOutput::MemorySize(new_size);
        }
        _context.set_memory_size(new_size);
    }
    Ok(())
}
