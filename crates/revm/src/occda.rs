/// OCCDA (Optimistic Concurrent Contract Deployment and Analysis)
/// 
/// This module implements parallel execution of EVM transactions using optimistic concurrency control.
/// The main idea is to:
/// 1. Execute transactions in parallel assuming no conflicts
/// 2. Track read/write sets during execution
/// 3. Validate and commit transactions in order
/// 4. Retry conflicting transactions
/// 
/// Design goals:
/// - Maximize throughput for non-conflicting transactions
/// - Maintain sequential consistency
/// - Provide detailed performance metrics
use crate::primitives::{ResultAndState, HashMap, Address, HashSet};
use crate::access_tracker::AccessTracker;
use crate::journaled_state::AccessType;
use crate::ssa_access_tracker::SsaAccessTracker;
use crate::task::{Task, TaskResultItem};
use crate::dag::TaskDag;
use crate::evm::Evm;
use crate::db::{Database, DatabaseCommit, DatabaseRef, WrapDatabaseRef, parallel_db::ParallelDB};
use crate::inspector::{GetInspector, Inspector};
use crate::inspector_handle_register;
use std::sync::Arc;
use rayon::ThreadPool;
use rayon::prelude::*;
use revm_primitives::LatestSpec;
use revm_ssa::logger::SsaRwSet;
use revm_ssa::{SSALogEntry, SSALogger};
use revm_ssa_graph::{ExecutionMode, SSAExecutor, SsaDatabaseCommit, SsaGraph};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::Duration;
use std::collections::HashSet as StdHashSet;
use metrics::histogram;
use once_cell::sync::OnceCell;

/// Main struct for handling parallel execution of EVM transactions
pub struct Occda {
    /// Dependency graph for tasks
    /// Used to determine execution order and detect conflicts
    dag: TaskDag,
    
    /// Number of worker threads for parallel execution
    num_threads: usize,
    
    /// Thread pool for managing parallel execution
    /// Pre-initialized to avoid creation overhead
    thread_pool: ThreadPool
}

impl Occda {
    /// Creates a new OCCDA instance with specified number of threads
    /// 
    /// The thread pool is created upfront to:
    /// - Avoid runtime overhead of creating threads
    /// - Maintain consistent thread affinity
    /// - Control system resource usage
    pub fn new(num_threads: usize) -> Self {
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(num_threads)
            .build()
            .unwrap();

        Occda {
            dag: TaskDag::new(),
            num_threads,
            thread_pool
        }
    }

    /// Initialize tasks with their dependencies from the graph
    /// 
    /// This method:
    /// 1. Assigns sequence IDs based on dependencies
    /// 2. Ensures correct execution order
    /// 3. Prepares tasks for parallel execution
    /// 
    /// Returns a vector of tasks with updated sequence IDs (sid)
    pub fn init(&mut self, tasks: Vec<Task>, graph: Option<&TaskDag>) -> Vec<Task> {
        let mut vec = Vec::with_capacity(tasks.len());
        for mut task in tasks {
            if let Some(g) = graph {
                // Find the maximum sid among dependencies
                let sid_max = g.get_dependencies(&task)
                    .into_iter()
                    .map(|node| g.get_task_tid(node).unwrap_or(-1))
                    .max()
                    .unwrap_or(-1);
                task.sid = sid_max;
            } else {
                task.sid = -1;
            }
            vec.push(task);
        }
        vec
    }

