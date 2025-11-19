//! Accurate deep memory size calculation for SSA graphs.
//!
//! This module provides functions to calculate the actual heap memory consumption
//! of SSA graphs and their components, accounting for:
//! - Vec<T> capacity and nested heap allocations
//! - Box<T> allocations
//! - Bytes (heap-allocated byte buffers)
//! - Arc<T> shared allocations
//! - Complex nested structures like Vec<Vec<T>>

use crate::graph::SsaGraph;
use revm_primitives::{AccountInfo, Bytecode, Bytes, Eof, Log};
use revm_ssa::{
    ContractEnv, MemoryDep, SSACallInput, SSACallOutcome, SSACreateInput, SSACreateOutcome,
    SSAInput, SSAInterpreterResult, SSALogEntry, SSAOutput, StorageKey, StorageValue,
};
use std::mem::size_of;

/// Calculate the deep memory size of a Bytes object.
/// Bytes is a reference-counted byte buffer, we count the full capacity.
#[inline]
fn bytes_deep_size(bytes: &Bytes) -> usize {
    // Bytes uses Arc internally, capacity gives the allocated size
    bytes.len() // Bytes doesn't expose capacity, but len() is the allocated size
}

/// Calculate the deep memory size of a Vec<T> where T is a primitive (no heap allocations).
#[inline]
fn vec_primitive_deep_size<T>(vec: &Vec<T>) -> usize {
    vec.capacity() * size_of::<T>()
}

/// Calculate the deep memory size of a Vec<T> where each element may have heap allocations.
#[inline]
fn vec_deep_size<T, F>(vec: &Vec<T>, elem_deep_size: F) -> usize
where
    F: Fn(&T) -> usize,
{
    let vec_overhead = vec.capacity() * size_of::<T>();
    let elements_heap_size: usize = vec.iter().map(elem_deep_size).sum();
    vec_overhead + elements_heap_size
}

/// Calculate the deep memory size of a Box<T>.
#[inline]
fn box_deep_size<T, F>(boxed: &Box<T>, content_deep_size: F) -> usize
where
    F: Fn(&T) -> usize,
{
    size_of::<T>() + content_deep_size(&**boxed)
}

/// Calculate deep size of StorageKey.
fn storage_key_deep_size(_key: &StorageKey) -> usize {
    // StorageKey variants are all stack-allocated (Address + U256, Address only, etc.)
    0
}

/// Calculate deep size of AccountInfo.
fn account_info_deep_size(info: &AccountInfo) -> usize {
    // AccountInfo has: balance (U256), nonce (u64), code_hash (B256), code (Option<Bytecode>)
    match &info.code {
        Some(bytecode) => bytecode_deep_size(bytecode),
        None => 0,
    }
}

/// Calculate deep size of StorageValue.
fn storage_value_deep_size(value: &StorageValue) -> usize {
    match value {
        StorageValue::AccountInfo(info) => account_info_deep_size(info),
        StorageValue::AccountStatus(_) | StorageValue::Slot(_) => 0,
    }
}

/// Calculate deep size of Bytecode enum.
fn bytecode_deep_size(bytecode: &Bytecode) -> usize {
    match bytecode {
        Bytecode::LegacyRaw(bytes) => bytes_deep_size(bytes),
        Bytecode::LegacyAnalyzed(analyzed) => {
            // LegacyAnalyzedBytecode has:
            // - bytecode: Bytes
            // - original_len: usize
            // - jump_table: JumpTable (Arc<BitVec<u8>>)
            let bytecode_size = bytes_deep_size(analyzed.bytecode());
            let jump_table_size = analyzed.jump_table().as_slice().len();
            bytecode_size + jump_table_size
        }
        Bytecode::Eof(eof) => {
            // Eof has: header (stack), body (complex), raw (Bytes)
            let raw_size = bytes_deep_size(&eof.raw);
            let body_size = eof_body_deep_size(eof);
            raw_size + body_size
        }
        Bytecode::Eip7702(eip7702) => {
            // Eip7702Bytecode has: delegated_address (Address), version (u8), raw (Bytes)
            bytes_deep_size(&eip7702.raw)
        }
    }
}

