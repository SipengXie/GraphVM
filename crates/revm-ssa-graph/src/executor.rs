// TODO: we may need to consider leveraging something like the TxEnv, BlockEnv here.
use std::{
    collections::HashSet,
    marker::PhantomData,
    sync::{atomic::{AtomicU64, Ordering}, Arc}, time::Instant, 
    // time::Instant
};

use crate::{
    context::ExecutionContext, graph::SsaGraph, tracer::ExecutionTracer, ExecutionError, Result
};
use metrics::histogram;
// use metrics::histogram;
use rayon::{iter::ParallelIterator, slice::ParallelSlice, ThreadPool};
use revm_primitives::{db::DatabaseRef, Bytes, Spec, Env};
use revm_ssa::{
    logger::{LsnType, LsnWithIndex}, MemoryDep, SSACallInput, SSACreateInput, SSAInput, SSAInstructionResult, SSALogEntry, SSAOutput, StorageKey
};

/// Execution mode
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// Execute all operations
    Full,
    /// Start execution from specified LSN
    Partial(Vec<LsnType>),
}


#[repr(align(64))] // Force cache line alignment
struct PaddedAtomicU64(AtomicU64);

struct AtomicBitMap {
    bits: Vec<PaddedAtomicU64> // Each AtomicU64 occupies a full cache line
}

impl AtomicBitMap {
   /// Create new bitmap with specified initialization state
    /// - max_lsn: Maximum LSN to support
    /// - initial_state: true = all bits set (mark as completed), false = all bits cleared
    fn new(max_lsn: LsnType, initial_state: bool) -> Self {
        let size = (max_lsn as usize + 63) / 64;
        let init_value = if initial_state { u64::MAX } else { 0 };
        
        let bits = (0..size)
            .map(|_| PaddedAtomicU64(AtomicU64::new(init_value)))
            .collect();

        Self { bits }
    }

    /// Check if specified LSN is marked as completed
    // #[inline]
    // fn check(&self, lsn: LsnType) -> bool {
    //     let (idx, mask) = (lsn as usize / 64, 1u64 << (lsn % 64));
    //     self.bits.get(idx)
    //         .map(|a| a.load(std::sync::atomic::Ordering::Acquire) & mask != 0)
    //         .unwrap_or(false)
    // }

    /// Atomically mark an LSN as completed
    #[inline]
    fn mark(&self, lsn: LsnType) {
        let (idx, mask) = (lsn as usize / 64, 1u64 << (lsn % 64));
        if let Some(atomic) = self.bits.get(idx) {
            // Modifying bits[idx] won't affect other elements in bits
            atomic.0.fetch_or(mask, Ordering::Release);
        }
    }

