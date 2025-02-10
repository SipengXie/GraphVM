# revm-ssa-graph

revm-ssa-graph is a Static Single Assignment (SSA) based analysis tool for the Ethereum Virtual Machine (EVM) built on top of revm. This tool aims to optimize smart contract execution by constructing and analyzing dependency graphs of EVM bytecode.

## Overview

The project implements a novel approach to EVM execution optimization through:
- SSA-based dependency tracking
- Instruction-level dependency graph construction
- Partial re-execution optimization
- Performance analysis and tracing

## ⚠️ Development Status

This project is currently under active development. Please note:

1. **Test Coverage**: Not all tests are passing yet. Current test suite includes:
   - Arithmetic operations
   - Bitwise operations
   - Stack operations
   - Memory operations
   - Control flow
   - System operations
   - Contract interactions
   - Partial re-execution

2. **Known Limitations**:
   - Some complex EVM operations may not be fully supported
   - Performance optimizations are still ongoing
   - Edge cases in dependency tracking might exist
   - Documentation is being improved

## Architecture

The codebase is organized into several key components:

- `context.rs`: Execution context management
- `executor.rs`: Core execution engine
- `graph.rs`: Dependency graph implementation
- `instructions/`: EVM instruction handlers
- `tracer.rs`: Execution tracing functionality

## Usage Example

```rust
use revm_ssa_graph::{DependencyGraph, SSAExecutor, ExecutionTracer};

// Create dependency graph
let mut graph = DependencyGraph::new();

// Add nodes and edges
graph.add_node(entry).unwrap();
graph.add_edges(entry.lsn).unwrap();

// Create executor
let mut executor = SSAExecutor::new(graph, db, env);

// Execute
executor.execute().unwrap();
```

## Current Development Focus

1. **Test Stability**
   - Improving test coverage
   - Fixing failing test cases
   - Adding more comprehensive test scenarios

2. **Performance Optimization**
   - Enhancing graph construction efficiency
   - Optimizing partial re-execution
   - Reducing memory overhead

3. **Feature Completion**
   - Implementing missing EVM operations
   - Improving error handling
   - Enhancing debugging capabilities

## Contributing

While contributions are welcome, please be aware that:
- The API is not yet stable and may change
- Some tests may fail
- Documentation might be incomplete

## License

This project is part of the revm ecosystem and is licensed under the MIT License. 