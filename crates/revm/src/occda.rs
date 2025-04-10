use crate::access_tracker::AccessTracker;
use crate::dag::TaskDag;
use crate::db::{parallel_db::ParallelDB, Database, DatabaseCommit, DatabaseRef};
use crate::evm::Evm;
use crate::graph_wrapper::GraphWrapper;
use crate::inspector_handle_register;
use crate::inspectors::{NoOpInspector, TracerEip3155};
use crate::journaled_state::AccessType;
/// OCCDA (Optimistic Concurrent Contract Deterministic Aborts)
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
use crate::primitives::{Address, HashMap, HashSet, ResultAndState};
use crate::profiler;
use crate::task::{Task, TaskResultItem};
use std::sync::Arc;
// use metrics::histogram;
use parking_lot::RwLock;
use rayon::prelude::*;
use rayon::ThreadPool;
use revm_primitives::SpecId::LONDON;
use revm_primitives::{fixed_bytes, Account, AccountStatus, EVMError, EvmStorageSlot, U256};
use revm_ssa::logger::LsnType;
use revm_ssa::{FrameInput, SSAOutput, StorageKey, StorageValue};
use revm_ssa_graph::{ExecutionMode, SSAExecutor};
use std::cmp::Reverse;
use std::collections::BinaryHeap;
use std::time::Duration;

// use metrics::histogram;

/// Main struct for handling parallel execution of EVM transactions
pub struct Occda {
    /// Dependency graph for tasks
    /// Used to determine execution order and detect conflicts
    dag: TaskDag,

    /// Number of worker threads for parallel execution
    num_threads: usize,

    /// Thread pool for managing parallel execution
    /// Pre-initialized to avoid creation overhead
    thread_pool: ThreadPool,

    /// to_re_execution_store
    to_re_execution_store: Vec<Vec<LsnType>>,

    /// dag_store
    dag_store: Vec<Arc<RwLock<GraphWrapper>>>,

    /// reads_store
    reads_store: Vec<HashMap<StorageKey, Vec<LsnType>>>,

