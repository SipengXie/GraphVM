use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion,
};
use revm::Evm;
use revm_primitives::{
    hex, AccountInfo, Address, Bytes, Env, LatestSpec, SpecId, TxKind, U256, B256, keccak256,
};
use revm::db::{CacheDB, EmptyDB};
use revm_ssa::{logger::LsnType, SSALogger, types::SSALogEntry};
use revm_ssa_graph::{ExecutionContext, SSAExecutor, SsaGraph};
use serde::{Deserialize, Serialize};
use std::cell::RefCell;
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use std::sync::atomic::{AtomicU32, Ordering};
use crossbeam::queue::SegQueue;
use rayon::prelude::*;

// JSON data structures for deserialization
#[derive(Debug, Deserialize, Serialize)]
struct TestSuite {
    #[serde(rename = "altius-test")]
    altius_test: TestCase,
}

#[derive(Debug, Deserialize, Serialize)]
struct TestCase {
    env: EnvironmentInfo,
    pre: HashMap<String, AccountState>,
    transaction: Vec<Transaction>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct EnvironmentInfo {
    current_base_fee: String,
    current_coinbase: String,
    current_difficulty: String,
    current_excess_blob_gas: String,
    current_gas_limit: String,
    current_number: String,
    current_random: String,
    current_timestamp: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct AccountState {
    balance: String,
    code: String,
    nonce: String,
    storage: HashMap<String, String>,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct Transaction {
    data: String,
    gas_limit: String,
    gas_price: Option<String>,
    nonce: String,
    secret_key: String,
    to: Option<String>,
    value: String,
    #[serde(default)]
    max_fee_per_gas: Option<String>,
    #[serde(default)]
    max_priority_fee_per_gas: Option<String>,
}

// Helper function to parse hex string to U256
fn parse_u256(s: &str) -> U256 {
    if s.is_empty() || s == "0x" {
        return U256::ZERO;
    }
    U256::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(U256::ZERO)
}

// Helper function to parse hex string to u64
fn parse_u64(s: &str) -> u64 {
    if s.is_empty() || s == "0x" {
        return 0;
    }
    u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0)
}

// Helper function to parse hex string to Address
fn parse_address(s: &str) -> Address {
    let bytes = hex::decode(s.trim_start_matches("0x")).unwrap_or_default();
    Address::from_slice(&bytes)
}

// Helper function to parse hex string to B256
fn parse_b256(s: &str) -> B256 {
    let bytes = hex::decode(s.trim_start_matches("0x")).unwrap_or_default();
    B256::from_slice(&bytes)
}

// Helper function to recover address from secret key
fn recover_address_from_secret_key(secret_key: &str) -> Address {
    use k256::ecdsa::SigningKey;

    let secret_bytes = hex::decode(secret_key.trim_start_matches("0x")).unwrap();
    let signing_key = SigningKey::from_slice(&secret_bytes).unwrap();
    let verifying_key = signing_key.verifying_key();
    let public_key = verifying_key.to_encoded_point(false);
    let public_key_bytes = public_key.as_bytes();

    // Keccak hash of public key (without 0x04 prefix)
    let hash = keccak256(&public_key_bytes[1..]);
    Address::from_slice(&hash[12..])
}

struct BenchSetup {
    env: Env,
    // Use Arc<RefCell<>> to avoid cloning the entire database on each iteration
    // This matches the pattern from D:\Dev\revm-exec-interp\crates\revm-ssa-integration-tests\benches\uniswap_json_bench.rs
    cache_db: Arc<RefCell<CacheDB<EmptyDB>>>,
    contract_addr: Address,
}

impl BenchSetup {
    fn new(json_path: &str) -> Self {
        // Load and parse JSON file
        let json_content = fs::read_to_string(json_path)
            .expect("Failed to read JSON file");
        let test_suite: TestSuite = serde_json::from_str(&json_content)
            .expect("Failed to parse JSON");

        let test_case = &test_suite.altius_test;

        // Get first transaction
        let tx = test_case.transaction.get(0)
            .expect("No transactions in JSON");

        // Set up EVM environment
        let mut env = Env::default();
        env.cfg.chain_id = 1;

        // Block env
        env.block.number = parse_u256(&test_case.env.current_number);
        env.block.coinbase = parse_address(&test_case.env.current_coinbase);
        env.block.timestamp = parse_u256(&test_case.env.current_timestamp);
        env.block.gas_limit = parse_u256(&test_case.env.current_gas_limit);
        env.block.basefee = parse_u256(&test_case.env.current_base_fee);
        env.block.difficulty = parse_u256(&test_case.env.current_difficulty);
        env.block.prevrandao = Some(parse_b256(&test_case.env.current_random));

        // Transaction env
        env.tx.caller = recover_address_from_secret_key(&tx.secret_key);
        env.tx.gas_limit = parse_u64(&tx.gas_limit);
        env.tx.gas_price = parse_u256(tx.gas_price.as_ref().unwrap_or(&tx.gas_limit));
        env.tx.data = Bytes::from(hex::decode(tx.data.trim_start_matches("0x")).unwrap_or_default());
        env.tx.value = parse_u256(&tx.value);
        env.tx.transact_to = if let Some(to) = &tx.to {
            TxKind::Call(parse_address(to))
        } else {
            TxKind::Create
        };

        // Set up cache database with pre-state
        let mut cache_db = CacheDB::new(EmptyDB::default());

        for (addr_str, account) in &test_case.pre {
            let address = parse_address(addr_str);
            let code_bytes = hex::decode(account.code.trim_start_matches("0x")).unwrap_or_default();
            let bytecode = if code_bytes.is_empty() {
                revm_primitives::Bytecode::new()
            } else {
                revm_primitives::Bytecode::new_raw(Bytes::from(code_bytes))
            };

            let account_info = AccountInfo {
                balance: parse_u256(&account.balance),
                nonce: parse_u64(&account.nonce),
                code_hash: bytecode.hash_slow(),
                code: Some(bytecode),
            };

            cache_db.insert_account_info(address, account_info);

            // Insert storage
            for (slot_str, value_str) in &account.storage {
                let slot = parse_u256(slot_str);
                let value = parse_u256(value_str);
                let _ = cache_db.insert_account_storage(address, slot, value);
            }
        }

        // Get the target contract address from the first transaction
        let contract_addr = if let TxKind::Call(addr) = env.tx.transact_to {
            addr
        } else {
            panic!("First transaction must be a call");
        };

        Self {
            env,
            cache_db: Arc::new(RefCell::new(cache_db)),
            contract_addr,
        }
    }
}

fn mk_group<'a>(c: &'a mut Criterion, name: &str) -> BenchmarkGroup<'a, WallTime> {
    let mut g = c.benchmark_group(name);
    g.warm_up_time(Duration::from_secs(2));
    g.measurement_time(Duration::from_secs(5));
    g
}

