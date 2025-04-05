use std::{marker::PhantomData, sync::Arc, time::Instant};

use crate::{
    context::ExecutionContext, graph::SsaGraph, tracer::ExecutionTracer, ExecutionError, Result,
};
use rayon::ThreadPool;
use revm_primitives::{db::DatabaseRef, Env, Spec};
use revm_ssa::{logger::LsnType, SSACallInput, SSACreateInput, SSAInstructionResult, SSALogEntry};

/// Execution mode
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// Execute all operations
    Full,
    /// Start execution from specified LSN
    Partial(Vec<LsnType>),
}

// #[repr(align(64))] // Force cache line alignment
// struct PaddedAtomicU64(AtomicU64);

// struct AtomicBitMap {
//     bits: Vec<PaddedAtomicU64> // Each AtomicU64 occupies a full cache line
// }

// impl AtomicBitMap {
//     /// Create new bitmap with specified initialization state
//     /// - max_lsn: Maximum LSN to support
//     /// - initial_state: true = all bits set (mark as completed), false = all bits cleared
//     fn new(max_lsn: LsnType, initial_state: bool) -> Self {
//         let size = (max_lsn as usize + 63) / 64;
//         let init_value = if initial_state { u64::MAX } else { 0 };

//         let bits = (0..size)
//             .map(|_| PaddedAtomicU64(AtomicU64::new(init_value)))
//             .collect();

//         Self { bits }
//     }

//     /// Atomically mark an LSN as completed
//     #[inline(always)]
//     fn mark(&self, lsn: LsnType) {
//         let (idx, mask) = (lsn as usize / 64, 1u64 << (lsn % 64));
//         if let Some(atomic) = self.bits.get(idx) {
//             // Modifying bits[idx] won't affect other elements in bits
//             atomic.0.fetch_or(mask, Ordering::Release);
//         }
//     }

//     /// Atomically clear a bit (set to 0)
//     #[inline(always)]
//     fn unmark(&self, lsn: LsnType) {
//         let (idx, mask) = (lsn as usize / 64, 1u64 << (lsn % 64));
//         if let Some(atomic) = self.bits.get(idx) {
//             atomic.0.fetch_and(!mask, std::sync::atomic::Ordering::Release);
//         }
//     }
// }

/// SSA Executor
pub struct SSAExecutor<'a, DB, SPEC>
where
    DB: DatabaseRef + Send + Sync + 'a,
    DB::Error: Send + Sync,
    SPEC: Spec + Send + Sync,
{
    /// Execution context
    pub context: Arc<ExecutionContext<'a, DB, SPEC>>,
    /// Dependency graph
    pub graph: Arc<SsaGraph>,
    /// Execution tracer (optional)
    pub tracer: Option<ExecutionTracer>,
    /// Execution mode
    pub mode: ExecutionMode,
    /// Hardfork specification
    pub spec: PhantomData<SPEC>,
    // thread_pool: Option<ThreadPool>,

    // completed_nodes: Arc<AtomicBitMap>,
}

