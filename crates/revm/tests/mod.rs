use std::{sync::Arc, time::Instant};

use revm_primitives::{
    keccak256, uint, AccountInfo, Address, Bytecode, Bytes, Env, LatestSpec, SpecId, TxKind, U256
};

use revm::{db::{CacheDB, EmptyDB}, Evm};
use revm_ssa::{logger::LsnType, SSACallInput, SSACreateInput, SSALogEntry, SSALogger};
use revm_ssa_graph::{SsaGraph, SSAExecutor, ExecutionTracer, ExecutionMode};

#[derive(Debug, Clone)]
pub enum TestMode {
    BaselineNoSSA,    // Only use evm.transact()
    SerialGraph,      // Use _graph_execute
    ParallelGraph     // Use _graph_execute_parallel
}

/// Execution configuration
#[derive(Debug, Clone)]
pub struct ExecutionConfig {
    /// Execution mode
    pub mode: ExecutionMode,
    /// Whether to collect performance metrics
    pub collect_metrics: bool,
    /// Pre-depolyed contract, for sub-call testing
    pub pre_deployed_contract: Vec<(Address, Bytes)>,
    /// Pre-determined storage slots
    pub pre_determined_slots: Vec<(U256,U256)>,
    /// Input data for the transaction
    pub input: Option<Bytes>,
    /// Thread number for parallel execution
    pub thread_number: Option<usize>,
    /// Test mode
    pub test_mode: TestMode,
    /// Whether to enable tracer
    pub enable_tracer: bool,
    /// Is Deployed Contract
    pub is_deployed_contract: bool,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            mode: ExecutionMode::Full,
            collect_metrics: false,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: None,
            thread_number: Some(8),
            test_mode: TestMode::SerialGraph,
            enable_tracer: true,
            is_deployed_contract: false,
        }
    }
}

/// Execution result
#[derive(Debug)]
pub struct ExecutionResult {
    /// Execution time (if performance metrics collection is enabled)
    pub execution_time: Option<std::time::Duration>,
}

/// Execute test case and verify results
pub fn execute_case(code: Bytes, case_name: &str, config: ExecutionConfig) -> ExecutionResult {
    println!("Testing case: {} with mode {:?}, test mode: {:?}", case_name, config.mode, config.test_mode);
    
    // 1. Prepare contract code and address
    let contract_addr = Address::from([0x2; 20]);
    let bytecode = Bytecode::new_raw(code.clone());
    let code_hash = keccak256(&code);
    let mut cdb = CacheDB::new(EmptyDB::default());
    
    // 2. Deploy contract code to database
    cdb.insert_account_info(
        contract_addr,
        AccountInfo {
            nonce: 0,
            balance: uint!{10000000000000000000000000_U256},
            code_hash,
            code: Some(bytecode),
        },
    );

    if !config.pre_determined_slots.is_empty() {
        for (slot, value) in config.pre_determined_slots.iter() {
            let _ = cdb.insert_account_storage(contract_addr, *slot, *value);
        }
    }

    if !config.pre_deployed_contract.is_empty() {
        for (addr, code) in config.pre_deployed_contract.iter() {
            cdb.insert_account_info(*addr, AccountInfo {
                nonce: 0,
                balance: uint!{10000000000000000000000000_U256},
                code_hash: keccak256(code),
                code: Some(Bytecode::new_raw(code.clone())),
            });
        }
    }
    
    match config.test_mode {
        TestMode::BaselineNoSSA => {
            // Create EVM without SSA Logger
            let mut evm = Evm::builder()
                .with_spec_id(SpecId::CANCUN)
                .with_ref_db(cdb.clone())
                .modify_tx_env(|tx| {
                    tx.caller = Address::from([0x1; 20]);
                    if config.is_deployed_contract {
                        tx.transact_to = TxKind::Create;
                    } else {
                        tx.transact_to = TxKind::Call(contract_addr);
                    }
                    if let Some(input) = config.input.clone() {
                        tx.data = input;
                    };
                    tx.gas_limit = 0x0f424000;
                })
                .build();

            // Execute transact and record time
            let start_time = Instant::now();
            let _result = evm.transact_preverified().unwrap();
            let execution_time = start_time.elapsed();

            ExecutionResult {
                execution_time: Some(execution_time),
            }
        },
        TestMode::SerialGraph | TestMode::ParallelGraph => {
            // Create EVM with SSA Logger
            let mut evm = Evm::builder()
                .with_spec_id(SpecId::LATEST)
                .with_ref_db(cdb.clone())
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
                    tx.gas_limit = 0x0f424000;
                })
                .with_ssa_logger(SSALogger::new())
                .build_with_ssa_logger();

            let env = evm.context.evm.env().clone();
            
            // Execute and get logs
            let start_time = Instant::now();
            let _result = evm.transact().unwrap();
            let execution_time = start_time.elapsed();
            eprintln!("SSA transact time: {:?}", execution_time);
            let mut logger = evm.take_ssa_logger().unwrap();
            // eprintln!("{:?}",logger.get_first_reads());
            let logs = logger.take_logs();
            let first_call = logger.take_first_call_input();
            let first_create = logger.take_first_create_input();
            
            // Choose execution method based on test mode
            let (tracer, execution_time) = match config.test_mode {
                TestMode::SerialGraph => graph_execute(logs, config.clone(), &mut cdb, &env, first_call, first_create),
                TestMode::ParallelGraph => graph_execute_parallel(logs, config.clone(), &mut cdb, &env, first_call, first_create),
                _ => unreachable!(),
            };
            
            if config.enable_tracer {
                let report = tracer.unwrap().generate_report();
                println!("{}", report);
                assert!(report.contains("All results match"), "Results mismatch in case '{}': {}", case_name, report);
            }

            ExecutionResult {
                execution_time,
            }
        }
    }
}


/// Re-execute using graph execution engine
fn graph_execute(entries: Vec<SSALogEntry>, config: ExecutionConfig, db: &mut CacheDB<EmptyDB>, env: &Env, first_call: Option<SSACallInput>, first_create: Option<SSACreateInput>) -> (Option<ExecutionTracer>, Option<std::time::Duration>) {
    // Create dependency graph
    let mut graph = SsaGraph::new(entries.len(), 2*entries.len());
    
    // Collect all LSNs first
    let lsns: Vec<LsnType> = entries.iter().map(|entry| entry.lsn).collect();
    
    // Record original results
    let tracer = if config.enable_tracer {
        let mut tracer = ExecutionTracer::new();
        for entry in entries.iter() {
            tracer.record_original(entry.lsn, entry.outputs.clone().to_vec());
        }
        Some(tracer)
    } else {
        None
    };
    
    // Add all nodes
    for entry in entries {
        graph.add_node(entry).unwrap();
    }
    
    // Add all edges in batch
    for lsn in lsns {
        graph.add_edges(lsn).unwrap();
    }

    // Create executor and tracer
    let mut executor = SSAExecutor::<_, LatestSpec>::new(
        Arc::new(graph), 
        db, 
        env, 
        None, 
        first_call, 
        first_create
        ).with_mode(config.mode).with_tracer(tracer);

    
    // Execute
    let res = executor.execute().unwrap();
    
    (executor.into_tracer(), Some(res.1))
}

/// Re-execute using graph execution engine
fn graph_execute_parallel(
    entries: Vec<SSALogEntry>, 
    config: ExecutionConfig, 
    db: &mut CacheDB<EmptyDB>, 
    env: &Env, 
    first_call: Option<SSACallInput>, 
    first_create: Option<SSACreateInput>) -> (Option<ExecutionTracer>, Option<std::time::Duration>) {
    // Create dependency graph
    let mut graph = SsaGraph::new(entries.len(), 2*entries.len());
    
    // Collect all LSNs first
    let lsns: Vec<LsnType> = entries.iter().map(|entry| entry.lsn).collect();
    
    // Record original results
    let tracer = if config.enable_tracer {
        let mut tracer = ExecutionTracer::new();
        for entry in entries.iter() {
            tracer.record_original(entry.lsn, entry.outputs.clone().to_vec());
        }
        Some(tracer)
    } else {
        None
    };
    
    // Add all nodes
    for entry in entries {
        graph.add_node(entry).unwrap();
    }
    
    // Add all edges in batch
    for lsn in lsns {
        graph.add_edges(lsn).unwrap();
    }

    let cp_ratio = graph.calculate_parallelism_ratio().unwrap();
    println!("Parallelism Ratio: {}", cp_ratio);

    let thread_pool = rayon::ThreadPoolBuilder::new()
        .num_threads(config.thread_number.unwrap_or(8))
        .build()
        .unwrap();
    // Create executor and tracer
    let mut executor = SSAExecutor::<_, LatestSpec>::new(
        Arc::new(graph), 
        db,
        env, 
        Some(thread_pool), 
        first_call, 
        first_create
        ).with_mode(config.mode).with_tracer(tracer);
    
    
    // Execute
    // let execution_time = executor.execute_parallel_batches().unwrap();
    let execution_time = executor.execute().unwrap();
    
    (executor.into_tracer(), Some(execution_time.1))
}

