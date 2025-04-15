use std::collections::HashMap;
use std::time::Instant;

use revm::{
    db::{CacheDB, EmptyDB},
    Evm,
};
use revm_interpreter::SharedMemory;
use revm_primitives::{
    keccak256, uint, AccountInfo, AccountStatus, Address, Bytecode, Bytes, Env, SpecId, TxKind,
    U256,
};
use revm_ssa::{FrameInput, SSALogEntry};
use std::cell::RefCell;
use std::rc::Rc;
use typed_graph::{context::ExternalContext, ssa_converter::SsaConverter};
// TODO: Add necessary imports from typed_graph crate
// use typed_graph::{TypedGraph, /* other necessary items */};

/// Execution configuration for typed-graph tests
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    // TODO: Define necessary configuration fields, similar to revm/tests/mod.rs but simplified for serial execution.
    //       Example: mode (Full, Partial?), collect_metrics, input, pre_deployed_contract, pre_determined_slots etc.
    pub pre_deployed_contract: Vec<(Address, Bytes)>,
    pub pre_determined_slots: Vec<(U256, U256)>,
    pub input: Option<Bytes>,
    pub is_deployed_contract: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: None,
            is_deployed_contract: false,
        }
    }
}

/// Execute test case using EVM and then TypedGraph, verifying results
pub fn execute_case(code: Bytes, case_name: &str, config: ExecutionConfig) -> std::time::Duration {
    println!(
        "Testing typed-graph case: {} with config: {:?}",
        case_name, config
    );

    // --- Part 1: Execute with standard REVM + SSA Logger ---

    let contract_addr = Address::from([0x2; 20]);
    let bytecode = Bytecode::new_raw(code.clone());
    let code_hash = keccak256(&code);
    let mut cdb = CacheDB::new(EmptyDB::default());

    cdb.insert_account_info(
        contract_addr,
        AccountInfo {
            nonce: 0,
            balance: uint! {10000000000000000000000000_U256},
            code_hash,
            code: Some(bytecode.clone()), // Clone bytecode for graph execution if needed
        },
    );

    if !config.pre_determined_slots.is_empty() {
        for (slot, value) in config.pre_determined_slots.iter() {
            let _ = cdb.insert_account_storage(contract_addr, *slot, *value);
        }
    }

    if !config.pre_deployed_contract.is_empty() {
        for (addr, code) in config.pre_deployed_contract.iter() {
            cdb.insert_account_info(
                *addr,
                AccountInfo {
                    nonce: 0,
                    balance: uint! {10000000000000000000000000_U256},
                    code_hash: keccak256(code),
                    code: Some(Bytecode::new_raw(code.clone())),
                },
            );
        }
    }

    let mut evm = Evm::builder()
        .with_spec_id(SpecId::LATEST) // Use latest spec
        .with_ref_db(cdb.clone()) // Clone DB for potential reuse in graph execution
        .modify_tx_env(|tx| {
            tx.caller = Address::from([0x1; 20]);
            if config.is_deployed_contract {
                tx.transact_to = TxKind::Create;
            } else {
                tx.transact_to = TxKind::Call(contract_addr);
            }
            if let Some(input) = config.input.clone() {
                tx.data = input;
            }
            tx.gas_limit = 0x0f424000; // Consistent gas limit
        })
        .with_ssa_logger()
        .build_with_ssa_logger();

    let env = evm.context.evm.env().clone();

    let _revm_result = evm.transact().unwrap(); // Execute transaction
    let mut logger = evm.take_ssa_logger().unwrap();
    let logs = logger.take_logs(); // Get SSA logs
    let first_call = logger.take_first_frame_input().unwrap();

    // --- Part 2: Convert SSA Logs to TypedGraph and Execute ---

    let execution_time = typed_graph_execute(logs, config.clone(), &env, &first_call, &bytecode);

    execution_time
}

