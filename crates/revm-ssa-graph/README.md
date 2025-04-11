# revm-ssa-graph

revm-ssa-graph is a Static Single Assignment (SSA) based analysis tool for the Ethereum Virtual Machine (EVM) built on top of revm. This tool optimizes smart contract execution by constructing and analyzing dependency graphs of EVM bytecode.

## Core Features

### 1. Dependency Graph Implementation
```rust
pub struct SsaGraph {
    // Graph structure
    graph: DiGraph<SSALogEntry, ()>,
    // Mapping from LSN to node index
    lsn_to_node: Vec<NodeIndex>,
    // LSNs of storage write operations
    storage_write: Vec<LsnType>,
    // LSNs of log operations
    logs: Vec<LsnType>,
    // Last return operation
    last_return: LsnType,
    // Gas calculation
    pub gas_calc: LsnType,
}
```

### 2. Executor
```rust
pub struct SSAExecutor<'a, DB> 
where
    DB: DatabaseRef + Send + Sync + 'a,
    DB::Error: Send + Sync,
{
    // Execution context
    pub context: Arc<ExecutionContext<'a, DB>>,
    // Dependency graph
    pub graph: Arc<SsaGraph>,
    // Execution tracer (optional)
    pub tracer: Option<ExecutionTracer>,
    // Execution mode
    pub mode: ExecutionMode,
}
```

### 3. Execution Mode
```rust
pub enum ExecutionMode {
    // Execute all operations
    Full,
    // Start execution from specified LSNs
    Partial(Vec<LsnType>),
}
```

## Key Features

### 1. Graph Analysis and Optimization
- Complete dependency tracking
- Instruction-level dependency graph construction
- Partial re-execution support
- Parallelism analysis
- Execution layer analysis

### 2. Instruction Support
- Arithmetic operations (ADD, SUB, MUL, DIV, etc.)
- Bitwise operations (AND, OR, XOR, etc.)
- Memory operations (MLOAD, MSTORE, MCOPY, etc.)
- Control flow (JUMP, JUMPI)
- System operations (BALANCE, SELFBALANCE, etc.)
- Host environment interactions (TLOAD, TSTORE, etc.)

### 3. Performance Optimizations
- Efficient graph construction algorithms
- Smart partial execution strategies
- Optimized memory management
- Cache-friendly data structures

### 4. Debugging Support
- Detailed execution tracing
- Result comparison functionality
- Error localization
- Performance analysis tools

## Usage Examples

### Basic Usage
```rust
use revm_ssa_graph::{SSAExecutor, SsaGraph, ExecutionTracer};

// Create dependency graph
let mut graph = SsaGraph::new(node_num, edge_num);

// Add nodes and edges
graph.add_node(entry)?;
graph.add_edges(entry.lsn)?;

// Create executor
let mut executor = SSAExecutor::new::<SPEC>(
    Arc::new(graph),
    db,
    env,
    first_frame_input
);

// Execute
let (executed_nodes, duration) = executor.execute::<SPEC>(tx_hash)?;
```

### Partial Execution
```rust
// Set partial execution mode
let executor = executor.with_mode(ExecutionMode::Partial(vec![start_lsn]));

// Execute specified nodes and their dependencies
executor.execute::<SPEC>(tx_hash)?;
```

### Execution Tracing
```rust
// Enable tracing
let tracer = ExecutionTracer::new();
let executor = executor.with_tracer(Some(tracer));

// Get tracing results
if let Some(tracer) = executor.into_tracer() {
    // Analyze trace results
}
```

## Performance Considerations

### Graph Construction Optimization
- Pre-allocated node and edge capacity
- Efficient LSN to node index mapping
- Optimized storage access recording

### Execution Optimization
- Smart partial execution strategies
- Parallel execution support
- Cache-friendly data access patterns

### Memory Management
- Pre-allocated buffers
- Minimized memory allocations and copies
- Efficient data structure choices

## Development Status

The project is currently under active development:

1. **Test Coverage**
   - Basic operations well tested
   - Complex scenario testing in progress
   - Edge case handling optimization ongoing

2. **Known Limitations**
   - Some complex EVM operations may require special handling
   - Performance optimizations still in progress
   - Edge cases in dependency tracking need attention

3. **Development Focus**
   - Improving test coverage
   - Optimizing performance bottlenecks
   - Enhancing documentation and examples

4. **Ongoing Refactoring**
   - Current instruction factory implementation has limitations
   - Planning to refactor instruction handling with a new table-based approach:
   ```rust
   pub struct InstructionTable {
       pub instructions: [fn(&mut ExecutionContext, &mut SSALogEntry, &SsaGraph) -> Result<()>; 256],
       pub spec_id: SpecId,
   }
   ```
   - Goals of the refactoring:
     * Better separation of concerns
     * Improved compile-time optimization
     * More flexible spec-dependent instruction handling
     * Enhanced performance through inline optimizations
     * Clearer error handling mechanisms

## Contributing

Contributions are welcome! Please note:
1. APIs may change
2. Follow Rust best practices
3. Maintain code performance
4. Update relevant documentation