impl<'a, DB, SPEC> SSAExecutor<'a, DB, SPEC>
where
    DB: DatabaseRef + Send + Sync + 'a,
    DB::Error: Send + Sync,
    SPEC: Spec + Send + Sync,
{
    pub fn new(
        graph: Arc<SsaGraph>,
        db: DB,
        env: &'a Env,
        _thread_pool: Option<ThreadPool>,
        first_call_input: Option<SSACallInput>,
        first_create_input: Option<SSACreateInput>,
    ) -> Self {
        // let max_lsn = graph.num_nodes();
        Self {
            context: Arc::new(ExecutionContext::new(
                env,
                db,
                first_call_input,
                first_create_input,
            )),
            graph,
            tracer: None,
            mode: ExecutionMode::Full,
            spec: PhantomData,
            // thread_pool,
            // completed_nodes: Arc::new(AtomicBitMap::new(max_lsn as LsnType, false)),
        }
    }

    /// Set execution mode
    pub fn with_mode(mut self, mode: ExecutionMode) -> Self {
        self.mode = mode;
        self
    }

    /// Enable tracer
    pub fn with_tracer(mut self, tracer: Option<ExecutionTracer>) -> Self {
        self.tracer = tracer;
        self
    }

    /// Get mutable reference to execution tracer
    pub fn tracer_mut(&mut self) -> Option<&mut ExecutionTracer> {
        self.tracer.as_mut()
    }

    /// Consume executor and return tracer
    pub fn into_tracer(self) -> Option<ExecutionTracer> {
        self.tracer
    }

    /// Execute the entire graph
    #[inline(always)]
    pub fn execute(&mut self) -> Result<(usize, std::time::Duration)> {
        let graph = unsafe { Self::get_mut_graph(&self.graph) };

        let nodes_to_execute = match &self.mode {
            ExecutionMode::Full => self.graph.topological_sort()?,
            ExecutionMode::Partial(start_lsns) => {
                let mut reachable_nodes = Vec::new();
                let mut seen_lsns = std::collections::HashSet::new();
                for &start_lsn in start_lsns {
                    for node_index in self.graph.get_reachable_nodes(start_lsn)? {
                        let node = self.graph.get_node_by_index(node_index)?;
                        if seen_lsns.insert(node.lsn) {
                            reachable_nodes.push(node_index);
                        }
                    }
                }
                reachable_nodes
            }
        };

        if let Some(tracer) = &mut self.tracer {
            let graph = self.graph.clone();
            for node_index in &nodes_to_execute {
                let node = graph.get_node_by_index(*node_index)?;
                let outputs = graph.get_original_outputs(node.lsn)?.unwrap();
                tracer.record_graph(node.lsn, outputs.into(), node.opcode);
            }
        }

        let len = nodes_to_execute.len();
        let execute_start = Instant::now();
        for node_index in nodes_to_execute {
            let node = graph.get_node_by_index_mut(node_index);
            Self::execute_node(node, &self.graph, &self.context)?;
        }
        let execute_duration = execute_start.elapsed();

        Ok((len, execute_duration))
    }

    // pub fn execute_parallel_batches(&mut self) -> Result<std::time::Duration> {
    //     let threshold = 1024;
    //     // const PARALLEL_THRESHOLD : usize = 1024;

    //     let layers = self.graph.execution_layers()?;
    //     let thread_pool = self.thread_pool.as_ref().unwrap();
    //     let thread_number = thread_pool.current_num_threads();
    //     let graph = unsafe { Self::get_mut_graph(&self.graph) };

    //     let start = Instant::now();
    //     for (_layer_idx, layer) in layers.iter().enumerate() {
    //         let layer_size = layer.len();
    //         let layer_start = Instant::now();

    //         if layer_size <= threshold {
    //             break; // ! FOR TEST CASE
    //             // for node in layer {
    //             //     let exec_result = Self::execute_node(node, graph, &self.context);
    //             //     if exec_result.is_err() {
    //             //         panic!("Execution failed: {:?}", exec_result.err().unwrap());
    //             //     }
    //             // }
    //         } else {
    //             let batch_size = self.dynamic_batch_size(layer.len(), thread_number);
    //             // thread_pool.install(|| {
    //             //     layer.par_chunks(batch_size).for_each(|batch| {
    //             //         let graph = unsafe { Self::get_mut_graph(&self.graph) };
    //             //         for node in batch {
    //             //             let exec_result = Self::execute_node(node, graph, &self.context);
    //             //             if exec_result.is_err() {
    //             //                 panic!("Execution failed: {:?}", exec_result.err().unwrap());
    //             //             }
    //             //         }
    //             //     })
    //             // });

    //             for node in layer {
    //                 let exec_result = Self::execute_node(node, graph, &self.context);
    //                 if exec_result.is_err() {
    //                     panic!("Execution failed: {:?}", exec_result.err().unwrap());
    //                 }
    //             }
    //             let layer_duration = layer_start.elapsed();
    //             println!("Layer {}: size = {}, batch_size = {}, thread_number = {}, execution time = {:?}", _layer_idx, layer_size, batch_size, thread_number, layer_duration);
    //         }

    //     }
    //     let duration = start.elapsed();
    //     Ok(duration)
    // }

    // // Calculate dynamic batch size based on layer size
    // #[inline(always)]
    // fn dynamic_batch_size(&self, layer_len: usize, thread_number: usize) -> usize {

    //     // let min_per_thread = 4;
    //     // let max_per_thread = 256;

    //     let base_size = (layer_len + thread_number - 1) / thread_number;

    //     base_size
    //         // .next_power_of_two()
    //         // .clamp(min_per_thread, max_per_thread)
    // }

    // pub fn execute_parallel(&mut self) -> Result<std::time::Duration> {
    //     // Get nodes to execute based on execution mode
    //     let nodes_to_execute: Vec<_> = match &self.mode {
    //         ExecutionMode::Full => self.graph.topological_sort()?,
    //         ExecutionMode::Partial(start_lsns) => {
    //             // Get all nodes that need to be executed using BFS
    //             let mut reachable_nodes = Vec::new();
    //             let mut seen_lsns = std::collections::HashSet::new();
    //             for &start_lsn in start_lsns {
    //                 for node in self.graph.get_reachable_nodes(start_lsn)? {
    //                     if seen_lsns.insert(node.lsn) {
    //                         reachable_nodes.push(node);
    //                     }
    //                 }
    //             }

    //             let max_lsn = self.graph.num_nodes();
    //             let bitmap = AtomicBitMap::new(max_lsn as LsnType, true);
    //             // Mark all non-reachable nodes as completed
    //             let reachable_lsns: HashSet<_> = reachable_nodes.iter().map(|node| node.lsn).collect();
    //             for lsn in reachable_lsns {
    //                 bitmap.unmark(lsn);
    //             }
    //             self.completed_nodes = Arc::new(bitmap);
    //             reachable_nodes
    //         }
    //     }.into_iter().collect();

    //     // Preprocess dependencies for all nodes
    //     let nodes_with_masks: Vec<_> = nodes_to_execute.iter()
    //         .map(|node| {
    //             // Generate dependency mask for this node
    //             let mut deps_mask = vec![0u64; self.completed_nodes.bits.len()];
    //             for input in &node.inputs {
    //                 let lsn_vec = SsaGraph::get_lsn_from_input(input);
    //                 for lsn in lsn_vec {
    //                     if lsn == 0 { continue; }
    //                     let (idx, mask) = (lsn as usize / 64, 1u64 << (lsn % 64));
    //                     if let Some(bits) = deps_mask.get_mut(idx) {
    //                         *bits |= mask;
    //                     }
    //                 }
    //             }
    //             (node, deps_mask)
    //         })
    //         .collect();

    //     let graph = unsafe { Self::get_mut_graph(&self.graph) };
    //     let thread_pool = self.thread_pool.as_ref().unwrap();

    //     let start = Instant::now();
    //     thread_pool.install(|| {
    //         nodes_with_masks.into_iter().for_each(|(node, deps_mask)| {
    //             // Use bitmask for batch checking
    //             // Wait for all dependencies to complete with spinning
    //             // let wait_start = Instant::now();
    //             while {
    //                 let mut all_ready = true;
    //                 for (idx, mask) in deps_mask.iter().enumerate() {
    //                     if *mask != 0 && (self.completed_nodes.bits[idx].0.load(Ordering::Relaxed) & mask) != *mask {
    //                         all_ready = false;
    //                         break;
    //                     }
    //                 }
    //                 !all_ready
    //             } {
    //                 std::hint::spin_loop();
    //             }
    //             // let wait_duration = wait_start.elapsed();
    //             // histogram!("revm.ssa.executor.wait_time", wait_duration);

    //             // let execute_start = Instant::now();
    //             let exec_result = Self::execute_node(node, graph, &self.context);
    //             // let execute_duration = execute_start.elapsed();
    //             // histogram!("revm.ssa.executor.execute_time", execute_duration);

    //             if exec_result.is_err() {
    //                 panic!("Execution failed: {:?}", exec_result.err().unwrap());
    //             }

    //             // let set_result_start = Instant::now();
    //             self.completed_nodes.mark(node.lsn);
    //             // let set_result_duration = set_result_start.elapsed();
    //             // histogram!("revm.ssa.executor.set_result_time", set_result_duration);
    //         })
    //     });
    //     let duration = start.elapsed();

    //     if let Some(tracer) = &mut self.tracer {
    //         let graph = self.graph.clone();
    //         for node in &nodes_to_execute {
    //             let outputs = graph.get_original_outputs(node.lsn)?.unwrap();
    //             tracer.record_graph(node.lsn, outputs.into(), node.opcode);
    //         }
    //     }
    //     self.thread_pool.as_ref().unwrap().spawn(move || {
    //         drop(nodes_to_execute);
    //     });
    //     Ok(duration)
    // }

    /// Unsafely get mutable reference to context
    #[inline(always)]
    unsafe fn get_mut_context(
        context: &Arc<ExecutionContext<'a, DB, SPEC>>,
    ) -> &'a mut ExecutionContext<'a, DB, SPEC> {
        &mut *(Arc::as_ptr(context) as *mut ExecutionContext<'a, DB, SPEC>)
    }

    /// Unsafely get mutable reference to graph
    #[inline(always)]
    unsafe fn get_mut_graph(graph: &Arc<SsaGraph>) -> &'a mut SsaGraph {
        &mut *(Arc::as_ptr(graph) as *mut SsaGraph)
    }

    /// Execute operation based on opcode
    #[inline(always)]
    fn execute_node(
        node: &mut SSALogEntry,
        graph: &SsaGraph,
        context: &Arc<ExecutionContext<'a, DB, SPEC>>,
    ) -> Result<()> {
        let context = unsafe { Self::get_mut_context(context) };
        match node.opcode {
            // Arithmetic Operations (0x00-0x0B)
            0x00 => context.execute_change_instruction_result(node, graph, 0x00), // STOP
            0x01 => context.execute_add(node, graph),                             // ADD
            0x02 => context.execute_mul(node, graph),                             // MUL
            0x03 => context.execute_sub(node, graph),                             // SUB
            0x04 => context.execute_div(node, graph),                             // DIV
            0x05 => context.execute_sdiv(node, graph),                            // SDIV
            0x06 => context.execute_mod(node, graph),                             // MOD
            0x07 => context.execute_smod(node, graph),                            // SMOD
            0x08 => context.execute_addmod(node, graph),                          // ADDMOD
            0x09 => context.execute_mulmod(node, graph),                          // MULMOD
            0x0A => context.execute_exp(node, graph),                             // EXP
            0x0B => context.execute_signextend(node, graph),                      // SIGNEXTEND

            // Comparison & Bitwise Operations (0x10-0x1D)
            0x10 => context.execute_lt(node, graph),  // LT
            0x11 => context.execute_gt(node, graph),  // GT
            0x12 => context.execute_slt(node, graph), // SLT
            0x13 => context.execute_sgt(node, graph), // SGT
            0x14 => context.execute_eq(node, graph),  // EQ
            0x15 => context.execute_iszero(node, graph), // ISZERO
            0x16 => context.execute_and(node, graph), // AND
            0x17 => context.execute_or(node, graph),  // OR
            0x18 => context.execute_xor(node, graph), // XOR
            0x19 => context.execute_not(node, graph), // NOT
            0x1A => context.execute_byte(node, graph), // BYTE
            0x1B => context.execute_shl(node, graph), // SHL
            0x1C => context.execute_shr(node, graph), // SHR
            0x1D => context.execute_sar(node, graph), // SAR

            // SHA3 & Environmental Information (0x20-0x3F)
            0x20 => context.execute_keccak256(node, graph), // KECCAK256
            0x30 => context.execute_address(node, graph),   // ADDRESS
            0x31 => context.execute_balance(node, graph),   // BALANCE
            0x32 => context.execute_host_env(node, graph, node.opcode), // ORIGIN
            0x33 => context.execute_caller(node, graph),    // CALLER
            0x34 => context.execute_callvalue(node, graph), // CALLVALUE
            0x35 => context.execute_calldataload(node, graph), // CALLDATALOAD
            0x36 => context.execute_calldatasize(node, graph), // CALLDATASIZE
            0x37 => context.execute_calldatacopy(node, graph), // CALLDATACOPY
            0x38 => context.execute_codesize(node, graph),  // CODESIZE
            0x39 => context.execute_codecopy(node, graph),  // CODECOPY
            0x3A => context.execute_host_env(node, graph, node.opcode), // GASPRICE
            0x3B => context.execute_extcodesize(node, graph), // EXTCODESIZE
            0x3C => context.execute_extcodecopy(node, graph), // EXTCODECOPY
            0x3D => context.execute_returndatasize(node, graph), // RETURNDATASIZE
            0x3E => context.execute_returndatacopy(node, graph), // RETURNDATACOPY
            0x3F => context.execute_extcodehash(node, graph), // EXTCODEHASH

            // Block Information (0x40-0x4A)
            0x40 => context.execute_blockhash(node, graph), // BLOCKHASH
            0x41..=0x46 => context.execute_host_env(node, graph, node.opcode), // COINBASE/TIMESTAMP/NUMBER/DIFFICULTY/GASLIMIT/CHAINID
            0x47 => context.execute_selfbalance(node, graph),                  // SELFBALANCE
            0x48 => context.execute_host_env(node, graph, node.opcode),        // BASEFEE
            0x49 => context.execute_blobhash(node, graph),                     // BLOBHASH
            0x4A => context.execute_host_env(node, graph, node.opcode),        // BLOBBASEFEE

            // Stack, Memory, Storage and Flow Operations (0x50-0x5F)
            0x50 => Ok(()),                               // POP
            0x51 => context.execute_mload(node, graph),   // MLOAD
            0x52 => context.execute_mstore(node, graph),  // MSTORE
            0x53 => context.execute_mstore8(node, graph), // MSTORE8
            0x54 => context.execute_sload(node, graph),   // SLOAD
            0x55 => context.execute_sstore(node, graph),  // SSTORE
            0x56 => context.execute_jump(node, graph),    // JUMP
            0x57 => context.execute_jumpi(node, graph),   // JUMPI
            0x58 => Ok(()),                               // PC
            0x59 => context.execute_msize(node, graph),   // MSIZE
            0x5A => context.execute_gas(node, graph),     // GAS
            0x5C => context.execute_tload(node, graph),   // TLOAD
            0x5D => context.execute_tstore(node, graph),  // TSTORE
            0x5E => context.execute_mcopy(node, graph),   // MCOPY
            0x5F..=0x7f => Ok(()),                        // PUSH0-32

            // Duplication Operations (0x80-0x8F)
            0x80..=0x8f => Ok(()), // DUP1-DUP16

            // Exchange Operations (0x90-0x9F)
            0x90..=0x9f => Ok(()), // SWAP1-SWAP16

            // Logging Operations (0xA0-0xA4)
            0xA0..=0xA4 => context.execute_log(node, graph), // LOG0-LOG4

            // Internal Operations (0xD4-0xD9)
            0xD4 => context.execute_make_create_frame(node, graph), // MAKE_CREATE_FRAME
            0xD5 => context.execute_create_return(node, graph),     // CREATE_RETURN
            0xD6 => context.execute_insert_create_outcome(node, graph), // INSERT_CREATE_OUTCOME
            0xD7 => context.execute_make_call_frame(node, graph),   // MAKE_CALL_FRAME
            0xD8 => context.execute_call_return(node, graph),       // CALL_RETURN
            0xD9 => context.execute_insert_call_outcome(node, graph), // INSERT_CALL_OUTCOME
            0xDA => context.execute_deduct_caller(node, graph),     // DEDUCT_CALLER
            0xDB => context.execute_refund_gas(node, graph),        // REFUND_GAS
            0xDC => context.execute_reward_beneficiary(node, graph), // REWARD_BENEFICIARY

            // System Operations (0xF0-0xFF)
            0xF0 => context.execute_create(node, graph), // CREATE
            0xF1 => context.execute_call(node, graph, node.opcode), // CALL
            0xF2 => context.execute_callcode(node, graph, node.opcode), // CALLCODE
            0xF3 => context.execute_ret(node, graph, SSAInstructionResult::Ok), // RETURN
            0xF4 => context.execute_delegatecall(node, graph, node.opcode), // DELEGATECALL
            0xF5 => context.execute_create(node, graph), // CREATE2
            0xFA => context.execute_staticcall(node, graph, node.opcode), // STATICCALL
            0xFD => context.execute_ret(node, graph, SSAInstructionResult::Revert), // REVERT
            0xFE => context.execute_change_instruction_result(node, graph, 0xFE), // INVALID
            0xFF => context.execute_selfdestruct(node, graph), // SELFDESTRUCT

            _ => Err(ExecutionError::ExecutionError(format!(
                "Unsupported opcode: 0x{:02x}",
                node.opcode
            ))),
        }
    }
}