    /// Execute tasks in parallel using thread pool
    /// 
    /// Parameters:
    /// - ready_tasks: Tasks ready for parallel execution
    /// - h_tx: Reference to all transactions
    /// - db: Database reference
    /// - result_ptr: Raw pointer to result store
    /// - inspector_setup: Function to create inspector instances
    /// - is_prefetch: Boolean indicating whether the execution is for the prefetch phase
    /// 
    /// Returns:
    /// - Duration: Total time spent in parallel execution
    fn execute_parallel_tasks<DB, I>(
        &self,
        ready_tasks: &Vec<usize>,
        h_tx: &[Task],
        db: &mut ParallelDB<&DB>,
        result_store: &mut Vec<TaskResultItem<I>>,
        logs_store: &mut Vec<Option<Vec<SSALogEntry>>>,
        dag_store: &mut Vec<Arc<OnceCell<Arc<SsaGraph>>>>,
        to_re_execution_store: &Vec<Option<Vec<usize>>>,
        inspector_setup: impl Fn() -> I + Send + Sync,
        is_prefetch: bool,
        enable_dep_graph: bool,
        enable_ssa: bool,
    ) -> Duration 
    where
        DB: DatabaseRef + Database + DatabaseCommit + SsaDatabaseCommit + Send + Sync,
        I: Send + Sync + 'static + 
           for<'db> GetInspector<WrapDatabaseRef<&'db ParallelDB<&'db DB>>> +
           for<'db> Inspector<WrapDatabaseRef<&'db ParallelDB<&'db DB>>>,
        <DB as DatabaseRef>::Error: Send + Sync,
    {
        let result_ptr = result_store.as_mut_ptr() as usize;
        let logs_ptr = logs_store.as_mut_ptr() as usize;
        let parallel_start = std::time::Instant::now();
        
        // Initialize thread task queues
        let mut chunks: Vec<Vec<usize>> = vec![Vec::new(); self.num_threads];
        
        if is_prefetch || !enable_dep_graph {
            // Prefetch phase: evenly distribute tasks across threads
            let chunk_size = (ready_tasks.len() + self.num_threads - 1) / self.num_threads;
            for (i, chunk) in ready_tasks.chunks(chunk_size).enumerate() {
                chunks[i].extend(chunk.iter().cloned());
            }
        } else {
            // Execution phase: group tasks based on DAG dependencies
            let mut task_groups: Vec<Vec<usize>> = Vec::new();
            let mut visited: StdHashSet<usize> = StdHashSet::new();

            // Iterate through tasks in reverse order since later transactions may depend on earlier ones
            for &task_idx in ready_tasks.iter().rev() {
                if visited.contains(&task_idx) {
                    continue;
                }

                let mut group = Vec::new();
                let mut stack = vec![task_idx];
                
                // Depth-first search to find all related tasks
                while let Some(idx) = stack.pop() {
                    if visited.insert(idx) {
                        group.push(idx);
                        
                        // Get dependencies from DAG
                        let task = Task { tid: idx as i32, ..Default::default() };
                        let deps = self.dag.get_dependencies(&task);
                        
                        // Add all unvisited dependencies to the stack
                        for dep_node in deps {
                            if let Some(dep_tid) = self.dag.get_task_tid(dep_node) {
                                let dep_idx = dep_tid as usize;
                                if ready_tasks.contains(&dep_idx) && !visited.contains(&dep_idx) {
                                    stack.push(dep_idx);
                                }
                            }
                        }
                    }
                }

                if !group.is_empty() {
                    // Sort tasks within group by transaction ID to ensure correct execution order
                    group.sort_by_key(|&idx| h_tx[idx].tid);
                    task_groups.push(group);
                }
            }

            // Sort task groups first by minimum TID (earlier transactions first),
            // then by group size (larger groups first for better load balancing)
            task_groups.sort_by(|a, b| {
                let min_tid_a = a.iter().map(|&idx| h_tx[idx].tid).min().unwrap_or(i32::MAX);
                let min_tid_b = b.iter().map(|&idx| h_tx[idx].tid).min().unwrap_or(i32::MAX);
                
                min_tid_a.cmp(&min_tid_b)
                    .then_with(|| b.len().cmp(&a.len()))
            });

            // Distribute task groups to threads, maintaining load balance
            for group in task_groups {
                // Find thread with minimum workload
                let target_thread = chunks
                    .iter()
                    .enumerate()
                    .min_by_key(|(_, chunk)| chunk.len())
                    .map(|(i, _)| i)
                    .unwrap();
                chunks[target_thread].extend(group);
            }
        }

        // Execute tasks in parallel using thread pool
        let thread_times = Arc::new(parking_lot::RwLock::new(vec![(
            Duration::from_secs(0), Duration::from_secs(0),
            Duration::from_secs(0), Duration::from_secs(0)); self.num_threads]));

        self.thread_pool.install(|| {
            chunks.into_par_iter()
                .enumerate()
                .for_each(|(thread_id, indexes)| {
                    // Clone the shared database instance for this thread
                    // Each thread gets its own view of the database with independent cache
                    
                    // Measure database initialization time
                    // This includes time to set up the database reference
                    let db_read_start = std::time::Instant::now();
                    let db_read_end = std::time::Instant::now();
                    // let db_ref = &*db;
                    let db_read_time = db_read_end - db_read_start;
                    
                    // Initialize timing metrics for this thread
                    // init_time: Time spent setting up EVM instances
                    // transact_time: Time spent in actual transaction execution
                    // write_result_time: Time spent writing results back
                    let mut init_time = Duration::from_secs(0);
                    let mut transact_time = Duration::from_secs(0);
                    let mut write_result_time = Duration::from_secs(0);
                    // Track individual transaction times for performance analysis
                    let mut transact_times = Vec::with_capacity(indexes.len());

                    // Process each transaction assigned to this thread
                    for idx in indexes {
                        let task = &h_tx[idx];
                        // Create new inspector instance for this transaction
                        // Each transaction needs its own inspector to track execution
                        let mut inspector = inspector_setup();
                        let db_ref = &*db;
                        
                        // use ssa to re-execute the transaction
                        if to_re_execution_store[idx].is_some() {
                            let to_re_execute = to_re_execution_store[idx].as_ref().unwrap();
                            let wait_start = std::time::Instant::now();
                            while dag_store[idx].get().is_none() {
                                std::hint::spin_loop();
                            }
                            let wait_end = std::time::Instant::now();
                            let wait_time: Duration = wait_end - wait_start;
                            histogram!("ssa_graph_wait", wait_time);
                            let graph = dag_store[idx].get().unwrap().clone();
                            let execution_mode = ExecutionMode::Partial(to_re_execute.clone()); 
                            let mut executor = SSAExecutor::<_, LatestSpec>::new(graph, db_ref, &task.env, None).with_mode(execution_mode);
                            let ssa_execute_start = std::time::Instant::now();
                            match executor.execute() {
                                Ok(()) => {
                                    let ssa_execute_end = std::time::Instant::now();
                                    let ssa_execute_elapse = ssa_execute_end - ssa_execute_start;
                                    histogram!("ssa_execute",ssa_execute_elapse);
                                    let (result_state, storage_keys) = executor.graph.get_storage_write_outputs().unwrap();
                                    let mut task_result: TaskResultItem<I> = TaskResultItem::default();
                                    task_result.ssa_state = Some(result_state);
                                    task_result.ssa_rw_set = Some(SsaRwSet::new_with_write_set(storage_keys));
                                    let result_raw_ptr = result_ptr as *mut TaskResultItem<I>;
                                    unsafe {
                                        *result_raw_ptr.add(idx) = task_result;
                                    }
                                    drop(executor);
                                    continue;
                                }
                                Err(e) => {
                                    let ssa_execute_end = std::time::Instant::now();
                                    let ssa_execute_elapse = ssa_execute_end - ssa_execute_start;
                                    histogram!("ssa_execute",ssa_execute_elapse);
                                    eprintln!("SSA re-execution failed: {:?}, fall back to EVM re-execution.", e);
                                    drop(executor);
                                    // fall through to EVM re-execution path below
                                }
                            }
                        }

                        // Initialize EVM instance with task-specific configuration
                        // Measure setup time separately from execution time
                        let init_start = std::time::Instant::now();
                        let mut evm = if !enable_ssa {
                            Evm::builder()
                            .with_ref_db(db_ref)
                            .modify_env(|env| env.clone_from(&task.env))
                            .with_external_context(&mut inspector)
                            .with_spec_id(task.spec_id)
                            .append_handler_register(inspector_handle_register)
                            .build()
                        } else {
                            Evm::builder()
                            .with_ref_db(db_ref)
                            .modify_env(|env| env.clone_from(&task.env))
                            .with_external_context(&mut inspector)
                            .with_spec_id(task.spec_id)
                            .append_handler_register(inspector_handle_register)
                            .with_ssa_logger(SSALogger::new())
                            .build_with_ssa_logger()
                        };
                        let init_end = std::time::Instant::now();
                        init_time += init_end - init_start;

                        // Execute the transaction and measure execution time
                        // This is the core EVM execution phase
                        let transact_start = std::time::Instant::now();
                        let result = evm.transact();
                        let transact_end = std::time::Instant::now();
                        let this_transact_time = transact_end - transact_start;
                        histogram!("evm.transact", this_transact_time);
                        transact_time += this_transact_time;
                        transact_times.push(this_transact_time);

                        // Process and store execution results
                        // This phase includes collecting execution data and storing results
                        let write_start = std::time::Instant::now();
                        let mut task_result = TaskResultItem::default();
                        task_result.gas_limit = task.gas;

                        // Track read-write access for conflict detection
                        // This information is crucial for maintaining consistency
                        // Track read-write access for conflict detection
                        // This information is crucial for maintaining consistency
                        if enable_ssa {
                            if let Some(mut logger) = evm.take_ssa_logger() {
                                task_result.ssa_rw_set = Some(logger.get_read_write_set());
                                let ssa_logs_raw_ptr = logs_ptr as *mut Option<Vec<SSALogEntry>>;
                                unsafe {
                                    *ssa_logs_raw_ptr.add(idx) = Some(logger.take_logs());
                                }
                                // ! current_lsn is the num of nodes.
                                // eprintln!("logger.current_lsn: {:?}", logger.current_lsn);
                            }
                        } else {
                            let mut read_write_set = evm.get_read_write_set();
                            read_write_set.add_write(task.env.tx.caller, AccessType::Balance);
                            read_write_set.add_write(task.env.tx.caller, AccessType::Nonce);
                            task_result.read_write_set = Some(read_write_set);
                        }

                        // Clean up EVM instance to free resources
                        drop(evm);
                        task_result.inspector = Some(inspector);

                        // Store execution results based on success/failure
                        match result {
                            Ok(result_and_state) => {
                                let ResultAndState { state, result } = result_and_state;
                                task_result.state = Some(state);
                                task_result.result = Some(result);
                            }
                            Err(_) => {
                                task_result.state = None;
                                task_result.result = None;
                            }
                        }

                        // Store results using direct pointer access for performance
                        // This avoids unnecessary copying and allocation
                        let result_raw_ptr = result_ptr as *mut TaskResultItem<I>;
                        unsafe {
                            *result_raw_ptr.add(idx) = task_result;
                        }
                        let write_end = std::time::Instant::now();
                        write_result_time += write_end - write_start;
                    }

                    // Store timing metrics for this thread
                    thread_times.write()[thread_id] = (db_read_time, init_time, transact_time, write_result_time);
                    
                    // Log detailed transaction timing statistics
                    // This helps identify performance patterns and outliers
                    
                });
        });
        
        let parallel_end = std::time::Instant::now();
        parallel_end - parallel_start
    }

