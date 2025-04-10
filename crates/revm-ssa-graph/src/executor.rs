use std::{sync::Arc, time::Instant};

use crate::{
    context::ExecutionContext, graph::SsaGraph, instruction_table::InstructionTable,
    tracer::ExecutionTracer, Result,
};

use revm_primitives::{db::DatabaseRef, fixed_bytes, spec_to_generic, Env, FixedBytes, Spec, SpecId};
use revm_ssa::{logger::LsnType, FrameInput};

/// Execution mode
#[derive(Debug, Clone, PartialEq)]
pub enum ExecutionMode {
    /// Execute all operations
    Full,
    /// Start execution from specified LSN
    Partial(Vec<LsnType>),
}

/// SSA Executor
pub struct SSAExecutor<'a, DB>
where
    DB: DatabaseRef + Send + Sync + 'a,
    DB::Error: Send + Sync,
{
    /// Execution context
    pub context: Arc<ExecutionContext<'a, DB>>,
    /// Instruction Table
    pub table: InstructionTable<DB>,
    /// Dependency graph
    pub graph: Arc<SsaGraph>,
    /// Execution tracer (optional)
    pub tracer: Option<ExecutionTracer>,
    /// Execution mode
    pub mode: ExecutionMode,
}

impl<'a, DB> SSAExecutor<'a, DB>
where
    DB: DatabaseRef + Send + Sync + 'a,
    DB::Error: Send + Sync,
{
    pub fn new<SPEC: Spec>(
        graph: Arc<SsaGraph>,
        db: DB,
        env: &'a Env,
        first_frame_input: Option<FrameInput>,
    ) -> Self {
        Self {
            context: Arc::new(ExecutionContext::new::<SPEC>(env, db, first_frame_input)),
            table: InstructionTable::create_instruction_table::<SPEC>(),
            graph,
            tracer: None,
            mode: ExecutionMode::Full,
        }
    }

    pub fn new_with_spec(
        graph: Arc<SsaGraph>,
        db: DB,
        env: &'a Env,
        first_frame_input: Option<FrameInput>,
        spec_id: SpecId,
    ) -> Self {
        spec_to_generic!(
            spec_id,
            Self::new::<SPEC>(graph, db, env, first_frame_input)
        )
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
    pub fn execute(&mut self, _tx_hash: FixedBytes<32>) -> Result<(usize, std::time::Duration)> {
        let graph = unsafe { Self::get_mut_graph(&self.graph) };

        let mut nodes_to_execute = match &self.mode {
            ExecutionMode::Full => self.graph.topological_sort()?,
            ExecutionMode::Partial(start_lsns) => {
                let mut reachable_lsns = Vec::new();
                let mut seen_lsns = std::collections::HashSet::new();
                for &start_lsn in start_lsns {
                    for node_index in self.graph.get_reachable_nodes(start_lsn)? {
                        let node = self.graph.get_node_by_index(node_index)?;
                        if seen_lsns.insert(node.lsn) {
                            reachable_lsns.push(node.lsn);
                        }
                    }
                }
                reachable_lsns
            }
        };

        // ! Used for SSA unit test
        if let Some(tracer) = &mut self.tracer {
            let graph = self.graph.clone();
            for &lsn in &nodes_to_execute {
                let node = graph.get_node(lsn)?;
                let outputs = graph.get_original_outputs(lsn)?.unwrap();
                tracer.record_graph(lsn, outputs.into(), node.opcode);
            }
        }
        let len = nodes_to_execute.len();
        let execute_start = Instant::now();
        nodes_to_execute.sort();

        let context = unsafe { Self::get_mut_context(&self.context) };
        for lsn in nodes_to_execute {
            let node = graph.get_node_mut(lsn)?;
            self.table.instructions[node.opcode as usize](context, node, &self.graph)?;
        }

        // ! Debug for SSA
        // let first_lsn = nodes_to_execute[0];
        // let last_lsn = nodes_to_execute[nodes_to_execute.len() - 1];

        // for lsn in first_lsn..=last_lsn {
        //     if let Ok(node) = graph.get_node(lsn) {
        //         if nodes_to_execute.contains(&lsn) {
        //             let node = graph.get_node_mut(lsn)?;
        //             self.table.instructions[node.opcode as usize](context, node, &self.graph)?;
        //             if _tx_hash == fixed_bytes!("39303416f7396544e603c37217b617d6464a16fa2299c26cfb35ab1fc515fe87") {
        //                 eprintln!("after execute node: {}", node);
        //             }
        //         } else {
        //             if _tx_hash == fixed_bytes!("39303416f7396544e603c37217b617d6464a16fa2299c26cfb35ab1fc515fe87") {
        //                 eprintln!("no re-execute node: {}", node);
        //             }
        //         }
        //     }
        // }

        let execute_duration = execute_start.elapsed();

        Ok((len, execute_duration))
    }

    /// Unsafely get mutable reference to context
    #[inline(always)]
    unsafe fn get_mut_context(
        context: &Arc<ExecutionContext<'a, DB>>,
    ) -> &'a mut ExecutionContext<'a, DB> {
        &mut *(Arc::as_ptr(context) as *mut ExecutionContext<'a, DB>)
    }

    /// Unsafely get mutable reference to graph
    #[inline(always)]
    unsafe fn get_mut_graph(graph: &Arc<SsaGraph>) -> &'a mut SsaGraph {
        &mut *(Arc::as_ptr(graph) as *mut SsaGraph)
    }
}
