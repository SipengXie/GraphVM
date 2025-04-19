use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion,
};
use revm::db::{CacheDB, EmptyDB};
use revm::Evm;
use revm_interpreter::SharedMemory;
use revm_primitives::{
    b256, hex, uint, AccountInfo, AccountStatus, Address, Bytes, Env, HashMap, LatestSpec, SpecId, TxKind, U256
};
use revm_ssa::logger::LsnType;
use revm_ssa::FrameInput;
use revm_ssa_graph::instruction_table::InstructionTable;
use revm_ssa_graph::{ExecutionContext, SsaGraph};
use typed_graph::context::ExternalContext;
use typed_graph::ssa_converter::{ConstantPool, SsaConverter};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

// 用于基准测试的代码和输入数据
struct BenchCase {
    name: &'static str,
    bytecode: Vec<u8>,
    calldata: Vec<u8>,
    pre_determined_slots: Vec<(U256, U256)>,
}

impl BenchCase {
    // 创建一个新的基准测试用例
    fn new(name: &'static str, bytecode: &str, calldata: &str) -> Self {
        let bytecode = hex::decode(&bytecode).unwrap_or_default();
        let calldata = if calldata.is_empty() {
            vec![]
        } else {
            hex::decode(&calldata[2..]).unwrap_or_default()
        };
        Self {
            name,
            bytecode,
            calldata,
            pre_determined_slots: vec![],
        }
    }

    fn new_with_pre_determined_slots(
        name: &'static str,
        bytecode: &str,
        calldata: &str,
        pre_determined_slots: Vec<(U256, U256)>,
    ) -> Self {
        let bytecode = hex::decode(&bytecode).unwrap_or_default();
        let calldata = if calldata.is_empty() {
            vec![]
        } else {
            hex::decode(&calldata[2..]).unwrap_or_default()
        };
        Self {
            name,
            bytecode,
            calldata,
            pre_determined_slots,
        }
    }
}
// 获取基准测试用例
fn get_bench_cases() -> Vec<BenchCase> {
    vec![
        // ERC20测试
        BenchCase::new_with_pre_determined_slots(
            "erc20_runtime", 
            include_str!("../../../data/erc20_runtime.rt.hex"),
            "0x40c10f1900000000000000000000000001010101010101010101010101010101010101010000000000000000000000000000000000000000000000000000000000010000",
            vec![
                (b256!("d2869508550c71a0ebfe05ddd28ce832b357803f6f387154b1a5451da28aca19").into(), U256::from(10000000000 as u64)),
                (b256!("ac0ab67043ecc9a2f17c6f6ba97786b2b1051a49d0101c2e2da0641d9a0e6da7").into(), U256::from(9900000000 as u64)),
            ]
        ),
        // 斐波那契自定义输入测试
        BenchCase::new(
            "fibonacci_calldata", 
            include_str!("../../../data/fibonacci_calldata.rt.hex"),
            "0xc6c2ea1700000000000000000000000000000000000000000000000000000000000003e8"
        ),
        // 斐波那契常量输入测试
        BenchCase::new(
            "fibonacci_constant",
            include_str!("../../../data/fibonacci_constant.rt.hex"),
            "0x9246aa9a"
        ),
        // 阶乘测试自定义输入测试
        BenchCase::new(
            "factorial_calldata", 
            include_str!("../../../data/factorial_calldata.rt.hex"),
            "0x8371483400000000000000000000000000000000000000000000000000000000000003e8"
        ),
        // 阶乘常量输入测试
        BenchCase::new(
            "factorial_constant",
            include_str!("../../../data/factorial_constant.rt.hex"),
            "0x981111ef"
        ),
        // Snailtracer测试
        BenchCase::new(
            "snailtracer",
            include_str!("../../../data/snailtracer.rt.hex"),
            "0x30627b7c"
        ),
        // Hash 10k测试
        BenchCase::new(
            "hash_10k",
            include_str!("../../../data/hash_10k.rt.hex"),
            "0xdc6bf8a7000000000000000000000000000000000000000000000000000000000000002000000000000000000000000000000000000000000000000000000000000000021234000000000000000000000000000000000000000000000000000000000000"
        ),
        // Uniswap V2测试
        BenchCase::new(
            "uniswap_v2",
            include_str!("../../../data/uniswap_v2.rt.hex"),
            "0xdfa5235e"
        ),
        // WETH测试
        BenchCase::new(
            "weth",
            include_str!("../../../data/weth.rt.hex"),
            "0x6b7c477a"
        ),
    ]
}