    /// Main execution function that processes transactions in parallel
    /// 
    /// This is the core of OCCDA implementation, featuring:
    /// - Dynamic task scheduling
    /// - Parallel execution with conflict detection
    /// - Ordered commit phase
    /// - Performance monitoring
    /// 
    /// The execution follows these phases:
    /// 1. Preparation: Find ready tasks
    /// 2. Execution: Run tasks in parallel
    /// 3. Validation: Check for conflicts
    /// 4. Commit/Retry: Commit successful tasks, retry conflicts
    /// 
    /// Performance tracking includes:
    /// - Preparation time
    /// - Parallel execution time
    /// - Sequential execution time
    /// - Commit time
    /// - Conflict rates
    /// 
    /// TODO: Consider implementing:
    /// - Adaptive batch sizing
    /// - More sophisticated conflict resolution
    /// - Better load balancing strategies
    pub fn main_with_db<DB, I>(
        &mut self,
        h_tx: &mut Vec<Task>,
        db: &mut DB,
        result_store: &mut Vec<TaskResultItem<I>>,
        logs_store: &mut Vec<Option<Vec<SSALogEntry>>>,
        to_re_execution_store: &mut Vec<Option<Vec<usize>>>,
        dag_store: &mut Vec<Arc<OnceCell<Arc<SsaGraph>>>>,
        inspector_setup: impl Fn() -> I + Send + Sync,
        enable_dep_graph: bool,
        enable_ssa: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        DB: DatabaseRef + Database + DatabaseCommit + SsaDatabaseCommit + Send + Sync,
        I: Send + Sync + 'static + 
           for<'db> GetInspector<WrapDatabaseRef<&'db ParallelDB<&'db DB>>> +
           for<'db> Inspector<WrapDatabaseRef<&'db ParallelDB<&'db DB>>>,
        <DB as DatabaseRef>::Error: Send + Sync,
    {
        let raw_db_ptr = db as *mut DB;

        let db_ref_for_parallel: &DB = unsafe { &*raw_db_ptr };
        let mut parallel_db = ParallelDB::new(Arc::new(db_ref_for_parallel));

        // Initialize core metrics
        // tx_size: Total number of unique transactions to process
        // exec_size: Total execution count including retries (used for conflict rate calculation)
        let tx_size = h_tx.len();
        let mut exec_size = 0;
        let mut seq_exec_size = 0;
        let mut parallel_exec_size = 0;
        
        // Task management queues:
        // h_ready: Holds tasks that are ready to execute (all dependencies satisfied)
        // h_exec: Priority queue of tasks ordered by sequence ID (sid)
        // h_commit: Tasks that have finished execution and await commit
        let mut h_ready: Vec<usize> = Vec::new();          // Tasks ready for execution
        let mut h_exec: BinaryHeap<Reverse<(i32, usize)>> = BinaryHeap::new(); // Tasks executing
        let mut h_commit: BinaryHeap<Reverse<usize>> = BinaryHeap::new(); // Tasks ready for commit
        let len = h_tx.len();
        let mut next = 0;

        // Performance monitoring timers for different execution phases
        // These help identify bottlenecks and optimize performance
        let mut perpare_time = Duration::from_secs(0);
        let mut commit_time = Duration::from_secs(0);
        let mut parallel_time = Duration::from_secs(0);
        let mut seq_time = Duration::from_secs(0);
        let mut prefetch_time = Duration::from_secs(0);

        // AccessTracker monitors read/write sets to detect conflicts between transactions
        // This is crucial for maintaining consistency in parallel execution
        let mut access_tracker = if !enable_ssa {
            Some(AccessTracker::new())
        } else {
            None
        };

        let mut ssa_access_tracker = if enable_ssa {
            Some(SsaAccessTracker::new())
        } else {
            None
        };
        
        let prefetch_start = std::time::Instant::now();
        // Set parallel mode for prefetch phase
        if enable_dep_graph {
            parallel_db.set_parallel_mode(true);
            self.execute_parallel_tasks(
                &(0..=len-1).collect(),
                h_tx,
                &mut parallel_db,
                result_store,
                logs_store,
                dag_store,
                to_re_execution_store,
                &inspector_setup,
                true,
                enable_dep_graph,
                enable_ssa
            );
            self.dag = self.build_dag_from_results(result_store);
            self.update_task_sids(h_tx, &self.dag);
            parallel_db.reset_stats();
        }
        
        let prefetch_end = std::time::Instant::now();
        prefetch_time += prefetch_end - prefetch_start;

        // Initialize execution queue with all transactions
        // Each transaction is ordered by its sequence ID for dependency tracking
        for i in 0..len {
            h_exec.push(Reverse((h_tx[i].sid, h_tx[i].tid as usize)));
        }

        while next < len {
            // let shared_parallel_db = Arc::new(parallel_db);
            let perpare_start = std::time::Instant::now();
            // Find all tasks that can be executed in parallel
            // A task is ready when all its dependencies (lower sid) have been committed
            while let Some(Reverse((sid, tid))) = h_exec.pop() {
                if sid <= next as i32 - 1 {
                    h_ready.push(tid);
                } else {
                    h_exec.push(Reverse((sid, tid)));
                    break;
                }
            }

            if h_ready.is_empty() {
                break;
            }

            let perpare_end = std::time::Instant::now();
            perpare_time += perpare_end - perpare_start;

            // Prepare batch of tasks for execution
            exec_size += h_ready.len();
            let ready_tasks = std::mem::take(&mut h_ready);
            
            // Set mode based on execution path
            if ready_tasks.len() < self.num_threads {
                parallel_db.set_parallel_mode(false);
                let seq_start = std::time::Instant::now();
                seq_exec_size += ready_tasks.len();
                
                for &idx in &ready_tasks {
                    let task = &mut h_tx[idx];
                    let mut inspector = inspector_setup();

                    // Handle re-execution case (using SSA)
                    if to_re_execution_store[idx].is_some() {
                        let to_re_execute = to_re_execution_store[idx].as_ref().unwrap();
                        let wait_start = std::time::Instant::now();
                        while dag_store[idx].get().is_none() {
                            std::hint::spin_loop();
                        }
                        let wait_end = std::time::Instant::now();
                        let wait_time = wait_end - wait_start;
                        histogram!("ssa_graph_wait", wait_time);
                        let graph = dag_store[idx].get().unwrap().clone();
                        let execution_mode = ExecutionMode::Partial(to_re_execute.clone());
                        let mut executor = SSAExecutor::<_, LatestSpec>::new(graph, &parallel_db, &task.env, None)
                            .with_mode(execution_mode);
                        
                        let ssa_execute_start = std::time::Instant::now();
                        match executor.execute() {
                            Ok(()) => {
                                let ssa_execute_end = std::time::Instant::now();
                                let ssa_execute_elapse = ssa_execute_end - ssa_execute_start;
                                histogram!("ssa_execute",ssa_execute_elapse);
                                let (result_state, storage_keys) = executor.graph.get_storage_write_outputs().unwrap();
                                let mut task_result: TaskResultItem<I> = TaskResultItem::default();
                                task_result.ssa_state = Some(result_state);
                                task_result.ssa_rw_set = Some(SsaRwSet::new_with_write_set(storage_keys));
                                result_store[idx] = task_result;
                                drop(executor);
                                continue;
                            }
                            Err(e) => {
                                let ssa_execute_end = std::time::Instant::now();
                                let ssa_execute_elapse = ssa_execute_end - ssa_execute_start;
                                histogram!("ssa_execute",ssa_execute_elapse);
                                eprintln!("SSA re-execution failed: {:?}, fall back to EVM re-execution.", e);
                                drop(executor);
                                // fall through to EVM re-execution path below
                            }
                        }
                    }

                   // Normal execution path
                   let mut evm = if !enable_ssa {
                        Evm::builder()
                            .with_ref_db(&parallel_db)
                            .modify_env(|env| env.clone_from(&task.env))
                            .with_external_context(&mut inspector)
                            .with_spec_id(task.spec_id)
                            .append_handler_register(inspector_handle_register)
                            .build()
                    } else {
                        // Enable SSA recording
                        Evm::builder()
                            .with_ref_db(&parallel_db)
                            .modify_env(|env| env.clone_from(&task.env))
                            .with_external_context(&mut inspector)
                            .with_spec_id(task.spec_id)
                            .append_handler_register(inspector_handle_register)
                            .with_ssa_logger(SSALogger::new())
                            .build_with_ssa_logger()
                    };

                    let transact_start = std::time::Instant::now();
                    let result = evm.transact();
                    let transact_end = std::time::Instant::now();
                    let transact_time = transact_end - transact_start;
                    histogram!("evm.transact", transact_time);
                    let mut task_result = TaskResultItem::default();    
                     // Record SSA read-write set
                     if enable_ssa {
                        if let Some(mut logger) = evm.take_ssa_logger() {
                            task_result.ssa_rw_set = Some(logger.get_read_write_set());
                            logs_store[idx] = Some(logger.take_logs());
                        }
                    } else {
                        let mut read_write_set = evm.get_read_write_set();
                        read_write_set.add_write(task.env.tx.caller, AccessType::Balance);
                        read_write_set.add_write(task.env.tx.caller, AccessType::Nonce);
                        task_result.read_write_set = Some(read_write_set);
                    }
                    drop(evm);
                    task_result.inspector = Some(inspector);
                    
                    match result {
                        Ok(result_and_state) => {
                            let ResultAndState { state, result } = result_and_state;
                            task_result.state = Some(state);
                            task_result.result = Some(result);
                        }
                        Err(_) => {
                            task_result.state = None;
                            task_result.result = None;
                        }
                    }
                    
                    result_store[idx] = task_result;
                }
                
                let seq_end = std::time::Instant::now();
                seq_time += seq_end - seq_start;
                
            } else {
                parallel_exec_size += ready_tasks.len();
                parallel_db.set_parallel_mode(true);
                parallel_time += self.execute_parallel_tasks(
                    &ready_tasks,
                    h_tx,
                    &mut parallel_db,
                    result_store,
                    logs_store,
                    dag_store,
                    to_re_execution_store,
                    &inspector_setup,
                    false,
                    enable_dep_graph,
                    enable_ssa
                );
            }


            // Prepare completed tasks for commit phase
            h_commit.extend(ready_tasks.iter().map(|&idx| Reverse(idx)));

            let commit_start = std::time::Instant::now();
            
            // Commit phase: process transactions in sequential order
            // This ensures consistency and handles conflicts
            loop {
                let Some(Reverse(task_idx)) = h_commit.pop() else {
                    break;
                };

                // Ensure sequential commit order
                if h_tx[task_idx].tid != next as i32 {
                    h_commit.push(Reverse(task_idx));
                    break;
                }

                let task_result = &mut result_store[task_idx as usize];

                let is_conflict = if h_tx[task_idx].sid ==  h_tx[task_idx].tid - 1 {
                    false
                } else if !enable_ssa {
                    let read_write_set = task_result.read_write_set.as_ref().unwrap();
                    let check_conflict_start = std::time::Instant::now();
                    let conflict = access_tracker.as_ref().unwrap().check_conflict_in_range(
                        &read_write_set.read_set,
                        h_tx[task_idx].sid + 1,
                        h_tx[task_idx].tid,
                    );
                    let check_conflict_end = std::time::Instant::now();
                    let check_conflict_time = check_conflict_end - check_conflict_start;
                    histogram!("access_tracker.check_conflict", check_conflict_time);
                    conflict.is_some()
                } else {
                    let ssa_rw_set = task_result.ssa_rw_set.as_ref().unwrap();
                    let check_conflict_start = std::time::Instant::now();
                    let conflicts = ssa_access_tracker.as_ref().unwrap().query_conflicts(
                        &ssa_rw_set.get_read_keys(),
                        h_tx[task_idx].sid + 1,
                        h_tx[task_idx].tid,
                    );
                    let check_conflict_end = std::time::Instant::now();
                    let check_conflict_time = check_conflict_end - check_conflict_start;
                    histogram!("ssa_access_tracker.check_conflict", check_conflict_time);
                    let lsns = conflicts.iter().map(|key| ssa_rw_set.read_set[key]).collect::<Vec<_>>();
                    if !conflicts.is_empty() {
                        to_re_execution_store[task_idx] = Some(lsns);
                    }
                    !conflicts.is_empty()
                };

                // Handle conflicts or commit changes
                if is_conflict {
                    // Conflict detected: update sid and retry
                    h_tx[task_idx].sid = h_tx[task_idx].tid - 1;
                    if enable_ssa {
                        let cell = dag_store[task_idx].clone();
                        let entries = logs_store[task_idx].take().unwrap();
                        std::thread::spawn(move || {
                            // eprintln!("building ssa graph for idx: {}", task_idx);
                            build_ssa_graph(entries, cell);
                            // eprintln!("building ssa graph for idx: {} done", task_idx);
                        });
                    }
                    h_exec.push(Reverse((h_tx[task_idx].sid, h_tx[task_idx].tid as usize)));
                    // eprintln!("Sending task {} to re-execution", task_idx);
                } else {
                    if !enable_ssa {
                        if let Some(state) = task_result.state.clone() {
                            let parallel_commit_start = std::time::Instant::now();
                            parallel_db.commit(state.clone());
                            let parallel_commit_end = std::time::Instant::now();
                            let parallel_commit_time = parallel_commit_end - parallel_commit_start;
                            histogram!("parallel_db.commit", parallel_commit_time);

                            let db_commit_start = std::time::Instant::now();
                            unsafe {
                                (*raw_db_ptr).commit(state);
                            }
                            let db_commit_end = std::time::Instant::now();
                            let db_commit_time = db_commit_end - db_commit_start;
                            histogram!("db.commit", db_commit_time);
                        }
                        let read_write_set = task_result.read_write_set.as_ref().unwrap();
                        let record_access_start = std::time::Instant::now();
                        access_tracker.as_mut().unwrap().record_write_set(
                            h_tx[task_idx].tid,
                            &read_write_set.write_set
                        );
                        let record_access_end = std::time::Instant::now();
                        let record_access_time = record_access_end - record_access_start;
                        histogram!("access_tracker.record_write_set", record_access_time);
                    } else {
                        if let Some(state) = task_result.ssa_state.clone() {
                            let parallel_commit_start = std::time::Instant::now();
                            parallel_db.commit_ssa_storage(state.clone());
                            let parallel_commit_end = std::time::Instant::now();
                            let parallel_commit_time = parallel_commit_end - parallel_commit_start;
                            histogram!("parallel_db.commit_ssa_storage", parallel_commit_time);

                            let db_commit_start = std::time::Instant::now();
                            unsafe {
                                (*raw_db_ptr).commit_ssa_storage(state);
                            }
                            let db_commit_end = std::time::Instant::now();
                            let db_commit_time = db_commit_end - db_commit_start;
                            histogram!("db.commit_ssa_storage", db_commit_time);
                        } else if let Some(state) = task_result.state.clone() {
                            let parallel_commit_start = std::time::Instant::now();
                            parallel_db.commit(state.clone());
                            let parallel_commit_end = std::time::Instant::now();
                            let parallel_commit_time = parallel_commit_end - parallel_commit_start;
                            histogram!("parallel_db.commit", parallel_commit_time);

                            let db_commit_start = std::time::Instant::now();
                            unsafe {
                                (*raw_db_ptr).commit(state);
                            }
                            let db_commit_end = std::time::Instant::now();
                            let db_commit_time = db_commit_end - db_commit_start;
                            histogram!("db.commit", db_commit_time);
                        }
                        let ssa_rw_set = task_result.ssa_rw_set.as_ref().unwrap();
                        let record_access_start = std::time::Instant::now();
                        ssa_access_tracker.as_mut().unwrap().record_access(
                            &ssa_rw_set.write_set,
                            h_tx[task_idx].tid,
                        );
                        let record_access_end = std::time::Instant::now();
                        let record_access_time = record_access_end - record_access_start;
                        histogram!("ssa_access_tracker.record_access", record_access_time);
                    }
                    next += 1;
                }
            }
            let commit_end = std::time::Instant::now();
            commit_time += commit_end - commit_start;
        }

        // Calculate final statistics and conflict rate
        // High conflict rates might indicate need for better task scheduling
        let conflict_rate = ((exec_size - tx_size) as f64) / (tx_size as f64) * 100.0;
        println!(
            "finished execute tasks size: {} with conflict rate: {:.2}%",
            result_store.len(),
            conflict_rate
        );
        
        // Log detailed timing breakdown for performance analysis
        println!("perpare_time: {:?}", perpare_time);
        println!("parallel_time: {:?}", parallel_time);
        println!("seq_time: {:?}", seq_time);
        println!("commit_time: {:?}", commit_time); 
        println!("prefetch_time: {:?}", prefetch_time);
        // Clean up resources in background to avoid blocking
        // This includes access tracker and task management queues
        std::thread::spawn(move || {
            drop(access_tracker);
            drop(h_exec);
            drop(h_commit);
            drop(h_ready);
        });

        for i in 0..result_store.len() {
            if result_store[i].state.is_none() && result_store[i].ssa_state.is_none() {
                println!("failed task: {:?}", h_tx[i].tid);
            }
        }

        // Print both parallel and sequential stats at the end
        println!("Parallel execution stats:");
        let (hit_rate, hits, misses, db_time, cache_time, max_read, avg_read) = {
            parallel_db.set_parallel_mode(true);
            parallel_db.get_stats()
        };
        println!("  hit_rate: {:.2}%, hits: {}, misses: {}", hit_rate, hits, misses);
        println!("  db_read: {:?}, cache_access: {:?}", db_time, cache_time);
        println!("  max_read: {:?}, avg_read: {:?}", max_read, avg_read);

        println!("\nSequential execution stats:");
        let (hit_rate, hits, misses, db_time, cache_time, max_read, avg_read) = {
            parallel_db.set_parallel_mode(false);
            parallel_db.get_stats()
        };
        println!("  hit_rate: {:.2}%, hits: {}, misses: {}", hit_rate, hits, misses);
        println!("  db_read: {:?}, cache_access: {:?}", db_time, cache_time);
        println!("  max_read: {:?}, avg_read: {:?}", max_read, avg_read);
        println!("  seq_exec_size: {}, parallel_exec_size: {}", seq_exec_size, parallel_exec_size);
        
        // let addr1 = address!("7d902220f0c3c53281d310a5ad4e9514e1d24296");
        // let addr2 = address!("c8d700eb8cfbfa08552e7f63a6fcedd3672d1c41");
        // let addr3 = address!("ecded4f38f7cca4f472086b9a26d4de2a3cf903b");
        // let addr4 = address!("f8e95297dba53ccf8cb62dbd8a28b934580884ee");
        // let addr5 = address!("ff69d3dba117a55ba29a24610d67135b82dc0e58");
        // let account1 = parallel_db.basic_ref(addr1).map_err(|_|());
        // let account2 = parallel_db.basic_ref(addr2).map_err(|_|());
        // let account3 = parallel_db.basic_ref(addr3).map_err(|_|());
        // let account4 = parallel_db.basic_ref(addr4).map_err(|_|());
        // let account5 = parallel_db.basic_ref(addr5).map_err(|_|());

        // let contract_addr = address!("b30df92bb107e6f1e46f7df4fd31a316ceb4e7d9");
        // let storage = parallel_db.cache.read().accounts.get(&contract_addr).unwrap().clone().storage;
        // eprintln!("\n===========================================");
        // eprintln!("              ParallelDB State             ");
        // eprintln!("===========================================");
        // eprintln!("\n------------- Normal Accounts -------------");
        // eprintln!("Account 1: {:?}", account1);
        // eprintln!("Account 2: {:?}", account2);
        // eprintln!("Account 3: {:?}", account3); 
        // eprintln!("Account 4: {:?}", account4);
        // eprintln!("Account 5: {:?}", account5);

        // eprintln!("\n------------- Contract Storage -------------");
        // eprintln!("Storage Content: {:?}", storage);
        // eprintln!("===========================================\n");

        Ok(())
    }

    /// Checks if two access sets have any overlapping addresses with matching access types
    /// 
    /// This function determines if there are any conflicts between two access sets by checking:
    /// 1. If they share any common addresses
    /// 2. If the AccessType sets for those addresses have any elements in common
    /// 
    /// # Parameters
    /// * `set1`, `set2` - HashMaps mapping addresses to their AccessType sets
    /// 
    /// # Returns
    /// * `bool` - true if there exists at least one address where both sets have a common AccessType
    fn has_intersection(
        set1: &HashMap<Address, HashSet<AccessType>>,
        set2: &HashMap<Address, HashSet<AccessType>>
    ) -> bool {
        set1.iter().any(|(addr, types1)| {
            if let Some(types2) = set2.get(addr) {
                // Check if there's any common AccessType for this address
                types1.intersection(types2).next().is_some()
            } else {
                false
            }
        })
    }

    /// Builds a dependency graph (DAG) from transaction execution results
    /// 
    /// Analyzes read/write sets of executed transactions to construct a directed
    /// acyclic graph representing dependencies between transactions. An edge is added
    /// when a task's write set conflicts with any previous task's read set.
    /// 
    /// # Parameters
    /// * `result_store` - Vector of transaction execution results containing access patterns
    /// 
    /// # Returns
    /// * `TaskDag` - Constructed dependency graph
    fn build_dag_from_results<I>(&self, result_store: &[TaskResultItem<I>]) -> TaskDag {
        let dag_start = std::time::Instant::now();
        let mut dag = TaskDag::new();
        
        // Create nodes for all transactions
        for i in 0..result_store.len() {
            dag.add_task(&Task { tid: i as i32, ..Default::default() });
        }
        
        // For each task i, check conflicts with all previous tasks
        for i in 0..result_store.len() {
            let tx_i = &result_store[i];
            if let Some(rw_set_i) = &tx_i.read_write_set {
                // Check against all previous tasks (with smaller indices)
                for j in 0..i {
                    let tx_j = &result_store[j];
                    if let Some(rw_set_j) = &tx_j.read_write_set {
                        // Check if task i's write set conflicts with task j's read set
                        if Self::has_intersection(&rw_set_i.write_set, &rw_set_j.read_set) {
                            // Add dependency: j must complete before i
                            dag.add_dependency(
                                &Task { tid: i as i32, ..Default::default() },
                                &Task { tid: j as i32, ..Default::default() }
                            );
                        }
                    }
                }
            }
        }
        
        let dag_end = std::time::Instant::now();
        println!("dag_time: {:?}", dag_end - dag_start);
        dag
    }

    /// Updates sequence IDs (sid) of tasks based on their dependencies in the DAG
    /// 
    /// For each task, sets its sequence ID to the maximum transaction ID (tid) among
    /// its dependencies. This ensures that a task won't execute until all its
    /// dependencies have completed.
    /// 
    /// # Parameters
    /// * `h_tx` - Mutable slice of tasks to update
    /// * `dag` - Reference to the dependency graph
    fn update_task_sids(&self, h_tx: &mut [Task], dag: &TaskDag) {
        for task in h_tx.iter_mut() {
            let dependencies = dag.get_dependencies(task);
            let sid_max = dependencies
                .into_iter()
                .map(|node| dag.get_task_tid(node).unwrap_or(-1))
                .max()
                .unwrap_or(-1);
            task.sid = sid_max;
        }
    }
}

fn build_ssa_graph(entries: Vec<SSALogEntry>, cell: Arc<OnceCell<Arc<SsaGraph>>>) {
    let mut graph = SsaGraph::new(entries.len(), 2*entries.len());
    let lsns: Vec<usize> = entries.iter().map(|entry| entry.lsn).collect();
    for entry in entries {
        graph.add_node(entry).unwrap();
    }
    for lsn in lsns {
        graph.add_edges(lsn).unwrap();
    }
    if cell.set(Arc::new(graph)).is_err() {
        eprintln!("SSA graph was already set");
    }
}