/// Calculate deep size of EofBody.
fn eof_body_deep_size(eof: &Eof) -> usize {
    // EofBody has:
    // - types_section: Vec<TypesSection> (TypesSection is small struct, ~8 bytes)
    // - code_section: Vec<Bytes>
    // - container_section: Vec<Bytes>
    // - data_section: Bytes
    let types_size = vec_primitive_deep_size(&eof.body.types_section);
    let code_size = vec_deep_size(&eof.body.code_section, bytes_deep_size);
    let container_size = vec_deep_size(&eof.body.container_section, bytes_deep_size);
    let data_size = bytes_deep_size(&eof.body.data_section);
    types_size + code_size + container_size + data_size
}

/// Calculate deep size of SSAInterpreterResult.
fn ssa_interpreter_result_deep_size(result: &SSAInterpreterResult) -> usize {
    // SSAInterpreterResult has: result (enum, 1 byte), output (Bytes)
    bytes_deep_size(&result.output)
}

/// Calculate deep size of SSACallInput.
fn ssa_call_input_deep_size(input: &SSACallInput) -> usize {
    // SSACallInput has one Bytes field: input
    bytes_deep_size(&input.input)
}

/// Calculate deep size of SSACallOutcome.
fn ssa_call_outcome_deep_size(outcome: &SSACallOutcome) -> usize {
    // SSACallOutcome has: result (SSAInterpreterResult), ret_range (Range<usize>)
    ssa_interpreter_result_deep_size(&outcome.result)
}

/// Calculate deep size of SSACreateInput.
fn ssa_create_input_deep_size(input: &SSACreateInput) -> usize {
    // SSACreateInput has: caller (Address), value (U256), init_code (Bytes), scheme (enum)
    bytes_deep_size(&input.init_code)
}

/// Calculate deep size of SSACreateOutcome.
fn ssa_create_outcome_deep_size(outcome: &SSACreateOutcome) -> usize {
    // SSACreateOutcome has: result (SSAInterpreterResult), address (Option<Address>)
    ssa_interpreter_result_deep_size(&outcome.result)
}

/// Calculate deep size of ContractEnv.
fn contract_env_deep_size(env: &ContractEnv) -> usize {
    // ContractEnv has:
    // - input: Bytes
    // - bytecode: Bytecode (complex)
    // - hash: Option<B256> (stack)
    // - target_address, bytecode_address, caller: Address (stack)
    // - call_value: U256 (stack)
    let input_size = bytes_deep_size(&env.input);
    let bytecode_size = bytecode_deep_size(&env.bytecode);
    input_size + bytecode_size
}

/// Calculate deep size of Log (from alloy_primitives).
fn log_deep_size(log: &Log) -> usize {
    // Log has:
    // - address: Address (stack)
    // - data: LogData which has:
    //   - topics: Vec<B256> or slice
    //   - data: Bytes
    // topics() returns a slice, so we calculate its size as if it were a Vec
    let topics_slice = log.topics();
    let topics_size = topics_slice.len() * size_of::<revm_primitives::B256>();
    let data_size = bytes_deep_size(&log.data.data);
    topics_size + data_size
}

/// Calculate deep size of MemoryDep.
fn memory_dep_deep_size(_dep: &MemoryDep) -> usize {
    // MemoryDep is all stack-allocated (lsn, offsets, length)
    0
}

/// Calculate deep size of SSAInput.
fn ssa_input_deep_size(input: &SSAInput) -> usize {
    match input {
        SSAInput::Memory(deps) => vec_deep_size(deps, memory_dep_deep_size),
        // All other variants are stack-allocated or reference other nodes
        _ => 0,
    }
}