fn bench_uniswap_json(c: &mut Criterion) {
    let json_path = "../../data/uniswap-t100-c20.json";
    let setup = BenchSetup::new(json_path);

    let mut group = mk_group(c, "uniswap_json_first_tx");

    // 2. SSA-logger execution benchmark
    group.bench_function("ssa-logger", |b| {
        b.iter(|| {
            // Use borrow_mut() to get mutable reference without cloning the entire database
            let mut db_ref = setup.cache_db.borrow_mut();
            let mut evm = Evm::builder()
                .with_spec_id(SpecId::CANCUN)
                .with_ref_db(&mut *db_ref)
                .with_env(Box::new(setup.env.clone()))
                .with_ssa_logger(SSALogger::new())
                .build_with_ssa_logger();
            evm.transact_preverified()
        })
    });

    // 3. Prepare graph for GraphVM benchmark
    // Execute once to collect logs
    // For this one-time setup, we clone the db (only once, not per iteration)
    let mut db_for_graph = setup.cache_db.borrow().clone();
    let mut evm = Evm::builder()
        .with_spec_id(SpecId::CANCUN)
        .with_ref_db(&mut db_for_graph)
        .with_env(Box::new(setup.env.clone()))
        .with_ssa_logger(SSALogger::new())
        .build_with_ssa_logger();
    let _ = evm.transact_preverified();

    // Get logs and build graph
    let mut logger = evm.take_ssa_logger().unwrap();
    let logs = logger.take_logs();
    let first_call = logger.take_first_call_input();
    let first_create = logger.take_first_create_input();

    // Store node count before consuming logs
    let node_count = logs.len();
    let lsns: Vec<LsnType> = logs.iter().map(|log| log.lsn).collect();

    // Create dependency graph
    let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());

    for log in logs {
        graph.add_node(log).unwrap();
    }

    for lsn in lsns {
        graph.add_edges(lsn).unwrap();
    }

    // Calculate graph memory size (approximate)
    let base_size = std::mem::size_of_val(&graph);
    let node_size_estimate = node_count * std::mem::size_of::<SSALogEntry>();
    let graph_memory_size = base_size + node_size_estimate;

    println!("\n========================================");
    println!("Graph Memory Usage:");
    println!("  - Node count: {}", node_count);
    println!("  - Base graph size: {} bytes", base_size);
    println!("  - Estimated node data: {} bytes", node_size_estimate);
    println!("  - Total (approx): {} bytes ({:.2} KB, {:.2} MB)",
             graph_memory_size,
             graph_memory_size as f64 / 1024.0,
             graph_memory_size as f64 / (1024.0 * 1024.0));
    println!("========================================\n");

    // Create execution context
    // Note: We need to clone here for the execution context as it will be used across iterations
    let env_clone = setup.env.clone();
    let db_clone = setup.cache_db.borrow().clone();
    let context = Arc::new(ExecutionContext::<'_, CacheDB<EmptyDB>, LatestSpec>::new(
        &env_clone,
        db_clone,
        first_call,
        first_create,
    ));

    // Get topological sorted nodes
    let nodes_to_execute = graph.topological_sort().unwrap();

    // Clone graph for serial benchmark (so parallel can use the original)
    let arc_graph = Arc::new(graph.clone());
    let mut_graph = unsafe { &mut *(Arc::as_ptr(&arc_graph) as *mut SsaGraph) };

    // 4. GraphVM execution benchmark (uses cloned graph)
    group.bench_function("graph", |b| {
        b.iter(|| {
            for node_index in nodes_to_execute.clone() {
                let node = mut_graph.get_node_by_index_mut(node_index);
                SSAExecutor::execute_node(node, &arc_graph, &context).unwrap();
            }
        });
    });

    // 5. Parallel GraphVM execution benchmark (8 threads, uses original graph)
    let arc_parallel_graph = Arc::new(graph);
    group.bench_function("graph-parallel-8t", |b| {
        // Preprocessing: Build successors list for each node
        let mut successors: Vec<Vec<u32>> = vec![Vec::new(); node_count + 1];
        for lsn in 1..=node_count {
            if let Ok(succs) = arc_parallel_graph.get_successors(lsn as u32) {
                successors[lsn] = succs.to_vec();
            }
        }

        // Initialize predecessor counters (atomic)
        let pred_counters = (0..=node_count)
            .map(|lsn| {
                if lsn == 0 {
                    AtomicU32::new(0)
                } else {
                    match arc_parallel_graph.get_predecessors(lsn as u32) {
                        Ok(preds) => AtomicU32::new(preds.len() as u32),
                        Err(_) => AtomicU32::new(0)
                    }
                }
            })
            .collect::<Vec<_>>();

        // Create thread pool with 8 workers
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(8)
            .build()
            .unwrap();

        let successors = Arc::new(successors);

        b.iter(|| {
            // Reset predecessor counters for each iteration
            // Use Release to ensure previous iteration's modifications are visible
            for (i, counter) in pred_counters.iter().enumerate() {
                if i > 0 {
                    let count = match arc_parallel_graph.get_predecessors(i as u32) {
                        Ok(preds) => preds.len() as u32,
                        Err(_) => 0
                    };
                    counter.store(count, Ordering::Release);  // Changed from Relaxed
                }
            }

            // Memory fence to ensure all stores are visible
            std::sync::atomic::fence(Ordering::SeqCst);

            // Initialize task queue with nodes that have no predecessors
            // Use Acquire to see the Reset stores
            let task_queue = SegQueue::new();
            for node_index in nodes_to_execute.iter() {
                let node = arc_parallel_graph.get_node_by_index(*node_index).unwrap();
                let lsn = node.lsn;
                if pred_counters[lsn as usize].load(Ordering::Acquire) == 0 {  // Changed from Relaxed
                    task_queue.push(lsn);
                }
            }

            // Parallel execution using thread pool
            thread_pool.install(|| {
                (0..8).into_par_iter().for_each(|_| {
                    // Each thread continuously pulls tasks from the queue
                    while let Some(lsn) = task_queue.pop() {
                        // Get mutable reference to graph (unsafe, but safe in benchmark context)
                        let mut_graph_local = unsafe {
                            &mut *(Arc::as_ptr(&arc_parallel_graph) as *mut SsaGraph)
                        };
                        let node = mut_graph_local.get_node_mut(lsn).unwrap();

                        // Execute the node
                        let _ = SSAExecutor::execute_node(node, &arc_parallel_graph, &context);

                        // Update successor predecessor counters
                        for &succ_lsn in &successors[lsn as usize] {
                            // Atomically decrement; if this was the last predecessor, enqueue successor
                            if pred_counters[succ_lsn as usize]
                                .fetch_sub(1, Ordering::AcqRel) == 1 {
                                task_queue.push(succ_lsn);
                            }
                        }
                    }
                });
            });
        });
    });

    group.finish();
}

criterion_group!(benches, bench_uniswap_json);
criterion_main!(benches);
