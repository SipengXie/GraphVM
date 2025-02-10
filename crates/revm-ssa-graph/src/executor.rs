use std::{
    collections::HashSet,
    marker::PhantomData,
    sync::{atomic::{AtomicU64, Ordering}, Arc}
};

use crate::{
    context::ExecutionContext, graph::SsaGraph, tracer::ExecutionTracer, ExecutionError, Result
};
use revm_primitives::{db::DatabaseRef, Bytes, Spec, Env};
use revm_interpreter::OpCode;
use revm_ssa::{
    ContractEnv, MemoryDep, SSACallInput, SSACreateInput, SSAInput, SSAInstructionResult, SSALogEntry, SSAOutput, StorageKey
};
use either::Either;
use metrics::{self, histogram};

/// Execution mode
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// Execute all operations
    Full,
    /// Start execution from specified LSN
    Partial(Vec<usize>),
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
    fn new(max_lsn: usize, initial_state: bool) -> Self {
        let size = (max_lsn + 63) / 64;
        let init_value = if initial_state { u64::MAX } else { 0 };
        
        let bits = (0..size)
            .map(|_| PaddedAtomicU64(AtomicU64::new(init_value)))
            .collect();

        Self { bits }
    }

    /// Check if specified LSN is marked as completed
    // #[inline]
    // fn check(&self, lsn: usize) -> bool {
    //     let (idx, mask) = (lsn / 64, 1 << (lsn % 64));
    //     self.bits.get(idx)
    //         .map(|a| a.load(std::sync::atomic::Ordering::Acquire) & mask != 0)
    //         .unwrap_or(false)
    // }

    /// Atomically mark an LSN as completed
    #[inline]
    fn mark(&self, lsn: usize) {
        let (idx, mask) = (lsn / 64, 1 << (lsn % 64));
        if let Some(atomic) = self.bits.get(idx) {
            // Modifying bits[idx] won't affect other elements in bits
            atomic.0.fetch_or(mask, Ordering::Release);
        }
    }

    /// Atomically clear a bit (set to 0)
    #[inline]
    fn unmark(&self, lsn: usize) {
        let (idx, mask) = (lsn / 64, 1 << (lsn % 64));
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
    thread_pool: Option<rayon::ThreadPool>,
    /// Completed nodes
    completed_nodes: Arc<AtomicBitMap>,
}