/// Calculate deep size of SSAOutput.
fn ssa_output_deep_size(output: &SSAOutput) -> usize {
    match output {
        SSAOutput::Memory(bytes) | SSAOutput::ReturnDataBuffer(bytes) => bytes_deep_size(bytes),
        SSAOutput::InterpreterResult(result) => ssa_interpreter_result_deep_size(result),
        SSAOutput::Storage { key, value } => {
            let key_size = box_deep_size(key, storage_key_deep_size);
            let value_size = box_deep_size(value, storage_value_deep_size);
            key_size + value_size
        }
        SSAOutput::CreateInput(input) => box_deep_size(input, ssa_create_input_deep_size),
        SSAOutput::CreateOutcome(outcome) => box_deep_size(outcome, ssa_create_outcome_deep_size),
        SSAOutput::CallInput(input) => box_deep_size(input, ssa_call_input_deep_size),
        SSAOutput::CallOutcome(outcome) => box_deep_size(outcome, ssa_call_outcome_deep_size),
        SSAOutput::Log(log) => box_deep_size(log, log_deep_size),
        SSAOutput::ContractEnv(env) => box_deep_size(env, contract_env_deep_size),
        // Stack-allocated variants
        SSAOutput::Constant(_)
        | SSAOutput::Stack(_)
        | SSAOutput::Jump(_)
        | SSAOutput::MemorySize(_) => 0,
    }
}

/// Calculate deep size of SSALogEntry.
fn ssa_log_entry_deep_size(entry: &SSALogEntry) -> usize {
    let inputs_size = vec_deep_size(&entry.inputs, ssa_input_deep_size);
    let outputs_size = vec_deep_size(&entry.outputs, ssa_output_deep_size);
    inputs_size + outputs_size
}

/// Calculate the total memory consumption of an SSA graph.
///
/// This function accurately accounts for:
/// - The DiGraph structure (nodes and edges)
/// - All Vec allocations and their capacities
/// - All Box allocations in SSAOutput variants
/// - All Bytes allocations
/// - Nested structures like Vec<Vec<T>>
/// - Deep heap allocations within SSALogEntry
pub fn calculate_ssa_graph_memory(graph: &SsaGraph) -> usize {
    let mut total_size = 0;

    // 1. Calculate size of the DiGraph structure
    // petgraph's DiGraph stores nodes in a Vec and edges in a Vec
    // We need to account for the node storage which contains SSALogEntry
    let node_count = graph.graph().node_count();
    let edge_count = graph.graph().edge_count();

    // DiGraph allocates space for nodes
    // Each node has overhead + the actual SSALogEntry
    let digraph_node_overhead = node_count * size_of::<petgraph::graph::Node<SSALogEntry>>();

    // DiGraph allocates space for edges
    // Each edge has overhead (source, target, weight which is () here)
    let digraph_edge_overhead = edge_count * size_of::<petgraph::graph::Edge<()>>();

    total_size += digraph_node_overhead + digraph_edge_overhead;

    // 2. Calculate deep size of all SSALogEntry nodes
    for node_idx in graph.graph().node_indices() {
        if let Some(entry) = graph.graph().node_weight(node_idx) {
            total_size += ssa_log_entry_deep_size(entry);
        }
    }

    // 3. lsn_to_node: Vec<NodeIndex>
    total_size += vec_primitive_deep_size(graph.lsn_to_node());

    // 4. storage_write: Vec<LsnType>
    total_size += vec_primitive_deep_size(graph.storage_write());

    // 5. successors: Vec<Vec<LsnType>> (nested Vec)
    total_size += vec_deep_size(graph.successors(), vec_primitive_deep_size);

    // 6. predecessors: Vec<Vec<LsnType>> (nested Vec)
    total_size += vec_deep_size(graph.predecessors(), vec_primitive_deep_size);

    total_size
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bytes_deep_size() {
        let bytes = Bytes::from(vec![0u8; 100]);
        assert!(bytes_deep_size(&bytes) >= 100);
    }

    #[test]
    fn test_vec_primitive_deep_size() {
        let vec = vec![1u32, 2, 3, 4, 5];
        let size = vec_primitive_deep_size(&vec);
        assert_eq!(size, vec.capacity() * size_of::<u32>());
    }

    #[test]
    fn test_vec_deep_size_nested() {
        let vec: Vec<Vec<u32>> = vec![vec![1, 2, 3], vec![4, 5]];
        let size = vec_deep_size(&vec, vec_primitive_deep_size);
        // Should account for outer Vec capacity + both inner Vecs' capacities
        assert!(size > 0);
    }
}
