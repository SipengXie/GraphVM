# revm-ssa

SSA (Static Single Assignment) analysis for REVM - A powerful logging and analysis system for EVM execution.

## Table of Contents

- [Introduction](#introduction)
- [Core Features](#core-features)
- [Supported Operations](#supported-operations)
- [Installation](#installation)
- [Usage Examples](#usage-examples)
- [Detailed Documentation](#detailed-documentation)
- [Performance Considerations](#performance-considerations)
- [Testing](#testing)
- [Contributing](#contributing)

## Introduction

revm-ssa is a core component of REVM (Rust Ethereum Virtual Machine) that provides comprehensive tracking and analysis of EVM execution in SSA (Static Single Assignment) form. It maintains detailed operation logs and dependency relationships, offering powerful support for smart contract analysis, optimization, and debugging.

### Key Features

- Complete EVM operation tracking
- Precise dependency analysis
- Detailed storage access tracking
- Efficient frame management
- Rich analysis tools

## Core Features

### SSA Logger Structure

```rust
pub struct SSALogger {
    // Current LSN - increments with each operation
    pub current_lsn: usize,
    // Initial LSN - set at frame creation
    pub init_lsn: usize,
    // Log entries for all operations
    logs: Vec<SSALogEntry>,
    // Shadow stack - tracks stack item definitions
    pub stack: ShadowStack,
    // Maps storage slots to their last write LSN
    latest_writes: HashMap<StorageKey, usize>,
    // Maps storage slots to their first read LSN
    first_reads: HashMap<StorageKey, usize>,
    // Last LSN that modified memory
    last_memory: usize,
}
```

### Value Types

```rust
pub enum SSAValue {
    U256(U256),        // 256-bit unsigned integer
    I32(i32),          // 32-bit signed integer
    U64(u64),          // 64-bit unsigned integer
    Bytes(Bytes),      // Byte array
    Address(Address),  // Ethereum address
    LOG(Log),         // Event log
}
```

### Dependency Types

```rust
pub enum OperandDep {
    // Stack value dependency
    Stack(Option<usize>),
    // Storage value dependency
    Storage(Option<usize>),
    // Direct LSN dependency
    LSN(usize),
    // Memory size dependency
    MemorySize(usize),
}
```

## Supported Operations

### EVM Operations

#### 1. Stack Operations
- **Push Operations**: PUSH1 to PUSH32 (32 operations)
- **Pop Operations**: POP
- **Duplication**: DUP1 to DUP16 (16 operations)
- **Swap Operations**: SWAP1 to SWAP16 (16 operations)

#### 2. Arithmetic Operations
- **Basic Arithmetic**
  - ADD, MUL, SUB, DIV
  - SDIV, MOD, SMOD
  - ADDMOD, MULMOD
- **Comparison**
  - LT, GT, SLT, SGT, EQ
- **Bitwise Operations**
  - AND, OR, XOR, NOT
  - BYTE, SHL, SHR, SAR

#### 3. Memory Operations
- **Basic Memory**
  - MLOAD: Load word from memory
  - MSTORE: Save word to memory
  - MSTORE8: Save byte to memory
- **Memory Info**
  - MSIZE: Get memory size
- **Memory Expansion**: Automatic tracking

#### 4. Storage Operations
- **Storage Access**
  - SLOAD: Load from storage
  - SSTORE: Save to storage
- **Access Tracking**
  - Storage slot tracking
  - First read/last write tracking

#### 5. Flow Operations
- **Jumps**
  - JUMP: Unconditional jump
  - JUMPI: Conditional jump
- **Execution Control**
  - STOP: Halt execution
  - RETURN: Return data
  - REVERT: Revert state
  - INVALID: Invalid instruction

#### 6. Environmental Operations
- **Block Information**
  - BLOCKHASH, COINBASE
  - TIMESTAMP, NUMBER
  - DIFFICULTY, GASLIMIT
- **Execution Context**
  - ADDRESS, BALANCE
  - ORIGIN, CALLER
  - CALLVALUE
  - CALLDATALOAD, CALLDATASIZE, CALLDATACOPY
  - CODESIZE, CODECOPY
  - EXTCODESIZE, EXTCODECOPY
  - RETURNDATASIZE, RETURNDATACOPY
  - EXTCODEHASH
- **Gas Operations**
  - GAS: Remaining gas
  - GASPRICE: Transaction gas price

#### 7. System Operations
- **Logging**
  - LOG0 to LOG4: Event logging (5 operations)
- **Contract Creation**
  - CREATE: Create new contract
  - CREATE2: Create new contract with salt
- **Contract Calls**
  - CALL: Regular call
  - CALLCODE: Code-only call
  - DELEGATECALL: Delegated call
  - STATICCALL: Static call
- **Self Destruction**
  - SELFDESTRUCT: Destroy contract

#### 8. Cryptographic Operations
- KECCAK256 (SHA3): Compute Keccak-256 hash

### Special Use Cases

#### 1. Contract Analysis
- Creation trace tracking
- Call chain analysis
- State change monitoring
- Dependency tracking between contracts

#### 2. Storage Analysis
- Storage access pattern detection
- Storage conflict identification
- State dependency analysis
- Write/Read sequence tracking

#### 3. Gas Analysis
- Operation cost tracking
- High-cost operation identification
- Optimization suggestions
- Gas usage patterns

#### 4. Security Analysis
- Reentrancy detection
- Access control verification
- Exception handling analysis
- State consistency checking

#### 5. Data Flow Analysis
- Value origin tracking
- Dependency relationship analysis
- Optimization opportunity identification
- Cross-contract data flow

#### 6. Debugging Support
- Detailed execution tracing
- State backtracking
- Error localization
- Stack state inspection

#### 7. Performance Optimization
- Memory management
- Batch operation processing
- Caching strategies
- Resource usage optimization

### Internal Operations

The logger also tracks several internal operations:

1. **Frame Management**
   - Frame creation tracking
   - Frame transition logging
   - Call result recording
   - State preservation

2. **Dependency Tracking**
   - Inter-operation dependencies
   - Cross-frame dependencies
   - Storage dependencies
   - Memory dependencies

3. **State Transitions**
   - Contract creation states
   - Call execution states
   - Return data handling
   - Exception management

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
revm-ssa = "0.1.0"
```

## Usage Examples

### Basic Operation Tracking

```rust
use revm_ssa::SSALogger;

// Create logger
let mut logger = SSALogger::new();

// Track basic operations
logger.log_push_operation(PUSH1, &[1u8]);
logger.log_push_operation(PUSH1, &[2u8]);
logger.log_pop_top_operation(ADD, vec![...], result);

// Get logs
let logs = logger.get_logs();
```

### Storage Access Analysis

```rust
// Track storage access
logger.log_sstore_operation(address, key, value);
logger.log_sload_operation(address, key);

// Get storage access information
let latest_writes = logger.get_latest_writes();
let first_reads = logger.get_first_reads();
```

### Contract Analysis

```rust
// Analyze contract creation
logger.log_make_create_frame(caller, address);
logger.log_create_opcode(CREATE);
logger.log_memory_operation(...);
logger.log_insert_create_outcome(result);

// Get creation trace
let logs = logger.get_logs();
let deps = logger.get_dependencies();
```

## Detailed Documentation

### Lifecycle in REVM

1. **Initialization**
```rust
// Basic initialization
let logger = SSALogger::new();

// With specific LSN
let logger = SSALogger::new_with_lsn(start_lsn);
```

2. **Transfer Flow**
```rust
// 1. Logger starts in EVM context
assert!(context.evm.ssa_logger.is_some());

// 2. Transferred to interpreter
context.evm.inner_context_mut().transfer_ssa_logger_to_interpreter(&mut frame.interpreter);

// 3. Used during execution
interpreter.log_operation(...);

// 4. Recovered back to context
context.evm.inner_context_mut().recover_ssa_logger_from_interpreter(&mut frame.interpreter);
```

### API Reference

#### Basic Stack Operations
- `log_push_operation`: PUSH1-PUSH32
- `log_pop_operation`: POP
- `log_dup_operation`: DUP1-DUP16

#### Memory Operations
- `log_mload_operation`: MLOAD
- `log_mstore_operation`: MSTORE/MSTORE8
- `log_msize_operation`: MSIZE

#### Storage Operations
- `log_sload_operation`: SLOAD
- `log_sstore_operation`: SSTORE

#### Contract Operations
- `log_create_opcode`: CREATE/CREATE2
- `log_call_opcode`: CALL/CALLCODE/DELEGATECALL/STATICCALL

#### Environment Information
- `log_gas_operation`: GAS
- `log_balance_operation`: BALANCE
- `log_codesize_operation`: EXTCODESIZE
- `log_codehash_operation`: EXTCODEHASH

#### Other Operations
- `log_keccak256_operation`: KECCAK256/SHA3
- `log_log_operation`: LOG0-LOG4

## Performance Considerations

### Memory Management
```rust
impl SSALogger {
    // Pre-allocate common buffer sizes
    fn pre_allocate(&mut self) {
        self.logs.reserve(1024);
        self.stack.data.reserve(1024);
    }
    
    // Reuse buffers when possible
    fn clear(&mut self) {
        self.logs.clear();  // Keeps capacity
        self.stack.clear();
    }
}
```

### Error Handling

```rust
// Memory limit check
if memory.would_exceed_limit(new_size) {
    return Err(EvmError::OutOfGas);
}

// Stack overflow protection
if stack.len() >= STACK_LIMIT {
    return Err(EvmError::StackOverflow);
}
```

## Testing

### Unit Tests

```rust
#[test]
fn test_logger_basic() {
    let mut logger = SSALogger::new();
    logger.log_push_operation(PUSH1, &[1u8]);
    assert_eq!(logger.get_logs().len(), 1);
}
```

### Integration Tests

```rust
#[test]
fn test_contract_execution() {
    let mut logger = SSALogger::new();
    // Setup contract environment
    execute_contract(&mut logger);
    // Verify execution trace
    verify_execution(&logger);
}
```

### Property Tests

```rust
#[test]
fn test_logger_properties() {
    // Test LSN monotonicity
    assert!(logger.current_lsn > logger.init_lsn);
    
    // Test stack consistency
    assert_eq!(logger.stack.len(), expected_size);
    
    // Test memory tracking
    assert!(logger.last_memory <= logger.current_lsn);
}
```

## Contributing

Contributions are welcome! Before submitting a Pull Request, please:

1. Update tests
2. Update documentation
3. Follow the code style guide
4. Add necessary comments

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details. 