impl<'a, DB, SPEC> SSAExecutor<'a, DB, SPEC> 
where
    DB: DatabaseRef + Send + Sync + 'a,
    DB::Error: Send + Sync,
    SPEC: Spec + Send + Sync,
{
    pub fn new(graph: SsaGraph, db: DB, env: &'a Env, num_threads: Option<usize>) -> Self {
        let thread_pool = if let Some(num_threads) = num_threads {
            Some(rayon::ThreadPoolBuilder::new()
                .num_threads(num_threads)
                .build()
                .unwrap())
        } else {
            None
        };
        let max_lsn = graph.num_nodes();
        Self {
            context: Arc::new(ExecutionContext::new(env, db)),
            graph: Arc::new(graph),
            tracer: None,
            mode: ExecutionMode::Full,
            spec: PhantomData,
            thread_pool,
            completed_nodes: Arc::new(AtomicBitMap::new(max_lsn, false)),
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
    pub fn execute(&mut self) -> Result<()> {
        // let start = std::time::Instant::now();
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
        // Find nodes with storage write operations
        // eprintln!("Write nodes:");
        // for node in &nodes_to_execute {
        //     if is_storage_write!(node.opcode) {
        //         eprintln!("LSN: {}, OpCode: {:?}, Inputs: {:?}, Outputs: {:?}", 
        //             node.lsn,
        //             OpCode::new(node.opcode).unwrap_or(OpCode::INVALID).as_str(),
        //             node.inputs,
        //             node.outputs
        //         );
        //     }
        // }
        // eprintln!();
        // eprintln!("Nodes to execute (sorted by LSN):");
        // let mut sorted_nodes = nodes_to_execute.clone();
        // sorted_nodes.sort_by_key(|node| node.lsn);
        // for node in sorted_nodes {
        //     eprintln!("LSN: {}, OpCode: {:?}, Inputs: {:?}, Outputs: {:?}", 
        //         node.lsn, 
        //         OpCode::new(node.opcode).unwrap_or(OpCode::INVALID).as_str(),
        //         node.inputs,
        //         node.outputs
        //     );
        // }
        // histogram!("graph_execution.nodes_to_execute", start.elapsed().as_micros() as f64);

        let graph = unsafe { Self::get_mut_graph(&self.graph) };
        for node in &nodes_to_execute {
            Self::execution_node(node, graph, &self.context.clone())?;
        }

        if let Some(tracer) = &mut self.tracer {
            let graph = self.graph.clone();
            for node in &nodes_to_execute {
                let outputs = graph.get_original_outputs(node.lsn)?.unwrap();
                tracer.record_graph(node.lsn, outputs.into(), node.opcode);
            }
        }
        
        Ok(())
    }

    pub fn execute_parallel(&mut self) -> Result<()> {
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
                let bitmap = AtomicBitMap::new(max_lsn, true);
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
                    if let Some(lsn) = SsaGraph::get_lsn_from_input(input) {
                        let (idx, mask) = (lsn / 64, 1 << (lsn % 64));
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
        let start = std::time::Instant::now();
        
        thread_pool.install(|| {
            nodes_with_masks.into_iter().for_each(|(node, deps_mask)| {
                // Use bitmask for batch checking
                let mut spin_count = 1;
                'wait_loop: loop {
                    // Check if all dependencies are completed
                    for (idx, mask) in deps_mask.iter().enumerate() {
                        if *mask == 0 { continue; }
                        let current = self.completed_nodes.bits[idx].0.load(Ordering::Acquire);
                        if (current & mask) != *mask {
                            // Exponential backoff strategy
                            for _ in 0..spin_count {
                                std::hint::spin_loop();
                            }
                            spin_count = std::cmp::min(spin_count * 2, 1024);
                            continue 'wait_loop;
                        }
                    }
                    break;
                }

                let exec_result = Self::execution_node(node, graph, &self.context.clone());
                if exec_result.is_err() {
                    panic!("Execution failed: {:?}", exec_result.err().unwrap());
                }
                self.completed_nodes.mark(node.lsn);
            })
        });
        // eprintln!("Parallel Execution time: {:?}", start.elapsed());

        if let Some(tracer) = &mut self.tracer {
            let graph = self.graph.clone();
            for node in &nodes_to_execute {
                let outputs = graph.get_original_outputs(node.lsn)?.unwrap();
                tracer.record_graph(node.lsn, outputs.into(), node.opcode);
            }
        }
        std::thread::spawn(move || {
            drop(nodes_to_execute);
        });
        Ok(())
    }

    pub fn execution_node(node: &SSALogEntry, graph: &mut SsaGraph, context: &Arc<ExecutionContext<'a, DB, SPEC>>) -> Result<()> {
        let lsn = node.lsn;
        let inputs = Self::resolve_dependencies(graph, &context, &node)?;
        let outputs = Self::execute_operation(&context, node.opcode, inputs)?;

        if node.opcode == 0x56 || node.opcode == 0x57 {
            Self::verify_control_flow(node, &outputs)?;
        }

        graph.set_result(lsn, outputs)?;
        Ok(())
    }

    /// Unsafely get mutable reference to context
    unsafe fn get_mut_context(context: &Arc<ExecutionContext<'a,DB, SPEC>>) -> &'a mut ExecutionContext<'a, DB, SPEC> {
        &mut *(Arc::as_ptr(context) as *mut ExecutionContext<'a, DB, SPEC>)
    }

    /// Unsafely get mutable reference to graph
    unsafe fn get_mut_graph(graph: &Arc<SsaGraph>) -> &'a mut SsaGraph {
        &mut *(Arc::as_ptr(graph) as *mut SsaGraph)
    }

    /// Execute operation based on opcode
    fn execute_operation(context: &Arc<ExecutionContext<'a, DB, SPEC>>, opcode: u8, inputs: Vec<SSAInput>) -> Result<Vec<SSAOutput>> {
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
            if let Ok(Some(memory_data)) = graph.get_result(dep.lsn, |results: &[SSAOutput]| {
                results.iter().find_map(|result| {
                    if let SSAOutput::Memory(src_bytes) = result {
                        Some(src_bytes.clone())
                    } else {
                        None
                    }
                })
            }) {
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


    /// Generic function for getting results and type conversion
    fn get_dependency_result<T, F>(
        graph: &SsaGraph,
        lsn: usize,
        extractor: F,
        error_msg: &str
    ) -> Result<T>
    where
        F: FnOnce(&[SSAOutput]) -> Option<T>,
    {
        let result = graph.get_result(lsn, extractor)?
            .ok_or_else(|| ExecutionError::ExecutionError(format!("{} dependency must exist", error_msg)))?;
        
        Ok(result)
    }

    /// Handle Stack type input
    fn resolve_stack_input(
        graph: &SsaGraph,
        source: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_stack_start = std::time::Instant::now();
        match source {
            Some(lsn) => {
                let stack_value = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::Stack(v) = output {
                            Some(*v)
                        } else {
                            None
                        }
                    }),
                    "Stack"
                )?;
                // metrics::histogram!("resolve_dependencies.resolve_stack_duration_us", resolve_stack_start.elapsed().as_micros() as f64);
                Ok(SSAInput::Stack { value: stack_value, source: Some(lsn) })
            }
            None => {
                // metrics::histogram!("resolve_dependencies.resolve_stack_duration_us", resolve_stack_start.elapsed().as_micros() as f64);
                Err(ExecutionError::ExecutionError("Stack input must have a source".to_string()))
            }
        }
    }

    /// Handle Memory type input
    fn resolve_memory_input(
        graph: &SsaGraph,
        source: &[MemoryDep],
        value: &Bytes
    ) -> Result<SSAInput> {
        if source.is_empty() {
            Ok(SSAInput::Memory {
                value: value.clone(),
                source: vec![]
            })
        } else {
            // let memory_start = std::time::Instant::now();
            let memory = Self::resolve_memory_deps(graph, source)?;
            // metrics::histogram!("resolve_dependencies.memory_resolve_duration_us", memory_start.elapsed().as_micros() as f64);
            Ok(SSAInput::Memory {
                value: memory,
                source: source.to_vec()
            })
        }
    }

    /// Handle Storage type input
    fn resolve_storage_input(
        graph: &SsaGraph,
        context: &Arc<ExecutionContext<'a, DB, SPEC>>,
        source: Option<usize>,
        key: &StorageKey
    ) -> Result<SSAInput> {
        // let resolve_storage_start = std::time::Instant::now();
        let result = match source {
            Some(lsn) => {
                let (storage_key, storage_value) = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::Storage { key, value } = output {
                            Some((key.clone(), value.clone()))
                        } else {
                            None
                        }
                    }),
                    "Storage"
                )?;
                
                Ok(SSAInput::Storage {
                    key: storage_key,
                    value: storage_value,
                    source: Some(lsn)
                })
            }
            None => {
                let context = unsafe { Self::get_mut_context(context) };
                let value = context.get_storage_value(key);
                Ok(SSAInput::Storage {
                    key: key.clone(),
                    value,
                    source: None
                })
            }
        };
        // eprintln!("resolve_storage_input: {:?}", result);
        // metrics::histogram!("resolve_dependencies.resolve_storage_duration_us", resolve_storage_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle ReturnDataBuffer type input
    fn resolve_return_data_input(
        graph: &SsaGraph,
        source: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_return_data_start = std::time::Instant::now();
        let result = match source {
            Some(lsn) => {
                let return_data = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::ReturnDataBuffer(data) = output {
                            Some(data.clone())
                        } else {
                            None
                        }
                    }),
                    "ReturnData"
                )?;
                
                Ok(SSAInput::ReturnDataBuffer {
                    value: return_data,
                    source: Some(lsn)
                })
            }
            None => Ok(SSAInput::ReturnDataBuffer {
                value: Bytes::default(),
                source: None
            })
        };
        // metrics::histogram!("resolve_dependencies.resolve_return_data_duration_us", resolve_return_data_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle MemorySizeChange type input
    fn resolve_memory_size_input(
        graph: &SsaGraph,
        size: usize,
        last_memory: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_memory_size_start = std::time::Instant::now();
        let result = match last_memory {
            Some(lsn) => {
                let memory_size = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::MemorySize(size) = output {
                            Some(*size)
                        } else {
                            None
                        }
                    }),
                    "Memory"
                )?;
                
                Ok(SSAInput::MemorySizeChange {
                    size: memory_size,
                    last_memory: Some(lsn)
                })
            }
            None => Ok(SSAInput::MemorySizeChange {
                size,
                last_memory: None
            })
        };
        // metrics::histogram!("resolve_dependencies.resolve_memory_size_duration_us", resolve_memory_size_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle ContractEntry type input
    fn resolve_contract_entry_input(
        graph: &SsaGraph,
        value: &ContractEnv,
        entry_lsn: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_contract_entry_start = std::time::Instant::now();
        let result = match entry_lsn {
            Some(lsn) => {
                let frame_input = Self::get_frame_input(graph, lsn)?;
                let result = Self::resolve_contract_entry(value, frame_input, lsn)?;
                Ok(result)
            }
            None => Ok(SSAInput::ContractEntry {
                value: value.clone(),
                entry_lsn: None
            })
        };
        // metrics::histogram!("resolve_dependencies.resolve_contract_entry_duration_us", resolve_contract_entry_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle CreateInput type input
    fn resolve_create_input(
        graph: &SsaGraph,
        input: &SSACreateInput,
        entry: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_create_start = std::time::Instant::now();
        let result = match entry {
            Some(lsn) => {
                let create_input = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::CreateFrame(input) = output {
                            Some(input.clone())
                        } else {
                            None
                        }
                    }),
                    "Create"
                )?;
                
                Ok(SSAInput::CreateInput {
                    input: create_input,
                    entry: Some(lsn)
                })
            }
            None => Ok(SSAInput::CreateInput {
                input: input.clone(),
                entry: None
            })
        };
        // metrics::histogram!("resolve_dependencies.resolve_create_duration_us", resolve_create_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle CallInput type input
    fn resolve_call_input(
        graph: &SsaGraph,
        input: &SSACallInput,
        entry: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_call_start = std::time::Instant::now();
        let result = match entry {
            Some(lsn) => {
                let call_input = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::CallFrame(input) = output {
                            Some(input.clone())
                        } else {
                            None
                        }
                    }),
                    "Call"
                )?;
                
                Ok(SSAInput::CallInput {
                    input: call_input,
                    entry: Some(lsn)
                })
            }
            None => Ok(SSAInput::CallInput {
                input: input.clone(),
                entry: None
            })
        };
        // metrics::histogram!("resolve_dependencies.resolve_call_duration_us", resolve_call_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle InterpreterResult type input
    fn resolve_interpreter_result(
        graph: &SsaGraph,
        source: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_interpreter_start = std::time::Instant::now();
        let result = match source {
            Some(lsn) => {
                let interpreter_result = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::InterpreterResult(result) = output {
                            Some(result.clone())
                        } else {
                            None
                        }
                    }),
                    "InterpreterResult"
                )?;
                
                Ok(SSAInput::InterpreterResult {
                    result: interpreter_result,
                    source: Some(lsn)
                })
            }
            None => Err(ExecutionError::ExecutionError(
                "InterpreterResult must have a source".to_string()
            ))
        };
        // metrics::histogram!("resolve_dependencies.resolve_interpreter_duration_us", resolve_interpreter_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle CallOutcome type input
    fn resolve_call_outcome(
        graph: &SsaGraph,
        source: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_call_outcome_start = std::time::Instant::now();
        let result = match source {
            Some(lsn) => {
                let call_outcome = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::CallOutcome(outcome) = output {
                            Some(outcome.clone())
                        } else {
                            None
                        }
                    }),
                    "CallOutcome"
                )?;
                
                Ok(SSAInput::CallOutcome {
                    outcome: call_outcome,
                    source: Some(lsn)
                })
            }
            None => Err(ExecutionError::ExecutionError(
                "CallOutcome must have a source".to_string()
            ))
        };
        // metrics::histogram!("resolve_dependencies.resolve_call_outcome_duration_us", resolve_call_outcome_start.elapsed().as_micros() as f64);
        result
    }

    /// Handle CreateOutcome type input
    fn resolve_create_outcome(
        graph: &SsaGraph,
        source: Option<usize>
    ) -> Result<SSAInput> {
        // let resolve_create_outcome_start = std::time::Instant::now();
        let result = match source {
            Some(lsn) => {
                let create_outcome = Self::get_dependency_result(
                    graph,
                    lsn,
                    |results| results.iter().find_map(|output| {
                        if let SSAOutput::CreateOutcome(outcome) = output {
                            Some(outcome.clone())
                        } else {
                            None
                        }
                    }),
                    "CreateOutcome"
                )?;
                
                Ok(SSAInput::CreateOutcome {
                    outcome: create_outcome,
                    source: Some(lsn)
                })
            }
            None => Err(ExecutionError::ExecutionError(
                "CreateOutcome must have a source".to_string()
            ))
        };
        // metrics::histogram!("resolve_dependencies.resolve_create_outcome_duration_us", resolve_create_outcome_start.elapsed().as_micros() as f64);
        result
    }

    /// Parse dependencies to get input values
    fn resolve_dependencies(
        graph: &SsaGraph,
        context: &Arc<ExecutionContext<'a, DB, SPEC>>,
        entry: &SSALogEntry
    ) -> Result<Vec<SSAInput>> {

        let mut inputs = Vec::with_capacity(entry.inputs.len());

        for input in &entry.inputs {
            let resolved_input = match input {
                SSAInput::Constant(value) => SSAInput::Constant(*value),
                SSAInput::Stack { source, .. } => Self::resolve_stack_input(graph, *source)?,
                SSAInput::Memory { source, value } => Self::resolve_memory_input(graph, source, value)?,
                SSAInput::Storage { source, key, .. } => Self::resolve_storage_input(graph, context, *source, key)?,
                SSAInput::ReturnDataBuffer { source, .. } => Self::resolve_return_data_input(graph, *source)?,
                SSAInput::ContractEntry { value, entry_lsn } => Self::resolve_contract_entry_input(graph, value, *entry_lsn)?,
                SSAInput::MemorySizeChange { size, last_memory } => Self::resolve_memory_size_input(graph, *size, *last_memory)?,
                SSAInput::CreateInput { input, entry } => Self::resolve_create_input(graph, input, *entry)?,
                SSAInput::CallInput { input, entry } => Self::resolve_call_input(graph, input, *entry)?,
                SSAInput::InterpreterResult { source, .. } => Self::resolve_interpreter_result(graph, *source)?,
                SSAInput::CallOutcome { source, .. } => Self::resolve_call_outcome(graph, *source)?,
                SSAInput::CreateOutcome { source, .. } => Self::resolve_create_outcome(graph, *source)?,
            };
            inputs.push(resolved_input);
        }

        Ok(inputs)
    }

    /// Get call/create input from frame results
    fn get_frame_input(graph: &SsaGraph, entry_lsn: usize) -> Result<Either<SSACallInput, SSACreateInput>> {
        // Try to get CallFrame
        if let Some(call_input) = graph.get_result(entry_lsn, |results| {
            results.iter().find_map(|output| {
                match output {
                    SSAOutput::CallFrame(input) => Some(input.clone()),
                    _ => None
                }
            })
        })? {
            return Ok(Either::Left(call_input));
        }
        
        // Try to get CreateFrame
        if let Some(create_input) = graph.get_result(entry_lsn, |results| {
            results.iter().find_map(|output| {
                match output {
                    SSAOutput::CreateFrame(input) => Some(input.clone()),
                    _ => None
                }
            })
        })? {
            return Ok(Either::Right(create_input));
        }
        
        Err(ExecutionError::ExecutionError(
            format!("No frame found for entry_lsn: {}", entry_lsn)
        ))
    }

    /// Handle ContractEntry dependencies by resolving environment values from call or create frames
    /// 
    /// # Arguments
    /// * `value` - The contract environment value to resolve
    /// * `frame_input` - The input frame (either call or create)
    /// * `lsn` - The logical sequence number
    /// 
    /// # Returns
    /// * `Result<SSAInput>` - The resolved contract entry input
    fn resolve_contract_entry(
        value: &ContractEnv,
        frame_input: Either<SSACallInput, SSACreateInput>,
        lsn: usize
    ) -> Result<SSAInput> {
        use ContractEnv::*;

        // Helper closure to create error with detailed context
        let create_error = |msg: &str| {
            Err(ExecutionError::ExecutionError(format!(
                "Contract entry error - {msg}. Value: {value:?}"
            )))
        };

        // Helper to handle code-related operations for call and create frames
        let handle_code_operation = |input: &Either<SSACallInput, SSACreateInput>, is_size: bool| {
            match input {
                Either::Left(call) => {
                    call.code
                        .as_ref()
                        .ok_or_else(|| ExecutionError::ExecutionError(
                            "Call frame must have code for Code/Size operations".to_string()
                        ))
                        .map(|code| {
                            if is_size {
                                ContractEnv::Size(code.len())
                            } else {
                                ContractEnv::Code(code.clone())
                            }
                        })
                }
                Either::Right(create) => {
                    let code = &create.init_code;
                    Ok(if is_size {
                        ContractEnv::Size(code.len())
                    } else {
                        ContractEnv::Code(code.clone())
                    })
                }
            }
        };

        let resolved_value = match (value, frame_input) {
            // Caller resolution
            (Caller(_), input) => Caller(match input {
                Either::Left(call) => call.caller,
                Either::Right(create) => create.caller,
            }),
            
            // Target resolution
            (Target(_), input) => Target(match input {
                Either::Left(call) => call.target_address,
                Either::Right(create) => create.target,
            }),
            
            // CallValue resolution
            (CallValue(_), input) => CallValue(match input {
                Either::Left(call) => call.value,
                Either::Right(create) => create.value,
            }),
            
            // CallData resolution
            (CallData(_), input) => CallData(match input {
                Either::Left(call) => call.input.clone(),
                Either::Right(_) => Bytes::default(),
            }),
            
            // CallDataSize resolution
            (CallDataSize(_), input) => CallDataSize(match input {
                Either::Left(call) => call.input.len(),
                Either::Right(create) => create.init_code.len(),
            }),

            // Code resolution
            (Code(_), input) => handle_code_operation(&input, false)?,
            
            // Size resolution 
            (Size(_), input) => handle_code_operation(&input, true)?,
            
            // Invalid combinations
            _ => return create_error("Invalid contract entry type"),
        };

        Ok(SSAInput::ContractEntry {
            value: resolved_value,
            entry_lsn: Some(lsn)
        })
    }

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
