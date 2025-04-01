// 基准测试库文件
// 这个库主要包含revm基准测试

pub mod benches;

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use revm::Evm;
    use revm_primitives::{AccountInfo, Address, Bytes, Env, LatestSpec, SpecId, TxKind};
    use revm::db::{CacheDB, EmptyDB};
    use revm_ssa::SSALogger;
    use revm_ssa_graph::{ExecutionContext, SSAExecutor, SsaGraph};
    use std::{collections::HashMap, sync::Arc, time::Instant};

    // 从benches模块导入get_bench_cases
    use crate::benches::revm_ssa_bench::{get_bench_cases, BenchCase};

    #[test]
    fn test_execution() {
        let cases = get_bench_cases();
        let BenchCase {bytecode, calldata, name:_, pre_determined_slots:_ } = &cases[7]; // hash_10k Case
        // 获取WETH合约字节码
        let bytecode = Bytes::copy_from_slice(bytecode);
        
        // 设置基本参数
        let gas_limit = 1_000_000_000;
        let caller = Address::from([0x1; 20]);
        let contract_addr = Address::from([0x2; 20]);

        // 设置EVM环境
        let mut env = Env::default();
        env.tx.caller = caller;
        env.tx.transact_to = TxKind::Call(contract_addr);
        env.tx.data = calldata.clone().into();
        env.tx.gas_limit = gas_limit;
        
        // 准备合约和字节码
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

        // 创建和执行EVM
        let mut evm = Evm::builder()
            .with_spec_id(SpecId::LATEST)
            .with_ref_db(cache)
            .with_env(Box::new(env))
            .build();
        
        // 执行交易并打印结果
        let start = Instant::now();
        let result = evm.transact_preverified();
        let end = Instant::now();
        println!("Execution time: {:?}", end.duration_since(start));
        println!("execution result: {:#?}", result);
    }

    #[test]
    fn test_parallelism_ratio() {
        // 直接使用benches中的测试用例
        let cases = get_bench_cases();

        for case in cases {
            // 准备测试环境和数据
            let bytecode = Bytes::from(case.bytecode.clone());
            let input = Bytes::from(case.calldata.clone());
            let gas_limit = 1_000_000_000;
            let caller = Address::from([0x1; 20]);
            let contract_addr = Address::from([0x2; 20]);

            // 设置EVM环境
            let mut env = Env::default();
            env.tx.caller = caller;
            env.tx.transact_to = TxKind::Call(contract_addr);
            env.tx.data = input.clone().into();
            env.tx.gas_limit = gas_limit;
            
            // 准备合约和字节码
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
            for (slot, value) in case.pre_determined_slots {
                let _ = cache.insert_account_storage(contract_addr, slot, value);
            }

            // 创建和执行EVM（带SSA logger）
            let mut evm = Evm::builder()
                .with_spec_id(SpecId::LATEST)
                .with_ref_db(cache)
                .with_env(Box::new(env))
                .with_ssa_logger(SSALogger::new())
                .build_with_ssa_logger();
            
            // 执行交易
            let _ = evm.transact_preverified();

            // 获取日志
            let mut logger = evm.take_ssa_logger().unwrap();
            let logs = logger.take_logs();
            let lsns = logs.iter().map(|log| log.lsn).collect::<Vec<_>>();

            // 创建依赖图
            let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());

            // 构建图结构
            for log in logs {
                graph.add_node(log).unwrap();
            }

            for lsn in lsns {
                graph.add_edges(lsn).unwrap();
            }

            // 计算并打印并行度
            let parallelism = graph.calculate_parallelism_ratio().unwrap();
            println!("{} parallelism ratio: {:.4}", case.name, parallelism);
        }
    }

    #[test]
    fn test_execution_layers() {

        let case = get_bench_cases().into_iter()
            .find(|case| case.name == "hash_10k")
            .expect("hash_10k case not found");

        // 准备测试环境和数据
        let bytecode = Bytes::from(case.bytecode.clone());
        let input = Bytes::from(case.calldata.clone());
        let gas_limit = 1_000_000_000;
        let caller = Address::from([0x1; 20]);
        let contract_addr = Address::from([0x2; 20]);

        // 设置EVM环境
        let mut env = Env::default();
        env.tx.caller = caller;
        env.tx.transact_to = TxKind::Call(contract_addr);
        env.tx.data = input.clone().into();
        env.tx.gas_limit = gas_limit;
        
        // 准备合约和字节码
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

        // 创建和执行EVM（带SSA logger）
        let mut evm = Evm::builder()
            .with_spec_id(SpecId::LATEST)
            .with_ref_db(cache)
            .with_env(Box::new(env))
            .with_ssa_logger(SSALogger::new())
            .build_with_ssa_logger();
        
        // 执行交易
        let _ = evm.transact_preverified();

        // 获取日志
        let mut logger = evm.take_ssa_logger().unwrap();
        let logs = logger.take_logs();
        let lsns = logs.iter().map(|log| log.lsn).collect::<Vec<_>>();

        // 创建依赖图
        let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());

        // 构建图结构
        for log in logs {
            graph.add_node(log).unwrap();
        }

        for lsn in lsns {
            graph.add_edges(lsn).unwrap();
        }

        // 获取执行层
        let layers = graph.execution_layers().unwrap();

        // 统计每层的opcode分布
        for (layer_idx, layer) in layers.iter().enumerate() {
            let mut opcode_counts = HashMap::new();
            
            // 统计当前层的opcode
            for log in layer {
                let opcode_name = format!("0x{:02X}", log.opcode);
                let count = opcode_counts.entry(opcode_name).or_insert(0);
                *count += 1;
            }

            // 打印统计结果
            println!("\nLayer {}: (total ops: {})", layer_idx + 1, layer.len());
            for (opcode, count) in opcode_counts.iter() {
                println!("  {}: {}", opcode, count);
            }
        }
    }

    #[test]
    fn test_graph_execution() {

        let case = get_bench_cases().into_iter()
            .find(|case| case.name == "hash_10k")
            .expect("hash_10k case not found");

        // 准备测试环境和数据
        let bytecode = Bytes::from(case.bytecode.clone());
        let input = Bytes::from(case.calldata.clone());
        let gas_limit = 1_000_000_000;
        let caller = Address::from([0x1; 20]);
        let contract_addr = Address::from([0x2; 20]);

        // 设置EVM环境
        let mut env = Env::default();
        env.tx.caller = caller;
        env.tx.transact_to = TxKind::Call(contract_addr);
        env.tx.data = input.clone().into();
        env.tx.gas_limit = gas_limit;
        
        // 准备合约和字节码
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

        // 创建和执行EVM（带SSA logger）
        let mut evm = Evm::builder()
            .with_spec_id(SpecId::LATEST)
            .with_ref_db(cache.clone())
            .with_env(Box::new(env.clone()))
            .with_ssa_logger(SSALogger::new())
            .build_with_ssa_logger();
        
        // 执行交易
        let _ = evm.transact_preverified();

        // 获取日志
        let mut logger = evm.take_ssa_logger().unwrap();
        let logs = logger.take_logs();
        let lsns = logs.iter().map(|log| log.lsn).collect::<Vec<_>>();
        let first_call = logger.take_first_call_input();
        let first_create = logger.take_first_create_input();

        // 创建依赖图
        let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());

        // 构建图结构
        for log in logs {
            graph.add_node(log).unwrap();
        }

        for lsn in lsns {
            graph.add_edges(lsn).unwrap();
        }

        let env_clone = env.clone();
        let context = Arc::new(ExecutionContext::<'_, CacheDB<EmptyDB>, LatestSpec>::new(
            &env_clone, 
            cache, 
            first_call, 
            first_create
        ));

        // 获取拓扑排序的节点
        let nodes_to_execute = graph.topological_sort().unwrap();
        
        // 不安全地获取图的可变引用（用于基准测试）
        let arc_graph = Arc::new(graph);
        let mut_graph = unsafe { &mut *(Arc::as_ptr(&arc_graph) as *mut SsaGraph) };

        let start = Instant::now();
        for node_index in nodes_to_execute.clone() {
            let node = mut_graph.get_node_by_index_mut(node_index);
            SSAExecutor::execute_node(node, &arc_graph, &context).unwrap();
        }
        let end = Instant::now();
        println!("Execution time: {:?}", end.duration_since(start));
        
    }
} 