    /// Atomically clear a bit (set to 0)
    #[inline]
    fn unmark(&self, lsn: LsnType) {
        let (idx, mask) = (lsn as usize / 64, 1u64 << (lsn % 64));
        if let Some(atomic) = self.bits.get(idx) {
            atomic.0.fetch_and(!mask, std::sync::atomic::Ordering::Release);
        }
    }
}

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
    /// Thread pool
    thread_pool: Option<ThreadPool>,
    /// Completed nodes
    completed_nodes: Arc<AtomicBitMap>,
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
        thread_pool: Option<ThreadPool>,
        first_call_input: Option<SSACallInput>,
        first_create_input: Option<SSACreateInput>,
    ) -> Self {
        let max_lsn = graph.num_nodes();
        Self {
            context: Arc::new(ExecutionContext::new(env, db, first_call_input, first_create_input)),
            graph,
            tracer: None,
            mode: ExecutionMode::Full,
            spec: PhantomData,
            thread_pool,
            completed_nodes: Arc::new(AtomicBitMap::new(max_lsn as LsnType, false)),
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
    pub fn execute(&mut self) -> Result<(usize, std::time::Duration)> {
        let nodes_to_execute = match &self.mode {
            ExecutionMode::Full => self.graph.topological_sort()?,
            ExecutionMode::Partial(start_lsns) => {
                let mut reachable_nodes = Vec::new();
                let mut seen_lsns = std::collections::HashSet::new();
                for &start_lsn in start_lsns {
                    for node in self.graph.get_reachable_nodes(start_lsn)? {
                        if seen_lsns.insert(node.lsn) {
                            reachable_nodes.push(node);
                        }
                    }
                }
                reachable_nodes
            }
        };
        // eprintln!("Nodes to execute (sorted by LSN):");
        // let mut sorted_nodes = nodes_to_execute.clone();
        // sorted_nodes.sort_by_key(|node| node.lsn);
        // for node in sorted_nodes {
        //     eprintln!("LSN: {}, OpCode: 0x{:02X}, Inputs: {:?}, Old Outputs: {:?}", 
        //         node.lsn, 
        //         node.opcode,
        //         node.inputs,
        //         node.outputs
        //     );
        // }
        let graph = unsafe { Self::get_mut_graph(&self.graph) };
        let execute_start = Instant::now();
        for node in &nodes_to_execute {
            Self::execute_node(node, graph, &self.context)?;
        }
        let execute_duration = execute_start.elapsed();

        if let Some(tracer) = &mut self.tracer {
            let graph = self.graph.clone();
            for node in &nodes_to_execute {
                let outputs = graph.get_original_outputs(node.lsn)?.unwrap();
                tracer.record_graph(node.lsn, outputs.into(), node.opcode);
            }
        }
        
        Ok((nodes_to_execute.len(), execute_duration))
    }

    pub fn execute_parallel_batches(&mut self) -> Result<std::time::Duration> {
        let threshold = 1024;
        // const PARALLEL_THRESHOLD : usize = 1024;

        let layers = self.graph.execution_layers()?;
        let thread_pool = self.thread_pool.as_ref().unwrap();
        let thread_number = thread_pool.current_num_threads();
        let graph = unsafe { Self::get_mut_graph(&self.graph) };

        let start = Instant::now();
        for (_layer_idx, layer) in layers.iter().enumerate() {
            let layer_size = layer.len();
            let layer_start = Instant::now();

            if layer_size <= threshold {
                for node in layer {
                    let exec_result = Self::execute_node(node, graph, &self.context);
                    if exec_result.is_err() {
                        panic!("Execution failed: {:?}", exec_result.err().unwrap());
                    }
                }
            } else {
                let batch_size = self.dynamic_batch_size(layer.len(), thread_number);
                thread_pool.install(|| {
                    layer.par_chunks(batch_size).for_each(|batch| {
                        let graph = unsafe { Self::get_mut_graph(&self.graph) };
                        for node in batch {
                            let exec_result = Self::execute_node(node, graph, &self.context);
                            if exec_result.is_err() {
                                panic!("Execution failed: {:?}", exec_result.err().unwrap());
                            }
                        }
                    })
                });
                let layer_duration = layer_start.elapsed();
                println!("Layer {}: size = {}, batch_size = {}, thread_number = {}, execution time = {:?}", _layer_idx, layer_size, batch_size, thread_number, layer_duration);
            }
            
        }
        let duration = start.elapsed();
        Ok(duration)
    }

    // Calculate dynamic batch size based on layer size
    #[inline]
    fn dynamic_batch_size(&self, layer_len: usize, thread_number: usize) -> usize {
        
        // let min_per_thread = 4;
        // let max_per_thread = 256;
        
        let base_size = (layer_len + thread_number - 1) / thread_number;
        
        base_size
            // .next_power_of_two() 
            // .clamp(min_per_thread, max_per_thread)
    }

    pub fn execute_parallel(&mut self) -> Result<std::time::Duration> {
        // Get nodes to execute based on execution mode
        let nodes_to_execute: Vec<_> = match &self.mode {
            ExecutionMode::Full => self.graph.topological_sort()?,
            ExecutionMode::Partial(start_lsns) => {
                // Get all nodes that need to be executed using BFS
                let mut reachable_nodes = Vec::new();
                let mut seen_lsns = std::collections::HashSet::new();
                for &start_lsn in start_lsns {
                    for node in self.graph.get_reachable_nodes(start_lsn)? {
                        if seen_lsns.insert(node.lsn) {
                            reachable_nodes.push(node);
                        }
                    }
                }

                let max_lsn = self.graph.num_nodes();
                let bitmap = AtomicBitMap::new(max_lsn as LsnType, true);
                // Mark all non-reachable nodes as completed
                let reachable_lsns: HashSet<_> = reachable_nodes.iter().map(|node| node.lsn).collect();
                for lsn in reachable_lsns {
                    bitmap.unmark(lsn);
                }
                self.completed_nodes = Arc::new(bitmap);
                reachable_nodes
            }
        }.into_iter().collect();

        // Preprocess dependencies for all nodes
        let nodes_with_masks: Vec<_> = nodes_to_execute.iter()
            .map(|node| {
                // Generate dependency mask for this node
                let mut deps_mask = vec![0u64; self.completed_nodes.bits.len()];
                for input in &node.inputs {
                    let lsn_vec = SsaGraph::get_lsn_from_input(input);
                    for lsn in lsn_vec {
                        if lsn == 0 { continue; }
                        let (idx, mask) = (lsn as usize / 64, 1u64 << (lsn % 64));
                        if let Some(bits) = deps_mask.get_mut(idx) {
                            *bits |= mask;
                        }
                    }
                }
                (node, deps_mask)
            })
            .collect();

        let graph = unsafe { Self::get_mut_graph(&self.graph) };
        let thread_pool = self.thread_pool.as_ref().unwrap();
        
        let start = Instant::now();
        thread_pool.install(|| {
            nodes_with_masks.into_iter().for_each(|(node, deps_mask)| {
                // Use bitmask for batch checking
                // Wait for all dependencies to complete with spinning
                // let wait_start = Instant::now();
                while {
                    let mut all_ready = true;
                    for (idx, mask) in deps_mask.iter().enumerate() {
                        if *mask != 0 && (self.completed_nodes.bits[idx].0.load(Ordering::Relaxed) & mask) != *mask {
                            all_ready = false;
                            break;
                        }
                    }
                    !all_ready
                } {
                    std::hint::spin_loop();
                }
                // let wait_duration = wait_start.elapsed();
                // histogram!("revm.ssa.executor.wait_time", wait_duration);

                // let execute_start = Instant::now();
                let exec_result = Self::execute_node(node, graph, &self.context);
                // let execute_duration = execute_start.elapsed();
                // histogram!("revm.ssa.executor.execute_time", execute_duration);

                if exec_result.is_err() {
                    panic!("Execution failed: {:?}", exec_result.err().unwrap());
                }

                // let set_result_start = Instant::now();
                self.completed_nodes.mark(node.lsn);
                // let set_result_duration = set_result_start.elapsed();
                // histogram!("revm.ssa.executor.set_result_time", set_result_duration);
            })
        });
        let duration = start.elapsed();
        
        if let Some(tracer) = &mut self.tracer {
            let graph = self.graph.clone();
            for node in &nodes_to_execute {
                let outputs = graph.get_original_outputs(node.lsn)?.unwrap();
                tracer.record_graph(node.lsn, outputs.into(), node.opcode);
            }
        }
        self.thread_pool.as_ref().unwrap().spawn(move || {
            drop(nodes_to_execute);
        });
        Ok(duration)
    }

    #[inline]
    pub fn execute_node(node: &SSALogEntry, graph: &mut SsaGraph, context: &Arc<ExecutionContext<'a, DB, SPEC>>) -> Result<()> {
        let lsn = node.lsn;

        // let get_inputs_start = Instant::now();
        let inputs = Self::resolve_dependencies(graph, &context, &node)?;
        // let get_inputs_duration = get_inputs_start.elapsed();
        // histogram!("revm.ssa.executor.get_inputs_time", get_inputs_duration);

        // let execute_start = Instant::now();
        let outputs = Self::execute_operation(&context, node.opcode, inputs)?;
        // let execute_duration = execute_start.elapsed();
        // histogram!("revm.ssa.executor.generate_outputs_time", execute_duration);

        if node.opcode == 0x56 || node.opcode == 0x57 {
            Self::verify_control_flow(node, &outputs)?;
        }

        // let set_result_start = Instant::now();
        graph.set_result(lsn, outputs)?;
        // let set_result_duration = set_result_start.elapsed();
        // histogram!("revm.ssa.executor.set_result_time", set_result_duration);
        Ok(())
    }

    /// Unsafely get mutable reference to context
    #[inline]
    unsafe fn get_mut_context(context: &Arc<ExecutionContext<'a,DB, SPEC>>) -> &'a mut ExecutionContext<'a, DB, SPEC> {
        &mut *(Arc::as_ptr(context) as *mut ExecutionContext<'a, DB, SPEC>)
    }

    /// Unsafely get mutable reference to graph
    #[inline]
    unsafe fn get_mut_graph(graph: &Arc<SsaGraph>) -> &'a mut SsaGraph {
        &mut *(Arc::as_ptr(graph) as *mut SsaGraph)
    }

    /// Execute operation based on opcode
    #[inline]
    fn execute_operation(context: &Arc<ExecutionContext<'a, DB, SPEC>>, opcode: u8, inputs: Vec<SSAOutput>) -> Result<Vec<SSAOutput>> {
        let context = unsafe { Self::get_mut_context(context) };
        match opcode {
            // Arithmetic Operations (0x00-0x0B)
            0x00 => context.execute_change_instruction_result(0x00),   // STOP
            0x01 => context.execute_add(inputs),                       // ADD
            0x02 => context.execute_mul(inputs),                       // MUL
            0x03 => context.execute_sub(inputs),                       // SUB
            0x04 => context.execute_div(inputs),                       // DIV
            0x05 => context.execute_sdiv(inputs),                      // SDIV
            0x06 => context.execute_mod(inputs),                       // MOD
            0x07 => context.execute_smod(inputs),                      // SMOD
            0x08 => context.execute_addmod(inputs),                    // ADDMOD
            0x09 => context.execute_mulmod(inputs),                    // MULMOD
            0x0A => context.execute_exp(inputs),                       // EXP
            0x0B => context.execute_signextend(inputs),                // SIGNEXTEND

            // Comparison & Bitwise Operations (0x10-0x1D)
            0x10 => context.execute_lt(inputs),                        // LT
            0x11 => context.execute_gt(inputs),                        // GT
            0x12 => context.execute_slt(inputs),                       // SLT
            0x13 => context.execute_sgt(inputs),                       // SGT
            0x14 => context.execute_eq(inputs),                        // EQ
            0x15 => context.execute_iszero(inputs),                    // ISZERO
            0x16 => context.execute_and(inputs),                       // AND
            0x17 => context.execute_or(inputs),                        // OR
            0x18 => context.execute_xor(inputs),                       // XOR
            0x19 => context.execute_not(inputs),                       // NOT
            0x1A => context.execute_byte(inputs),                      // BYTE
            0x1B => context.execute_shl(inputs),                       // SHL
            0x1C => context.execute_shr(inputs),                       // SHR
            0x1D => context.execute_sar(inputs),                       // SAR

            // SHA3 & Environmental Information (0x20-0x3F)
            0x20 => context.execute_keccak256(inputs),                 // KECCAK256
            0x30 => context.execute_address(inputs),                   // ADDRESS
            0x31 => context.execute_balance(inputs),                   // BALANCE
            0x32 => context.execute_host_env(inputs, opcode),          // ORIGIN
            0x33 => context.execute_caller(inputs),                    // CALLER
            0x34 => context.execute_callvalue(inputs),                 // CALLVALUE
            0x35 => context.execute_calldataload(inputs),              // CALLDATALOAD
            0x36 => context.execute_calldatasize(inputs),              // CALLDATASIZE
            0x37 => context.execute_calldatacopy(inputs),              // CALLDATACOPY
            0x38 => context.execute_codesize(inputs),                  // CODESIZE
            0x39 => context.execute_codecopy(inputs),                  // CODECOPY
            0x3A => context.execute_host_env(inputs, opcode),          // GASPRICE
            0x3B => context.execute_extcodesize(inputs),               // EXTCODESIZE
            0x3C => context.execute_extcodecopy(inputs),               // EXTCODECOPY
            0x3D => context.execute_returndatasize(inputs),            // RETURNDATASIZE
            0x3E => context.execute_returndatacopy(inputs),            // RETURNDATACOPY
            0x3F => context.execute_extcodehash(inputs),               // EXTCODEHASH

            // Block Information (0x40-0x4A)
            0x40 => context.execute_blockhash(inputs),                 // BLOCKHASH    
            0x41..=0x46 => context.execute_host_env(inputs, opcode),   // COINBASE/TIMESTAMP/NUMBER/DIFFICULTY/GASLIMIT/CHAINID
            0x47 => context.execute_selfbalance(inputs),               // SELFBALANCE
            0x48 => context.execute_host_env(inputs, opcode),           // BASEFEE
            0x49 => context.execute_blobhash(inputs, opcode),           // BLOBHASH
            0x4A => context.execute_host_env(inputs, opcode),           // BLOBBASEFEE
            // Stack, Memory, Storage and Flow Operations (0x50-0x5F)
            0x50 => Ok(vec![]),                                     // POP
            0x51 => context.execute_mload(inputs),                     // MLOAD
            0x52 => context.execute_mstore(inputs),                    // MSTORE
            0x53 => context.execute_mstore8(inputs),                   // MSTORE8
            0x54 => context.execute_sload(inputs),                     // SLOAD
            0x55 => context.execute_sstore(inputs),                    // SSTORE
            0x56 => context.execute_jump(inputs),                      // JUMP
            0x57 => context.execute_jumpi(inputs),                     // JUMPI
            0x58 => context.execute_pc(inputs),                        // PC
            0x59 => context.execute_msize(inputs),                     // MSIZE
            0x5A => context.execute_gas(inputs),                       // GAS
            0x5E => context.execute_mcopy(inputs),                     // MCOPY
            0x5F => context.execute_push(inputs, 1),                   // PUSH0

            // Push Operations (0x60-0x7F)
            0x60..=0x7f => {
                let size = (opcode - 0x60 + 1) as usize;
                context.execute_push(inputs, size)
            }

            // Duplication Operations (0x80-0x8F)
            0x80..=0x8f => Ok(vec![]),                             // DUP1-DUP16

            // Exchange Operations (0x90-0x9F)
            0x90..=0x9f => Ok(vec![]),                             // SWAP1-SWAP16

            // Logging Operations (0xA0-0xA4)
            0xA0..=0xA4 => context.execute_log(inputs),               // LOG0-LOG4

            // Internal Operations (0xD4-0xD9)
            0xD4 => context.execute_make_create_frame(inputs),         // MAKE_CREATE_FRAME
            0xD5 => context.execute_create_return(inputs),             // CREATE_RETURN
            0xD6 => context.execute_insert_create_outcome(inputs),     // INSERT_CREATE_OUTCOME
            0xD7 => context.execute_make_call_frame(inputs),          // MAKE_CALL_FRAME
            0xD8 => context.execute_call_return(inputs),              // CALL_RETURN
            0xD9 => context.execute_insert_call_outcome(inputs),      // INSERT_CALL_OUTCOME
            0xDA => context.execute_deduct_caller(inputs),            // DEDUCT_CALLER
            0xDB => context.execute_refund_gas(inputs),               // REFUND_GAS
            0xDC => context.execute_reward_beneficiary(inputs),       // REWARD_BENEFICIARY

            // System Operations (0xF0-0xFF)
            0xF0 => context.execute_create(inputs),                   // CREATE
            0xF1 => context.execute_call(inputs, opcode),            // CALL
            0xF2 => context.execute_callcode(inputs, opcode),        // CALLCODE
            0xF3 => context.execute_ret(inputs, SSAInstructionResult::Ok), // RETURN
            0xF4 => context.execute_delegatecall(inputs, opcode),    // DELEGATECALL
            0xF5 => context.execute_create(inputs),                  // CREATE2
            0xFA => context.execute_staticcall(inputs, opcode),      // STATICCALL
            0xFD => context.execute_ret(inputs, SSAInstructionResult::Revert), // REVERT
            0xFE => context.execute_change_instruction_result(0xFE), // INVALID
            0xFF => context.execute_selfdestruct(inputs),            // SELFDESTRUCT

            _ => Err(ExecutionError::ExecutionError(
                format!("Unsupported opcode: 0x{:02x}", opcode)
            )),
        }
    }

    /// Handle memory dependencies, combine multiple memory fragments into complete memory
    #[inline]
    fn resolve_memory_deps(graph: &SsaGraph, deps: &[MemoryDep]) -> Result<Bytes> {
        // Calculate required memory size - find the maximum end position
        let max_size = deps.iter()
            .map(|dep| dep.self_offset + dep.length)
            .max()
            .unwrap_or(0);
        
        // Create a zero-filled memory
        let mut memory = vec![0u8; max_size];
        
        // Fill memory according to each dependency's offset and length
        for dep in deps {
            if let Ok(memory_data) = Self::get_dependency_result::<Bytes>(
                graph,
                dep.lsn,
                "Memory dependency"
            ) {
                // According to MemoryDep definition:
                // mem[self_offset:self_offset+length] = mem[lsn_offset:lsn_offset+length]
                let dst_start = dep.self_offset;
                let dst_end = dst_start + dep.length;
                let src_start = dep.lsn_offset;
                let src_end = src_start + dep.length;
                
                // Ensure range is valid
                if src_end <= memory_data.len() {
                    memory[dst_start..dst_end].copy_from_slice(&memory_data[src_start..src_end]);
                } else {
                    return Err(ExecutionError::ExecutionError(
                        format!("Invalid memory range: dst [{},{}], src [{},{}], src len {}",
                            dst_start, dst_end, src_start, src_end, memory_data.len())
                    ));
                }
            }
        }
        
        Ok(memory.into())
    }

    /// Get output result for a specific LSN and index from the graph
    #[inline]
    fn get_dependency_result<T>(
        graph: &SsaGraph,
        source: LsnWithIndex,
        error_msg: &str
    ) -> Result<T>
    where
        T: TryFrom<SSAOutput>,
        <T as TryFrom<SSAOutput>>::Error: std::fmt::Debug,
    {
        let (lsn, index) = source;
        let outputs = graph.get_result_by_lsn(lsn)?
            .ok_or_else(|| ExecutionError::ExecutionError(
                format!("Output not found for LSN: {}", lsn)
            ))?;
        
        if index >= outputs.len() as u8 {
            return Err(ExecutionError::ExecutionError(
                format!("{} index out of bounds. LSN: {}, index: {}, available outputs: {}", 
                    error_msg, lsn, index, outputs.len())
            ));
        }
        
        let output = &outputs[index as usize];
        T::try_from(output.clone()).map_err(|e| {
            ExecutionError::ExecutionError(
                format!("{} type conversion failed. LSN: {}, index: {}, actual output: {:?}, error: {:?}", 
                    error_msg, lsn, index, output, e)
            )
        })
    }

    /// Handle Stack type input
    #[inline]
    fn resolve_stack_input(
        graph: &SsaGraph,
        source: LsnWithIndex
    ) -> Result<SSAOutput> {
        if source.0 == 0 {
            return Err(ExecutionError::ExecutionError("Stack input must have a source".to_string()));
        }

        Self::get_dependency_result(
            graph,
            source,
            "Stack"
        )
    }

    /// Handle Memory type input
    #[inline]
    fn resolve_memory_input(
        graph: &SsaGraph,
        source: &[MemoryDep],
    ) -> Result<SSAOutput> {
        if source.is_empty() {
            Ok(SSAOutput::Memory(Bytes::default()))
        } else {
            let memory = Self::resolve_memory_deps(graph, source)?;
            Ok(SSAOutput::Memory(memory))
        }
    }

    /// Handle Storage type input
    #[inline]
    fn resolve_storage_input(
        graph: &SsaGraph,
        context: &Arc<ExecutionContext<'a, DB, SPEC>>,
        source: LsnWithIndex,
        key: &StorageKey
    ) -> Result<SSAOutput> {
        let result = if source.0 != 0 {
           Self::get_dependency_result(
                graph,
                source,
                "Storage"
            )
        } else {
            let context = unsafe { Self::get_mut_context(context) };
            let value = context.get_storage_value_from_db(key);
            Ok(SSAOutput::Storage {
                key: Box::new(key.clone()),
                value: Box::new(value),
            })
        };
        result
    }

    /// Handle ReturnDataBuffer type input
    #[inline]
    fn resolve_return_data_input(
        graph: &SsaGraph,
        source: LsnWithIndex
    ) -> Result<SSAOutput> {
        let result = if source.0 != 0 {
            Self::get_dependency_result(
                graph,
                source,
                "ReturnData"
            )
        } else {
            Ok(SSAOutput::ReturnDataBuffer(Bytes::default()))
        };
        result
    }

    /// Handle MemorySizeChange type input
    #[inline]
    fn resolve_memory_size_input(
        graph: &SsaGraph,
        last_memory: LsnWithIndex 
    ) -> Result<SSAOutput> {
        let result = if last_memory.0 != 0 {
            Self::get_dependency_result(
                graph,
                last_memory,
                "Memory"
            )
        } else {
            Ok(SSAOutput::MemorySize(0))
        };
        result
    }

    /// Handle ContractEntry type input
    #[inline]
    fn resolve_contract_env_input(
        graph: &SsaGraph,
        entry_lsn: LsnWithIndex 
    ) -> Result<SSAOutput> {
        let result = if entry_lsn.0 != 0 {
            Self::get_dependency_result(
                graph,
                entry_lsn,
                "ContractEnv"
            )
        } else {
            Err(ExecutionError::ExecutionError(
                "ContractEnv must have a source".to_string()
            ))
        };
        result
    }

    /// Handle CreateInput type input
    #[inline]
    fn resolve_create_input(
        graph: &SsaGraph,
        entry: LsnWithIndex
    ) -> Result<SSAOutput> {
        if entry.0 == 0 {
            return Err(ExecutionError::ExecutionError(
                "Internal CreateInput must have a source".to_string()
            ));
        }

        Self::get_dependency_result(
            graph,
            entry,
            "Create"
        )
    }

    /// Handle CallInput type input
    #[inline]
    fn resolve_call_input(
        graph: &SsaGraph,
        entry: LsnWithIndex
    ) -> Result<SSAOutput> {
        if entry.0 == 0 {
            return Err(ExecutionError::ExecutionError(
                "CallInput must have a source".to_string()
            ));
        }
     
        Self::get_dependency_result(
            graph,
            entry,
            "Call"
        )
    }

    /// Handle InterpreterResult type input
    #[inline]
    fn resolve_interpreter_result(
        graph: &SsaGraph,
        source: LsnWithIndex 
    ) -> Result<SSAOutput> {
        if source.0 == 0 {
            return Err(ExecutionError::ExecutionError(
                "InterpreterResult must have a source".to_string()
            ));
        }

        Self::get_dependency_result(
            graph,
            source,
            "InterpreterResult"
        )
    }

    /// Handle CallOutcome type input
    #[inline]
    fn resolve_call_outcome(
        graph: &SsaGraph,
        source: LsnWithIndex 
    ) -> Result<SSAOutput> {
        if source.0 == 0 {
            return Err(ExecutionError::ExecutionError(
                "CallOutcome must have a source".to_string()
            ));
        }

        Self::get_dependency_result(
            graph,
            source,
            "CallOutcome"
        )
    }

    /// Handle CreateOutcome type input
    #[inline]
    fn resolve_create_outcome(
        graph: &SsaGraph,
        source: LsnWithIndex
    ) -> Result<SSAOutput> {
        if source.0 == 0 {
            return Err(ExecutionError::ExecutionError(
                "CreateOutcome must have a source".to_string()
            ));
        }

        Self::get_dependency_result(
            graph,
            source,
            "CreateOutcome"
        )
    }

    /// Parse dependencies to get input values
    /// we output vec<SSAOutput> because we do not need to record the value of the input
    /// only need to find the 
    #[inline]
    fn resolve_dependencies(
        graph: &SsaGraph,
        context: &Arc<ExecutionContext<'a, DB, SPEC>>,
        entry: &SSALogEntry
    ) -> Result<Vec<SSAOutput>> {

        let mut inputs = Vec::with_capacity(entry.inputs.len());

        for input in &entry.inputs {
            let resolved_input = match input {
                SSAInput::Constant(value) => SSAOutput::Constant(*value),
                SSAInput::Stack { source, .. } => Self::resolve_stack_input(graph, *source)?,
                SSAInput::Memory { source } => Self::resolve_memory_input(graph, source)?,
                SSAInput::Storage { source, key, .. } => Self::resolve_storage_input(graph, context, *source, key)?,
                SSAInput::ReturnDataBuffer { source, .. } => Self::resolve_return_data_input(graph, *source)?,
                SSAInput::ContractEnv { source: entry_lsn } => Self::resolve_contract_env_input(graph, *entry_lsn)?,
                SSAInput::MemorySizeChange { source: last_memory } => Self::resolve_memory_size_input(graph, *last_memory)?,
                SSAInput::CallInput { source } => {
                    if source.0 == 0 {
                        SSAOutput::CallInput(Box::new(context.get_first_call_input().unwrap()))
                    } else {
                        Self::resolve_call_input(graph, *source)?   
                    }
                },
                SSAInput::CreateInput { source } => {
                    if source.0 == 0 {
                        SSAOutput::CreateInput(Box::new(context.get_first_create_input().unwrap()))
                    } else {
                        Self::resolve_create_input(graph, *source)?
                    }
                },
                SSAInput::InterpreterResult { source, .. } => Self::resolve_interpreter_result(graph, *source)?,
                SSAInput::CallOutcome { source, .. } => Self::resolve_call_outcome(graph, *source)?,
                SSAInput::CreateOutcome { source, .. } => Self::resolve_create_outcome(graph, *source)?,
            };
            inputs.push(resolved_input);
        }

        Ok(inputs)
    }

    #[inline]
    fn verify_control_flow(node: &SSALogEntry, outputs: &[SSAOutput]) -> Result<()> {
        let old_jump_output = match node.outputs[0] {
            SSAOutput::Jump { relative_offset } => relative_offset,
            _ => return Err(ExecutionError::ExecutionError(
                "Jump operation must have a relative offset".to_string()
            )),
        };
        let new_jump_output = match outputs[0] {
            SSAOutput::Jump { relative_offset } => relative_offset,
            _ => return Err(ExecutionError::ExecutionError(
                "Jump operation must have a relative offset".to_string()
            )),
        };

        if old_jump_output != new_jump_output {
            return Err(ExecutionError::ExecutionError(
                format!("Control flow is not deterministic. Node: {:?}, Old jump: {}, New jump: {}", 
                    node, old_jump_output, new_jump_output)
            ));
        }
        Ok(())
    }
}
