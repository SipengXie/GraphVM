use revm::Evm;
use revm_primitives::{
    hex, AccountInfo, Address, Bytes, Env, LatestSpec, SpecId, TxKind, U256, B256, keccak256,
};
use revm::db::{CacheDB, EmptyDB};
use revm_ssa::{logger::LsnType, SSALogger};
use revm_ssa_graph::{ExecutionContext, SSAExecutor, SsaGraph};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use crossbeam::queue::SegQueue;
use rayon::prelude::*;

// JSON data structures
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

fn parse_u256(s: &str) -> U256 {
    if s.is_empty() || s == "0x" {
        return U256::ZERO;
    }
    U256::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(U256::ZERO)
}

fn parse_u64(s: &str) -> u64 {
    if s.is_empty() || s == "0x" {
        return 0;
    }
    u64::from_str_radix(s.trim_start_matches("0x"), 16).unwrap_or(0)
}

fn parse_address(s: &str) -> Address {
    let bytes = hex::decode(s.trim_start_matches("0x")).unwrap_or_default();
    Address::from_slice(&bytes)
}

fn parse_b256(s: &str) -> B256 {
    let bytes = hex::decode(s.trim_start_matches("0x")).unwrap_or_default();
    B256::from_slice(&bytes)
}

fn recover_address_from_secret_key(secret_key: &str) -> Address {
    use k256::ecdsa::SigningKey;
    let secret_bytes = hex::decode(secret_key.trim_start_matches("0x")).unwrap();
    let signing_key = SigningKey::from_slice(&secret_bytes).unwrap();
    let verifying_key = signing_key.verifying_key();
    let public_key = verifying_key.to_encoded_point(false);
    let public_key_bytes = public_key.as_bytes();
    let hash = keccak256(&public_key_bytes[1..]);
    Address::from_slice(&hash[12..])
}

