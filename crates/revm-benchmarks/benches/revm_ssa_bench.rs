use criterion::{
    criterion_group, criterion_main, measurement::WallTime, BenchmarkGroup, Criterion,
};
use revm::Evm;
use revm_primitives::{
    b256, hex, AccountInfo, Address, Bytes, Env, LatestSpec, SpecId, TxKind, U256
};
use revm::db::{CacheDB, EmptyDB};
use revm_ssa::logger::LsnType;
use revm_ssa_graph::instruction_table::InstructionTable;
use revm_ssa_graph::{ExecutionContext, SsaGraph};
use std::sync::Arc;
use std::time::Duration;

// 用于基准测试的代码和输入数据
struct BenchCase {
    name: &'static str,
    bytecode: Vec<u8>,
    calldata: Vec<u8>,
    pre_determined_slots: Vec<(U256,U256)>,
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

    fn new_with_pre_determined_slots(name: &'static str, bytecode: &str, calldata: &str, pre_determined_slots: Vec<(U256,U256)>) -> Self {
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
        // WETH测试
        BenchCase::new(
            "weth",
            include_str!("../../../data/weth.rt.hex"),
            "0x6b7c477a"
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
        let bytecode = revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));
        
        // 准备缓存数据库
        let mut cache = CacheDB::new(EmptyDB::default());
        cache.insert_account_info(
            contract_addr,
            AccountInfo {
                code_hash: bytecode.hash_slow(),
                code: Some(bytecode),
                ..Default::default()
            },
        );
    
        // 添加存储数据
        for (slot, value) in pre_determined_slots {
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
        let first_frame_input = logger.take_first_frame_input();
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
            first_frame_input
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
                    table.instructions[node.opcode as usize](&mut context, node, &arc_graph).unwrap();
                }
            });
        });
    }
}

criterion_group!(
    benches, 
    bench_ssa_vs_nonssa
);
criterion_main!(benches); 