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
use crate::profiler;
use std::sync::Arc;
use rayon::ThreadPool;
use rayon::prelude::*;
use revm_primitives::{address, LatestSpec};
use revm_ssa::logger::SsaRwSet;
use revm_ssa::SSALogger;
use revm_ssa_graph::{ExecutionMode, SSAExecutor, SsaGraph};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::Duration;

/// Main struct for handling parallel execution of EVM transactions
pub struct Occda {
    /// Dependency graph for tasks
    /// Used to determine execution order and detect conflicts
    _dag: TaskDag,
    
    /// Number of worker threads for parallel execution
    num_threads: usize,
    
    /// Thread pool for managing parallel execution
    /// Pre-initialized to avoid creation overhead
    thread_pool: ThreadPool,

    /// A flag to control whether SSA is enabled
    _enable_ssa: bool
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
            _dag: TaskDag::new(),
            num_threads,
            thread_pool,
            _enable_ssa: false
        }
    }

    pub fn enable_ssa(&mut self) {
        self._enable_ssa = true;
    }

    pub fn disable_ssa(&mut self) {
        self._enable_ssa = false;
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
    /// 
    /// Returns:
    /// - Duration: Total time spent in parallel execution
    fn execute_parallel_tasks<DB, I>(
        &self,
        ready_tasks: &Vec<usize>,
        h_tx: &[Task],
        db: &mut ParallelDB<&DB>,
        result_store: &mut Vec<TaskResultItem<I>>,
        inspector_setup: impl Fn() -> I + Send + Sync,
    ) -> Duration 
    where
        DB: DatabaseRef + Database + DatabaseCommit + Send + Sync,
        I: Send + Sync + 'static + 
           for<'db> GetInspector<WrapDatabaseRef<&'db ParallelDB<&'db DB>>> +
           for<'db> Inspector<WrapDatabaseRef<&'db ParallelDB<&'db DB>>>,
        <DB as DatabaseRef>::Error: Send + Sync,
    {
        // Get raw pointer to result store for direct memory access in parallel execution
        // This avoids unnecessary copying and improves performance
        let result_ptr = result_store.as_mut_ptr() as usize;


        let parallel_start = std::time::Instant::now();
        
        // Initialize chunks and gas counters for load balancing
        let mut chunks: Vec<Vec<usize>> = vec![Vec::new(); self.num_threads];
        let mut thread_gas: Vec<u64> = vec![0; self.num_threads];
        
        // Sort tasks by gas cost for better load balancing
        let mut sorted_tasks: Vec<_> = ready_tasks.iter()
            .map(|&idx| (idx, h_tx[idx].gas))
            .collect();
        sorted_tasks.sort_by_key(|&(_, gas)| std::cmp::Reverse(gas));
        
        // Assign tasks using greedy algorithm
        for (idx, gas) in sorted_tasks {
            let target_thread = thread_gas
                .iter()
                .enumerate()
                .min_by_key(|&(_, g)| g)
                .map(|(i, _)| i)
                .unwrap();
            
            chunks[target_thread].push(idx);
            thread_gas[target_thread] += gas;
        }

        // Track thread execution times
        let thread_times = Arc::new(parking_lot::RwLock::new(vec![(
            Duration::from_secs(0), Duration::from_secs(0),
            Duration::from_secs(0), Duration::from_secs(0)); self.num_threads]));

        // Execute chunks in parallel
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
                        if task.to_re_execute.is_some() {
                            let entries = task.logs.as_ref().unwrap();
                            // eprintln!("entries: {:?}", entries);
                            let to_re_execute = task.to_re_execute.as_ref().unwrap();
                            // eprintln!("to_re_execute: {:?}", to_re_execute);
                            let mut graph = SsaGraph::new();
                            let lsns: Vec<usize> = entries.iter().map(|entry| entry.lsn).collect();
                            for entry in entries {
                                graph.add_node(entry.clone()).unwrap();
                            }
                            for lsn in lsns {
                                graph.add_edges(lsn).unwrap();
                            }

                            let execution_mode = ExecutionMode::Partial(to_re_execute.clone()); 
                            let mut executor = SSAExecutor::<_, LatestSpec>::new(graph, db_ref, &task.env, None).with_mode(execution_mode);
                            match executor.execute() {
                                Ok(()) => {
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
                                    eprintln!("SSA re-execution failed: {:?}, fall back to EVM re-execution.", e);
                                    drop(executor);
                                    // fall through to EVM re-execution path below
                                }
                            }
                        }

                        // Initialize EVM instance with task-specific configuration
                        // Measure setup time separately from execution time
                        let init_start = std::time::Instant::now();
                        let mut evm = if !self._enable_ssa {
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
                        transact_time += this_transact_time;
                        transact_times.push(this_transact_time);

                        // Process and store execution results
                        // This phase includes collecting execution data and storing results
                        let write_start = std::time::Instant::now();
                        let mut task_result = TaskResultItem::default();
                        task_result.gas = task.gas;

                        // Track read-write access for conflict detection
                        // This information is crucial for maintaining consistency
                        if self._enable_ssa {
                            if let Some(logger) = evm.take_ssa_logger() {
                                task_result.ssa_rw_set = Some(logger.get_read_write_set());
                                task_result.logs = Some(logger.get_logs().to_vec());
                            }
                        } else {
                            let mut read_write_set = evm.get_read_write_set();
                            read_write_set.add_write(task.env.tx.caller, AccessType::AccountInfo);
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
                                task_result.gas = 0;
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
                });
        });
        
        let parallel_end = std::time::Instant::now();
        let parallel_time = parallel_end - parallel_start;

        parallel_time
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
        inspector_setup: impl Fn() -> I + Send + Sync,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        DB: DatabaseRef + Database + DatabaseCommit + Send + Sync,
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

        // AccessTracker monitors read/write sets to detect conflicts between transactions
        // This is crucial for maintaining consistency in parallel execution
        let mut access_tracker = if !self._enable_ssa {
            Some(AccessTracker::new())
        } else {
            None
        };

        let mut ssa_access_tracker = if self._enable_ssa {
            Some(SsaAccessTracker::new())
        } else {
            None
        };
        

        self.execute_parallel_tasks(
            &(0..=len-1).collect(),
            h_tx,
            &mut parallel_db,
            result_store,
            &inspector_setup
        );
        let dag = self.build_dag_from_results(result_store);
        self.update_task_sids(h_tx, &dag);
        parallel_db.reset_stats();

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
            
            if ready_tasks.len() < self.num_threads {
                let seq_start = std::time::Instant::now();
                profiler::start_multiple_times_in_main_thread("non-parallel");
                profiler::note_str_multiple_times_in_main_thread("non-parallel", "type", "non-parallel");
 
                // Use &ready_tasks to iterate over references instead of taking ownership
                for &idx in &ready_tasks {
                    let task = &mut h_tx[idx];
                    let mut inspector = inspector_setup();

                    // Handle re-execution case (using SSA)
                    if task.to_re_execute.is_some() {
                        let entries = task.logs.as_ref().unwrap();
                        let to_re_execute = task.to_re_execute.as_ref().unwrap();

                        let mut graph = SsaGraph::new();
                        let lsns: Vec<usize> = entries.iter().map(|entry| entry.lsn).collect();
                        for entry in entries {
                            graph.add_node(entry.clone()).unwrap();
                        }
                        for lsn in lsns {
                            graph.add_edges(lsn).unwrap();
                        }

                        let execution_mode = ExecutionMode::Partial(to_re_execute.clone());
                        let mut executor = SSAExecutor::<_, LatestSpec>::new(graph, &parallel_db, &task.env, None)
                            .with_mode(execution_mode);
                        match executor.execute() {
                            Ok(()) => {
                                let (result_state, storage_keys) = executor.graph.get_storage_write_outputs().unwrap();
                                let mut task_result: TaskResultItem<I> = TaskResultItem::default();
                                task_result.ssa_state = Some(result_state);
                                task_result.ssa_rw_set = Some(SsaRwSet::new_with_write_set(storage_keys));
                                result_store[idx] = task_result;
                                drop(executor);
                                continue;
                            }
                            Err(e) => {
                                eprintln!("SSA re-execution failed: {:?}, fall back to EVM re-execution.", e);
                                drop(executor);
                                // fall through to EVM re-execution path below
                            }
                        }
                    }

                    // Normal execution path
                    let mut evm = if !self._enable_ssa {
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

                    let result = evm.transact();

                    let mut task_result = TaskResultItem::default();    
                    task_result.gas = task.gas;

                    // Record SSA read-write set
                    if self._enable_ssa {
                        if let Some(logger) = evm.take_ssa_logger() {
                            task_result.ssa_rw_set = Some(logger.get_read_write_set());
                            task_result.logs = Some(logger.get_logs().to_vec());
                        }
                    } else {
                        let mut read_write_set = evm.get_read_write_set();
                        read_write_set.add_write(task.env.tx.caller, AccessType::AccountInfo);
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
                            task_result.gas = 0;
                        }
                    }
                    result_store[idx] = task_result;
                }
                
                let seq_end = std::time::Instant::now();
                seq_time += seq_end - seq_start;
                profiler::end_multiple_times_in_main_thread("non-parallel");
                
            } else {
                profiler::start_multiple_times_in_main_thread("parallel");
                profiler::note_str_multiple_times_in_main_thread("parallel", "type", "parallel");
                parallel_time += self.execute_parallel_tasks(
                    &ready_tasks,
                    h_tx,
                    &mut parallel_db,
                    result_store,
                    &inspector_setup
                );
                profiler::end_multiple_times_in_main_thread("parallel");
            }

            // Prepare completed tasks for commit phase
            h_commit.extend(ready_tasks.iter().map(|&idx| Reverse(idx)));

            let commit_start = std::time::Instant::now();
            profiler::start_multiple_times_in_main_thread("commit-all");
            profiler::note_str_multiple_times_in_main_thread("commit-all", "type", "commit");
            
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
                // eprintln!("idx:{}, rw_set:{:?}", task_idx, task_result.ssa_rw_set);
                // Check for conflicts
                let is_conflict = if h_tx[task_idx].sid ==  h_tx[task_idx].tid - 1 {
                    false
                } else if !self._enable_ssa {
                    let read_write_set = task_result.read_write_set.as_ref().unwrap();
                    let conflict = access_tracker.as_ref().unwrap().check_conflict_in_range(
                        &read_write_set.read_set,
                        h_tx[task_idx].sid + 1,
                        h_tx[task_idx].tid,
                    );
                    conflict.is_some()
                } else {
                    let ssa_rw_set = task_result.ssa_rw_set.as_ref().unwrap();
                    let conflicts = ssa_access_tracker.as_ref().unwrap().query_conflicts(
                        &ssa_rw_set.get_read_keys(),
                        h_tx[task_idx].sid + 1,
                        h_tx[task_idx].tid,
                    );
                    // eprintln!("idx: {:?}, conflicts: {:?}", task_idx, conflicts);
                    let lsns = conflicts.iter().map(|key| ssa_rw_set.read_set[key]).collect::<Vec<_>>();
                    if !conflicts.is_empty() {
                        h_tx[task_idx].to_re_execute = Some(lsns);

                        h_tx[task_idx].logs = Some(task_result.logs.as_ref().unwrap().to_vec());
                    }
                    !conflicts.is_empty()
                };

                // eprintln!("idx: {:?}, is_conflict: {:?}", task_idx, is_conflict);
                // Handle conflicts or commit changes
                if is_conflict {
                    // Conflict detected: update sid and retry
                    h_tx[task_idx].sid = h_tx[task_idx].tid - 1;
                    // h_tx.push(value);
                    h_exec.push(Reverse((h_tx[task_idx].sid, h_tx[task_idx].tid as usize)));
                } else {
                    if !self._enable_ssa {
                        if let Some(state) = task_result.state.clone() {
                            parallel_db.commit(state.clone());
                            unsafe {
                                (*raw_db_ptr).commit(state);
                            }
                        }
                        let read_write_set = task_result.read_write_set.as_ref().unwrap();
                        access_tracker.as_mut().unwrap().record_write_set(
                            h_tx[task_idx].tid,
                            &read_write_set.write_set
                        );
                    } else {
                        if let Some(state) = task_result.ssa_state.clone() {
                            parallel_db.commit_ssa_storage(state);
                            // TODO: we did not implemente commit_ssa_storage to the raw_db_ptr
                            // unsafe {
                            //     (*raw_db_ptr).commit(state);
                            // }
                        } else if let Some(state) = task_result.state.clone() {
                            parallel_db.commit(state.clone());
                            unsafe {
                                (*raw_db_ptr).commit(state);
                            }
                        }
                        let ssa_rw_set = task_result.ssa_rw_set.as_ref().unwrap();
                        ssa_access_tracker.as_mut().unwrap().record_access(
                            &ssa_rw_set.write_set,
                            h_tx[task_idx].tid,
                        );
                    }
                    next += 1;
                }
            }
            let commit_end = std::time::Instant::now();
            commit_time += commit_end - commit_start;
            profiler::end_multiple_times_in_main_thread("commit-all");
        }

        // Calculate final statistics and conflict rate
        // High conflict rates might indicate need for better task scheduling
        let conflict_rate = ((exec_size - tx_size) as f64) / (tx_size as f64) * 100.0;
        profiler::start("conflict-rate");
        profiler::note_str("conflict-rate", "type", "conflict-rate");
        profiler::note_str("conflict-rate", "value", &conflict_rate.to_string());
        profiler::end("conflict-rate");
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
        // let addr1 = address!("b30df92bb107e6f1e46f7df4fd31a316ceb4e7d9");
        // let addr2 = address!("ffb2aeba702be6e5d0b8d09b28e0196455e41272");
        // let addr3 = address!("1d9e7ceb63304f68eb5f2f27b21a6a974d691251");
        // let account1 = parallel_db.cache.read().accounts.get(&addr1).unwrap().clone();
        // let account2 = parallel_db.basic_ref(addr2).map_err(|_|());

        // let account3 = parallel_db.basic_ref(addr3).map_err(|_|());
        // eprintln!("account1:{:?}", account1.storage);
        // eprintln!("account2:{:?}", account2);
        // eprintln!("account3:{:?}", account3);
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
    /// from transaction i to j if:
    /// when a task's write set conflicts with any previous task's read set.
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