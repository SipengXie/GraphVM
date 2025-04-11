use criterion::{criterion_group, criterion_main, Criterion};
use revm_primitives::{U256, Env, LatestSpec};
use revm_ssa::{logger::LsnType, SSAInput, SSALogEntry, SSAOutput};
use revm_ssa_graph::{instruction_table::InstructionTable, ExecutionContext, SsaGraph};
use revm::db::{EmptyDB, CacheDB};
use rand::Rng;
use std::{time::Duration, sync::Arc};
use typed_graph::{instructions::arithmetic::AddNode, typed_graph::TypedNode};

// Constants for the benchmark
const NODE_COUNT: usize = 10_000;
const OPCODE_ADD: u8 = 0x01; // ADD opcode in EVM

// Generate random dependencies and constants for the test
fn generate_test_data() -> (Vec<usize>, Vec<U256>) {
    let mut rng = rand::thread_rng();
    let mut dependencies = Vec::with_capacity(NODE_COUNT);
    let mut constants = Vec::with_capacity(NODE_COUNT);

    // First node has no dependencies
    dependencies.push(0);
    constants.push(U256::from(rng.gen::<u64>()));

    // Generate dependencies and constants for remaining nodes
    for i in 1..NODE_COUNT {
        // Each node depends on a random previous node
        dependencies.push(rng.gen_range(0..i));
        constants.push(U256::from(rng.gen::<u64>()));
    }

    (dependencies, constants)
}

// Prepare SSA Graph for benchmarking
struct SSABenchData {
    graph: Arc<SsaGraph>,
    context: Arc<ExecutionContext<'static, CacheDB<EmptyDB>>>,
    nodes_to_execute: Vec<LsnType>,
}

// New struct for TypedGraph benchmark data
struct TypedGraphBenchData {
    nodes: Vec<AddNode>,
}

fn prepare_typed_graph(dependencies: &[usize], constants: &[U256]) -> TypedGraphBenchData {
    let mut nodes = Vec::with_capacity(NODE_COUNT);
    
    // First node: add constant with itself
    let first_node = AddNode::new(
        &constants[0] as *const U256,
        &constants[0] as *const U256
    );
    nodes.push(first_node);
    
    // Create remaining nodes
    for i in 1..NODE_COUNT {
        // Create node with dependency pointer and constant pointer
        let node = AddNode::new(
            nodes[dependencies[i]].get_u256_output(0).unwrap(),
            &constants[i] as *const U256
        );
        nodes.push(node);
    }

    TypedGraphBenchData {
        nodes,
    }
}

fn prepare_ssa_graph(dependencies: &[usize], constants: &[U256]) -> SSABenchData {
    // Create SSA Graph
    let mut graph = SsaGraph::new(NODE_COUNT, NODE_COUNT * 2);

    // Build nodes
    for i in 0..NODE_COUNT {
        let mut inputs = Vec::with_capacity(2);
        if i > 0 {
            // Add dependency input
            inputs.push(SSAInput::Stack((dependencies[i] as u32, 0)));
        } else {
            // Add constant input for first node
            inputs.push(SSAInput::Constant(constants[i]));
        }
        // Add constant input
        inputs.push(SSAInput::Constant(constants[i]));

        let outputs = vec![SSAOutput::Stack(U256::ZERO)];
        let entry = SSALogEntry::new(i as u32, OPCODE_ADD, inputs, outputs);
        graph.add_node(entry).unwrap();
    }

    // Add edges
    for i in 0..NODE_COUNT {
        graph.add_edges(i as u32).unwrap();
    }

    // Create execution context with static env
    let env = Box::leak(Box::new(Env::default()));
    let cache = CacheDB::new(EmptyDB::default());
    let context = Arc::new(ExecutionContext::<'static, CacheDB<EmptyDB>>::new::<LatestSpec>(
        env,
        cache,
        None,
    ));

    // Get topological sort and create Arc for graph
    let nodes_to_execute = graph.topological_sort().unwrap();
    let arc_graph = Arc::new(graph);

    SSABenchData {
        graph: arc_graph,
        context,
        nodes_to_execute,
    }
}

// Execute SSA Graph nodes and collect results
fn execute_ssa_graph(data: &SSABenchData) -> Vec<U256> {
    let mut results = vec![U256::ZERO; NODE_COUNT];
    
    // Get mutable reference to graph (unsafe but necessary for benchmark)
    let mut_graph = unsafe { &mut *(Arc::as_ptr(&data.graph) as *mut SsaGraph) };
    let table = InstructionTable::create_instruction_table::<LatestSpec>();
    let mut_context = unsafe { &mut *(Arc::as_ptr(&data.context) as *mut ExecutionContext<CacheDB<EmptyDB>>) };
    
    // Execute nodes using SSAExecutor
    for node_index in &data.nodes_to_execute {
        let node = mut_graph.get_node_mut(*node_index).unwrap();
        table.instructions[node.opcode as usize](mut_context, node, &data.graph).unwrap();
        
        // Store result
        if let SSAOutput::Stack(value) = node.outputs[0] {
            results[node.lsn as usize] = value;
        }
    }

    results
}

// Execute TypedGraph nodes
fn execute_typed_graph(data: &mut TypedGraphBenchData) {
    // Execute nodes in order
    for node in data.nodes.iter_mut() {
        node.execute().unwrap();
    }
}

// Benchmark traditional U256 calculation
fn bench_traditional(dependencies: &[usize], constants: &[U256]) -> Vec<U256> {
    let mut results = vec![U256::ZERO; NODE_COUNT];
    
    // First node just takes its constant
    results[0] = constants[0];

    // Process remaining nodes
    for i in 1..NODE_COUNT {
        results[i] = results[dependencies[i]].overflowing_add(constants[i]).0;
    }

    results
}

fn bench_ssa_vs_traditional(c: &mut Criterion) {
    let mut group = c.benchmark_group("ssa_add_comparison");
    group.sample_size(20);
    group.warm_up_time(Duration::from_secs(2));
    group.measurement_time(Duration::from_secs(5));

    // Generate test data
    let (dependencies, constants) = generate_test_data();

    // Benchmark SSA Graph execution
    group.bench_function("ssa_graph", |b| {
        b.iter_with_setup(
            || prepare_ssa_graph(&dependencies, &constants),
            |data| execute_ssa_graph(&data)
        )
    });

    // Benchmark TypedGraph execution
    group.bench_function("typed_graph", |b| {
        b.iter_with_setup(
            || prepare_typed_graph(&dependencies, &constants),
            |mut data| execute_typed_graph(&mut data)
        )
    });

    // Benchmark traditional implementation
    group.bench_function("traditional", |b| {
        b.iter(|| bench_traditional(&dependencies, &constants))
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_ssa_vs_traditional
);
criterion_main!(benches); 