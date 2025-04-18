// 基准测试库文件
// 这个库主要包含revm基准测试

pub mod benches;

pub fn add(a: i32, b: i32) -> i32 {
    a + b
}

#[cfg(test)]
mod tests {
    use crossbeam::queue::SegQueue;
    use rayon::prelude::*;
    use revm::db::{CacheDB, EmptyDB};
    use revm::{inspector_handle_register, Evm};
    use revm_interpreter::SharedMemory;
    use revm_primitives::{uint, AccountInfo, AccountStatus, Address, Bytes, Env, LatestSpec, SpecId, TxKind};
    use revm_ssa::FrameInput;
    use revm_ssa_graph::instruction_table::InstructionTable;
    use revm_ssa_graph::{ExecutionContext, SsaGraph};
    use typed_graph::context::ExternalContext;
    use typed_graph::ssa_converter::{ConstantPool, SsaConverter};
    use std::cell::RefCell;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::{collections::HashMap, sync::Arc, time::Instant};

    // 从benches模块导入get_bench_cases
    use crate::benches::revm_ssa_bench::{get_bench_cases, BenchCase};
    /// A simple inspector that counts the number of steps executed
    #[derive(Default, Debug)]
    struct StepCounter {
        count: usize,
    }

    impl<DB: revm::primitives::db::Database> revm::Inspector<DB> for StepCounter {
        fn step(
            &mut self,
            _interp: &mut revm::interpreter::Interpreter,
            _context: &mut revm::EvmContext<DB>,
        ) {
            self.count += 1;
        }
    }