    /// first_frame_input_store
    first_frame_input_store: Vec<Option<FrameInput>>,
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
            thread_pool: thread_pool,
            to_re_execution_store: vec![],
            dag_store: vec![],
            reads_store: vec![],
            first_frame_input_store: vec![],
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
    pub fn init(
        &mut self,
        tasks: Vec<Task>,
        graph: Option<&TaskDag>,
        enable_ssa: bool,
    ) -> Vec<Task> {
        let len: usize = tasks.len();
        if enable_ssa {
            self.to_re_execution_store = Vec::<Vec<LsnType>>::with_capacity(len);
            self.dag_store = Vec::<Arc<RwLock<GraphWrapper>>>::with_capacity(len);
            self.reads_store = Vec::<HashMap<StorageKey, Vec<LsnType>>>::with_capacity(len);
            for _ in 0..len {
                self.to_re_execution_store.push(vec![]);
                self.dag_store
                    .push(Arc::new(RwLock::new(GraphWrapper::new())));
                self.reads_store.push(HashMap::default());
            }
        }
        let mut vec = Vec::with_capacity(tasks.len());
        for mut task in tasks {
            if let Some(g) = graph {
                // Find the maximum sid among dependencies
                let sid_max = g
                    .get_dependencies(&task)
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
        &mut self,
        ready_tasks: &Vec<usize>,
        h_tx: &[Task],
        db: &mut ParallelDB<&DB>,
        result_store: &mut Vec<TaskResultItem<I>>,
        opcode_counts_store: &mut Vec<usize>,
        inspector_setup: impl Fn() -> I + Send + Sync,
        is_prefetch: bool,
        enable_dep_graph: bool,
        enable_ssa: bool,
    ) -> (Duration, Vec<usize>)
    where
        I: Send + Sync,
        DB: DatabaseRef + Database + DatabaseCommit + Send + Sync,
        <DB as DatabaseRef>::Error: Send + Sync,
    {
        let failed_task = Arc::new(parking_lot::Mutex::new(Vec::<usize>::new()));
        let failed_task_clone = failed_task.clone();
        let result_ptr = result_store.as_mut_ptr() as usize;
        let reads_ptr = self.reads_store.as_mut_ptr() as usize;
        let first_frame_input_ptr = self.first_frame_input_store.as_mut_ptr() as usize;
        let opcode_counts_ptr = opcode_counts_store.as_mut_ptr() as usize;
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
            let mut visited: HashSet<usize> = HashSet::default();

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
                        let task = Task {
                            tid: idx as i32,
                            ..Default::default()
                        };
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

                min_tid_a
                    .cmp(&min_tid_b)
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
        let thread_times = Arc::new(parking_lot::RwLock::new(vec![
            (
                Duration::from_secs(0),
                Duration::from_secs(0),
                Duration::from_secs(0),
                Duration::from_secs(0)
            );
            self.num_threads
        ]));

        self.thread_pool.install(|| {
            chunks
                .into_par_iter()
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
                    let mut gas_used = 0;
                    let mut re_execution_opcodes = 0;

                    let mut prefetch_time = 0;

                    // Process each transaction assigned to this thread
                    for idx in indexes {
                        let task = &h_tx[idx];
                        // Create new inspector instance for this transaction
                        // Each transaction needs its own inspector to track execution
                        let inspector = inspector_setup();
                        let db_ref = &*db;

                        // use ssa to re-execute the transaction
                        if enable_ssa && !self.to_re_execution_store[idx].is_empty() {
                            // eprintln!("re-execute task: {} with ssa.", idx);
                            let to_re_execute = &self.to_re_execution_store[idx];

                            while !self.dag_store[idx].read().is_built() {
                                std::hint::spin_loop();
                            }
                            let graph = self.dag_store[idx].read().get_graph();
                            let execution_mode = ExecutionMode::Partial(
                                to_re_execute.iter().map(|x| *x).collect::<Vec<_>>(),
                            );
                            let mut executor = SSAExecutor::new_with_spec(
                                graph,
                                db_ref,
                                &task.env,
                                self.first_frame_input_store[idx].clone(),
                                task.spec_id,
                            )
                            .with_mode(execution_mode);

                            profiler::start("ssa-execution");
                            let ssa_execution = executor.execute(task.tx_hash.unwrap_or_default());
                            profiler::end("ssa-execution");

                            match ssa_execution {
                                Ok(nodes_to_execute_len) => {
                                    let result_state =
                                        executor.graph.get_storage_write_outputs().unwrap();
                                    let mut task_result: TaskResultItem<I> =
                                        TaskResultItem::default();
                                    task_result.gas_limit = task.gas;
                                    task_result.inspector = Some(inspector);
                                    task_result.ssa_output = Some(result_state);
                                    task_result.result = Some(
                                        executor
                                            .graph
                                            .generate_result(
                                                task.gas,
                                                task.tx_hash.unwrap_or_default(),
                                            )
                                            .unwrap(),
                                    );
                                    let result_raw_ptr = result_ptr as *mut TaskResultItem<I>;
                                    unsafe {
                                        *result_raw_ptr.add(idx) = task_result;
                                    }
                                    drop(executor);
                                    re_execution_opcodes += nodes_to_execute_len.0;
                                    continue;
                                }
                                Err(_err) => {
                                    // eprintln!("TxHash: {:?} SSA re-execution failed: {:?}, fall back to EVM re-execution.", task.tx_hash, _err);
                                    drop(executor);
                                    re_execution_opcodes += opcode_counts_store[idx];

                                    failed_task_clone.lock().push(idx);
                                    // fall through to EVM re-execution path below
                                }
                            }
                        }
                        // ! Debug for SSA
                        let _tracer_inspector = if task.tx_hash != Some(fixed_bytes!("23f544dfae3fa09b7e7dbf755743d3e05384aa643b1a4acb40642ba8d3a1f639")) {
                            TracerEip3155::new(
                                Box::new(std::io::sink()),
                            ).without_summary()
                        } else {
                            TracerEip3155::new(
                                Box::new(std::fs::File::create("tracer_parallel_prefetch_23f5.json").unwrap()),
                            )
                        };

                        // Initialize EVM instance with task-specific configuration
                        // Measure setup time separately from execution time
                        let init_start = std::time::Instant::now();
                        let mut evm = if is_prefetch && enable_ssa {
                            // enable ssa and prefetch, then we will pre-process the ssa graph
                            let prefetch_start = std::time::Instant::now();
                            let evm_inside = Evm::builder()
                                .with_ref_db(db_ref)
                                .modify_env(|env| env.clone_from(&task.env))
                                .with_external_context(NoOpInspector)
                                .with_spec_id(task.spec_id)
                                .append_handler_register(inspector_handle_register)
                                .with_ssa_logger()
                                .build_with_ssa_logger();
                            prefetch_time += prefetch_start.elapsed().as_nanos();
                            evm_inside
                        } else {
                            Evm::builder()
                                .with_ref_db(db_ref)
                                .modify_env(|env| env.clone_from(&task.env))
                                .with_external_context(NoOpInspector)
                                .with_spec_id(task.spec_id)
                                .append_handler_register(inspector_handle_register)
                                .build()
                        };
                        evm.disable_beneficiary();
                        let init_end = std::time::Instant::now();
                        init_time += init_end - init_start;

                        // Execute the transaction and measure execution time
                        // This is the core EVM execution phase
                        let transact_start = std::time::Instant::now();
                        let result = if is_prefetch {
                            evm.transact_preverified()
                        } else {
                            // let standard_transact_start = std::time::Instant::now();
                            let ret = evm.transact();
                            // let standard_transact_end = std::time::Instant::now();
                            // histogram!("revm.transact.time", standard_transact_end - standard_transact_start);
                            ret
                        };
                        let transact_end = std::time::Instant::now();
                        let this_transact_time = transact_end - transact_start;
                        transact_time += this_transact_time;
                        transact_times.push(this_transact_time);

                        // Process and store execution results
                        // This phase includes collecting execution data and storing results
                        let write_start = std::time::Instant::now();
                        let mut task_result = TaskResultItem::default();
                        task_result.gas_limit = task.gas;
                        // Track read-write access for conflict detection
                        // This information is crucial for maintaining consistency
                        // TODO: modify the logic of rwset and ssa_rwset
                        let read_write_set = evm.get_read_write_set();
                        task_result.read_write_set = Some(read_write_set);

                        if is_prefetch && enable_ssa {
                            let mut logger = evm.take_ssa_logger().unwrap();
                            let logs = logger.take_logs();
                            let graph_wrapper = self.dag_store[idx].clone();
                            self.thread_pool.spawn(move || {
                                let mut graph = graph_wrapper.write();
                                graph.build(logs);
                            });
                            let reads_raw_ptr = reads_ptr as *mut HashMap<StorageKey, Vec<LsnType>>;
                            unsafe {
                                *reads_raw_ptr.add(idx) = logger.take_first_reads();
                            }
                            let first_frame_input_raw_ptr =
                                first_frame_input_ptr as *mut Option<FrameInput>;
                            unsafe {
                                *first_frame_input_raw_ptr.add(idx) =
                                    logger.take_first_frame_input();
                            }
                            let opcode_counts_raw_ptr = opcode_counts_ptr as *mut usize;
                            unsafe {
                                *opcode_counts_raw_ptr.add(idx) = logger.current_lsn as usize;
                            }
                        }

                        // Clean up EVM instance to free resources
                        drop(evm);
                        task_result.inspector = Some(inspector);

                        // Store execution results based on success/failure
                        match result {
                            Ok(result_and_state) => {
                                let ResultAndState { state, result } = result_and_state;
                                gas_used += result.gas_used();
                                task_result.state = Some(state);
                                task_result.result = Some(result);
                            }
                            Err(_err) => {
                                // match _err {
                                //     EVMError::Transaction(error) => println!("TxHash: {:?} failed: Transaction error: {:?}", task.tx_hash, error),
                                //     EVMError::Header(error) => println!("TxHash: {:?} failed: Header error: {:?}", task.tx_hash, error),
                                //     EVMError::Database(_) => println!("TxHash: {:?} failed: DB error", task.tx_hash),
                                //     EVMError::Custom(msg) => println!("TxHash: {:?} failed: Custom error: {}", task.tx_hash, msg),
                                //     EVMError::Precompile(msg) => println!("TxHash: {:?} failed: Precompile error: {}", task.tx_hash, msg),
                                // }
                                failed_task_clone.lock().push(idx);
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
                    thread_times.write()[thread_id] =
                        (db_read_time, init_time, transact_time, write_result_time);

                    profiler::note_str_unchecked(
                        "gas-used",
                        &thread_id.to_string(),
                        &gas_used.to_string(),
                    );
                    profiler::note_str_unchecked(
                        "re-execution-opcodes",
                        &thread_id.to_string(),
                        &re_execution_opcodes.to_string(),
                    );

                    if is_prefetch && enable_ssa {
                        profiler::note_str_unchecked(
                            "metrics",
                            "prefetch",
                            &(prefetch_time as f64 / 1e6).to_string(),
                        );
                    }
                    // Log detailed transaction timing statistics
                    // This helps identify performance patterns and outliers
                });
        });
        // for (thread_id, (db_read, init, transact, write)) in thread_times.read().iter().enumerate() {
        //     println!("Thread {}: DB read time: {:?}, Init time: {:?}, Transaction time: {:?}, Write time: {:?}",
        //         thread_id, db_read, init, transact, write);
        // }
        let parallel_end = std::time::Instant::now();
        let failed_tasks = failed_task.lock().clone();
        (parallel_end - parallel_start, failed_tasks)
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
        enable_prefetch: bool,
        enable_dep_graph: bool,
        enable_ssa: bool,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>>
    where
        I: Send + Sync,
        DB: DatabaseRef + Database + DatabaseCommit + Send + Sync,
        <DB as DatabaseRef>::Error: Send + Sync,
    {
        let len = h_tx.len();
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
        let mut h_ready: Vec<usize> = Vec::new(); // Tasks ready for execution
        let mut h_exec: BinaryHeap<Reverse<(i32, usize)>> = BinaryHeap::new(); // Tasks executing
        let mut h_commit: BinaryHeap<Reverse<usize>> = BinaryHeap::new(); // Tasks ready for commit
        let mut next = 0;

        // Performance monitoring timers for different execution phases
        // These help identify bottlenecks and optimize performance
        let mut prepare_time = Duration::from_secs(0);
        let mut commit_time = Duration::from_secs(0);
        let mut parallel_time = Duration::from_secs(0);
        let mut seq_time = Duration::from_secs(0);

        // AccessTracker monitors read/write sets to detect conflicts between transactions
        // This is crucial for maintaining consistency in parallel execution
        let mut access_tracker = AccessTracker::new();

        let mut opcode_counts_store = Vec::<usize>::with_capacity(len);
        // Initialize the store for ssa re-execution, we count the time in the prefetch phase.
        if enable_ssa {
            self.to_re_execution_store = Vec::<Vec<LsnType>>::with_capacity(len);
            self.dag_store = Vec::<Arc<RwLock<GraphWrapper>>>::with_capacity(len);
            self.reads_store = Vec::<HashMap<StorageKey, Vec<LsnType>>>::with_capacity(len);
            self.first_frame_input_store = Vec::<Option<FrameInput>>::with_capacity(len);
            for _ in 0..len {
                self.to_re_execution_store.push(vec![]);
                self.dag_store
                    .push(Arc::new(RwLock::new(GraphWrapper::new())));
                self.reads_store.push(HashMap::default());
                self.first_frame_input_store.push(None);
                opcode_counts_store.push(0);
            }
        }
        // Set parallel mode for prefetch phase
        if enable_prefetch {
            parallel_db.set_parallel_mode(true);
            self.execute_parallel_tasks(
                &(0..=len - 1).collect(),
                h_tx,
                &mut parallel_db,
                result_store,
                &mut opcode_counts_store,
                &inspector_setup,
                true,
                enable_dep_graph, //  we may enbale prefetch without constructing dependency graph
                enable_ssa,
            );

            if enable_dep_graph {
                self.dag = self.build_dag_from_results(result_store);
                self.update_task_sids(h_tx, &self.dag);
            }
            parallel_db.reset_stats();
        }

        let mut redo_gas_used = 0;
        let mut re_execution_opcodes = 0;
        let mut total_opcodes = 0;

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
            prepare_time += perpare_end - perpare_start;

            // Prepare batch of tasks for execution
            exec_size += h_ready.len();
            let ready_tasks = std::mem::take(&mut h_ready);

            // Set mode based on execution path
            if ready_tasks.len() < self.num_threads {
                parallel_db.set_parallel_mode(false);

                profiler::start("non-parallel");
                let seq_start = std::time::Instant::now();
                seq_exec_size += ready_tasks.len();
                for &idx in &ready_tasks {
                    let task = &mut h_tx[idx];
                    let inspector = inspector_setup();

                    // Handle re-execution case (using SSA)
                    if enable_ssa && !self.to_re_execution_store[idx].is_empty() {
                        // eprintln!("re-execute task: {:?} with ssa.", task.tx_hash.unwrap());
                        let to_re_execute = &self.to_re_execution_store[idx];

                        while !self.dag_store[idx].read().is_built() {
                            std::hint::spin_loop();
                        }
                        let graph = self.dag_store[idx].read().get_graph();
                        let execution_mode = ExecutionMode::Partial(
                            to_re_execute.iter().map(|x| *x).collect::<Vec<_>>(),
                        );
                        let mut executor = SSAExecutor::new_with_spec(
                            graph,
                            db_ref_for_parallel,
                            &task.env,
                            self.first_frame_input_store[idx].clone(),
                            task.spec_id,
                        )
                        .with_mode(execution_mode);

                        profiler::start("ssa-execution");
                        let ssa_execution = executor.execute(task.tx_hash.unwrap_or_default());
                        profiler::end("ssa-execution");

                        match ssa_execution {
                            Ok(nodes_to_execute_len) => {
                                let tx_hash = task.tx_hash.unwrap_or_default();
                                // eprintln!("SSA re-execution success: {:?}", tx_hash);
                                let result_state =
                                    executor.graph.get_storage_write_outputs().unwrap();
                                let mut task_result: TaskResultItem<I> = TaskResultItem::default();
                                task_result.gas_limit = task.gas;
                                task_result.inspector = Some(inspector);
                                task_result.ssa_output = Some(result_state);
                                task_result.result = Some(
                                    executor.graph.generate_result(task.gas, tx_hash).unwrap(),
                                );
                                result_store[idx] = task_result;
                                drop(executor);
                                re_execution_opcodes += nodes_to_execute_len.0;
                                continue;
                            }
                            Err(_err) => {
                                // eprintln!("TxHash: {:?} SSA re-execution failed: {:?}, fall back to EVM re-execution.", task.tx_hash, _err);
                                drop(executor);
                                re_execution_opcodes += opcode_counts_store[idx];
                                // fall through to EVM re-execution path below
                            }
                        }
                    }

                    // ! Debug for SSA
                    // let _tracer_inspector = if task.tx_hash != Some(fixed_bytes!("11dd4578015c5c9a50eb85cd16cf2554b2e8a8c624bdf1659a41bab522186cd4")) {
                    //     TracerEip3155::new(
                    //         Box::new(std::io::sink()),
                    //     ).without_summary()
                    // } else {
                    //     TracerEip3155::new(
                    //         Box::new(std::fs::File::create("tracer_re_execution.json").unwrap()),
                    //     )
                    // };

                    // Normal execution path
                    let mut evm = Evm::builder()
                        .with_ref_db(&parallel_db)
                        .modify_env(|env| env.clone_from(&task.env))
                        .with_external_context(NoOpInspector)
                        .with_spec_id(task.spec_id)
                        .append_handler_register(inspector_handle_register)
                        .build();
                    evm.disable_beneficiary();

                    profiler::start("evm-transact");
                    let result = evm.transact();
                    profiler::end("evm-transact");

                    let mut task_result = TaskResultItem::default();
                    // Track read-write access for conflict detection
                    // This information is crucial for maintaining consistency
                    let read_write_set = evm.get_read_write_set();
                    task_result.read_write_set = Some(read_write_set);

                    drop(evm);
                    task_result.inspector = Some(inspector);

                    match result {
                        Ok(result_and_state) => {
                            let ResultAndState { state, result } = result_and_state;
                            redo_gas_used += result.gas_used();
                            task_result.state = Some(state);
                            task_result.result = Some(result);
                        }
                        Err(_err) => {
                            task_result.state = None;
                            task_result.result = None;

                            // match _err {
                            //     EVMError::Transaction(error) => println!("TxHash: {:?} failed: Transaction error: {:?}", task.tx_hash, error),
                            //     EVMError::Header(error) => println!("TxHash: {:?} failed: Header error: {:?}", task.tx_hash, error),
                            //     EVMError::Database(_) => println!("TxHash: {:?} failed: DB error", task.tx_hash),
                            //     EVMError::Custom(msg) => println!("TxHash: {:?} failed: Custom error: {}", task.tx_hash, msg),
                            //     EVMError::Precompile(msg) => println!("TxHash: {:?} failed: Precompile error: {}", task.tx_hash, msg),
                            // }
                        }
                    }

                    result_store[idx] = task_result;
                }

                let seq_end = std::time::Instant::now();
                seq_time += seq_end - seq_start;
                profiler::end("non-parallel");
                h_commit.extend(ready_tasks.iter().map(|&idx| Reverse(idx)));
            } else {
                profiler::start("parallel");
                parallel_exec_size += ready_tasks.len();
                parallel_db.set_parallel_mode(true);
                let (duration, failed_tasks) = self.execute_parallel_tasks(
                    &ready_tasks,
                    h_tx,
                    &mut parallel_db,
                    result_store,
                    &mut opcode_counts_store,
                    &inspector_setup,
                    false,
                    enable_dep_graph,
                    enable_ssa,
                );
                parallel_time += duration;
                let failed_tasks = failed_tasks.clone();
                for task_idx in failed_tasks.iter() {
                    h_tx[*task_idx].sid = h_tx[*task_idx].tid - 1;
                    h_exec.push(Reverse((h_tx[*task_idx].sid, h_tx[*task_idx].tid as usize)));
                }
                h_commit.extend(
                    ready_tasks
                        .iter()
                        .filter(|&&idx| !failed_tasks.contains(&idx))
                        .map(|&idx| Reverse(idx)),
                );
                profiler::end("parallel");
            }

            if h_commit.len() == 0 {
                break;
            }

            let commit_start = std::time::Instant::now();
            profiler::start("commit-all");
            profiler::note_str("commit-all", "type", "commit");

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

                let is_conflict = if h_tx[task_idx].sid == h_tx[task_idx].tid - 1 {
                    false
                } else {
                    let read_write_set = task_result.read_write_set.as_ref().unwrap();
                    let conflict = access_tracker.check_conflict_in_range(
                        &read_write_set.read_set,
                        h_tx[task_idx].sid + 1,
                        h_tx[task_idx].tid,
                        enable_ssa,
                    );
                    // for failed tx, we cannot apply ssa re-execution.
                    if !conflict.is_empty() && enable_ssa && task_result.result.is_some() {
                        let first_reads = &self.reads_store[task_idx];
                        self.to_re_execution_store[task_idx] =
                            Self::get_storage_first_reads(first_reads, &conflict);
                    }
                    !conflict.is_empty()
                };

                // Handle conflicts or commit changes
                if is_conflict {
                    // Conflict detected: update sid and retry
                    h_tx[task_idx].sid = h_tx[task_idx].tid - 1;
                    h_exec.push(Reverse((h_tx[task_idx].sid, h_tx[task_idx].tid as usize)));
                    if enable_ssa {
                        total_opcodes += opcode_counts_store[task_idx];
                    }
                } else {
                    let state: HashMap<Address, Account> =
                        if let Some(ssa_state) = &task_result.ssa_output {
                            // Convert SSA state to normal state
                            let state = self
                                .convert_ssa_to_state(&mut parallel_db, ssa_state)
                                .map_err(|_| ())
                                .unwrap_or_default();
                            // Clone a state to task_result, for the reth usage.
                            task_result.state = Some(state.clone());
                            state
                        } else if let Some(state) = task_result.state.clone() {
                            state
                        } else {
                            continue;
                        };
                    // println!("idx:{}, state: {:?}", task_idx, state);

                    parallel_db.commit(state.clone());
                    unsafe {
                        (*raw_db_ptr).commit(state);
                    }

                    let rw_set = task_result.get_read_write_set();
                    access_tracker.record_write_set(h_tx[task_idx].tid, &rw_set.write_set);
                    next += 1;
                }
            }
            let commit_end = std::time::Instant::now();
            commit_time += commit_end - commit_start;
            profiler::end("commit-all");
        }

        self.apply_beneficiary(db, h_tx, result_store);

        // Calculate final statistics and conflict rate
        // High conflict rates might indicate need for better task scheduling
        let conflict_rate = ((exec_size - tx_size) as f64) / (tx_size as f64) * 100.0;
        profiler::note_str_unchecked("metrics", "type", "metrics");
        profiler::note_str_unchecked("metrics", "conflict-rate", &conflict_rate.to_string());
        profiler::note_str_unchecked("metrics", "redo-gas-used", &redo_gas_used.to_string());
        profiler::note_str_unchecked("metrics", "block-tx-num", &tx_size.to_string());
        profiler::note_str_unchecked("metrics", "total-opcodes", &total_opcodes.to_string());
        profiler::note_str_unchecked(
            "re-execution-opcodes",
            "main",
            &re_execution_opcodes.to_string(),
        );

        println!(
            "\nFinished execute tasks size: {} with conflict rate: {:.2}%",
            result_store.len(),
            conflict_rate
        );

        // Log detailed timing breakdown for performance analysis
        println!("- prepare time: {:?}", prepare_time);
        println!("- parallel execution time: {:?}", parallel_time);
        println!("- sequential execution time: {:?}", seq_time);
        println!("- commit time: {:?}", commit_time);
        // Clean up resources in background to avoid blocking
        // This includes access tracker and task management queues
        self.thread_pool.spawn(move || {
            drop(access_tracker);
            drop(h_exec);
            drop(h_commit);
            drop(h_ready);
        });
        // Print both parallel and sequential stats at the end
        // println!("\nParallel execution stats:");
        // let (hit_rate, hits, misses, db_time, cache_time, max_read, avg_read) = {
        //     parallel_db.set_parallel_mode(true);
        //     parallel_db.get_stats()
        // };
        // println!(
        //     "  hit_rate: {:.2}%, hits: {}, misses: {}",
        //     hit_rate, hits, misses
        // );
        // println!("  db_read: {:?}, cache_access: {:?}", db_time, cache_time);
        // println!("  max_read: {:?}, avg_read: {:?}", max_read, avg_read);

        // println!("\nSequential execution stats:");
        // let (hit_rate, hits, misses, db_time, cache_time, max_read, avg_read) = {
        //     parallel_db.set_parallel_mode(false);
        //     parallel_db.get_stats()
        // };
        // println!(
        //     "  hit_rate: {:.2}%, hits: {}, misses: {}",
        //     hit_rate, hits, misses
        // );
        // println!("  db_read: {:?}, cache_access: {:?}", db_time, cache_time);
        // println!("  max_read: {:?}, avg_read: {:?}", max_read, avg_read);
        println!(
            "  seq_exec_size: {}, parallel_exec_size: {}",
            seq_exec_size, parallel_exec_size
        );

        Ok(())
    }

    fn apply_beneficiary<DB: DatabaseCommit + DatabaseRef, I>(
        &self,
        db: &mut DB,
        tasks: &[Task],
        result_store: &mut [TaskResultItem<I>],
    ) {
        let beneficiary = tasks[0].env.block.coinbase;
        let mut coinbase_refund = U256::ZERO;
        for task in tasks.iter() {
            let env = &task.env;
            let effective_gas_price = env.effective_gas_price();

            let coinbase_gas_price = if task.spec_id.is_enabled_in(LONDON) {
                effective_gas_price.saturating_sub(env.block.basefee)
            } else {
                effective_gas_price
            };

            let result = match result_store[task.tid as usize].result.as_ref() {
                Some(result) => result,
                None => {
                    panic!("Transaction execution failed for hash: {:?}, to_re_execution_store: {:?}", task.tx_hash, self.to_re_execution_store[task.tid as usize]);
                }
            };
            let gas = result.gas_used();
            coinbase_refund += coinbase_gas_price * U256::from(gas as u64);
        }

        let coinbase_account = db.basic_ref(beneficiary).map_err(|_| ());
        if let Ok(Some(account)) = coinbase_account {
            let new_balance = account.balance.saturating_add(coinbase_refund);
            let mut updated_account = account;
            updated_account.balance = new_balance;
            let mut state = HashMap::default();
            state.insert(beneficiary, Account::from(updated_account));
            db.commit(state);
        }
    }

    /// Convert SSA state to normal state by applying storage updates
    ///
    /// This function takes SSA execution results and converts them to normal state updates by:
    /// 1. Using result cache as primary source for account data
    /// 2. Falling back to DB lookup if account not in cache
    /// 3. Applying storage updates to accounts in order
    ///
    /// # Parameters
    /// * `db` - Database to read accounts from
    /// * `ssa_state` - SSA execution results containing storage updates
    ///
    /// # Returns
    /// * Result containing updated account states or error
    pub fn convert_ssa_to_state<DB>(
        &self,
        db: &mut DB,
        ssa_state: &[SSAOutput],
    ) -> Result<HashMap<Address, Account>, EVMError<DB::Error>>
    where
        DB: DatabaseRef,
    {
        let mut result = HashMap::default();

        for output in ssa_state {
            if let SSAOutput::Storage { key, value } = output {
                match **key {
                    StorageKey::AccountInfo(address) | StorageKey::AccountStatus(address) => {
                        let account = result.entry(address).or_insert_with(|| {
                            db.basic_ref(address)
                                .map(|info| info.map_or_else(Account::new_not_existing, Into::into))
                                .unwrap_or_else(|_| Account::new_not_existing())
                        });

                        account.status |= AccountStatus::Touched;

                        if let Some(info) = value.as_account_info() {
                            account.info = info.clone();
                        }

                        if let Some(status) = value.as_account_status() {
                            account.status |= *status;
                        }
                    }
                    StorageKey::Slot(address, index) => {
                        let account = result.entry(address).or_insert_with(|| {
                            db.basic_ref(address)
                                .map(|info| info.map_or_else(Account::new_not_existing, Into::into))
                                .unwrap_or_else(|_| Account::new_not_existing())
                        });

                        account.status |= AccountStatus::Touched;

                        if let StorageValue::Slot(new_value) = **value {
                            let slot = account.storage.entry(index).or_insert_with(|| {
                                let value = db.storage_ref(address, index).unwrap_or(U256::ZERO);
                                EvmStorageSlot::new(value)
                            });

                            slot.present_value = new_value;
                        }
                    }
                }
            }
        }

        Ok(result)
    }

    /// Returns an array of first read LSNs from the SSA logger
    /// Input is a HashMap<Address, HashSet<AccessType>> representing the read set
    fn get_storage_first_reads(
        first_reads: &HashMap<StorageKey, Vec<LsnType>>,
        read_set: &Vec<(Address, AccessType)>,
    ) -> Vec<LsnType> {
        let mut result = Vec::new();

        // Iterate through the read set
        for (addr, access_type) in read_set {
            let lsn = match access_type {
                AccessType::AccountInfo => first_reads.get(&StorageKey::AccountInfo(*addr)),
                AccessType::AccountStatus => first_reads.get(&StorageKey::AccountStatus(*addr)),
                AccessType::StorageSlot(slot) => first_reads.get(&StorageKey::Slot(*addr, *slot)),
                _ => continue,
            };
            if let Some(lsns) = lsn {
                result.extend(lsns.clone());
            } else {
                return vec![];
            }
        }
        result
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
        set2: &HashMap<Address, HashSet<AccessType>>,
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
            dag.add_task(&Task {
                tid: i as i32,
                ..Default::default()
            });
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
                                &Task {
                                    tid: i as i32,
                                    ..Default::default()
                                },
                                &Task {
                                    tid: j as i32,
                                    ..Default::default()
                                },
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