#[test]
fn test_parallel_graph_execution_debug() {
    println!("\n========================================");
    println!("Testing Parallel Graph Execution");
    println!("========================================\n");

    // Load JSON test case
    let json_path = "../../data/uniswap-t100-c20.json";
    let json_content = fs::read_to_string(json_path)
        .expect("Failed to read JSON file");
    let test_suite: TestSuite = serde_json::from_str(&json_content)
        .expect("Failed to parse JSON");

    let test_case = &test_suite.altius_test;
    let tx = test_case.transaction.get(0).expect("No transactions in JSON");

    // Setup EVM environment
    let mut env = Env::default();
    env.cfg.chain_id = 1;
    env.block.number = parse_u256(&test_case.env.current_number);
    env.block.coinbase = parse_address(&test_case.env.current_coinbase);
    env.block.timestamp = parse_u256(&test_case.env.current_timestamp);
    env.block.gas_limit = parse_u256(&test_case.env.current_gas_limit);
    env.block.basefee = parse_u256(&test_case.env.current_base_fee);
    env.block.difficulty = parse_u256(&test_case.env.current_difficulty);
    env.block.prevrandao = Some(parse_b256(&test_case.env.current_random));

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

    // Setup database
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

        for (slot_str, value_str) in &account.storage {
            let slot = parse_u256(slot_str);
            let value = parse_u256(value_str);
            let _ = cache_db.insert_account_storage(address, slot, value);
        }
    }

    // Execute with SSA logger to collect logs
    println!("Executing transaction to collect SSA logs...");
    let mut db_for_graph = cache_db.clone();
    let mut evm = Evm::builder()
        .with_spec_id(SpecId::CANCUN)
        .with_ref_db(&mut db_for_graph)
        .with_env(Box::new(env.clone()))
        .with_ssa_logger(SSALogger::new())
        .build_with_ssa_logger();

    let _ = evm.transact_preverified();

    // Get logs and build graph
    let mut logger = evm.take_ssa_logger().unwrap();
    let logs = logger.take_logs();
    let first_call = logger.take_first_call_input();
    let first_create = logger.take_first_create_input();

    let node_count = logs.len();
    let lsns: Vec<LsnType> = logs.iter().map(|log| log.lsn).collect();

    println!("Collected {} SSA log entries", node_count);

    // Build dependency graph
    let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());
    for log in logs {
        graph.add_node(log).unwrap();
    }
    for lsn in lsns {
        graph.add_edges(lsn).unwrap();
    }

    println!("Dependency graph constructed");

    // Debug: Print LSN=2 node information
    println!("\n========================================");
    println!("Debug Info: LSN=2 Node");
    println!("========================================");
    if let Ok(node_2) = graph.get_node(2) {
        println!("Node LSN=2:");
        println!("  Opcode: {} (215=CALL/MAKE_CALL_FRAME)", node_2.opcode);
        println!("  Inputs: {} inputs", node_2.inputs.len());
        println!("  Outputs: {} outputs", node_2.outputs.len());
        println!("  Outputs:{:?}", node_2.outputs);
    } else {
        println!("ERROR: Could not find node with LSN=2");
    }
    println!("========================================\n");

    // Create execution context
    let env_clone = env.clone();
    let db_clone = cache_db.clone();
    let context = Arc::new(ExecutionContext::<'_, CacheDB<EmptyDB>, LatestSpec>::new(
        &env_clone,
        db_clone,
        first_call,
        first_create,
    ));

    // Get topological sorted nodes
    let nodes_to_execute = graph.topological_sort().unwrap();

    // Use original graph for parallel execution
    let arc_parallel_graph = Arc::new(graph);

    // Preprocess: Build successors list
    let mut successors: Vec<Vec<u32>> = vec![Vec::new(); node_count + 1];
    for lsn in 1..=node_count {
        if let Ok(succs) = arc_parallel_graph.get_successors(lsn as u32) {
            successors[lsn] = succs.to_vec();
        }
    }

    // Initialize predecessor counters
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

    // Create thread pool
    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(8)
        .build()
        .unwrap();

    let successors = Arc::new(successors);

    println!("\n========================================");
    println!("Running 3 iterations of parallel execution");
    println!("========================================\n");

    // Run multiple iterations to test
    for iteration in 1..=3 {
        println!("--- Iteration {} ---", iteration);

        // Reset predecessor counters for each iteration
        for (i, counter) in pred_counters.iter().enumerate() {
            if i > 0 {
                let count = match arc_parallel_graph.get_predecessors(i as u32) {
                    Ok(preds) => preds.len() as u32,
                    Err(_) => 0
                };
                counter.store(count, Ordering::Release);
            }
        }

        // Memory fence
        std::sync::atomic::fence(Ordering::SeqCst);

        // Initialize task queue
        let task_queue = SegQueue::new();
        for node_index in nodes_to_execute.iter() {
            let node = arc_parallel_graph.get_node_by_index(*node_index).unwrap();
            let lsn = node.lsn;
            if pred_counters[lsn as usize].load(Ordering::Acquire) == 0 {
                task_queue.push(lsn);
            }
        }

        println!("Initial queue size: {}", {
            let mut count = 0;
            while task_queue.pop().is_some() {
                count += 1;
            }
            // Re-initialize queue
            for node_index in nodes_to_execute.iter() {
                let node = arc_parallel_graph.get_node_by_index(*node_index).unwrap();
                let lsn = node.lsn;
                if pred_counters[lsn as usize].load(Ordering::Acquire) == 0 {
                    task_queue.push(lsn);
                }
            }
            count
        });

        // Parallel execution
        let start = std::time::Instant::now();
        thread_pool.install(|| {
            (0..8).into_par_iter().for_each(|_| {
                while let Some(lsn) = task_queue.pop() {
                    let mut_graph_local = unsafe {
                        &mut *(Arc::as_ptr(&arc_parallel_graph) as *mut SsaGraph)
                    };
                    let node = mut_graph_local.get_node_mut(lsn).unwrap();

                    // Execute the node
                    let _ = SSAExecutor::execute_node(node, &arc_parallel_graph, &context);

                    // Update successor predecessor counters
                    for &succ_lsn in &successors[lsn as usize] {
                        if pred_counters[succ_lsn as usize]
                            .fetch_sub(1, Ordering::AcqRel) == 1 {
                            task_queue.push(succ_lsn);
                        }
                    }
                }
            });
        });
        let elapsed = start.elapsed();

        println!("Iteration {} completed in {:?}", iteration, elapsed);

        // Verify all counters are zero
        let mut non_zero_counters = 0;
        for (i, counter) in pred_counters.iter().enumerate() {
            if i > 0 {
                let val = counter.load(Ordering::Acquire);
                if val != 0 {
                    println!("WARNING: Counter[{}] = {} (expected 0)", i, val);
                    non_zero_counters += 1;
                }
            }
        }
        if non_zero_counters == 0 {
            println!("✓ All counters correctly decremented to 0");
        } else {
            println!("✗ {} counters not zero!", non_zero_counters);
        }
        println!();
    }

    println!("========================================");
    println!("Test completed successfully!");
    println!("========================================");
}