/// Convert SSA logs to TypedGraph and execute it
fn typed_graph_execute(
    entries: Vec<SSALogEntry>,
    config: ExecutionConfig,
    env: &Env,
    _first_frame: &FrameInput,
    bytecode: &Bytecode,
) -> std::time::Duration {
    println!(
        "Converting {} SSA log entries to TypedGraph...",
        entries.len()
    );

    // Initialize required components
    let shared_memory = Rc::new(RefCell::new(SharedMemory::new()));

    // Create ExternalContext with specific values
    let mut accounts = HashMap::new();
    let contract_addr = Address::from([0x2; 20]);
    accounts.insert(
        contract_addr,
        (
            AccountInfo {
                nonce: 0,
                balance: uint!(10000000000000000000000000_U256),
                code_hash: bytecode.hash_slow(),
                code: Some(bytecode.clone()),
            },
            AccountStatus::default(),
        ),
    );

    // Add pre-deployed contracts if any
    for (addr, code) in &config.pre_deployed_contract {
        accounts.insert(
            *addr,
            (
                AccountInfo {
                    nonce: 0,
                    balance: uint!(10000000000000000000000000_U256),
                    code_hash: keccak256(code),
                    code: Some(Bytecode::new_raw(code.clone())),
                },
                AccountStatus::default(),
            ),
        );
    }

    // Initialize storage with pre-determined slots
    let mut storage = HashMap::new();
    for (slot, value) in &config.pre_determined_slots {
        storage.insert((contract_addr, *slot), *value);
    }

    let external_context = ExternalContext::new(
        env.clone(),
        accounts,
        storage,
        HashMap::new(), // Empty block_hashes for now
    );

    let external_context = Rc::new(RefCell::new(external_context));

    // Create SsaConverter with the environment
    let mut converter = SsaConverter::new(
        external_context,
        shared_memory,
        env as *const Env,
        _first_frame as *const FrameInput,
    );

    // Convert SSA logs to TypedGraph
    let (mut typed_graph, _constant_pool) = converter.convert(entries);
    
    // eprintln!("== TypedGraph before execution ==");
    // typed_graph.print_graph();

    let start_time = Instant::now();
    // Execute the TypedGraph
    if let Err(e) = typed_graph.execute() {
        eprintln!("Error executing TypedGraph: {:?}", e);
    }

    // eprintln!("== TypedGraph after execution ==");
    // typed_graph.print_graph();

    start_time.elapsed()
}

// --- Test Modules ---

mod arithmetic_tests {
    use super::*;

    #[test]
    fn test_add_and_chainid() {
        // Test both arithmetic and environment operations
        // PUSH1 1 + PUSH1 2 + ADD + CHAINID + ADD
        let code = Bytes::from(vec![
            0x60, 0x01, // PUSH1 1
            0x60, 0x02, // PUSH1 2
            0x01, // ADD
            0x46, // CHAINID
            0x01, // ADD
        ]);

        let config = ExecutionConfig {
            ..Default::default()
        };

        let result = execute_case(code, "add and chainid test", config);
        println!("TypedGraph execution time: {:?}", result);
    }

    #[test]
    fn test_arithmetic_operations() {
        // Test multiple arithmetic operations
        // PUSH1 10 + PUSH1 5 + SUB + PUSH1 3 + MUL + PUSH1 2 + DIV
        let code = Bytes::from(vec![
            0x60, 0x0A, // PUSH1 10
            0x60, 0x05, // PUSH1 5
            0x03, // SUB
            0x60, 0x03, // PUSH1 3
            0x02, // MUL
            0x60, 0x02, // PUSH1 2
            0x04, // DIV
        ]);

        let config = ExecutionConfig {
            ..Default::default()
        };

        let result = execute_case(code, "arithmetic operations test", config);
        println!("TypedGraph execution time: {:?}", result);
    }
}

// TODO: Add other test modules (bitwise, memory, control, system, host_env, host, contract) adapting from revm/tests/mod.rs