fn mk_group<'a>(c: &'a mut Criterion, name: &str) -> BenchmarkGroup<'a, WallTime> {
    let mut g = c.benchmark_group(name);
    g.sample_size(20);
    g.warm_up_time(Duration::from_secs(2));
    g.measurement_time(Duration::from_secs(5));
    g
}

fn bench_ssa_vs_nonssa(c: &mut Criterion) {
    let cases = get_bench_cases();

    for case in cases {
        // 准备测试环境和数据
        let bytecode = Bytes::from(case.bytecode.clone());
        let input = Bytes::from(case.calldata.clone());
        let pre_determined_slots = case.pre_determined_slots.clone();
        let gas_limit = 1_000_000_000;
        let caller = Address::from([0x1; 20]);
        let contract_addr = Address::from([0x2; 20]);

        // 设置EVM环境
        let mut env = Env::default();
        env.tx.caller = caller;
        env.tx.transact_to = TxKind::Call(contract_addr);
        env.tx.data = input.clone().into();
        env.tx.gas_limit = gas_limit;

        // 准备合约和主机环境
        let bytecode =
            revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));

        // 准备缓存数据库
        let mut cache = CacheDB::new(EmptyDB::default());
        cache.insert_account_info(
            contract_addr,
            AccountInfo {
                code_hash: bytecode.hash_slow(),
                code: Some(bytecode.clone()),
                ..Default::default()
            },
        );

        // 添加存储数据
        for (slot, value) in pre_determined_slots.clone() {
            let _ = cache.insert_account_storage(contract_addr, slot, value);
        }

        // 创建基准测试组
        let mut group = mk_group(c, &format!("revm_ssa/{}", case.name));

        // 1. revm-interpreter基准测试（非SSA模式）
        group.bench_function("original", |b| {
            b.iter(|| {
                let mut evm = Evm::builder()
                    .with_spec_id(SpecId::LATEST)
                    .with_ref_db(cache.clone())
                    .with_env(Box::new(env.clone()))
                    .build();
                evm.transact_preverified()
            })
        });

        // 2. ssa-logger基准测试（SSA模式）
        group.bench_function("ssa-logger", |b| {
            b.iter(|| {
                let mut evm = Evm::builder()
                    .with_spec_id(SpecId::LATEST)
                    .with_ref_db(cache.clone())
                    .with_env(Box::new(env.clone()))
                    .with_ssa_logger()
                    .build_with_ssa_logger();
                evm.transact_preverified()
            })
        });

        // 3. 为GraphVm基准测试准备依赖图

        // 执行一次以收集日志
        let mut evm = Evm::builder()
            .with_spec_id(SpecId::LATEST)
            .with_ref_db(cache.clone())
            .with_env(Box::new(env.clone()))
            .with_ssa_logger()
            .build_with_ssa_logger();
        let _ = evm.transact_preverified();

        // 获取日志和调用信息
        let mut logger = evm.take_ssa_logger().unwrap();
        let logs = logger.take_logs();
        let logs_for_typed_graph = logs.clone();
        let first_frame_input = logger.take_first_frame_input();
        let first_frame_for_typed_graph = first_frame_input.clone().unwrap();
        let lsns: Vec<LsnType> = logs.iter().map(|log| log.lsn).collect();

        // 创建依赖图
        let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());

        // 构建图结构
        for log in logs {
            graph.add_node(log).unwrap();
        }

        for lsn in lsns {
            graph.add_edges(lsn).unwrap();
        }

        // 创建执行上下文
        let env_clone = env.clone();
        let mut context = ExecutionContext::<'_, CacheDB<EmptyDB>>::new::<LatestSpec>(
            &env_clone,
            cache,
            first_frame_input,
        );
        let table = InstructionTable::create_instruction_table::<LatestSpec>();

        // 获取拓扑排序的节点
        let nodes_to_execute = graph.topological_sort().unwrap();

        // 不安全地获取图的可变引用（用于基准测试）
        let arc_graph = Arc::new(graph);
        let mut_graph = unsafe { &mut *(Arc::as_ptr(&arc_graph) as *mut SsaGraph) };

        // 4. GraphVm基准测试
        group.bench_function("GraphVm", |b| {
            b.iter(|| {
                for node_index in nodes_to_execute.clone() {
                    let node = mut_graph.get_node_mut(node_index).unwrap();
                    table.instructions[node.opcode as usize](&mut context, node, &arc_graph)
                        .unwrap();
                }
            });
        });

        // 创建共享内存实例，用于存储和管理VM执行过程中的内存数据
        let shared_memory = Rc::new(RefCell::new(SharedMemory::new()));
        
        // 初始化账户映射，用于存储合约账户信息
        let mut accounts = HashMap::default();
        accounts.insert(
            contract_addr,
            (
                AccountInfo {
                    nonce: 0,
                    balance: uint!(10000000000000000000000000_U256), // 设置合约账户余额
                    code_hash: bytecode.hash_slow(),                 // 计算合约字节码的哈希值
                    code: Some(bytecode.clone()),                    // 存储合约字节码
                },
                AccountStatus::default(),                           // 设置账户状态为默认值
            ),
        );

        // 初始化存储映射，用于存储预设的存储槽位值
        let mut storage = HashMap::default();
        for (slot, value) in pre_determined_slots {
            storage.insert((contract_addr, slot), value);           // 将预设的存储槽位值插入存储映射
        }

        // 创建外部上下文，包含环境信息、账户信息、存储信息和区块哈希
        let external_context = ExternalContext::new(
            env.clone(),
            accounts,
            storage,
            HashMap::default(), // 区块哈希映射设为空
        );
        // 将外部上下文包装为可共享和可修改的引用
        let external_context = Rc::new(RefCell::new(external_context));
        let mut constant_pool = ConstantPool::new();
        // 创建SSA转换器实例，用于将执行日志转换为类型化图
        let mut converter = SsaConverter::new(
            external_context,
            shared_memory,
            &env as *const Env,                           // 环境指针
            &first_frame_for_typed_graph as *const FrameInput,  // 第一帧输入指针
            &mut constant_pool,
        );

        // 将执行日志转换为类型化图和常量池
        let mut typed_graph = converter.convert(logs_for_typed_graph);

        // 添加TypedGraphVm的基准测试
        group.bench_function("TypedGraphVm", |b| {
            b.iter(|| {
                typed_graph.execute()                     // 执行类型化图
            });
        });
    }
}