    #[test]
    fn test_execution() {
        let cases = get_bench_cases();
        let BenchCase {
            bytecode,
            calldata,
            name: _,
            pre_determined_slots: _,
        } = &cases[7]; // hash_10k Case
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
        let bytecode =
            revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));

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
            let bytecode = revm_interpreter::analysis::to_analysed(
                revm_primitives::Bytecode::new_raw(bytecode),
            );

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

            // 创建和执行EVM（带SSA logger和StepCounter）
            let mut evm = Evm::builder()
                .with_spec_id(SpecId::LATEST)
                .with_ref_db(cache)
                .with_env(Box::new(env))
                .with_ssa_logger()
                .with_external_context(StepCounter::default())
                .append_handler_register(inspector_handle_register)
                .build_with_ssa_logger();
            // 执行交易
            let _ = evm.transact_preverified();

            // 获取日志
            let mut logger = evm.take_ssa_logger().unwrap();
            let logs = logger.take_logs();
            let lsns = logs.iter().map(|log| log.lsn).collect::<Vec<_>>();
            // 获取步骤计数
            let step_count = evm.into_context().external.count;
            println!(
                "{} logs length: {}, step count: {}",
                case.name,
                logs.len(),
                step_count
            );
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
    fn test_typed_graph_execution() {
        // 直接使用benches中的测试用例
        let cases = get_bench_cases();
        let mut constant_pool = ConstantPool::new();

        for case in &cases {
            // let case = &cases[8];
            println!("{}", case.name);
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
            let bytecode = revm_interpreter::analysis::to_analysed(
                revm_primitives::Bytecode::new_raw(bytecode),
            );

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
            for (slot, value) in case.pre_determined_slots.clone() {
                let _ = cache.insert_account_storage(contract_addr, slot, value);
            }

            // 创建和执行EVM（带SSA logger和StepCounter）
            let mut evm = Evm::builder()
                .with_spec_id(SpecId::LATEST)
                .with_ref_db(cache)
                .with_env(Box::new(env.clone()))
                .with_ssa_logger()
                .with_external_context(StepCounter::default())
                .append_handler_register(inspector_handle_register)
                .build_with_ssa_logger();
            // 执行交易
            let _ = evm.transact_preverified();

            // 获取日志
            let mut logger = evm.take_ssa_logger().unwrap();
            let logs = logger.take_logs();
            let first_frame = logger.take_first_frame_input().unwrap();
           
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
            for (slot, value) in case.pre_determined_slots.clone() {
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

            // 创建SSA转换器实例，用于将执行日志转换为类型化图
            let mut converter = SsaConverter::new(
                external_context,
                shared_memory,
                &env as *const Env,                           // 环境指针
                &first_frame as *const FrameInput,           // 第一帧输入指针
                &mut constant_pool,
            );

            // 将执行日志转换为类型化图和常量池
            let mut typed_graph = converter.convert(logs);

            let _ = typed_graph.execute();
            // break;
        }
    }


    #[test]
    fn test_execution_layers() {
        let case = get_bench_cases()
            .into_iter()
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
        let bytecode =
            revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));

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
            .with_ssa_logger()
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
        let case = get_bench_cases()
            .into_iter()
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
        let bytecode =
            revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));

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
            .with_ssa_logger()
            .build_with_ssa_logger();

        // 执行交易
        let _ = evm.transact_preverified();

        // 获取日志
        let mut logger = evm.take_ssa_logger().unwrap();
        let logs = logger.take_logs();
        let lsns = logs.iter().map(|log| log.lsn).collect::<Vec<_>>();
        let first_frame_input = logger.take_first_frame_input();

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

        let start = Instant::now();
        for lsn in nodes_to_execute.clone() {
            let node = mut_graph.get_node_mut(lsn).unwrap();
            table.instructions[node.opcode as usize](&mut context, node, &arc_graph).unwrap();
        }
        let end = Instant::now();
        println!("Execution time: {:?}", end.duration_since(start));
    }

    #[test]
    fn test_parallel_graph_execution() {
        let case = get_bench_cases()
            .into_iter()
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
        let bytecode =
            revm_interpreter::analysis::to_analysed(revm_primitives::Bytecode::new_raw(bytecode));

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
            .with_ssa_logger()
            .build_with_ssa_logger();

        // 执行交易
        let _ = evm.transact_preverified();

        // 获取日志
        let mut logger = evm.take_ssa_logger().unwrap();
        let logs = logger.take_logs();
        let lsns = logs.iter().map(|log| log.lsn).collect::<Vec<_>>();
        let first_frame_input = logger.take_first_frame_input();
        let len = logs.len();

        // 创建依赖图
        let mut graph = SsaGraph::new(logs.len(), 2 * logs.len());

        // 构建图结构
        for log in logs {
            graph.add_node(log).unwrap();
        }

        for lsn in lsns {
            graph.add_edges(lsn).unwrap();
        }

        // 预处理每个节点的后继列表
        let mut successors: Vec<Vec<u32>> = vec![Vec::new(); len + 1];
        for lsn in 1..=len {
            if let Ok(succs) = graph.get_successors(lsn as u32) {
                successors[lsn] = succs.to_vec();
            }
        }

        let env_clone = env.clone();
        let context = Arc::new(ExecutionContext::<'_, CacheDB<EmptyDB>>::new::<LatestSpec>(
            &env_clone,
            cache,
            first_frame_input,
        ));

        let table = InstructionTable::create_instruction_table::<LatestSpec>();

        // 获取拓扑排序的节点
        let nodes_to_execute = graph.topological_sort().unwrap();

        // 创建线程池
        let thread_pool = rayon::ThreadPoolBuilder::new()
            .num_threads(4)
            .build()
            .unwrap();

        // 使用原子计数器追踪前驱完成情况
        let pred_counters = (0..=len)
            .map(|lsn| {
                if lsn == 0 {
                    AtomicU32::new(0)
                } else {
                    match graph.get_predecessors(lsn as u32) {
                        Ok(preds) => AtomicU32::new(preds.len() as u32),
                        Err(_) => AtomicU32::new(0),
                    }
                }
            })
            .collect::<Vec<_>>();

        // 创建任务队列
        let task_queue = SegQueue::new();

        // 初始化：将没有前驱的节点加入队列
        for lsn in nodes_to_execute.iter() {
            let lsn = *lsn;
            if pred_counters[lsn as usize].load(Ordering::Relaxed) == 0 {
                task_queue.push(lsn);
            }
        }

        let arc_graph = Arc::new(graph);
        let successors = Arc::new(successors);
        // let num_threads = thread_pool.current_num_threads();

        let start = Instant::now();
        thread_pool.install(|| {
            (0..thread_pool.current_num_threads())
                .into_par_iter()
                .for_each(|_| {
                    while let Some(lsn) = task_queue.pop() {
                        let mut_graph = unsafe { &mut *(Arc::as_ptr(&arc_graph) as *mut SsaGraph) };
                        let mut_context = unsafe {
                            &mut *(Arc::as_ptr(&context) as *mut ExecutionContext<CacheDB<EmptyDB>>)
                        };
                        let node = mut_graph.get_node_mut(lsn).unwrap();

                        // 执行节点
                        let _ =
                            table.instructions[node.opcode as usize](mut_context, node, &arc_graph);

                        // 更新所有后继节点的前驱计数器
                        for &succ_lsn in &successors[lsn as usize] {
                            if pred_counters[succ_lsn as usize].fetch_sub(1, Ordering::AcqRel) == 1
                            {
                                task_queue.push(succ_lsn);
                            }
                        }
                    }
                });
        });
        let end = Instant::now();

        println!("Parallel execution time: {:?}", end.duration_since(start));
    }
}
