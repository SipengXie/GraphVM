use revm::Evm;
use revm_primitives::{
    hex, AccountInfo, Address, Bytes, Env, SpecId, TxKind, U256, B256, keccak256,
};
use revm::db::{CacheDB, EmptyDB};
use revm_ssa::{logger::LsnType, SSALogger};
use revm_ssa_graph::SsaGraph;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;

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

fn main() {
    let json_path = "../../data/uniswap-t100-c20.json";

    println!("Loading Uniswap JSON test case from: {}", json_path);

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

    // Execute transaction with SSA logger
    println!("\nExecuting transaction with SSA logger...");
    let mut evm = Evm::builder()
        .with_spec_id(SpecId::CANCUN)
        .with_ref_db(&mut cache_db)
        .with_env(Box::new(env.clone()))
        .with_ssa_logger(SSALogger::new())
        .build_with_ssa_logger();

    let result = evm.transact_preverified();
    println!("Transaction execution result: {:?}", result.is_ok());

    // Get SSA logs
    let mut logger = evm.take_ssa_logger().unwrap();
    let logs = logger.take_logs();
    let node_count = logs.len();
    let lsns: Vec<LsnType> = logs.iter().map(|log| log.lsn).collect();

    println!("Collected {} SSA log entries", node_count);

    // Build dependency graph
    println!("\nBuilding dependency graph...");
    let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());

    for log in logs {
        graph.add_node(log).unwrap();
    }

    for lsn in lsns {
        graph.add_edges(lsn).unwrap();
    }

    println!("Dependency graph constructed successfully");

    // Calculate parallelism ratio
    println!("\n========================================");
    println!("  Uniswap Transaction Parallelism Analysis");
    println!("========================================");

    let parallelism_ratio = graph.calculate_parallelism_ratio().unwrap();

    println!("\n📊 Graph Statistics:");
    println!("  • Total nodes: {}", node_count);

    // Calculate critical path length from parallelism ratio
    let critical_path_length = (parallelism_ratio * node_count as f64).round() as usize;
    println!("  • Critical path length: {}", critical_path_length);

    println!("\n⚡ Parallelism Metrics:");
    println!("  • Parallelism ratio: {:.4} ({:.2}%)",
             parallelism_ratio, parallelism_ratio * 100.0);

    let theoretical_speedup = if parallelism_ratio > 0.0 {
        1.0 / parallelism_ratio
    } else {
        0.0
    };
    println!("  • Theoretical max speedup: {:.2}x", theoretical_speedup);

    // Calculate dependency statistics
    println!("\n🔗 Dependency Statistics:");
    let mut total_in_degree = 0;
    let mut total_out_degree = 0;
    let mut max_in_degree = 0;
    let mut max_out_degree = 0;
    let mut nodes_with_no_pred = 0;
    let mut nodes_with_no_succ = 0;

    for lsn in 1..=node_count {
        let in_degree = match graph.get_predecessors(lsn as u32) {
            Ok(preds) => preds.len(),
            Err(_) => 0,
        };
        let out_degree = match graph.get_successors(lsn as u32) {
            Ok(succs) => succs.len(),
            Err(_) => 0,
        };

        total_in_degree += in_degree;
        total_out_degree += out_degree;
        max_in_degree = max_in_degree.max(in_degree);
        max_out_degree = max_out_degree.max(out_degree);

        if in_degree == 0 {
            nodes_with_no_pred += 1;
        }
        if out_degree == 0 {
            nodes_with_no_succ += 1;
        }
    }

    let avg_in_degree = total_in_degree as f64 / node_count as f64;
    let avg_out_degree = total_out_degree as f64 / node_count as f64;

    println!("  • Average in-degree: {:.2}", avg_in_degree);
    println!("  • Average out-degree: {:.2}", avg_out_degree);
    println!("  • Max in-degree: {}", max_in_degree);
    println!("  • Max out-degree: {}", max_out_degree);
    println!("  • Source nodes (no predecessors): {}", nodes_with_no_pred);
    println!("  • Sink nodes (no successors): {}", nodes_with_no_succ);

    // Calculate execution layers for additional insight
    println!("\n📈 Execution Layer Analysis:");
    match graph.execution_layers() {
        Ok(layers) => {
            let num_layers = layers.len();
            let avg_nodes_per_layer = node_count as f64 / num_layers as f64;
            let max_layer_size = layers.iter().map(|layer| layer.len()).max().unwrap_or(0);
            let min_layer_size = layers.iter().map(|layer| layer.len()).min().unwrap_or(0);

            println!("  • Total execution layers: {}", num_layers);
            println!("  • Average nodes per layer: {:.2}", avg_nodes_per_layer);
            println!("  • Max parallelism in single layer: {}", max_layer_size);
            println!("  • Min parallelism in single layer: {}", min_layer_size);

            // Show distribution of first few layers
            println!("\n  Layer-by-layer breakdown (first 10 layers):");
            for (i, layer) in layers.iter().take(10).enumerate() {
                println!("    Layer {:2}: {:4} nodes", i + 1, layer.len());
            }
            if num_layers > 10 {
                println!("    ... {} more layers", num_layers - 10);
            }
        }
        Err(e) => {
            println!("  ⚠ Could not calculate execution layers: {:?}", e);
        }
    }

    println!("\n========================================");
    println!("\n💡 Interpretation:");
    if parallelism_ratio < 0.1 {
        println!("  ✓ Excellent parallelism potential! Most operations can run concurrently.");
    } else if parallelism_ratio < 0.3 {
        println!("  ✓ Good parallelism potential. Significant speedup possible with parallel execution.");
    } else if parallelism_ratio < 0.5 {
        println!("  ⚠ Moderate parallelism. Some speedup possible but limited by dependencies.");
    } else {
        println!("  ⚠ Limited parallelism. Most operations must execute sequentially.");
    }

    println!("\n  The parallelism ratio of {:.4} indicates that the critical path", parallelism_ratio);
    println!("  contains {:.1}% of all nodes. With unlimited parallel resources,", parallelism_ratio * 100.0);
    println!("  theoretical speedup could reach {:.2}x compared to sequential execution.", theoretical_speedup);

    println!("\n========================================\n");
}