// 新增：只对 SsaGraph (GraphVm) 进行基准测试的函数
fn bench_ssa_graph(c: &mut Criterion) {
    let cases = get_bench_cases();

    for case in cases {
        // --- 通用设置代码 (从 bench_ssa_vs_nonssa 复制) ---
        let bytecode = Bytes::from(case.bytecode.clone());
        let input = Bytes::from(case.calldata.clone());
        let pre_determined_slots = case.pre_determined_slots.clone(); // Clone here for SsaGraph setup
        let gas_limit = 1_000_000_000;
        let caller = Address::from([0x1; 20]);
        let contract_addr = Address::from([0x2; 20]);

        let mut env = Env::default();
        env.tx.caller = caller;
        env.tx.transact_to = TxKind::Call(contract_addr);
        env.tx.data = input.clone().into();
        env.tx.gas_limit = gas_limit;

        let bytecode_analysed = // Renamed to avoid conflict if original bytecode needed later
            revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));

        let mut cache = CacheDB::new(EmptyDB::default());
        cache.insert_account_info(
            contract_addr,
            AccountInfo {
                code_hash: bytecode_analysed.hash_slow(),
                code: Some(bytecode_analysed.clone()),
                ..Default::default()
            },
        );

        // Add storage data specific to SsaGraph setup
        for (slot, value) in pre_determined_slots.clone() {
             let _ = cache.insert_account_storage(contract_addr, slot, value);
        }


        // --- 获取日志和输入 (运行一次 EVM Logger) ---
        let mut evm_logger = Evm::builder()
            .with_spec_id(SpecId::LATEST)
            .with_ref_db(cache.clone()) // Clone cache for the logger run
            .with_env(Box::new(env.clone())) // Clone env for the logger run
            .with_ssa_logger()
            .build_with_ssa_logger();
        let _ = evm_logger.transact_preverified();
        let mut logger = evm_logger.take_ssa_logger().unwrap();
        let logs = logger.take_logs(); // Logs for SsaGraph
        let first_frame_input = logger.take_first_frame_input(); // Input for SsaGraph context
        let lsns: Vec<LsnType> = logs.iter().map(|log| log.lsn).collect();


        // --- SsaGraph (GraphVm) 特定设置 ---
        let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());
        for log in logs { // Use the logs obtained above
            graph.add_node(log).unwrap();
        }
        for lsn in lsns {
            graph.add_edges(lsn).unwrap();
        }

        // 创建执行上下文 (使用上面为 logger run 克隆的 cache 和 env)
        // Note: Cache state might be modified by logger run if not cloned correctly before.
        // Re-create cache if a clean state is needed for ExecutionContext.
        let context_cache = cache.clone(); // Clone again for context if needed
        let context_env = env.clone(); // Clone env for context
        let mut context = ExecutionContext::<'_, CacheDB<EmptyDB>>::new::<LatestSpec>(
            &context_env, // Use context-specific env
            context_cache, // Use context-specific cache
            first_frame_input, // Use the input from logger run
        );
        let table = InstructionTable::create_instruction_table::<LatestSpec>();
        let nodes_to_execute = graph.topological_sort().unwrap();
        let arc_graph = Arc::new(graph);
        // UNSAFE: Required for mutable access in bench loop, same as original.
        let mut_graph = unsafe { &mut *(Arc::as_ptr(&arc_graph) as *mut SsaGraph) };

        // --- SsaGraph (GraphVm) 基准测试 ---
        let mut group = mk_group(c, &format!("ssa_graph_only/{}", case.name));
        group.bench_function("GraphVm", |b| {
            b.iter(|| {
                // IMPORTANT: Reset context state if necessary between iterations?
                // Currently reuses the same context object. If context holds state
                // that should be reset (like gas counter, stack, etc.), this needs adjustment.
                // For now, matches original bench logic.
                 for node_index in nodes_to_execute.clone() {
                    // Need to re-borrow context mutably if it was used immutably elsewhere
                     let node = mut_graph.get_node_mut(node_index).unwrap();
                     table.instructions[node.opcode as usize](&mut context, node, &arc_graph)
                         .unwrap();
                 }
            });
        });
    }
}


