use criterion::{black_box, criterion_group, criterion_main, Criterion, BenchmarkId};
use revm_ssa_graph::{SsaGraph, Result};
use revm_ssa::{SSAInput, SSAOutput, SSALogEntry, logger::LsnType};
use revm_primitives::U256;

// 宏模拟动态解析方法
macro_rules! get_ssa_output_stack_dynamic {
    ($graph:expr, $input:expr) => {
        match &$input {
            SSAInput::Constant(value) => *value,
            SSAInput::Stack(lsn_with_index) => {
                let node = $graph.get_node(lsn_with_index.0).unwrap();
                match &node.outputs[lsn_with_index.1 as usize] {
                    SSAOutput::Stack(value) => *value,
                    _ => panic!("Expected Stack output value"),
                }
            }
            _ => panic!("Input must be Stack or Constant value"),
        }
    };
}

struct MockExecutionContext<'a> {
    graph: &'a SsaGraph,
}

impl<'a> MockExecutionContext<'a> {
    // 动态解析实现 - ADD
    #[inline(always)]
    pub fn execute_add_dynamic(&self, node: &mut SSALogEntry) -> Result<()> {
        let a = get_ssa_output_stack_dynamic!(self.graph, node.inputs[0]);
        let b = get_ssa_output_stack_dynamic!(self.graph, node.inputs[1]);
        node.outputs[0] = SSAOutput::Stack(a.overflowing_add(b).0);
        Ok(())
    } 
}

// 生成ADD测试图
fn create_add_test_graph(ops_count: usize) -> SsaGraph {
    let mut graph = SsaGraph::new(ops_count + 1, ops_count * 3);
    
    // 添加初始节点
    let initial_node = SSALogEntry::new(
        1,
        0x01, // ADD
        vec![
            SSAInput::Constant(U256::from(100)),
            SSAInput::Constant(U256::from(200)),
        ],
        vec![SSAOutput::Stack(U256::from(300))],
    );
    graph.add_node(initial_node).expect("Failed to add node");
    
    // 添加连续的ADD操作，每个操作都依赖于前一个操作的结果
    for i in 2..=ops_count {
        let lsn_type = i as LsnType;
        let node = SSALogEntry::new(
            lsn_type,
            0x01, // ADD
            vec![
                SSAInput::Stack((lsn_type - 1, 0)),
                SSAInput::Constant(U256::from(i)), 
            ],
            vec![SSAOutput::Stack(U256::from(300 + i - 1))],
        );
        graph.add_node(node).expect("Failed to add node");
        graph.add_edges(lsn_type).expect("Failed to add edges");
    }
    
    graph
}

// 纯引用实现，专门用于测试纯直接引用性能
#[inline(always)]
fn pure_add(a: &U256, b: &U256) -> U256 {
    a.overflowing_add(*b).0
}

// 创建U256值数组用于纯引用测试
fn create_u256_chain(length: usize) -> Vec<U256> {
    let mut values = Vec::with_capacity(length + 1);
    
    // 初始值
    values.push(U256::from(300));
    
    // 添加后续值
    for i in 2..=length {
        let prev = values[i-2];
        let current = prev.overflowing_add(U256::from(i)).0;
        values.push(current);
    }
    
    values
}

pub fn bench_add_implementations(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssa_graph_add_execution");
    
    // 不同规模的测试
    for size in [100, 1000, 10000].iter() {
        let graph = create_add_test_graph(*size);
        let context = MockExecutionContext { graph: &graph };
        
        // 准备测试节点（用于前两种实现）
        let test_node = SSALogEntry::new(
            *size as LsnType + 1,
            0x01, // ADD
            vec![
                SSAInput::Stack((*size as LsnType, 0)),
                SSAInput::Constant(U256::from(42)),
            ],
            vec![SSAOutput::Stack(U256::from(0))],
        );
        
        // 获取常量值（用于真正的直接引用测试）
        let constant_value = U256::from(42);
        
        // 创建U256值链（用于纯引用测试）
        let u256_chain = create_u256_chain(*size);
        
        // 测试动态解析实现
        group.bench_with_input(
            BenchmarkId::new("add_dynamic_parse", size), 
            size,
            |b, _| {
                b.iter(|| {
                    let mut node = test_node.clone();
                    context.execute_add_dynamic(&mut node).unwrap();
                    black_box(node);
                })
            }
        );
        
        // 测试纯粹的引用实现（完全不依赖SSAGraph结构）
        group.bench_with_input(
            BenchmarkId::new("pure_direct_ref", size), 
            size,
            |b, _| {
                b.iter(|| {
                    // 直接使用最后一个值和常量进行纯引用加法
                    let last_value = &u256_chain[u256_chain.len() - 1];
                    let result = pure_add(last_value, &constant_value);
                    black_box(result);
                })
            }
        );
    }
    
    group.finish();
}

criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = bench_add_implementations
);
criterion_main!(benches); 