mod arithmetic_tests {
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_add() {
        // Simple addition test: 1 + 2
        let code = Bytes::from(vec![
            0x60, 0x01,  // PUSH1 1
            0x60, 0x02,  // PUSH1 2
            0x01,        // ADD
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple addition", ExecutionConfig::default());
    }

    #[test]
    fn test_sub() {
        // Simple subtraction test: 5 - 2
        let code = Bytes::from(vec![
            0x60, 0x02,  // PUSH1 2
            0x60, 0x05,  // PUSH1 5
            0x03,        // SUB
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple subtraction", ExecutionConfig::default());
    }

    #[test]
    fn test_mul() {
        // Simple multiplication test: 3 * 4
        let code = Bytes::from(vec![
            0x60, 0x03,  // PUSH1 3
            0x60, 0x04,  // PUSH1 4
            0x02,        // MUL
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple multiplication", ExecutionConfig::default());
    }

    #[test]
    fn test_div() {
        // Simple division test: 8 / 2
        let code = Bytes::from(vec![
            0x60, 0x02,  // PUSH1 2
            0x60, 0x08,  // PUSH1 8
            0x04,        // DIV
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple division", ExecutionConfig::default());
    }

    #[test]
    fn test_mod() {
        // Simple modulo test: 7 % 4
        let code = Bytes::from(vec![
            0x60, 0x04,  // PUSH1 4
            0x60, 0x07,  // PUSH1 7
            0x06,        // MOD
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple modulo", ExecutionConfig::default());
    }

    #[test]
    fn test_addmod() {
        // Add modulo test: (5 + 3) % 7
        let code = Bytes::from(vec![
            0x60, 0x07,  // PUSH1 7 (modulus)
            0x60, 0x03,  // PUSH1 3
            0x60, 0x05,  // PUSH1 5
            0x08,        // ADDMOD
            0x00,        // STOP
        ]);
        
        execute_case(code, "add modulo", ExecutionConfig::default());
    }

    #[test]
    fn test_mulmod() {
        // Multiply modulo test: (3 * 4) % 11
        let code = Bytes::from(vec![
            0x60, 0x0B,  // PUSH1 11 (modulus)
            0x60, 0x04,  // PUSH1 4
            0x60, 0x03,  // PUSH1 3
            0x09,        // MULMOD
            0x00,        // STOP
        ]);
        
        execute_case(code, "multiply modulo", ExecutionConfig::default());
    } 

    #[test]
    fn test_sdiv() {
        // Test signed division: -6 / 2 = -3
        let code = Bytes::from(vec![
            0x60, 0x02,  // PUSH1 2
            0x60, 0xFA,  // PUSH1 -6 (250 as signed)
            0x05,        // SDIV
            0x00,        // STOP
        ]);
        
        execute_case(code, "signed division", ExecutionConfig::default());
    }

    #[test]
    fn test_smod() {
        // Test signed modulo: -5 % 3 = -2
        let code = Bytes::from(vec![
            0x60, 0x03,  // PUSH1 3
            0x60, 0xFB,  // PUSH1 -5 (251 as signed)
            0x07,        // SMOD
            0x00,        // STOP
        ]);
        
        execute_case(code, "signed modulo", ExecutionConfig::default());
    }

    #[test]
    fn test_exp() {
        // Test exponentiation: 2 ** 3 = 8
        let code = Bytes::from(vec![
            0x60, 0x03,  // PUSH1 3 (exponent)
            0x60, 0x02,  // PUSH1 2 (base)
            0x0A,        // EXP
            0x00,        // STOP
        ]);
        
        execute_case(code, "exponentiation", ExecutionConfig::default());
    }

    #[test]
    fn test_signextend() {
        // Test sign extension: extend 1 byte to -1 as i8
        let code = Bytes::from(vec![
            0x60, 0xFF,  // PUSH1 0xFF
            0x60, 0x00,  // PUSH1 0 (extend 1 byte)
            0x0B,        // SIGNEXTEND
            0x00,        // STOP
        ]);
        
        execute_case(code, "sign extension", ExecutionConfig::default());
    }
}

mod bitwise_tests {
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_and() {
        // Simple AND operation test: 0xFF AND 0x0F = 0x0F
        let code = Bytes::from(vec![
            0x60, 0x0F,  // PUSH1 0x0F
            0x60, 0xFF,  // PUSH1 0xFF
            0x16,        // AND
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple AND", ExecutionConfig::default());
    }

    #[test]
    fn test_or() {
        // Simple OR operation test: 0xF0 OR 0x0F = 0xFF
        let code = Bytes::from(vec![
            0x60, 0x0F,  // PUSH1 0x0F
            0x60, 0xF0,  // PUSH1 0xF0
            0x17,        // OR
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple OR", ExecutionConfig::default());
    }

    #[test]
    fn test_xor() {
        // Simple XOR operation test: 0xFF XOR 0x0F = 0xF0
        let code = Bytes::from(vec![
            0x60, 0x0F,  // PUSH1 0x0F
            0x60, 0xFF,  // PUSH1 0xFF
            0x18,        // XOR
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple XOR", ExecutionConfig::default());
    }

    #[test]
    fn test_not() {
        // Simple NOT operation test: NOT 0x0F = 0xFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFFF0
        let code = Bytes::from(vec![
            0x60, 0x0F,  // PUSH1 0x0F
            0x19,        // NOT
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple NOT", ExecutionConfig::default());
    }

    #[test]
    fn test_shl() {
        // Simple left shift operation test: 1 << 2 = 4
        let code = Bytes::from(vec![
            0x60, 0x02,  // PUSH1 2 (shift amount)
            0x60, 0x01,  // PUSH1 1 (value to shift)
            0x1B,        // SHL
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple left shift", ExecutionConfig::default());
    }

    #[test]
    fn test_shr() {
        // Simple right shift operation test: 4 >> 2 = 1
        let code = Bytes::from(vec![
            0x60, 0x02,  // PUSH1 2 (shift amount)
            0x60, 0x04,  // PUSH1 4 (value to shift)
            0x1C,        // SHR
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple right shift", ExecutionConfig::default());
    }

    #[test]
    fn test_sar() {
        // Simple arithmetic right shift operation test: -4 >> 1 = -2 (sign preserved)
        let code = Bytes::from(vec![
            0x60, 0x01,  // PUSH1 1 (shift amount)
            0x60, 0xFC,  // PUSH1 0xFC (-4 as 8-bit signed)
            0x1D,        // SAR
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple arithmetic right shift", ExecutionConfig::default());
    }

    #[test]
    fn test_complex_bitwise() {
        // Complex bitwise operations test: ((A AND B) OR (C XOR D)) << 2
        let code = Bytes::from(vec![
            0x60, 0x0F,  // PUSH1 0x0F (D)
            0x60, 0xF0,  // PUSH1 0xF0 (C)
            0x18,        // XOR      -> 0xFF
            0x60, 0x0F,  // PUSH1 0x0F (B)
            0x60, 0xFF,  // PUSH1 0xFF (A)
            0x16,        // AND      -> 0x0F
            0x17,        // OR       -> 0xFF
            0x60, 0x02,  // PUSH1 2
            0x1B,        // SHL      -> 0x3FC
            0x00,        // STOP
        ]);
        
        execute_case(code, "complex bitwise operations", ExecutionConfig::default());
    }

    #[test]
    fn test_lt_gt() {
        // Test LT and GT operations
        let code = Bytes::from(vec![
            0x60, 0x0A,  // PUSH1 10
            0x60, 0x14,  // PUSH1 20
            0x10,        // LT (10 < 20 = 1)
            0x60, 0x14,  // PUSH1 20
            0x60, 0x0A,  // PUSH1 10
            0x11,        // GT (20 > 10 = 1)
            0x00,        // STOP
        ]);
        
        execute_case(code, "lt and gt", ExecutionConfig::default());
    }

    #[test]
    fn test_slt_sgt() {
        // Test SLT and SGT operations (signed comparison)
        let code = Bytes::from(vec![
            0x60, 0xFF,  // PUSH1 -1 (255 as signed)
            0x60, 0x01,  // PUSH1 1
            0x12,        // SLT (-1 < 1 = 1)
            0x60, 0x01,  // PUSH1 1
            0x60, 0xFF,  // PUSH1 -1
            0x13,        // SGT (1 > -1 = 1)
            0x00,        // STOP
        ]);
        
        execute_case(code, "signed lt and gt", ExecutionConfig::default());
    }

    #[test]
    fn test_eq_iszero() {
        // Test EQ and ISZERO operations
        let code = Bytes::from(vec![
            0x60, 0x0A,  // PUSH1 10
            0x60, 0x0A,  // PUSH1 10
            0x14,        // EQ (10 == 10 = 1)
            0x15,        // ISZERO (0)
            0x60, 0x00,  // PUSH1 0
            0x15,        // ISZERO (1)
            0x00,        // STOP
        ]);
        
        execute_case(code, "eq and iszero", ExecutionConfig::default());
    }

    #[test]
    fn test_and_or_xor() {
        // Test AND, OR, and XOR operations
        let code = Bytes::from(vec![
            0x60, 0x0F,  // PUSH1 15 (0000 1111)
            0x60, 0xF0,  // PUSH1 240 (1111 0000)
            0x16,        // AND (0000 0000)
            0x60, 0x0F,  // PUSH1 15
            0x60, 0xF0,  // PUSH1 240
            0x17,        // OR (1111 1111)
            0x60, 0xFF,  // PUSH1 255
            0x60, 0xFF,  // PUSH1 255
            0x18,        // XOR (0000 0000)
            0x00,        // STOP
        ]);
        
        execute_case(code, "and or xor", ExecutionConfig::default());
    }

    #[test]
    fn test_byte() {
        // Test BYTE operation
        let code = Bytes::from(vec![
            0x60, 0xFF,  // PUSH1 255
            0x60, 0x00,  // PUSH1 0 (get the 0th byte)       
            0x1A,        // BYTE (should get 255)
            0x60, 0xFF,  // PUSH1 255
            0x60, 0x01,  // PUSH1 1 (get the 1st byte)
            0x1A,        // BYTE (should get 0)
            0x00,        // STOP
        ]);
        
        execute_case(code, "byte", ExecutionConfig::default());
    }

    #[test]
    fn test_shift_operations() {
        // Test SHL, SHR, and SAR operations
        let code = Bytes::from(vec![
            0x60, 0x01,  // PUSH1 1
            0x60, 0x01,  // PUSH1 1 (shift left by 1)
            0x1B,        // SHL (2)
            0x60, 0x01,  // PUSH1 1
            0x60, 0x02,  // PUSH1 2 (shift right by 1)
            0x1C,        // SHR (1)
            0x60, 0x01,  // PUSH1 1
            0x60, 0xFF,  // PUSH1 -1 (arithmetic right shift)
            0x1D,        // SAR (remains -1)
            0x00,        // STOP
        ]);
        
        execute_case(code, "shift operations", ExecutionConfig::default());
    }
}

mod stack_tests {
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_push1() {
        // Test PUSH1 operation: push a byte onto the stack and verify with ADD
        let code = Bytes::from(vec![
            0x60, 0x42,  // PUSH1 0x42
            0x60, 0x01,  // PUSH1 0x01
            0x01,        // ADD (0x42 + 0x01 = 0x43)
            0x00,        // STOP
        ]);
        
        execute_case(code, "push1 validation", ExecutionConfig::default());
    }
}

mod memory_tests {
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_mload() {
        // Test MLOAD operation:
        // 1. Store a value in memory first
        // 2. Then load this value
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x00,     // PUSH1 0x00 (offset)
            0x52,           // MSTORE
            0x60, 0x00,     // PUSH1 0x00 (offset)
            0x51,           // MLOAD
            0x00,           // STOP
        ]);
        
        execute_case(code, "mload after mstore", ExecutionConfig::default());
    }

    #[test]
    fn test_mstore() {
        // Test MSTORE operation: store a value in memory
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x00,     // PUSH1 0x00 (offset)
            0x52,           // MSTORE
            0x00,           // STOP
        ]);
        
        execute_case(code, "simple mstore", ExecutionConfig::default());
    }

    #[test]
    fn test_mstore8() {
        // Test MSTORE8 operation: store a byte in memory
        let code = Bytes::from(vec![
            0x60, 0xFF,     // PUSH1 0xFF (value)
            0x60, 0x00,     // PUSH1 0x00 (offset)
            0x53,           // MSTORE8
            0x00,           // STOP
        ]);
        
        execute_case(code, "simple mstore8", ExecutionConfig::default());
    }

    #[test]
    fn test_msize() {
        // Test MSIZE operation:
        // 1. First extend memory with MSTORE
        // 2. Then get memory size
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x40,     // PUSH1 0x40 (offset=64)
            0x52,           // MSTORE
            0x59,           // MSIZE
            0x00,           // STOP
        ]);
        
        execute_case(code, "msize after mstore", ExecutionConfig::default());
    }

    #[test]
    fn test_mcopy() {
        // Test MCOPY operation:
        // 1. First store some data
        // 2. Then copy using MCOPY
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x00,     // PUSH1 0x00 (src offset)
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 0x20 (length = 32 bytes)
            0x60, 0x00,     // PUSH1 0x00 (src offset)
            0x60, 0x40,     // PUSH1 0x40 (dst offset = 64)
            0x5E,           // MCOPY
            0x00,           // STOP
        ]);
        
        execute_case(code, "mcopy after mstore", ExecutionConfig::default());
    }

    #[test]
    fn test_memory_expansion() {
        // Test memory expansion:
        // 1. First store at low address
        // 2. Then store at high address, triggering expansion
        // 3. Finally check memory size
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x00,     // PUSH1 0x00 (offset)
            0x52,           // MSTORE
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x40,     // PUSH1 0x40 (offset=64)
            0x52,           // MSTORE
            0x59,           // MSIZE
            0x00,           // STOP
        ]);
        
        execute_case(code, "memory expansion", ExecutionConfig::default());
    }

    #[test]
    fn test_memory_overlap() {
        // Test memory overlap:
        // 1. Store a value
        // 2. Store8 part of this value
        // 3. Load to verify
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x00,     // PUSH1 0x00 (offset)
            0x52,           // MSTORE
            0x60, 0xFF,     // PUSH1 0xFF (byte value)
            0x60, 0x1F,     // PUSH1 0x1F (offset=31)
            0x53,           // MSTORE8
            0x60, 0x00,     // PUSH1 0x00 (offset)
            0x51,           // MLOAD
            0x00,           // STOP
        ]);
        
        execute_case(code, "memory overlap", ExecutionConfig::default());
    }

}

mod control_tests {
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_jump() {
        // Test unconditional jump:
        // PUSH1 jump target
        // JUMP
        // PUSH1 (this instruction won't execute)
        // JUMPDEST (jump target)
        // PUSH1 success value
        let code = Bytes::from(vec![
            0x60, 0x05,  // PUSH1 5 (jump target)
            0x56,        // JUMP
            0x60, 0x00,  // PUSH1 0 (won't execute)
            0x5b,        // JUMPDEST
            0x60, 0x42,  // PUSH1 0x42 (success value)
            0x00,        // STOP
        ]);
        
        execute_case(code, "simple jump", ExecutionConfig::default());
    }

    #[test]
    fn test_jumpi_taken() {
        // Test conditional jump (condition is true):
        // PUSH1 1 (condition)
        // PUSH1 jump target
        // JUMPI
        // PUSH1 (this instruction won't execute)
        // JUMPDEST (jump target)
        // PUSH1 success value
        let code = Bytes::from(vec![
            0x60, 0x01,  // PUSH1 1 (condition is true)
            0x60, 0x07,  // PUSH1 6 (jump target)
            0x57,        // JUMPI
            0x60, 0x00,  // PUSH1 0 (won't execute)
            0x5b,        // JUMPDEST
            0x60, 0x42,  // PUSH1 0x42 (success value)
            0x00,        // STOP
        ]);
        
        execute_case(code, "jumpi taken", ExecutionConfig::default());
    }

    #[test]
    fn test_jumpi_not_taken() {
        // Test conditional jump (condition is false):
        // PUSH1 0 (condition)
        // PUSH1 jump target
        // JUMPI
        // PUSH1 success value (this instruction will execute)
        // JUMPDEST
        // PUSH1 (won't reach here)
        let code = Bytes::from(vec![
            0x60, 0x00,  // PUSH1 0 (condition is false)
            0x60, 0x06,  // PUSH1 6 (jump target)
            0x57,        // JUMPI
            0x60, 0x42,  // PUSH1 0x42 (will execute)
            0x5b,        // JUMPDEST
            0x60, 0x43,  // PUSH1 0x43 (won't execute)
            0x00,        // STOP
        ]);
        
        execute_case(code, "jumpi not taken", ExecutionConfig::default());
    }

    #[test]
    fn test_pc() {
        // Test PC instruction:
        // PC (get current program counter value)
        // PUSH1 expected value
        // EQ (compare)
        let code = Bytes::from(vec![
            0x58,        // PC (PC=0 at this point)
            0x60, 0x00,  // PUSH1 0 (expected value)
            0x14,        // EQ
            0x00,        // STOP
        ]);
        
        execute_case(code, "pc instruction", ExecutionConfig::default());
    }

    #[test]
    fn test_complex_jumps() {
        // Test complex jump logic:
        // 1. First unconditionally jump to the first JUMPDEST
        // 2. Then jump to the second JUMPDEST based on condition
        // 3. Finally jump unconditionally to the end
        let code = Bytes::from(vec![
            0x60, 0x04,  // PUSH1 4 (first jump target)
            0x56,        // JUMP
            0x00,        // STOP (won't execute)
            0x5b,        // JUMPDEST (first target)
            0x60, 0x01,  // PUSH1 1 (condition is true)
            0x60, 0x0b,  // PUSH1 12 (second jump target)
            0x57,        // JUMPI
            0x00,        // STOP (won't execute)
            0x5b,        // JUMPDEST (second target)
            0x60, 0x10,  // PUSH1 16 (third jump target)
            0x56,        // JUMP
            0x00,        // STOP (won't execute)
            0x5b,        // JUMPDEST (third target)
            0x60, 0x42,  // PUSH1 0x42 (success value)
            0x00,        // STOP
        ]);
        
        execute_case(code, "complex jumps", ExecutionConfig::default());
    }

    #[test]
    fn test_jumpdest() {
        // Test JUMPDEST as a normal instruction:
        // No jump, execute directly to JUMPDEST
        let code = Bytes::from(vec![
            0x60, 0x42,  // PUSH1 0x42
            0x5b,        // JUMPDEST (as normal instruction)
            0x60, 0x43,  // PUSH1 0x43
            0x01,        // ADD
            0x00,        // STOP
        ]);
        
        execute_case(code, "jumpdest as normal instruction", ExecutionConfig::default());
    }

    #[test]
    fn test_multiple_jumpdests() {
        // Test multiple JUMPDEST and conditional jumps
        let code = Bytes::from(vec![
            0x60, 0x01,  // PUSH1 1 (first condition)
            0x60, 0x08,  // PUSH1 10 (first jump target)
            0x57,        // JUMPI
            0x60, 0x40,  // PUSH1 0x40 (won't execute)
            0x00,        // STOP (won't execute)
            0x5b,        // JUMPDEST (first target)
            0x60, 0x00,  // PUSH1 0 (second condition)
            0x60, 0x12,  // PUSH1 15 (second jump target)
            0x57,        // JUMPI
            0x60, 0x41,  // PUSH1 0x41 (will execute)
            0x00,        // STOP
            0x5b,        // JUMPDEST (second target)
            0x60, 0x42,  // PUSH1 0x42 (won't execute)
            0x00,        // STOP
        ]);
        
        execute_case(code, "multiple jumpdests", ExecutionConfig::default());
    }
}

mod system_tests {
    use revm_primitives::address;
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_gas() {
        // Test GAS operation:
        // Push current available gas onto the stack
        let code = Bytes::from(vec![
            0x5A,        // GAS
            0x00,        // STOP
        ]);
        
        execute_case(code, "gas operation", ExecutionConfig::default());
    }

    #[test]
    fn test_address() {
        // Test ADDRESS operation:
        // Get current contract address and store it
        let code = Bytes::from(vec![
            0x30,        // ADDRESS
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "address operation", ExecutionConfig::default());
    }

    #[test]
    fn test_caller() {
        // Test CALLER operation:
        // Get caller address and store it
        let code = Bytes::from(vec![
            0x33,        // CALLER
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "caller operation", ExecutionConfig::default());
    }

    #[test]
    fn test_codesize() {
        // Test CODESIZE operation:
        // Get code size and store it
        let code = Bytes::from(vec![
            0x38,        // CODESIZE
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "codesize operation", ExecutionConfig::default());
    }

    #[test]
    fn test_codecopy() {
        // Test CODECOPY operation:
        // 1. Copy code to memory
        // 2. Then load from memory and store
        let code = Bytes::from(vec![
            0x60, 0x0A,  // PUSH1 10 (length)
            0x60, 0x00,  // PUSH1 0 (code offset)
            0x60, 0x00,  // PUSH1 0 (memory offset)
            0x39,        // CODECOPY
            0x60, 0x0A,  // PUSH1 10 (length)
            0x60, 0x00,  // PUSH1 0 (offset)
            0x51,        // MLOAD
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "codecopy operation", ExecutionConfig::default());
    }

    #[test]
    fn test_calldataload() {
        // Test CALLDATALOAD operation:
        // Load 32 bytes from call data and store
        let code = Bytes::from(vec![
            0x60, 0x00,  // PUSH1 0 (calldata offset)
            0x35,        // CALLDATALOAD
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "calldataload operation", ExecutionConfig::default());
    }

    #[test]
    fn test_calldatasize() {
        // Test CALLDATASIZE operation:
        // Get call data size and store
        let code = Bytes::from(vec![
            0x36,        // CALLDATASIZE
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "calldatasize operation", ExecutionConfig::default());
    }

    #[test]
    fn test_calldatacopy() {
        // Test CALLDATACOPY operation:
        // 1. Copy call data to memory
        // 2. Load from memory and store
        let code = Bytes::from(vec![
            0x60, 0x20,  // PUSH1 32 (length)
            0x60, 0x00,  // PUSH1 0 (calldata offset)
            0x60, 0x00,  // PUSH1 0 (memory offset)
            0x37,        // CALLDATACOPY
            0x60, 0x00,  // PUSH1 0 (memory offset)
            0x51,        // MLOAD
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "calldatacopy operation", ExecutionConfig::default());
    }

    #[test]
    fn test_callvalue() {
        // Test CALLVALUE operation:
        // Get call value and store
        let code = Bytes::from(vec![
            0x34,        // CALLVALUE
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "callvalue operation", ExecutionConfig::default());
    }

    #[test]
    fn test_returndatasize() {
        // Prepare callee contract code - will return a fixed value 0x42
        let callee_code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 32
            0x60, 0x00,     // PUSH1 0
            0xF3,           // RETURN
        ]);

        // Prepare caller code
        let caller_code = Bytes::from(vec![
            // Execute CALL
            0x60, 0x20,     // PUSH1 32 (retSize)
            0x60, 0x00,     // PUSH1 0 (retOffset)
            0x60, 0x00,     // PUSH1 0 (argsSize)
            0x60, 0x00,     // PUSH1 0 (argsOffset)
            0x60, 0x00,     // PUSH1 0 (value)
            0x73,           // PUSH20 (address opcode)
            0x12, 0x34, 0x56, 0x78, 0x9a,  // address bytes 1-5
            0xbc, 0xde, 0xf0, 0x12, 0x34,  // address bytes 6-10
            0x56, 0x78, 0x9a, 0xbc, 0xde,  // address bytes 11-15
            0xf0, 0x12, 0x34, 0x56, 0x78,  // address bytes 16-20
            0x60, 0xFF,     // PUSH1 255 (gas)
            0xF1,           // CALL
            
            // Get return data size and store
            0x3D,           // RETURNDATASIZE
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);

        let target_address = address!("123456789abcdef0123456789abcdef012345678");
        
        execute_case(caller_code, "returndatasize operation", ExecutionConfig{
            pre_deployed_contract: vec![(target_address, callee_code)],..Default::default()
        });
    }

    #[test]
    fn test_returndatacopy() {
        // Prepare callee contract code - will return a fixed value 0x42
        let callee_code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 32
            0x60, 0x00,     // PUSH1 0
            0xF3,           // RETURN
        ]);

        // Prepare caller code
        let caller_code = Bytes::from(vec![
            // Execute CALL
            0x60, 0x20,     // PUSH1 32 (retSize)
            0x60, 0x00,     // PUSH1 0 (retOffset)
            0x60, 0x00,     // PUSH1 0 (argsSize)
            0x60, 0x00,     // PUSH1 0 (argsOffset)
            0x60, 0x00,     // PUSH1 0 (value)
            0x73,           // PUSH20 (address opcode)
            0x12, 0x34, 0x56, 0x78, 0x9a,  // address bytes 1-5
            0xbc, 0xde, 0xf0, 0x12, 0x34,  // address bytes 6-10
            0x56, 0x78, 0x9a, 0xbc, 0xde,  // address bytes 11-15
            0xf0, 0x12, 0x34, 0x56, 0x78,  // address bytes 16-20
            0x60, 0xFF,     // PUSH1 255 (gas)
            0xF1,           // CALL
            
            // Use RETURNDATACOPY to copy return data
            0x60, 0x20,     // PUSH1 32 (length)
            0x60, 0x00,     // PUSH1 0 (returndata offset)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x3E,           // RETURNDATACOPY
            
            // Load copied data and store
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x51,           // MLOAD
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);

        let target_address = address!("123456789abcdef0123456789abcdef012345678");
        
        execute_case(caller_code, "returndatacopy operation", ExecutionConfig{
            pre_deployed_contract: vec![(target_address, callee_code)],..Default::default()
        });
    }

    #[test]
    fn test_complex_system_operations() {
        // Test complex system operations combination:
        // 1. Get caller address
        // 2. Get contract address
        // 3. Compare two addresses
        // 4. Store different values based on comparison result
        let code = Bytes::from(vec![
            0x33,        // CALLER
            0x30,        // ADDRESS
            0x14,        // EQ
            0x60, 0x0E,  // PUSH1 14 (jump dest if equal)
            0x57,        // JUMPI
            0x60, 0x00,  // PUSH1 0 (value if not equal)
            0x60, 0x00,  // PUSH1 0 (slot)
            0x55,        // SSTORE
            0x60, 0x14,  // PUSH1 20 (jump to end)
            0x56,        // JUMP
            0x5B,        // JUMPDEST
            0x60, 0x01,  // PUSH1 1 (value if equal)
            0x60, 0x00,  // PUSH1 0 (slot)
            0x55,        // SSTORE
            0x5B,        // JUMPDEST
            0x00,        // STOP
        ]);
        
        execute_case(code, "complex system operations", ExecutionConfig::default());
    }

    #[test]
    fn test_keccak256() {
        // Test KECCAK256 operation:
        // 1. Store data in memory
        // 2. Calculate hash value
        // 3. Store hash value in storage
        let code = Bytes::from(vec![
            // Store data in memory
            0x60, 0x04,     // PUSH1 4 (data length)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x52,           // MSTORE (store "0x00000004")
            
            // Calculate hash value
            0x60, 0x04,     // PUSH1 4 (length)
            0x60, 0x00,     // PUSH1 0 (offset)
            0x20,           // SHA3/KECCAK256
            
            // Store hash value in storage
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);
        
        execute_case(code, "keccak256 basic", ExecutionConfig::default());
    }

    #[test]
    fn test_keccak256_empty() {
        // Test KECCAK256 of empty data:
        // 1. Calculate hash value of empty data
        // 2. Store hash value in storage
        let code = Bytes::from(vec![
            // Calculate hash value of empty data
            0x60, 0x00,     // PUSH1 0 (length)
            0x60, 0x00,     // PUSH1 0 (offset)
            0x20,           // SHA3/KECCAK256
            
            // Store hash value in storage
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);
        
        execute_case(code, "keccak256 empty", ExecutionConfig::default());
    }

    #[test]
    fn test_keccak256_large() {
        // Test KECCAK256 of large data:
        // 1. Store multiple data in memory
        // 2. Calculate hash value of entire block of data
        // 3. Store hash value in storage
        let code = Bytes::from(vec![
            // Store first data
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x52,           // MSTORE
            
            // Store second data
            0x60, 0x43,     // PUSH1 0x43
            0x60, 0x20,     // PUSH1 32 (memory offset)
            0x52,           // MSTORE
            
            // Calculate hash value of 64 bytes of data
            0x60, 0x40,     // PUSH1 64 (length)
            0x60, 0x00,     // PUSH1 0 (offset)
            0x20,           // SHA3/KECCAK256
            
            // Store hash value in storage
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);
        
        execute_case(code, "keccak256 large data", ExecutionConfig::default());
    }

    #[test]
    fn test_keccak256_offset() {
        // Test KECCAK256 with offset:
        // 1. Store data in memory at different positions
        // 2. Calculate hash value starting from offset
        // 3. Store hash value in storage
        let code = Bytes::from(vec![
            // Store data
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x20,     // PUSH1 32 (memory offset)
            0x52,           // MSTORE
            
            // Calculate hash value starting from offset
            0x60, 0x20,     // PUSH1 32 (length)
            0x60, 0x20,     // PUSH1 32 (offset)
            0x20,           // SHA3/KECCAK256
            
            // Store hash value in storage
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);
        
        execute_case(code, "keccak256 with offset", ExecutionConfig::default());
    }
}

mod host_env_tests{
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_chainid() {
        // Test CHAINID operation: get current chain ID
        let code = Bytes::from(vec![
            0x46,        // CHAINID
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "chainid operation", ExecutionConfig::default());
    }

    #[test]
    fn test_coinbase() {
        // Test COINBASE operation: get current block miner address
        let code = Bytes::from(vec![
            0x41,        // COINBASE
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "coinbase operation", ExecutionConfig::default());
    }

    #[test]
    fn test_timestamp() {
        // Test TIMESTAMP operation: get current block timestamp
        let code = Bytes::from(vec![
            0x42,        // TIMESTAMP
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "timestamp operation", ExecutionConfig::default());
    }

    #[test]
    fn test_number() {
        // Test NUMBER operation: get current block number
        let code = Bytes::from(vec![
            0x43,        // NUMBER
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "block number operation", ExecutionConfig::default());
    }

    #[test]
    fn test_difficulty_prevrandao() {
        // Test DIFFICULTY/PREVRANDAO operation: get current block difficulty or random value
        let code = Bytes::from(vec![
            0x44,        // DIFFICULTY/PREVRANDAO
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "difficulty/prevrandao operation", ExecutionConfig::default());
    }

    #[test]
    fn test_gaslimit() {
        // Test GASLIMIT operation: get current block gas limit
        let code = Bytes::from(vec![
            0x45,        // GASLIMIT
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "gaslimit operation", ExecutionConfig::default());
    }

    #[test]
    fn test_gasprice() {
        // Test GASPRICE operation: get gas price of current transaction
        let code = Bytes::from(vec![
            0x3A,        // GASPRICE
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "gasprice operation", ExecutionConfig::default());
    }

    #[test]
    fn test_basefee() {
        // Test BASEFEE operation: get base fee of current block
        let code = Bytes::from(vec![
            0x48,        // BASEFEE
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "basefee operation", ExecutionConfig::default());
    }

    #[test]
    fn test_origin() {
        // Test ORIGIN operation: get transaction origin address
        let code = Bytes::from(vec![
            0x32,        // ORIGIN
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "origin operation", ExecutionConfig::default());
    }

    #[test]
    fn test_blobhash() {
        // Test BLOBHASH operation: get hash of blob at specified index
        let code = Bytes::from(vec![
            0x60, 0x00,  // PUSH1 0 (blob index)
            0x49,        // BLOBHASH
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "blobhash operation", ExecutionConfig::default());
    }

    #[test]
    fn test_blobbasefee() {
        // Test BLOBBASEFEE operation: get base fee of current block's blob
        let code = Bytes::from(vec![
            0x4A,        // BLOBBASEFEE
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "blobbasefee operation", ExecutionConfig::default());
    }

    #[test]
    fn test_complex_host_env() {
        // Test complex host environment operations combination:
        // 1. Get block number
        // 2. Get timestamp
        // 3. If block number is greater than timestamp, store gas price
        // 4. Otherwise store base fee
        let code = Bytes::from(vec![
            0x43,        // NUMBER
            0x42,        // TIMESTAMP
            0x11,        // GT
            0x60, 0x0A,  // PUSH1 10 (jump dest)
            0x57,        // JUMPI
            0x48,        // BASEFEE
            0x60, 0x0C,  // PUSH1 12 (jump to storage)
            0x56,        // JUMP
            0x5B,        // JUMPDEST (10)
            0x3A,        // GASPRICE
            0x5B,        // JUMPDEST (12)
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "complex host env operations", ExecutionConfig::default());
    }

    #[test]
    fn test_host_env_comparison() {
        // Test comparison of host environment values:
        // 1. Compare gas price and base fee
        // 2. Store different values based on comparison result
        let code = Bytes::from(vec![
            0x3A,        // GASPRICE
            0x48,        // BASEFEE
            0x11,        // GT
            0x60, 0x0B,  // PUSH1 11 (jump dest)
            0x57,        // JUMPI
            0x60, 0x00,  // PUSH1 0 (if gas price is not greater than base fee)
            0x60, 0x0E,  // PUSH1 14 (jump to storage)
            0x56,        // JUMP
            0x5B,        // JUMPDEST (11)
            0x60, 0x01,  // PUSH1 1 (if gas price is greater than base fee)
            0x5B,        // JUMPDEST (14)
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "host env comparison", ExecutionConfig::default());
    }

    #[test]
    fn test_host_env_arithmetic() {
        // Test arithmetic operations on host environment values:
        // 1. basefee + gasprice
        // 2. gaslimit - block.number
        // 3. Multiply the results
        let code = Bytes::from(vec![
            0x48,        // BASEFEE
            0x3A,        // GASPRICE
            0x01,        // ADD
            0x45,        // GASLIMIT
            0x43,        // NUMBER
            0x03,        // SUB
            0x02,        // MUL
            0x60, 0x00,  // PUSH1 0 (storage slot)
            0x55,        // SSTORE
            0x00,        // STOP
        ]);
        
        execute_case(code, "host env arithmetic", ExecutionConfig::default());
    }
}

mod host_tests{
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_sload() {
        // Test SLOAD operation:
        // 1. First store a value using SSTORE
        // 2. Then load this value using SLOAD
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x55,           // SSTORE
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x54,           // SLOAD
            0x00,           // STOP
        ]);
        
        execute_case(code, "sload after sstore", ExecutionConfig::default());
    }

    #[test]
    fn test_sstore() {
        // Test SSTORE operation: store a value to storage slot
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value)
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x55,           // SSTORE
            0x00,           // STOP
        ]);
        
        execute_case(code, "simple sstore", ExecutionConfig::default());
    }

    #[test]
    fn test_storage_update() {
        // Test storage update:
        // 1. First store a value
        // 2. Read and modify this value
        // 3. Store the modified value again
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (initial value)
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x55,           // SSTORE
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x54,           // SLOAD
            0x60, 0x01,     // PUSH1 0x01
            0x01,           // ADD
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x55,           // SSTORE
            0x00,           // STOP
        ]);
        
        execute_case(code, "storage update", ExecutionConfig::default());
    }

    #[test]
    fn test_multiple_slots() {
        // Test multiple storage slots:
        // Store different values in different slots
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (value for slot 0)
            0x60, 0x00,     // PUSH1 0x00 (slot 0)
            0x55,           // SSTORE
            0x60, 0x43,     // PUSH1 0x43 (value for slot 1)
            0x60, 0x01,     // PUSH1 0x01 (slot 1)
            0x55,           // SSTORE
            0x60, 0x00,     // PUSH1 0x00 (slot 0)
            0x54,           // SLOAD
            0x60, 0x01,     // PUSH1 0x01 (slot 1)
            0x54,           // SLOAD
            0x00,           // STOP
        ]);
        
        execute_case(code, "multiple storage slots", ExecutionConfig::default());
    }

    #[test]
    fn test_zero_slot() {
        // Test storage slot with zero value:
        // 1. Store non-zero value
        // 2. Store zero value (should delete storage slot)
        // 3. Verify
        let code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42 (non-zero value)
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x55,           // SSTORE
            0x60, 0x00,     // PUSH1 0x00 (zero value)
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x55,           // SSTORE
            0x60, 0x00,     // PUSH1 0x00 (slot)
            0x54,           // SLOAD
            0x00,           // STOP
        ]);
        
        execute_case(code, "zero value storage", ExecutionConfig::default());
    }

    #[test]
    fn test_selfbalance() {
        // Test SELFBALANCE operation:
        // Get current contract balance and store it in storage slot
        let code = Bytes::from(vec![
            0x47,           // SELFBALANCE
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            0x00,           // STOP
        ]);
        
        execute_case(code, "selfbalance operation", ExecutionConfig::default());
    }

    #[test]
    fn test_extcodesize_and_copy() {
        // Test EXTCODESIZE and EXTCODECOPY operations:
        // 1. Get external contract code size
        // 2. Copy code to memory
        // 3. Load from memory and store result
        let code = Bytes::from(vec![
            0x30,           // ADDRESS (get current contract address)
            0x3B,           // EXTCODESIZE
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE (store code size)
            
            // EXTCODECOPY
            0x60, 0x20,     // PUSH1 32 (length)
            0x60, 0x00,     // PUSH1 0 (code offset)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x30,           // ADDRESS
            0x3C,           // EXTCODECOPY
            
            // Load copied code and store
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x51,           // MLOAD
            0x60, 0x01,     // PUSH1 1 (storage slot)
            0x55,           // SSTORE
            0x00,           // STOP
        ]);
        
        execute_case(code, "extcodesize and copy operations", ExecutionConfig::default());
    }

    #[test]
    fn test_extcodehash() {
        // Test EXTCODEHASH operation:
        // 1. Get hash of external contract code
        // 2. Store hash in storage slot
        let code = Bytes::from(vec![
            0x30,           // ADDRESS
            0x3F,           // EXTCODEHASH
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            0x00,           // STOP
        ]);
        
        execute_case(code, "extcodehash operation", ExecutionConfig::default());
    }

    #[test]
    fn test_blockhash() {
        // Test BLOCKHASH operation:
        // 1. Get hash of specified block
        // 2. Store hash in storage slot
        let code = Bytes::from(vec![
            0x43,           // NUMBER (get current block number)
            0x60, 0x01,     // PUSH1 1
            0x03,           // SUB (current block number - 1)
            0x40,           // BLOCKHASH
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            0x00,           // STOP
        ]);
        
        execute_case(code, "blockhash operation", ExecutionConfig::default());
    }

    #[test]
    fn test_logs() {
        // Test LOG operation:
        // 1. Prepare log data
        // 2. Use different numbers of topics for logging
        let code = Bytes::from(vec![
            // Prepare log data
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x52,           // MSTORE
            
            // LOG0: no topic
            0x60, 0x20,     // PUSH1 32 (length)
            0x60, 0x00,     // PUSH1 0 (offset)
            0xA0,           // LOG0
            
            // LOG1: 1 topic
            0x60, 0x01,     // PUSH1 1 (topic1)
            0x60, 0x20,     // PUSH1 32 (length)
            0x60, 0x00,     // PUSH1 0 (offset)
            0xA1,           // LOG1
            
            // LOG4: 4 topics
            0x60, 0x04,     // PUSH1 1 (topic4)
            0x60, 0x03,     // PUSH1 2 (topic3)
            0x60, 0x02,     // PUSH1 3 (topic2)
            0x60, 0x01,     // PUSH1 4 (topic1)
            0x60, 0x20,     // PUSH1 32 (length)
            0x60, 0x00,     // PUSH1 0 (offset)
            0xA4,           // LOG4
            
            0x00,           // STOP
        ]);
        
        execute_case(code, "log operations", ExecutionConfig::default());
    }

    #[test]
    fn test_selfdestruct() {
        // Test SELFDESTRUCT operation:
        // 1. Store some values
        // 2. Execute self-destruct operation
        let code = Bytes::from(vec![
            // First store some values
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            // Get current contract balance
            0x47,           // SELFBALANCE
            0x60, 0x01,     // PUSH1 1 (storage slot)
            0x55,           // SSTORE
            
            // Execute self-destruct, send balance to address 0
            0x60, 0x00,     // PUSH1 0 (target address)
            0xFF,           // SELFDESTRUCT
            
            0x00,           // STOP (won't reach here)
        ]);
        
        execute_case(code, "selfdestruct operation", ExecutionConfig::default());
    }

    #[test]
    fn test_complex_host_interactions() {
        // Test complex host interaction scenario:
        // 1. Get contract balance
        // 2. Get code size
        // 3. Store these values
        // 4. Log values
        // 5. Conditional self-destruct
        let code = Bytes::from(vec![
            // Get and store contract balance
            0x47,           // SELFBALANCE
            0x60, 0x00,     // PUSH1 0 (slot 0)
            0x55,           // SSTORE
            
            // Get and store code size
            0x30,           // ADDRESS
            0x3B,           // EXTCODESIZE
            0x60, 0x01,     // PUSH1 1 (slot 1)
            0x55,           // SSTORE
            
            // Compare balance and code size
            0x60, 0x00,     // PUSH1 0 (slot 0)
            0x54,           // SLOAD (load balance)
            0x60, 0x01,     // PUSH1 1 (slot 1)
            0x54,           // SLOAD (load code size)
            0x11,           // GT (balance > code size?)
            
            // Record comparison result
            0x60, 0x20,     // PUSH1 32 (length)
            0x60, 0x00,     // PUSH1 0 (offset)
            0xA0,           // LOG0
            
            // If balance is greater than code size, self-destruct
            0x60, 0x1D,     // PUSH1 29 (jump dest)
            0x57,           // JUMPI
            0x60, 0x00,     // PUSH1 0 (target address)
            0xFF,           // SELFDESTRUCT
            0x5B,           // JUMPDEST
            
            0x00,           // STOP
        ]);
        
        execute_case(code, "complex host interactions", ExecutionConfig::default());
    }
}

mod contract_tests {
    use revm_primitives::address;
    use revm::primitives::Bytes;
    use super::*;

    #[test]
    fn test_create() {
        let code = Bytes::from(vec![
            // Step 1: Copy runtime code to memory
            0x60, 0x0c,     // PUSH1 12 (length of runtime code)
            0x60, 0x0f,     // PUSH1 15 (offset of runtime code in code)
            0x60, 0x00,     // PUSH1 0 (target memory position)
            0x39,           // CODECOPY

            // Step 2: Execute CREATE
            0x60, 0x0c,     // PUSH1 12 (length)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x60, 0x00,     // PUSH1 0 (value)
            0xF0,           // CREATE

            0x00,           // STOP

            // Runtime code starts (from offset 15)
            0x60, 0x03,     // PUSH1 3
            0x80,           // DUP1
            0x60, 0x00,     // PUSH1 0
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 32
            0x60, 0x00,     // PUSH1 0
            0xF3            // RETURN
        ]);
        
        execute_case(code, "create with proper runtime code", ExecutionConfig::default());
    }

    #[test]
    fn test_create2() {
        let code = Bytes::from(vec![
            // Step 1: Copy runtime code to memory
            0x60, 0x0c,     // PUSH1 12 (length of runtime code)
            0x60, 0x11,     // PUSH1 17 (offset of runtime code in code)
            0x60, 0x00,     // PUSH1 0 (target memory position)
            0x39,           // CODECOPY

            // Step 2: Execute CREATE2
            0x60, 0x00,     // PUSH1 0 (salt)
            0x60, 0x0c,     // PUSH1 12 (length)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x60, 0x00,     // PUSH1 0 (value)
            0xF5,           // CREATE2

            0x00,           // STOP

            // Runtime code starts (from offset 15)
            0x60, 0x03,     // PUSH1 3
            0x80,           // DUP1
            0x60, 0x00,     // PUSH1 0
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 32
            0x60, 0x00,     // PUSH1 0
            0xF3            // RETURN
        ]);
        
        execute_case(code, "create2 with proper runtime code", ExecutionConfig::default());
    }

    #[test]
    fn test_call() {
        // Prepare callee code
        let callee_code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 32
            0x60, 0x00,     // PUSH1 0
            0xF3,           // RETURN
        ]);

        // Prepare caller code
        let caller_code = Bytes::from(vec![
            // Prepare call data
            0x60, 0x01,     // PUSH1 1 (call data)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x52,           // MSTORE
            
            // Execute CALL
            0x60, 0x20,     // PUSH1 32 (retSize)
            0x60, 0x00,     // PUSH1 0 (retOffset)
            0x60, 0x20,     // PUSH1 32 (argsSize)
            0x60, 0x00,     // PUSH1 0 (argsOffset)
            0x60, 0x00,     // PUSH1 0 (value)
            0x73,           // PUSH20 (address opcode)
            0x12, 0x34, 0x56, 0x78, 0x9a,  // address bytes 1-5
            0xbc, 0xde, 0xf0, 0x12, 0x34,  // address bytes 6-10
            0x56, 0x78, 0x9a, 0xbc, 0xde,  // address bytes 11-15
            0xf0, 0x12, 0x34, 0x56, 0x78,  // address bytes 16-20
            0x60, 0xFF,     // PUSH1 255 (gas)
            0xF1,           // CALL
            
            // Store return data
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x51,           // MLOAD
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);

        let target_address = address!("123456789abcdef0123456789abcdef012345678");
        
        execute_case(caller_code, "simple call", ExecutionConfig{
            pre_deployed_contract: vec![(target_address, callee_code)],..Default::default()
        });
    }

    #[test]
    fn test_delegatecall() {
        // Prepare callee code
        let callee_code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 32
            0x60, 0x00,     // PUSH1 0
            0xF3,           // RETURN
        ]);

        // Prepare caller code
        let caller_code = Bytes::from(vec![
            // Prepare call data
            0x60, 0x01,     // PUSH1 1 (call data)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x52,           // MSTORE
            
            // Execute CALL
            0x60, 0x20,     // PUSH1 32 (retSize)
            0x60, 0x00,     // PUSH1 0 (retOffset)
            0x60, 0x20,     // PUSH1 32 (argsSize)
            0x60, 0x00,     // PUSH1 0 (argsOffset)
            0x73,           // PUSH20 (address opcode)
            0x12, 0x34, 0x56, 0x78, 0x9a,  // address bytes 1-5
            0xbc, 0xde, 0xf0, 0x12, 0x34,  // address bytes 6-10
            0x56, 0x78, 0x9a, 0xbc, 0xde,  // address bytes 11-15
            0xf0, 0x12, 0x34, 0x56, 0x78,  // address bytes 16-20
            0x60, 0xFF,     // PUSH1 255 (gas)
            0xF4,           // DELEGATECALL
            
            // Store return data
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x51,           // MLOAD
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);

        let target_address = address!("123456789abcdef0123456789abcdef012345678");
        
        execute_case(caller_code, "delegatecall with predeploy", ExecutionConfig{
            pre_deployed_contract: vec![(target_address, callee_code)],..Default::default()
        });
    }

    #[test]
    fn test_staticcall() {
         // Prepare callee code
         let callee_code = Bytes::from(vec![
            0x60, 0x42,     // PUSH1 0x42
            0x60, 0x00,     // PUSH1 0
            0x52,           // MSTORE
            0x60, 0x20,     // PUSH1 32
            0x60, 0x00,     // PUSH1 0
            0xF3,           // RETURN
        ]);

        // Prepare caller code
        let caller_code = Bytes::from(vec![
            // Prepare call data
            0x60, 0x01,     // PUSH1 1 (call data)
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x52,           // MSTORE
            
            // Execute CALL
            0x60, 0x20,     // PUSH1 32 (retSize)
            0x60, 0x00,     // PUSH1 0 (retOffset)
            0x60, 0x20,     // PUSH1 32 (argsSize)
            0x60, 0x00,     // PUSH1 0 (argsOffset)
            0x73,           // PUSH20 (address opcode)
            0x12, 0x34, 0x56, 0x78, 0x9a,  // address bytes 1-5
            0xbc, 0xde, 0xf0, 0x12, 0x34,  // address bytes 6-10
            0x56, 0x78, 0x9a, 0xbc, 0xde,  // address bytes 11-15
            0xf0, 0x12, 0x34, 0x56, 0x78,  // address bytes 16-20
            0x60, 0xFF,     // PUSH1 255 (gas)
            0xFA,           // DELEGATECALL
            
            // Store return data
            0x60, 0x00,     // PUSH1 0 (memory offset)
            0x51,           // MLOAD
            0x60, 0x00,     // PUSH1 0 (storage slot)
            0x55,           // SSTORE
            
            0x00,           // STOP
        ]);

        let target_address = address!("123456789abcdef0123456789abcdef012345678");
        
        execute_case(caller_code, "staticall with predeploy", ExecutionConfig{
            pre_deployed_contract: vec![(target_address, callee_code)],..Default::default()
        });
    }
}

mod erc20_tests {

    use revm_primitives::hex;
    use revm::primitives::Bytes;
    use super::*;

    /// ERC20 Deploy code
    const DEPLOY_CODE : &str = "0x608060405234801561000f575f80fd5b50604051610f27380380610f27833981810160405281019061003191906101a4565b815f908161003f9190610427565b50806001908161004f9190610427565b5050506104f6565b5f604051905090565b5f80fd5b5f80fd5b5f80fd5b5f80fd5b5f601f19601f8301169050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52604160045260245ffd5b6100b682610070565b810181811067ffffffffffffffff821117156100d5576100d4610080565b5b80604052505050565b5f6100e7610057565b90506100f382826100ad565b919050565b5f67ffffffffffffffff82111561011257610111610080565b5b61011b82610070565b9050602081019050919050565b8281835e5f83830152505050565b5f610148610143846100f8565b6100de565b9050828152602081018484840111156101645761016361006c565b5b61016f848285610128565b509392505050565b5f82601f83011261018b5761018a610068565b5b815161019b848260208601610136565b91505092915050565b5f80604083850312156101ba576101b9610060565b5b5f83015167ffffffffffffffff8111156101d7576101d6610064565b5b6101e385828601610177565b925050602083015167ffffffffffffffff81111561020457610203610064565b5b61021085828601610177565b9150509250929050565b5f81519050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52602260045260245ffd5b5f600282049050600182168061026857607f821691505b60208210810361027b5761027a610224565b5b50919050565b5f819050815f5260205f209050919050565b5f6020601f8301049050919050565b5f82821b905092915050565b5f600883026102dd7fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff826102a2565b6102e786836102a2565b95508019841693508086168417925050509392505050565b5f819050919050565b5f819050919050565b5f61032b610326610321846102ff565b610308565b6102ff565b9050919050565b5f819050919050565b61034483610311565b61035861035082610332565b8484546102ae565b825550505050565b5f90565b61036c610360565b61037781848461033b565b505050565b5b8181101561039a5761038f5f82610364565b60018101905061037d565b5050565b601f8211156103df576103b081610281565b6103b984610293565b810160208510156103c8578190505b6103dc6103d485610293565b83018261037c565b50505b505050565b5f82821c905092915050565b5f6103ff5f19846008026103e4565b1980831691505092915050565b5f61041783836103f0565b9150826002028217905092915050565b6104308261021a565b67ffffffffffffffff81111561044957610448610080565b5b6104538254610251565b61045e82828561039e565b5f60209050601f83116001811461048f575f841561047d578287015190505b610487858261040c565b8655506104ee565b601f19841661049d86610281565b5f5b828110156104c45784890151825560018201915060208501945060208101905061049f565b868310156104e157848901516104dd601f8916826103f0565b8355505b6001600288020188555050505b505050505050565b610a24806105035f395ff3fe608060405234801561000f575f80fd5b5060043610610060575f3560e01c806306fdde031461006457806323b872dd1461008257806340c10f19146100b257806370a08231146100ce57806395d89b41146100fe578063a9059cbb1461011c575b5f80fd5b61006c61014c565b60405161007991906106d1565b60405180910390f35b61009c60048036038101906100979190610782565b6101d7565b6040516100a991906107ec565b60405180910390f35b6100cc60048036038101906100c79190610805565b61036e565b005b6100e860048036038101906100e39190610843565b61042a565b6040516100f5919061087d565b60405180910390f35b61010661043f565b60405161011391906106d1565b60405180910390f35b61013660048036038101906101319190610805565b6104cb565b60405161014391906107ec565b60405180910390f35b5f8054610158906108c3565b80601f0160208091040260200160405190810160405280929190818152602001828054610184906108c3565b80156101cf5780601f106101a6576101008083540402835291602001916101cf565b820191905f5260205f20905b8154815290600101906020018083116101b257829003601f168201915b505050505081565b5f8160025f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f20541015610258576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161024f9061093d565b60405180910390fd5b8160025f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546102a49190610988565b925050819055508160025f8573ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546102f791906109bb565b925050819055508273ffffffffffffffffffffffffffffffffffffffff168473ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8460405161035b919061087d565b60405180910390a3600190509392505050565b8060025f8473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546103ba91906109bb565b925050819055508173ffffffffffffffffffffffffffffffffffffffff165f73ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8360405161041e919061087d565b60405180910390a35050565b6002602052805f5260405f205f915090505481565b6001805461044c906108c3565b80601f0160208091040260200160405190810160405280929190818152602001828054610478906108c3565b80156104c35780601f1061049a576101008083540402835291602001916104c3565b820191905f5260205f20905b8154815290600101906020018083116104a657829003601f168201915b505050505081565b5f8160025f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f2054101561054c576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016105439061093d565b60405180910390fd5b8160025f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546105989190610988565b925050819055508160025f8573ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546105eb91906109bb565b925050819055508273ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8460405161064f919061087d565b60405180910390a36001905092915050565b5f81519050919050565b5f82825260208201905092915050565b8281835e5f83830152505050565b5f601f19601f8301169050919050565b5f6106a382610661565b6106ad818561066b565b93506106bd81856020860161067b565b6106c681610689565b840191505092915050565b5f6020820190508181035f8301526106e98184610699565b905092915050565b5f80fd5b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f61071e826106f5565b9050919050565b61072e81610714565b8114610738575f80fd5b50565b5f8135905061074981610725565b92915050565b5f819050919050565b6107618161074f565b811461076b575f80fd5b50565b5f8135905061077c81610758565b92915050565b5f805f60608486031215610799576107986106f1565b5b5f6107a68682870161073b565b93505060206107b78682870161073b565b92505060406107c88682870161076e565b9150509250925092565b5f8115159050919050565b6107e6816107d2565b82525050565b5f6020820190506107ff5f8301846107dd565b92915050565b5f806040838503121561081b5761081a6106f1565b5b5f6108288582860161073b565b92505060206108398582860161076e565b9150509250929050565b5f60208284031215610858576108576106f1565b5b5f6108658482850161073b565b91505092915050565b6108778161074f565b82525050565b5f6020820190506108905f83018461086e565b92915050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52602260045260245ffd5b5f60028204905060018216806108da57607f821691505b6020821081036108ed576108ec610896565b5b50919050565b7f496e73756666696369656e742062616c616e63650000000000000000000000005f82015250565b5f61092760148361066b565b9150610932826108f3565b602082019050919050565b5f6020820190508181035f8301526109548161091b565b9050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f6109928261074f565b915061099d8361074f565b92508282039050818111156109b5576109b461095b565b5b92915050565b5f6109c58261074f565b91506109d08361074f565b92508282019050808211156109e8576109e761095b565b5b9291505056fea2646970667358221220dba3284dfdecc45a6c2071a74d0fa3c56b27d30dd24ad870e8112f3194eb449064736f6c634300081a0033000000000000000000000000000000000000000000000000000000000000004000000000000000000000000000000000000000000000000000000000000000800000000000000000000000000000000000000000000000000000000000000004555344540000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000045553445400000000000000000000000000000000000000000000000000000000";

    /// ERC20 Runtime code
    const RUNTIME_CODE : &str = "0x608060405234801561000f575f80fd5b5060043610610060575f3560e01c806306fdde031461006457806323b872dd1461008257806340c10f19146100b257806370a08231146100ce57806395d89b41146100fe578063a9059cbb1461011c575b5f80fd5b61006c61014c565b60405161007991906106d1565b60405180910390f35b61009c60048036038101906100979190610782565b6101d7565b6040516100a991906107ec565b60405180910390f35b6100cc60048036038101906100c79190610805565b61036e565b005b6100e860048036038101906100e39190610843565b61042a565b6040516100f5919061087d565b60405180910390f35b61010661043f565b60405161011391906106d1565b60405180910390f35b61013660048036038101906101319190610805565b6104cb565b60405161014391906107ec565b60405180910390f35b5f8054610158906108c3565b80601f0160208091040260200160405190810160405280929190818152602001828054610184906108c3565b80156101cf5780601f106101a6576101008083540402835291602001916101cf565b820191905f5260205f20905b8154815290600101906020018083116101b257829003601f168201915b505050505081565b5f8160025f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f20541015610258576040517f08c379a000000000000000000000000000000000000000000000000000000000815260040161024f9061093d565b60405180910390fd5b8160025f8673ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546102a49190610988565b925050819055508160025f8573ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546102f791906109bb565b925050819055508273ffffffffffffffffffffffffffffffffffffffff168473ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8460405161035b919061087d565b60405180910390a3600190509392505050565b8060025f8473ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546103ba91906109bb565b925050819055508173ffffffffffffffffffffffffffffffffffffffff165f73ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8360405161041e919061087d565b60405180910390a35050565b6002602052805f5260405f205f915090505481565b6001805461044c906108c3565b80601f0160208091040260200160405190810160405280929190818152602001828054610478906108c3565b80156104c35780601f1061049a576101008083540402835291602001916104c3565b820191905f5260205f20905b8154815290600101906020018083116104a657829003601f168201915b505050505081565b5f8160025f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f2054101561054c576040517f08c379a00000000000000000000000000000000000000000000000000000000081526004016105439061093d565b60405180910390fd5b8160025f3373ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546105989190610988565b925050819055508160025f8573ffffffffffffffffffffffffffffffffffffffff1673ffffffffffffffffffffffffffffffffffffffff1681526020019081526020015f205f8282546105eb91906109bb565b925050819055508273ffffffffffffffffffffffffffffffffffffffff163373ffffffffffffffffffffffffffffffffffffffff167fddf252ad1be2c89b69c2b068fc378daa952ba7f163c4a11628f55a4df523b3ef8460405161064f919061087d565b60405180910390a36001905092915050565b5f81519050919050565b5f82825260208201905092915050565b8281835e5f83830152505050565b5f601f19601f8301169050919050565b5f6106a382610661565b6106ad818561066b565b93506106bd81856020860161067b565b6106c681610689565b840191505092915050565b5f6020820190508181035f8301526106e98184610699565b905092915050565b5f80fd5b5f73ffffffffffffffffffffffffffffffffffffffff82169050919050565b5f61071e826106f5565b9050919050565b61072e81610714565b8114610738575f80fd5b50565b5f8135905061074981610725565b92915050565b5f819050919050565b6107618161074f565b811461076b575f80fd5b50565b5f8135905061077c81610758565b92915050565b5f805f60608486031215610799576107986106f1565b5b5f6107a68682870161073b565b93505060206107b78682870161073b565b92505060406107c88682870161076e565b9150509250925092565b5f8115159050919050565b6107e6816107d2565b82525050565b5f6020820190506107ff5f8301846107dd565b92915050565b5f806040838503121561081b5761081a6106f1565b5b5f6108288582860161073b565b92505060206108398582860161076e565b9150509250929050565b5f60208284031215610858576108576106f1565b5b5f6108658482850161073b565b91505092915050565b6108778161074f565b82525050565b5f6020820190506108905f83018461086e565b92915050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52602260045260245ffd5b5f60028204905060018216806108da57607f821691505b6020821081036108ed576108ec610896565b5b50919050565b7f496e73756666696369656e742062616c616e63650000000000000000000000005f82015250565b5f61092760148361066b565b9150610932826108f3565b602082019050919050565b5f6020820190508181035f8301526109548161091b565b9050919050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f6109928261074f565b915061099d8361074f565b92508282039050818111156109b5576109b461095b565b5b92915050565b5f6109c58261074f565b91506109d08361074f565b92508282019050808211156109e8576109e761095b565b5b9291505056fea2646970667358221220dba3284dfdecc45a6c2071a74d0fa3c56b27d30dd24ad870e8112f3194eb449064736f6c634300081a0033";

    /// Mint Input
    const MINT_INPUT : &str = "0x40c10f1900000000000000000000000001010101010101010101010101010101010101010000000000000000000000000000000000000000000000000000000000010000";
    
    /// Transfer Input
    const TRANSFER_INPUT : &str = "0x23b872dd000000000000000000000000010101010101010101010101010101010101010100000000000000000000000002020202020202020202020202020202020202020000000000000000000000000000000000000000000000000000000000010000";
    
    /// [01:20] Slot
    const SLOT1 : &str = "0xd2869508550c71a0ebfe05ddd28ce832b357803f6f387154b1a5451da28aca19";

    /// [02:20] Slot
    const SLOT2 : &str = "0xac0ab67043ecc9a2f17c6f6ba97786b2b1051a49d0101c2e2da0641d9a0e6da7";

    // Simple test runtime code
    const TEST_RUNTIME_CODE : &str = "0x608060405234801561000f575f80fd5b5060043610610055575f3560e01c80631a43c3381461005957806325aa322c14610078578063853255cc14610096578063bf9ce952146100b4578063f0ba8440146100d2575b5f80fd5b610061610102565b60405161006f9291906101c9565b60405180910390f35b610080610185565b60405161008d91906101f0565b60405180910390f35b61009e61018d565b6040516100ab91906101f0565b60405180910390f35b6100bc610193565b6040516100c991906101f0565b60405180910390f35b6100ec60048036038101906100e79190610237565b610199565b6040516100f991906101f0565b60405180910390f35b5f805f80600190505f5b61271081101561016657600181610123919061028f565b8361012e919061028f565b9250633b9aca03600182610142919061028f565b8361014d91906102c2565b6101579190610330565b9150808060010191505061010c565b5081606481905550806065819055506064546065549350935050509091565b633b9aca0381565b60645481565b60655481565b5f81606481106101a7575f80fd5b015f915090505481565b5f819050919050565b6101c3816101b1565b82525050565b5f6040820190506101dc5f8301856101ba565b6101e960208301846101ba565b9392505050565b5f6020820190506102035f8301846101ba565b92915050565b5f80fd5b610216816101b1565b8114610220575f80fd5b50565b5f813590506102318161020d565b92915050565b5f6020828403121561024c5761024b610209565b5b5f61025984828501610223565b91505092915050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601160045260245ffd5b5f610299826101b1565b91506102a4836101b1565b92508282019050808211156102bc576102bb610262565b5b92915050565b5f6102cc826101b1565b91506102d7836101b1565b92508282026102e5816101b1565b915082820484148315176102fc576102fb610262565b5b5092915050565b7f4e487b71000000000000000000000000000000000000000000000000000000005f52601260045260245ffd5b5f61033a826101b1565b9150610345836101b1565b92508261035557610354610303565b5b82820690509291505056fea2646970667358221220788e371fb21283a8f9e8fe6b3431df49a04de0efe5941b8d92c1e1e4e58f5ab364736f6c634300081a0033";

    // Simpe test input
    const TEST_INPUT : &str = "0x1a43c338";

    #[test]
    fn test_compute_parallel() {
        // Initialize prometheus metrics exporter
        let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        let _handle = builder
            .with_http_listener(([127, 0, 0, 1], 12345))
            .install()
            .expect("failed to install Prometheus recorder");
        let runtime_hex = hex::decode(TEST_RUNTIME_CODE).unwrap();
        let runtime_code = Bytes::from(runtime_hex);
        let input_hex = hex::decode(TEST_INPUT).unwrap();
        let input = Bytes::from(input_hex);

        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: false
        };
        let non_ssa_result = execute_case(runtime_code.clone(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        // Parallel full graph execution
        let parallel_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::ParallelGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: Some(8),
            enable_tracer: false,
            is_deployed_contract: false
        };
        let parallel_full_result = execute_case(runtime_code.clone(), "parallel_full", parallel_full_config);
        println!("Parallel Full Graph Time Cost: {:?}", parallel_full_result.execution_time);
        // println!("\nMetrics are available at http://127.0.0.1:12345/metrics");
        // println!("You can use curl http://127.0.0.1:12345/metrics to view them");
        // println!("The metrics will be in standard Prometheus format");
        std::thread::sleep(std::time::Duration::from_secs(15));
    }

    #[test]
    fn test_compute() {
        // Initialize prometheus metrics exporter
        let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        let _handle = builder
            .with_http_listener(([127, 0, 0, 1], 12345))
            .install()
            .expect("failed to install Prometheus recorder");
        let runtime_hex = hex::decode(TEST_RUNTIME_CODE).unwrap();
        let runtime_code = Bytes::from(runtime_hex);
        let input_hex = hex::decode(TEST_INPUT).unwrap();
        let input = Bytes::from(input_hex);

        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: false
        };
        let non_ssa_result = execute_case(runtime_code.clone(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        // Serial full graph execution
        let serial_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::SerialGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: false
        };
        let serial_full_result = execute_case(runtime_code.clone(), "serial_full", serial_full_config);
        println!("Serial Full Graph Time Cost: {:?}", serial_full_result.execution_time);
        println!("\nMetrics are available at http://127.0.0.1:12345/metrics");
        println!("You can use curl http://127.0.0.1:12345/metrics to view them");
        println!("The metrics will be in standard Prometheus format");
        // std::thread::sleep(std::time::Duration::from_secs(15));
    }

    #[test]
    fn test_create_contract() {
        let deploy_hex = hex::decode(DEPLOY_CODE).unwrap();
        let input = Bytes::from(deploy_hex);

        // Non-SSA execution
        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: true
        };
        let non_ssa_result = execute_case(Bytes::default(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        // Create Partial From LSN: 0, 456, 627, actually these storage slot won't produce conflicts.
        // Serial partial graph execution
        let serial_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::SerialGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: true,
            is_deployed_contract: true
        };
        let serial_full_result = execute_case(Bytes::default(), "serial_full", serial_full_config);
        println!("Serial Full Graph Time Cost: {:?}", serial_full_result.execution_time);
    }

    #[test]
    fn test_mint() {
        let runtime_hex = hex::decode(RUNTIME_CODE).unwrap();
        let runtime_code = Bytes::from(runtime_hex);
        let input_hex = hex::decode(MINT_INPUT).unwrap();
        let input = Bytes::from(input_hex);

        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: false
        };
        let non_ssa_result = execute_case(runtime_code.clone(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        // Serial full graph execution
        let serial_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::SerialGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: true,
            is_deployed_contract: false
        };
        let serial_full_result = execute_case(runtime_code.clone(), "serial_full", serial_full_config);
        println!("Serial Full Graph Time Cost: {:?}", serial_full_result.execution_time);
    }


    #[test]
    fn test_transfer() {
        // Initialize prometheus metrics exporter
        // let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        // let _handle = builder
        //     .with_http_listener(([127, 0, 0, 1], 9090))
        //     .install()
        //     .expect("failed to install Prometheus recorder");

        let runtime_hex = hex::decode(RUNTIME_CODE).unwrap();
        let runtime_code = Bytes::from(runtime_hex);
        let input_hex = hex::decode(TRANSFER_INPUT).unwrap();
        let input = Bytes::from(input_hex);
        let slot1_hex = hex::decode(SLOT1).unwrap();
        let slot2_hex = hex::decode(SLOT2).unwrap();
        let slot1_bytes: [u8;32] = slot1_hex.try_into().unwrap();
        let slot2_bytes: [u8;32] = slot2_hex.try_into().unwrap();
        let slot1 = U256::from_be_bytes(slot1_bytes);
        let slot2 = U256::from_be_bytes(slot2_bytes);
        let value = U256::from(65536);

        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![
                (slot1, value),
                (slot2, U256::ZERO)
            ],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: false
        };
        let non_ssa_result = execute_case(runtime_code.clone(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        let serial_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::SerialGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![
                (slot1, value),
                (slot2, U256::ZERO)
            ],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: true,
            is_deployed_contract: false
        };
        let serial_full_result = execute_case(runtime_code.clone(), "serial_full", serial_full_config);
        println!("Serial Full Graph Time Cost: {:?}", serial_full_result.execution_time);
        // println!("\nMetrics are available at http://127.0.0.1:9090/metrics");
        // println!("You can use curl http://127.0.0.1:9090/metrics to view them");
        // println!("The metrics will be in standard Prometheus format");
        // std::thread::sleep(std::time::Duration::from_secs(15));
    }

    #[test]
    fn test_create_contract_parallel() {
        let deploy_hex = hex::decode(DEPLOY_CODE).unwrap();
        let input = Bytes::from(deploy_hex);

        // Non-SSA execution
        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: true
        };
        let non_ssa_result = execute_case(Bytes::default(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        // Create Partial From LSN: 0, 456, 627, actually these storage slot won't produce conflicts.
        // Serial partial graph execution
        let parallel_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::ParallelGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: Some(16),
            enable_tracer: false,
            is_deployed_contract: true
        };
        let parallel_full_result = execute_case(Bytes::default(), "parallel_full", parallel_full_config);
        println!("Parallel Full Graph Time Cost: {:?}", parallel_full_result.execution_time);
    }


    #[test]
    fn test_mint_parallel() {
        let runtime_hex = hex::decode(RUNTIME_CODE).unwrap();
        let runtime_code = Bytes::from(runtime_hex);
        let input_hex = hex::decode(MINT_INPUT).unwrap();
        let input = Bytes::from(input_hex);

        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: false
        };
        let non_ssa_result = execute_case(runtime_code.clone(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        // Serial full graph execution
        let parallel_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::ParallelGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![],
            input: Some(input.clone()),
            thread_number: Some(16),
            enable_tracer: false,
            is_deployed_contract: false
        };
        let parallel_full_result = execute_case(runtime_code.clone(), "parallel_full", parallel_full_config);
        println!("Parallel Full Graph Time Cost: {:?}", parallel_full_result.execution_time);
    }


    #[test]
    fn test_transfer_parallel() {
        // Initialize prometheus metrics exporter
        // let builder = metrics_exporter_prometheus::PrometheusBuilder::new();
        // let _handle = builder
        //     .with_http_listener(([127, 0, 0, 1], 9090))
        //     .install()
        //     .expect("failed to install Prometheus recorder");

        let runtime_hex = hex::decode(RUNTIME_CODE).unwrap();
        let runtime_code = Bytes::from(runtime_hex);
        let input_hex = hex::decode(TRANSFER_INPUT).unwrap();
        let input = Bytes::from(input_hex);
        let slot1_hex = hex::decode(SLOT1).unwrap();
        let slot2_hex = hex::decode(SLOT2).unwrap();
        let slot1_bytes: [u8;32] = slot1_hex.try_into().unwrap();
        let slot2_bytes: [u8;32] = slot2_hex.try_into().unwrap();
        let slot1 = U256::from_be_bytes(slot1_bytes);
        let slot2 = U256::from_be_bytes(slot2_bytes);
        let value = U256::from(65536);

        let non_ssa_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::BaselineNoSSA,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![
                (slot1, value),
                (slot2, U256::ZERO)
            ],
            input: Some(input.clone()),
            thread_number: None,
            enable_tracer: false,
            is_deployed_contract: false
        };
        let non_ssa_result = execute_case(runtime_code.clone(), "non_ssa", non_ssa_config);
        println!("Non-SSA Time Cost: {:?}", non_ssa_result.execution_time);
        let parallel_full_config = ExecutionConfig {
            mode: ExecutionMode::Full,
            test_mode: TestMode::ParallelGraph,
            collect_metrics: true,
            pre_deployed_contract: vec![],
            pre_determined_slots: vec![
                (slot1, value),
                (slot2, U256::ZERO)
            ],
            input: Some(input.clone()),
            thread_number: Some(16),
            enable_tracer: false,
            is_deployed_contract: false
        };
        let parallel_full_result = execute_case(runtime_code.clone(), "parallel_full", parallel_full_config);
        println!("Parallel Full Graph Time Cost: {:?}", parallel_full_result.execution_time);
        // println!("\nMetrics are available at http://127.0.0.1:9090/metrics");
        // println!("You can use curl http://127.0.0.1:9090/metrics to view them");
        // println!("The metrics will be in standard Prometheus format");
        // std::thread::sleep(std::time::Duration::from_secs(15));
    }
  
}