// 新增：只对 TypedGraph (TypedGraphVm) 进行基准测试的函数
fn bench_typed_graph(c: &mut Criterion) {
    let cases = get_bench_cases();

    for case in cases {
        // --- 通用设置代码 (从 bench_ssa_vs_nonssa 复制) ---
        let bytecode = Bytes::from(case.bytecode.clone());
        let input = Bytes::from(case.calldata.clone());
        let pre_determined_slots = case.pre_determined_slots.clone();
        let gas_limit = 1_000_000_000;
        let caller = Address::from([0x1; 20]);
        let contract_addr = Address::from([0x2; 20]);

        let mut env = Env::default();
        env.tx.caller = caller;
        env.tx.transact_to = TxKind::Call(contract_addr);
        env.tx.data = input.clone().into();
        env.tx.gas_limit = gas_limit;

        let bytecode_analysed = // Use the same name consistently
            revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));

        let mut cache = CacheDB::new(EmptyDB::default());
        cache.insert_account_info(
            contract_addr,
            AccountInfo {
                code_hash: bytecode_analysed.hash_slow(),
                code: Some(bytecode_analysed.clone()),
                ..Default::default()
            },
        );

        // Add storage data specific to TypedGraph setup
        // This part is actually needed for the ExternalContext later, not logger run.
        // let mut initial_storage = HashMap::default(); // Needed later
        // for (slot, value) in pre_determined_slots.clone() {
        //      initial_storage.insert((contract_addr, slot), value);
        // }


        // --- 获取日志和输入 (运行一次 EVM Logger) ---
        // Need a cache instance for the logger run. Clone the one setup above.
        let logger_cache = cache.clone();
        let mut evm_logger = Evm::builder()
            .with_spec_id(SpecId::LATEST)
            .with_ref_db(logger_cache) // Use cloned cache for logger
            .with_env(Box::new(env.clone())) // Clone env for logger run
            .with_ssa_logger()
            .build_with_ssa_logger();
        let _ = evm_logger.transact_preverified();
        let mut logger = evm_logger.take_ssa_logger().unwrap();
        let logs_for_typed_graph = logger.take_logs(); // Logs for TypedGraph
        let first_frame_for_typed_graph = logger.take_first_frame_input().unwrap(); // Input for TypedGraph


        // --- TypedGraph (TypedGraphVm) 特定设置 ---
        let shared_memory = Rc::new(RefCell::new(SharedMemory::new()));
        let mut accounts = HashMap::default();
        accounts.insert(
            contract_addr,
            (
                AccountInfo {
                    nonce: 0,
                    balance: uint!(10000000000000000000000000_U256),
                    code_hash: bytecode_analysed.hash_slow(), // Use analysed bytecode hash
                    code: Some(bytecode_analysed.clone()), // Use analysed bytecode
                },
                AccountStatus::default(),
            ),
        );

        // Create storage map using the pre_determined_slots from the case
        let mut storage = HashMap::default();
        for (slot, value) in pre_determined_slots { // Use the slots from case setup
             storage.insert((contract_addr, slot), value);
        }

        let external_context = ExternalContext::new(
            env.clone(), // Clone env for external context
            accounts,
            storage,
            HashMap::default(),
        );
        let external_context_rc = Rc::new(RefCell::new(external_context)); // Renamed variable
        let mut constant_pool = ConstantPool::new();
        let mut converter = SsaConverter::new(
            external_context_rc.clone(), // Clone Rc for converter
            shared_memory.clone(), // Clone Rc for converter
            &env as *const Env, // Pointer to original env
            &first_frame_for_typed_graph as *const FrameInput, // Pointer to frame input
            &mut constant_pool, // Mutable borrow of pool
        );

        // Convert logs to TypedGraph (This is part of the setup cost)
        let mut typed_graph = converter.convert(logs_for_typed_graph);


        // --- TypedGraph (TypedGraphVm) 基准测试 ---
        let mut group = mk_group(c, &format!("typed_graph_only/{}", case.name));
        group.bench_function("TypedGraphVm", |b| {
            b.iter(|| {
                // IMPORTANT: Does typed_graph.execute() modify state?
                // If yes, need to clone typed_graph or reset its state inside b.iter()
                // to accurately measure repeated execution like criterion does.
                // let mut graph_to_run = typed_graph.clone(); // Requires TypedGraph impl Clone
                // graph_to_run.execute()
                // For now, matches original bench logic:
                typed_graph.execute()
            });
        });
    }
}


// 更新 criterion_group 以包含新的函数
criterion_group!(benches, bench_ssa_vs_nonssa, bench_ssa_graph, bench_typed_graph);
criterion_main!(benches);
